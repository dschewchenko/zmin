use std::fmt;
use std::io::Cursor;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use ipnet::IpNet;
use rustls::pki_types::{
    CertificateDer, PrivateKeyDer,
    pem::{PemObject, SectionKind, from_buf},
};
use url::Url;
use zeroize::Zeroize;

use crate::{HttpUrl, RequestHeader, RequestHeaders, TransportError};

const MAX_PROXY_URL_BYTES: usize = 8 * 1024;
const MAX_CERTIFICATE_BYTES: usize = 1024 * 1024;
const MAX_CERTIFICATE_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const MAX_IDENTITY_BYTES: usize = 1024 * 1024;

/// An HTTP(S) or SOCKS5 proxy URL. Debug output deliberately never exposes
/// the URL, because proxy URLs commonly contain credentials.
///
/// Both `socks5://` and `socks5h://` inputs are stored as `socks5h://`. This
/// deliberately forces destination-name resolution through the proxy, which
/// matches Git LFS v3.7.1's SOCKS behavior while avoiding local DNS leaks.
#[derive(Clone, PartialEq, Eq)]
pub struct ProxyUrl(Arc<ProxyUrlBytes>);

#[derive(PartialEq, Eq)]
struct ProxyUrlBytes {
    serialized: SecretBytes,
    origin: Box<[u8]>,
    scheme: String,
    host: String,
    username: Option<SecretBytes>,
    password: Option<SecretBytes>,
}

#[derive(PartialEq, Eq)]
struct SecretBytes(Box<[u8]>);

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

impl ProxyUrl {
    pub fn parse(raw: &str) -> Result<Self, TransportError> {
        if raw.is_empty()
            || raw.len() > MAX_PROXY_URL_BYTES
            || raw.contains(['\\', '#'])
            || raw
                .chars()
                .any(|character| character.is_control() || character.is_ascii_whitespace())
            || !has_valid_percent_encoding(raw)
        {
            return Err(TransportError::InvalidProxy);
        }
        let mut parsed = Url::parse(raw).map_err(|_| TransportError::InvalidProxy)?;
        if !matches!(parsed.scheme(), "http" | "https" | "socks5" | "socks5h")
            || parsed.host_str().is_none()
            || parsed.fragment().is_some()
            || parsed.query().is_some()
            || !matches!(parsed.path(), "" | "/")
        {
            return Err(TransportError::InvalidProxy);
        }
        if matches!(parsed.scheme(), "socks5" | "socks5h") {
            if parsed.port().is_none() {
                // Go's net/http proxy canonicalization, used by Git LFS
                // v3.7.1, assigns port 1080 to both SOCKS5 spellings.
                parsed
                    .set_port(Some(1080))
                    .map_err(|_| TransportError::InvalidProxy)?;
            }
            validate_socks_credentials(&parsed)?;
            parsed
                .set_scheme("socks5h")
                .map_err(|_| TransportError::InvalidProxy)?;
        }
        parsed.set_path("");
        if parsed.as_str().len() > MAX_PROXY_URL_BYTES {
            return Err(TransportError::InvalidProxy);
        }
        let scheme = parsed.scheme().to_owned();
        let host = parsed
            .host_str()
            .ok_or(TransportError::InvalidProxy)?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned();
        let port = parsed
            .port_or_known_default()
            .ok_or(TransportError::InvalidProxy)?;
        let has_userinfo = !parsed.username().is_empty() || parsed.password().is_some();
        let username = has_userinfo.then(|| SecretBytes(decode_userinfo(parsed.username())));
        let password = has_userinfo
            .then(|| SecretBytes(decode_userinfo(parsed.password().unwrap_or_default())));
        let authority = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let origin = format!("{scheme}://{authority}/")
            .into_bytes()
            .into_boxed_slice();
        Ok(Self(Arc::new(ProxyUrlBytes {
            serialized: SecretBytes(parsed.as_str().as_bytes().to_vec().into_boxed_slice()),
            origin,
            scheme,
            host,
            username,
            password,
        })))
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0.serialized.0).expect("validated proxy URL remains UTF-8")
    }

    /// Return a credential-free HTTPS origin for resolving proxy TLS policy.
    pub fn https_origin_url(&self) -> Result<Option<HttpUrl>, TransportError> {
        if self.0.scheme != "https" {
            return Ok(None);
        }
        let origin =
            std::str::from_utf8(&self.0.origin).map_err(|_| TransportError::InvalidProxy)?;
        HttpUrl::parse(origin).map(Some)
    }

    pub(crate) fn scheme(&self) -> &str {
        &self.0.scheme
    }

    pub(crate) fn host(&self) -> &str {
        &self.0.host
    }

    pub(crate) fn origin_str(&self) -> &str {
        std::str::from_utf8(&self.0.origin).expect("validated proxy origin remains UTF-8")
    }

    pub(crate) fn credentials(&self) -> Option<(&[u8], &[u8])> {
        Some((&self.0.username.as_ref()?.0, &self.0.password.as_ref()?.0))
    }

    /// Retain the validated proxy policy for an actual request target. The
    /// direct connector encodes literal IPv4/IPv6 SOCKS destinations as ATYP
    /// 0x01/0x04 and leaves domain-name resolution to the proxy as ATYP 0x03.
    pub fn for_request_url(&self, target: &HttpUrl) -> Result<Self, TransportError> {
        let _ = target;
        Ok(self.clone())
    }
}

impl fmt::Debug for ProxyUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProxyUrl(<redacted>)")
    }
}

/// Proxy selection for one actual request URL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProxyPolicy {
    #[default]
    System,
    Disabled,
    Explicit(ProxyUrl),
}

impl ProxyPolicy {
    pub(crate) fn for_request_url(&self, target: &HttpUrl) -> Result<Self, TransportError> {
        match self {
            Self::Explicit(proxy) => proxy.for_request_url(target).map(Self::Explicit),
            Self::System => Ok(Self::System),
            Self::Disabled => Ok(Self::Disabled),
        }
    }
}

struct SystemProxySnapshot {
    http: Option<ProxyUrl>,
    https: Option<ProxyUrl>,
    no_proxy: NoProxySnapshot,
}

impl SystemProxySnapshot {
    fn capture() -> Result<Self, TransportError> {
        if std::env::var_os("REQUEST_METHOD").is_some() {
            return Ok(Self {
                http: None,
                https: None,
                no_proxy: NoProxySnapshot::default(),
            });
        }
        let all = proxy_from_env(&["ALL_PROXY", "all_proxy"])?;
        let http = proxy_from_env(&["HTTP_PROXY", "http_proxy"])?.or_else(|| all.clone());
        let https = proxy_from_env(&["HTTPS_PROXY", "https_proxy"])?.or(all);
        let no_proxy = first_env(&["NO_PROXY", "no_proxy"])?
            .map(|value| NoProxySnapshot::parse(&value))
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            http,
            https,
            no_proxy,
        })
    }

    fn resolve(&self, url: &HttpUrl) -> ProxyPolicy {
        if self.no_proxy.matches(url) {
            return ProxyPolicy::Disabled;
        }
        let proxy = match url.parsed.scheme() {
            "http" => self.http.clone(),
            "https" => self.https.clone(),
            _ => None,
        };
        proxy.map_or(ProxyPolicy::Disabled, ProxyPolicy::Explicit)
    }
}

fn first_env(names: &[&str]) -> Result<Option<String>, TransportError> {
    for name in names {
        if let Some(value) = std::env::var_os(name) {
            return value
                .into_string()
                .map(Some)
                .map_err(|_| TransportError::InvalidProxy);
        }
    }
    Ok(None)
}

fn proxy_from_env(names: &[&str]) -> Result<Option<ProxyUrl>, TransportError> {
    let Some(value) = first_env(names)? else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let normalized = if value.contains("://") {
        value
    } else {
        format!("http://{value}")
    };
    ProxyUrl::parse(&normalized).map(Some)
}

#[derive(Default)]
struct NoProxySnapshot(Vec<NoProxyEntry>);

struct NoProxyEntry {
    host: NoProxyHost,
    port: Option<u16>,
}

enum NoProxyHost {
    Any,
    Address(IpAddr),
    Network(IpNet),
    Domain { suffix: String, matches_bare: bool },
}

impl NoProxySnapshot {
    fn parse(value: &str) -> Result<Self, TransportError> {
        let mut entries = Vec::new();
        for raw in value
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            let (host, port) = split_no_proxy_host_port(raw)?;
            let host = if host == "*" {
                NoProxyHost::Any
            } else if let Ok(network) = host.parse::<IpNet>() {
                NoProxyHost::Network(network)
            } else if let Ok(address) = host.parse::<IpAddr>() {
                NoProxyHost::Address(address)
            } else {
                let lower = host.trim_end_matches('.').to_ascii_lowercase();
                let (suffix, matches_bare) = if let Some(suffix) = lower.strip_prefix("*.") {
                    (suffix.to_owned(), false)
                } else if let Some(suffix) = lower.strip_prefix('.') {
                    (suffix.to_owned(), false)
                } else {
                    (lower, true)
                };
                if suffix.is_empty()
                    || suffix
                        .bytes()
                        .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')))
                {
                    return Err(TransportError::InvalidProxy);
                }
                NoProxyHost::Domain {
                    suffix,
                    matches_bare,
                }
            };
            entries.push(NoProxyEntry { host, port });
        }
        Ok(Self(entries))
    }

    fn matches(&self, url: &HttpUrl) -> bool {
        let host = url.origin.host.trim_end_matches('.');
        let port = url.origin.port;
        self.0.iter().any(|entry| {
            if entry.port.is_some_and(|expected| expected != port) {
                return false;
            }
            match &entry.host {
                NoProxyHost::Any => true,
                NoProxyHost::Address(expected) => host.parse::<IpAddr>() == Ok(*expected),
                NoProxyHost::Network(network) => host
                    .parse::<IpAddr>()
                    .is_ok_and(|address| network.contains(&address)),
                NoProxyHost::Domain {
                    suffix,
                    matches_bare,
                } => {
                    (*matches_bare && host.eq_ignore_ascii_case(suffix))
                        || host.len() > suffix.len()
                            && host
                                .get(host.len() - suffix.len()..)
                                .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix))
                            && host.as_bytes().get(host.len() - suffix.len() - 1) == Some(&b'.')
                }
            }
        })
    }
}

fn split_no_proxy_host_port(value: &str) -> Result<(&str, Option<u16>), TransportError> {
    if let Some(rest) = value.strip_prefix('[') {
        let (host, suffix) = rest.split_once(']').ok_or(TransportError::InvalidProxy)?;
        let port = if suffix.is_empty() {
            None
        } else {
            Some(
                suffix
                    .strip_prefix(':')
                    .ok_or(TransportError::InvalidProxy)?
                    .parse()
                    .map_err(|_| TransportError::InvalidProxy)?,
            )
        };
        return Ok((host, port));
    }
    if value.matches(':').count() == 1 {
        let (host, port) = value.rsplit_once(':').expect("one colon");
        if !host.contains('/') && port.bytes().all(|byte| byte.is_ascii_digit()) {
            return Ok((
                host,
                Some(port.parse().map_err(|_| TransportError::InvalidProxy)?),
            ));
        }
    }
    Ok((value, None))
}

/// One bounded certificate source used as a custom trust root.
#[derive(Clone, PartialEq, Eq)]
pub struct TlsRootCertificate {
    encoding: TlsRootCertificateEncoding,
    bytes: Arc<[u8]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TlsRootCertificateEncoding {
    Der,
    PemBundle,
}

impl fmt::Debug for TlsRootCertificate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TlsRootCertificate")
            .field("encoding", &self.encoding)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

impl TlsRootCertificate {
    pub fn from_der(mut bytes: Vec<u8>) -> Result<Self, TransportError> {
        if bytes.is_empty() || bytes.len() > MAX_CERTIFICATE_BYTES {
            bytes.fill(0);
            return Err(TransportError::InvalidCertificate);
        }
        if !valid_der_root(&bytes) {
            bytes.fill(0);
            return Err(TransportError::InvalidCertificate);
        }
        Ok(Self {
            encoding: TlsRootCertificateEncoding::Der,
            bytes: Arc::from(bytes),
        })
    }

    pub fn from_pem_bundle(mut bytes: Vec<u8>) -> Result<Self, TransportError> {
        if bytes.is_empty() || bytes.len() > MAX_CERTIFICATE_BYTES {
            bytes.fill(0);
            return Err(TransportError::InvalidCertificate);
        }
        if !valid_pem_roots(&bytes) {
            bytes.fill(0);
            return Err(TransportError::InvalidCertificate);
        }
        Ok(Self {
            encoding: TlsRootCertificateEncoding::PemBundle,
            bytes: Arc::from(bytes),
        })
    }

    fn add_to_rustls(&self, roots: &mut rustls::RootCertStore) -> Result<(), TransportError> {
        match self.encoding {
            TlsRootCertificateEncoding::Der => roots
                .add(CertificateDer::from(self.bytes.to_vec()))
                .map_err(|_| TransportError::InvalidCertificate),
            TlsRootCertificateEncoding::PemBundle => {
                for certificate in CertificateDer::pem_slice_iter(&self.bytes) {
                    roots
                        .add(
                            certificate
                                .map_err(|_| TransportError::InvalidCertificate)?
                                .into_owned(),
                        )
                        .map_err(|_| TransportError::InvalidCertificate)?;
                }
                Ok(())
            }
        }
    }
}

/// An immutable, cheaply cloned, bounded set of custom TLS trust roots.
#[derive(Clone, PartialEq, Eq)]
pub struct TlsRootCertificates {
    entries: Arc<[TlsRootCertificate]>,
    bytes: usize,
}

impl fmt::Debug for TlsRootCertificates {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TlsRootCertificates")
            .field("count", &self.entries.len())
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl TlsRootCertificates {
    pub fn new(entries: Vec<TlsRootCertificate>) -> Result<Self, TransportError> {
        if entries.is_empty() {
            return Err(TransportError::InvalidCertificate);
        }
        let bytes = entries
            .iter()
            .try_fold(0_usize, |total, entry| total.checked_add(entry.bytes.len()));
        let Some(bytes) = bytes else {
            return Err(TransportError::InvalidCertificate);
        };
        if bytes > MAX_CERTIFICATE_TOTAL_BYTES {
            return Err(TransportError::InvalidCertificate);
        }
        Ok(Self {
            entries: Arc::from(entries),
            bytes,
        })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn encoded_bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn to_rustls(&self) -> Result<rustls::RootCertStore, TransportError> {
        let mut roots = rustls::RootCertStore::empty();
        for entry in self.entries.iter() {
            entry.add_to_rustls(&mut roots)?;
        }
        if roots.is_empty() {
            return Err(TransportError::InvalidCertificate);
        }
        Ok(roots)
    }
}

/// TLS verification for one actual request URL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TlsVerification {
    #[default]
    Platform,
    CustomRoots(TlsRootCertificates),
    Disabled,
}

/// A bounded rustls client identity. Its bytes are never included in Debug.
#[derive(Clone, PartialEq, Eq)]
pub struct TlsClientIdentity(Arc<TlsClientIdentityBytes>);

#[derive(PartialEq, Eq)]
struct TlsClientIdentityBytes(Box<[u8]>);

impl Drop for TlsClientIdentityBytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

impl fmt::Debug for TlsClientIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TlsClientIdentity(<redacted>)")
    }
}

impl TlsClientIdentity {
    pub fn from_pem(mut bytes: Vec<u8>) -> Result<Self, TransportError> {
        if bytes.is_empty() || bytes.len() > MAX_IDENTITY_BYTES {
            bytes.fill(0);
            return Err(TransportError::InvalidCertificate);
        }
        let parsed = parse_rustls_identity(&bytes);
        if let Ok((_, mut key)) = parsed {
            key.zeroize();
        } else {
            bytes.fill(0);
            return Err(TransportError::InvalidCertificate);
        }
        Ok(Self(Arc::new(TlsClientIdentityBytes(
            bytes.into_boxed_slice(),
        ))))
    }
}

/// TCP keepalive configuration, independent of idle connection-pool expiry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TcpKeepalivePolicy {
    /// Preserve the platform verifier's default behavior.
    #[default]
    Default,
    Disabled,
    Enabled(Duration),
}

/// Connection-affecting settings resolved for an actual request URL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpConnectionPolicy {
    pub proxy: ProxyPolicy,
    pub tls: TlsVerification,
    pub tcp_keepalive: TcpKeepalivePolicy,
    pub client_identity: Option<TlsClientIdentity>,
}

/// TLS policy resolved independently for the outer HTTPS proxy origin.
/// Target trust relaxation and target mTLS never bleed into this profile.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProxyTlsPolicy {
    pub tls: TlsVerification,
    pub client_identity: Option<TlsClientIdentity>,
}

/// Connection-phase and rolling activity limits resolved for one actual URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HttpTimeoutPolicy {
    pub dial_timeout: Duration,
    pub tls_handshake_timeout: Duration,
    pub activity_timeout: Option<Duration>,
}

impl Default for HttpTimeoutPolicy {
    fn default() -> Self {
        Self {
            dial_timeout: Duration::from_secs(30),
            tls_handshake_timeout: Duration::from_secs(30),
            activity_timeout: None,
        }
    }
}

impl HttpTimeoutPolicy {
    pub(crate) fn validate(self) -> Result<Self, TransportError> {
        let now = std::time::Instant::now();
        if self.dial_timeout.is_zero()
            || self.tls_handshake_timeout.is_zero()
            || self
                .activity_timeout
                .is_some_and(|timeout| timeout.is_zero())
            || now.checked_add(self.dial_timeout).is_none()
            || now.checked_add(self.tls_handshake_timeout).is_none()
            || self
                .activity_timeout
                .is_some_and(|timeout| now.checked_add(timeout).is_none())
        {
            return Err(TransportError::InvalidTimeout);
        }
        Ok(self)
    }
}

/// Provenance of the effective configured-header list for one URL.
///
/// `None` differs from an explicitly empty `Generic` or `UrlScoped` list:
/// resolvers use the latter two to represent a reset after applying their
/// configuration precedence rules.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConfiguredRequestHeaderProvenance {
    #[default]
    None,
    Generic,
    UrlScoped,
}

/// Ordered configured headers. Every value is treated as secret, including
/// custom signed headers whose names are not normally credential-related.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ConfiguredRequestHeaders {
    provenance: ConfiguredRequestHeaderProvenance,
    headers: RequestHeaders,
}

impl fmt::Debug for ConfiguredRequestHeaders {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConfiguredRequestHeaders")
            .field("provenance", &self.provenance)
            .field("count", &self.headers.entries.len())
            .field("bytes", &self.headers.bytes)
            .finish()
    }
}

impl ConfiguredRequestHeaders {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn generic<I, N>(entries: I) -> Result<Self, TransportError>
    where
        I: IntoIterator<Item = (N, Vec<u8>)>,
        N: AsRef<str>,
    {
        Self::new(ConfiguredRequestHeaderProvenance::Generic, entries)
    }

    pub fn url_scoped<I, N>(entries: I) -> Result<Self, TransportError>
    where
        I: IntoIterator<Item = (N, Vec<u8>)>,
        N: AsRef<str>,
    {
        Self::new(ConfiguredRequestHeaderProvenance::UrlScoped, entries)
    }

    fn new<I, N>(
        provenance: ConfiguredRequestHeaderProvenance,
        entries: I,
    ) -> Result<Self, TransportError>
    where
        I: IntoIterator<Item = (N, Vec<u8>)>,
        N: AsRef<str>,
    {
        let mut headers = RequestHeaders::empty();
        let mut entries = entries.into_iter();
        while let Some((name, value)) = entries.next() {
            let result = RequestHeader::new_secret(name.as_ref(), value)
                .and_then(|header| headers.push(header));
            if let Err(error) = result {
                for (_, mut remaining_value) in entries {
                    remaining_value.fill(0);
                }
                return Err(error);
            }
        }
        Ok(Self {
            provenance,
            headers,
        })
    }

    pub fn provenance(&self) -> ConfiguredRequestHeaderProvenance {
        self.provenance
    }

    pub fn len(&self) -> usize {
        self.headers.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.headers.entries.is_empty()
    }

    pub(crate) fn headers(&self) -> &RequestHeaders {
        &self.headers
    }

    pub(crate) fn for_cross_origin(&self) -> Self {
        if self.provenance == ConfiguredRequestHeaderProvenance::UrlScoped {
            self.clone()
        } else {
            Self::none()
        }
    }
}

/// The complete policy resolved for the initial actual request URL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedRequestPolicy {
    pub connection: HttpConnectionPolicy,
    pub timeouts: HttpTimeoutPolicy,
    pub proxy_tls_policy: ProxyTlsPolicy,
    pub configured_headers: ConfiguredRequestHeaders,
}

/// Resolves URL-scoped transport configuration. The resolver is invoked for
/// each actual request target. Redirect targets replace the prior configured
/// header list with their complete post-precedence snapshot; provenance then
/// determines whether that list is eligible for a cross-origin hop. Results
/// must not be partial overrides of [`ClientConfig`](crate::ClientConfig).
pub trait RequestPolicyResolver: Send + Sync {
    fn resolve(&self, url: &HttpUrl) -> Result<ResolvedRequestPolicy, TransportError>;
}

pub(crate) struct StaticRequestPolicyResolver {
    policy: HttpConnectionPolicy,
    timeouts: HttpTimeoutPolicy,
    system_proxy: Option<SystemProxySnapshot>,
}

impl StaticRequestPolicyResolver {
    pub(crate) fn new(
        policy: HttpConnectionPolicy,
        timeouts: HttpTimeoutPolicy,
    ) -> Result<Self, TransportError> {
        let system_proxy = matches!(policy.proxy, ProxyPolicy::System)
            .then(SystemProxySnapshot::capture)
            .transpose()?;
        Ok(Self {
            policy,
            timeouts,
            system_proxy,
        })
    }
}

impl RequestPolicyResolver for StaticRequestPolicyResolver {
    fn resolve(&self, url: &HttpUrl) -> Result<ResolvedRequestPolicy, TransportError> {
        let mut connection = self.policy.clone();
        connection.proxy = match &connection.proxy {
            ProxyPolicy::System => self
                .system_proxy
                .as_ref()
                .expect("system proxy snapshot exists")
                .resolve(url),
            proxy => proxy.for_request_url(url)?,
        };
        Ok(ResolvedRequestPolicy {
            connection,
            timeouts: self.timeouts,
            proxy_tls_policy: ProxyTlsPolicy::default(),
            configured_headers: ConfiguredRequestHeaders::none(),
        })
    }
}

fn parse_rustls_identity(
    bytes: &[u8],
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), TransportError> {
    let mut reader = Cursor::new(bytes);
    let mut keys = Vec::new();
    let mut certificates = Vec::new();
    while let Some((kind, data)) =
        from_buf(&mut reader).map_err(|_| TransportError::InvalidCertificate)?
    {
        match kind {
            SectionKind::Certificate => certificates.push(data.into()),
            SectionKind::PrivateKey => keys.push(PrivateKeyDer::Pkcs8(data.into())),
            SectionKind::RsaPrivateKey => keys.push(PrivateKeyDer::Pkcs1(data.into())),
            SectionKind::EcPrivateKey => keys.push(PrivateKeyDer::Sec1(data.into())),
            _ => return Err(TransportError::InvalidCertificate),
        }
    }
    if keys.len() != 1 {
        return Err(TransportError::InvalidCertificate);
    }
    let key = keys.pop().expect("exactly one identity key was validated");
    if certificates.is_empty() {
        return Err(TransportError::InvalidCertificate);
    }
    Ok((certificates, key))
}

fn has_valid_percent_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            let decoded = hex_value(bytes[index + 1]) << 4 | hex_value(bytes[index + 2]);
            if decoded < 0x20 || decoded == 0x7f || decoded == b'\\' {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

fn validate_socks_credentials(url: &Url) -> Result<(), TransportError> {
    validate_socks_credential_component(url.username())?;
    if let Some(password) = url.password() {
        validate_socks_credential_component(password)?;
    }
    Ok(())
}

fn validate_socks_credential_component(value: &str) -> Result<(), TransportError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len().min(255));
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                decoded.fill(0);
                return Err(TransportError::InvalidProxy);
            }
            decoded.push(hex_value(bytes[index + 1]) << 4 | hex_value(bytes[index + 2]));
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
        if decoded.len() > u8::MAX as usize {
            decoded.fill(0);
            return Err(TransportError::InvalidProxy);
        }
    }
    let valid = std::str::from_utf8(&decoded).is_ok();
    decoded.fill(0);
    if valid {
        Ok(())
    } else {
        Err(TransportError::InvalidProxy)
    }
}

fn valid_der_root(bytes: &[u8]) -> bool {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(bytes.to_vec())).is_ok()
}

fn valid_pem_roots(bytes: &[u8]) -> bool {
    let mut roots = rustls::RootCertStore::empty();
    let mut found = false;
    for certificate in CertificateDer::pem_slice_iter(bytes) {
        let Ok(certificate) = certificate else {
            return false;
        };
        if roots.add(certificate.into_owned()).is_err() {
            return false;
        }
        found = true;
    }
    found
}

fn hex_value(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        b'A'..=b'F' => value - b'A' + 10,
        _ => 0,
    }
}

fn decode_userinfo(value: &str) -> Box<[u8]> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            decoded.push(hex_value(bytes[index + 1]) << 4 | hex_value(bytes[index + 2]));
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    decoded.into_boxed_slice()
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read, Write};
    use std::net::{Ipv4Addr, Ipv6Addr, TcpListener, TcpStream};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject as _};

    use super::*;
    use crate::{ClientConfig, HttpClient, HttpRequest, HttpUrl};

    const FIXTURE_TIMEOUT: Duration = Duration::from_secs(5);
    const TEST_CERTIFICATE: &[u8] = br#"-----BEGIN CERTIFICATE-----
MIIBaTCCARCgAwIBAgIJAKgvpeqxDyQLMAoGCCqGSM49BAMCMCUxIzAhBgNVBAMM
GnVucmVzb2x2YWJsZS1sb2NhbC5pbnZhbGlkMB4XDTI2MDgyMzExNTE0NVoXDTM2
MDgyMDExNTE0NVowJTEjMCEGA1UEAwwadW5yZXNvbHZhYmxlLWxvY2FsLmludmFs
aWQwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAARkNfz7d3M/W9hokH+Y1c2Ym32z
iUyyBmg0Qpj6aygiGUtY7dY4hO/i799QdLy/zX4JWwDtsiSaqRntl4rADHnFoykw
JzAlBgNVHREEHjAcghp1bnJlc29sdmFibGUtbG9jYWwuaW52YWxpZDAKBggqhkjO
PQQDAgNHADBEAiA0EVy8myIO/aQqCJNRuBkRtqQjbxNL2mIQWhUhHf6ePwIgYkL/
8fuyY7YO3/iNhHdIWUgyG2qP93wWmqXRC5/XpzY=
-----END CERTIFICATE-----
"#;
    const TEST_PRIVATE_KEY: &[u8] = br#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg7Muwi8mgcCj4MnTl
9mY8JDrkNVFJAxrZwSorjQOCOeGhRANCAARkNfz7d3M/W9hokH+Y1c2Ym32ziUyy
Bmg0Qpj6aygiGUtY7dY4hO/i799QdLy/zX4JWwDtsiSaqRntl4rADHnF
-----END PRIVATE KEY-----
"#;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ExpectedSocksAddress {
        Domain(&'static str),
        Ipv4(Ipv4Addr),
        Ipv6(Ipv6Addr),
    }

    #[derive(Clone, Copy)]
    struct SocksFixtureCredentials {
        serialized_username: &'static str,
        serialized_password: &'static str,
        expected_username: &'static str,
        expected_password: &'static str,
    }

    #[derive(Clone, Copy)]
    enum SocksApplication {
        Http,
        Https,
    }

    struct SocksFixture {
        listener: TcpListener,
        expected_address: ExpectedSocksAddress,
        expected_port: u16,
        expected_auth: Option<SocksFixtureCredentials>,
        application: SocksApplication,
    }

    impl SocksFixture {
        fn bind(
            expected_address: ExpectedSocksAddress,
            expected_port: u16,
            expected_auth: Option<SocksFixtureCredentials>,
            application: SocksApplication,
        ) -> Self {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("SOCKS listener");
            listener
                .set_nonblocking(true)
                .expect("bounded SOCKS accept");
            Self {
                listener,
                expected_address,
                expected_port,
                expected_auth,
                application,
            }
        }

        fn proxy_authority(&self) -> String {
            self.listener
                .local_addr()
                .expect("SOCKS address")
                .to_string()
        }

        fn spawn(self) -> std::thread::JoinHandle<()> {
            std::thread::spawn(move || self.serve())
        }

        fn serve(self) {
            let deadline = Instant::now() + FIXTURE_TIMEOUT;
            let mut stream = loop {
                match self.listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "bounded SOCKS accept timeout");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("SOCKS accept: {error}"),
                }
            };
            stream
                .set_nonblocking(false)
                .expect("blocking SOCKS fixture stream");
            stream
                .set_read_timeout(Some(FIXTURE_TIMEOUT))
                .expect("SOCKS read timeout");
            stream
                .set_write_timeout(Some(FIXTURE_TIMEOUT))
                .expect("SOCKS write timeout");
            negotiate_socks_auth(&mut stream, self.expected_auth);
            read_socks_connect(&mut stream, self.expected_address, self.expected_port);
            stream
                .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                .expect("SOCKS success response");
            match self.application {
                SocksApplication::Http => serve_http(&mut stream),
                SocksApplication::Https => serve_https(stream),
            }
        }
    }

    fn negotiate_socks_auth(stream: &mut TcpStream, expected: Option<SocksFixtureCredentials>) {
        let mut greeting = [0_u8; 2];
        stream.read_exact(&mut greeting).expect("SOCKS greeting");
        assert_eq!(greeting[0], 5);
        let mut methods = vec![0_u8; usize::from(greeting[1])];
        stream.read_exact(&mut methods).expect("SOCKS methods");
        let selected = if expected.is_some() { 2 } else { 0 };
        assert!(methods.contains(&selected), "SOCKS authentication method");
        stream
            .write_all(&[5, selected])
            .expect("SOCKS method response");
        let Some(expected) = expected else {
            return;
        };
        let mut header = [0_u8; 2];
        stream.read_exact(&mut header).expect("SOCKS auth header");
        assert_eq!(header[0], 1);
        let mut username = vec![0_u8; usize::from(header[1])];
        stream
            .read_exact(&mut username)
            .expect("SOCKS auth username");
        let mut password_length = [0_u8; 1];
        stream
            .read_exact(&mut password_length)
            .expect("SOCKS auth password length");
        let mut password = vec![0_u8; usize::from(password_length[0])];
        stream
            .read_exact(&mut password)
            .expect("SOCKS auth password");
        assert_eq!(username, expected.expected_username.as_bytes());
        assert_eq!(password, expected.expected_password.as_bytes());
        username.fill(0);
        password.fill(0);
        stream.write_all(&[1, 0]).expect("SOCKS auth response");
    }

    fn read_socks_connect(
        stream: &mut TcpStream,
        expected_address: ExpectedSocksAddress,
        expected_port: u16,
    ) {
        let mut header = [0_u8; 4];
        stream
            .read_exact(&mut header)
            .expect("SOCKS CONNECT header");
        assert_eq!(&header[..3], &[5, 1, 0]);
        let actual = match header[3] {
            1 => {
                let mut address = [0_u8; 4];
                stream.read_exact(&mut address).expect("SOCKS IPv4 address");
                ExpectedSocksAddress::Ipv4(Ipv4Addr::from(address))
            }
            3 => {
                let mut length = [0_u8; 1];
                stream.read_exact(&mut length).expect("SOCKS domain length");
                let mut domain = vec![0_u8; usize::from(length[0])];
                stream.read_exact(&mut domain).expect("SOCKS domain name");
                let domain = std::str::from_utf8(&domain).expect("SOCKS domain UTF-8");
                let ExpectedSocksAddress::Domain(expected_domain) = expected_address else {
                    panic!("expected a non-domain SOCKS address");
                };
                assert_eq!(domain, expected_domain);
                expected_address
            }
            4 => {
                let mut address = [0_u8; 16];
                stream.read_exact(&mut address).expect("SOCKS IPv6 address");
                ExpectedSocksAddress::Ipv6(Ipv6Addr::from(address))
            }
            atyp => panic!("unexpected SOCKS address type {atyp}"),
        };
        assert_eq!(actual, expected_address);
        let mut port = [0_u8; 2];
        stream.read_exact(&mut port).expect("SOCKS port");
        assert_eq!(u16::from_be_bytes(port), expected_port);
    }

    fn read_http_request(stream: &mut impl Read) {
        let mut request = Vec::new();
        let mut byte = [0_u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).expect("HTTP request");
            request.push(byte[0]);
            assert!(request.len() <= 64 * 1024, "HTTP request header bound");
        }
        assert!(request.starts_with(b"GET /object HTTP/1.1\r\n"));
    }

    fn serve_http(stream: &mut TcpStream) {
        read_http_request(stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nproxied")
            .expect("HTTP response");
    }

    fn serve_https(stream: TcpStream) {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let certificates = CertificateDer::pem_slice_iter(TEST_CERTIFICATE)
            .collect::<Result<Vec<_>, _>>()
            .expect("test certificate");
        let private_key =
            PrivateKeyDer::from_pem_slice(TEST_PRIVATE_KEY).expect("test private key");
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certificates, private_key)
            .expect("TLS server config");
        let connection =
            rustls::ServerConnection::new(Arc::new(config)).expect("TLS server connection");
        let mut stream = rustls::StreamOwned::new(connection, stream);
        read_http_request(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nproxied")
            .expect("HTTPS response");
        stream.flush().expect("HTTPS flush");
    }

    fn execute_fixture(
        scheme: &str,
        target: &str,
        fixture: SocksFixture,
        auth: Option<SocksFixtureCredentials>,
    ) {
        let authority = fixture.proxy_authority();
        let server = fixture.spawn();
        let proxy = auth.map_or_else(
            || format!("{scheme}://{authority}"),
            |auth| {
                format!(
                    "{scheme}://{}:{}@{authority}",
                    auth.serialized_username, auth.serialized_password
                )
            },
        );
        let mut config = ClientConfig::default();
        config.timeout_policy.dial_timeout = FIXTURE_TIMEOUT;
        config.timeout_policy.tls_handshake_timeout = FIXTURE_TIMEOUT;
        config.request_timeout = Some(FIXTURE_TIMEOUT);
        config.operation_timeout = Some(FIXTURE_TIMEOUT);
        config.retry.max_attempts = 1;
        config.connection_policy.proxy =
            ProxyPolicy::Explicit(ProxyUrl::parse(&proxy).expect("validated SOCKS proxy URL"));
        if target.starts_with("https://") {
            config.connection_policy.tls = TlsVerification::Disabled;
        }
        let response = HttpClient::new(config)
            .expect("SOCKS client")
            .execute(&HttpRequest::new(
                "GET".parse().expect("HTTP method"),
                HttpUrl::parse(target).expect("target URL"),
            ));
        let mut response = match response {
            Ok(response) => response,
            Err(error) => {
                server.join().expect("SOCKS fixture after request error");
                panic!("SOCKS request: {error}");
            }
        };
        let mut body = String::new();
        response.read_to_string(&mut body).expect("response body");
        assert_eq!(body, "proxied");
        server.join().expect("SOCKS fixture");
    }

    #[test]
    fn socks5_inputs_canonicalize_to_proxy_side_dns_and_stay_redacted() {
        let socks5 = ProxyUrl::parse("socks5://user:secret@proxy.example:1080").expect("socks5");
        let socks5h = ProxyUrl::parse("socks5h://user:secret@proxy.example:1080").expect("socks5h");
        assert_eq!(socks5, socks5h);
        assert_eq!(
            ProxyUrl::parse("socks5://proxy.example").expect("default SOCKS port"),
            ProxyUrl::parse("socks5h://proxy.example:1080").expect("explicit SOCKS port")
        );
        assert!(socks5.as_str().starts_with("socks5h://"));
        let debug = format!("{socks5:?}");
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("proxy.example"));

        for invalid in [
            "socks4://proxy.example:1080",
            "socks5://proxy.example:1080/path",
            "socks5://proxy.example:1080?query",
            "socks5://proxy.example:1080#fragment",
            "socks5://proxy.example:1080\\route",
            "socks5://user:%ff@proxy.example:1080",
            "socks5://user:%00@proxy.example:1080",
            "socks5://user:%0a@proxy.example:1080",
            "socks5://user:%7f@proxy.example:1080",
        ] {
            assert_eq!(ProxyUrl::parse(invalid), Err(TransportError::InvalidProxy));
        }
        let oversized = format!(
            "socks5://{}:secret@proxy.example:1080",
            "u".repeat(usize::from(u8::MAX) + 1)
        );
        assert_eq!(
            ProxyUrl::parse(&oversized),
            Err(TransportError::InvalidProxy)
        );
    }

    #[test]
    fn proxy_https_origin_never_contains_credentials() {
        let proxy = ProxyUrl::parse("https://user:secret@[2001:db8::7]:8443").expect("HTTPS proxy");
        let origin = proxy
            .https_origin_url()
            .expect("proxy origin")
            .expect("HTTPS origin");
        assert_eq!(origin.as_str(), "https://[2001:db8::7]:8443/");
        let debug = format!("{proxy:?} {origin:?}");
        assert!(!debug.contains("user"));
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("2001:db8"));
    }

    #[test]
    fn no_proxy_wildcard_bare_port_cidr_and_ipv6_vectors() {
        let wildcard = NoProxySnapshot::parse("*.example.test").expect("wildcard");
        assert!(
            wildcard.matches(&HttpUrl::parse("https://child.example.test/").expect("subdomain"))
        );
        assert!(!wildcard.matches(&HttpUrl::parse("https://example.test/").expect("bare domain")));

        let bare = NoProxySnapshot::parse("example.test").expect("bare");
        assert!(bare.matches(&HttpUrl::parse("https://example.test/").expect("bare URL")));
        assert!(bare.matches(&HttpUrl::parse("https://child.example.test/").expect("child URL")));
        assert!(!bare.matches(&HttpUrl::parse("https://notexample.test/").expect("different URL")));

        let port = NoProxySnapshot::parse("example.test:8443").expect("port");
        assert!(
            port.matches(&HttpUrl::parse("https://example.test:8443/").expect("matching port"))
        );
        assert!(!port.matches(&HttpUrl::parse("https://example.test/").expect("default port")));

        let addresses =
            NoProxySnapshot::parse("192.0.2.0/24,[2001:db8::8]:9443").expect("addresses");
        assert!(addresses.matches(&HttpUrl::parse("http://192.0.2.42/").expect("CIDR address")));
        assert!(
            addresses
                .matches(&HttpUrl::parse("https://[2001:db8::8]:9443/").expect("IPv6 address"))
        );
        assert!(
            !addresses.matches(&HttpUrl::parse("https://[2001:db8::8]/").expect("IPv6 wrong port"))
        );
    }

    #[test]
    fn client_identity_is_validated_then_fails_closed() {
        let mut pem = TEST_CERTIFICATE.to_vec();
        pem.extend_from_slice(TEST_PRIVATE_KEY);
        let identity = TlsClientIdentity::from_pem(pem).expect("valid identity");
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.connection_policy.client_identity = Some(identity);
        assert_eq!(
            HttpClient::new(config).expect_err("unsupported identity"),
            TransportError::UnsupportedClientIdentity
        );
    }

    #[test]
    fn socks5_and_socks5h_send_unresolvable_hostname_to_proxy() {
        for scheme in ["socks5", "socks5h"] {
            execute_fixture(
                scheme,
                "http://unresolvable-local.invalid:8080/object",
                SocksFixture::bind(
                    ExpectedSocksAddress::Domain("unresolvable-local.invalid"),
                    8080,
                    None,
                    SocksApplication::Http,
                ),
                None,
            );
        }
    }

    #[test]
    fn socks5_proxy_auth_is_preserved_without_debug_disclosure() {
        let auth = SocksFixtureCredentials {
            serialized_username: "proxy%2Duser",
            serialized_password: "proxy%2Dsecret",
            expected_username: "proxy-user",
            expected_password: "proxy-secret",
        };
        execute_fixture(
            "socks5",
            "http://unresolvable-local.invalid:8081/object",
            SocksFixture::bind(
                ExpectedSocksAddress::Domain("unresolvable-local.invalid"),
                8081,
                Some(auth),
                SocksApplication::Http,
            ),
            Some(auth),
        );
    }

    #[test]
    fn socks5_inputs_tunnel_literal_ipv4_as_an_ip_address() {
        for scheme in ["socks5", "socks5h"] {
            execute_fixture(
                scheme,
                "http://192.0.2.9:8082/object",
                SocksFixture::bind(
                    ExpectedSocksAddress::Ipv4(Ipv4Addr::new(192, 0, 2, 9)),
                    8082,
                    None,
                    SocksApplication::Http,
                ),
                None,
            );
        }
    }

    #[test]
    fn socks5_ipv6_literal_uses_atyp_04_without_brackets() {
        execute_fixture(
            "socks5h",
            "http://[2001:db8::9]:8083/object",
            SocksFixture::bind(
                ExpectedSocksAddress::Ipv6("2001:db8::9".parse().expect("IPv6 address")),
                8083,
                None,
                SocksApplication::Http,
            ),
            None,
        );
    }

    #[test]
    fn socks5h_tunnels_https_with_proxy_side_dns() {
        execute_fixture(
            "socks5h",
            "https://unresolvable-local.invalid:8443/object",
            SocksFixture::bind(
                ExpectedSocksAddress::Domain("unresolvable-local.invalid"),
                8443,
                None,
                SocksApplication::Https,
            ),
            None,
        );
    }
}
