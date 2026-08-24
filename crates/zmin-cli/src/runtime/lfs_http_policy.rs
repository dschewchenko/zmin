//! Immutable Git LFS HTTP transport policy assembled from ordinary Git config.

use std::collections::{HashSet, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::Read;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::io::{AsRawFd, FromRawFd};
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_TYPE_DISK, GetFileInformationByHandle, GetFileType,
};

use zmin_http_transport::{
    ConfiguredRequestHeaderProvenance, ConfiguredRequestHeaders, HttpConnectionPolicy,
    HttpTimeoutPolicy, HttpUrl, ProxyPolicy, ProxyTlsPolicy, ProxyUrl, RequestPolicyResolver,
    ResolvedRequestPolicy, TcpKeepalivePolicy, TlsRootCertificate, TlsRootCertificates,
    TlsVerification, TransportError,
};

use super::{ConfigEntry, LfsUrlScope};

const DEFAULT_LFS_TIMEOUT_SECONDS: u64 = 30;
const DEFAULT_KEEPALIVE_SECONDS: u64 = 1_800;
const MAX_CA_INPUT_BYTES: u64 = 1024 * 1024;
const MAX_CA_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const MAX_CA_ENTRIES: usize = 256;
const MAX_CA_PATH_BYTES: usize = 8 * 1024;
const MAX_CERTIFICATE_CACHE_ENTRIES: usize = 8;
const MAX_HTTP_POLICY_RULES: usize = 256;
const MAX_TIMEOUT_POLICY_RULES: usize = 256;
const MAX_NO_PROXY_BYTES: usize = 64 * 1024;
const MAX_NO_PROXY_RULES: usize = 256;

/// The only process-environment values consulted by the LFS HTTP adapter.
/// Capture this once at the CLI boundary and pass the snapshot down by value.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct LfsHttpEnvironmentSnapshot {
    https_proxy_upper: Option<OsString>,
    https_proxy_lower: Option<OsString>,
    http_proxy_upper: Option<OsString>,
    http_proxy_lower: Option<OsString>,
    no_proxy_upper: Option<OsString>,
    no_proxy_lower: Option<OsString>,
    ssl_ca_info: Option<OsString>,
    ssl_ca_path: Option<OsString>,
    ssl_no_verify: Option<OsString>,
}

impl fmt::Debug for LfsHttpEnvironmentSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsHttpEnvironmentSnapshot")
            .field("https_proxy_upper", &self.https_proxy_upper.is_some())
            .field("https_proxy_lower", &self.https_proxy_lower.is_some())
            .field("http_proxy_upper", &self.http_proxy_upper.is_some())
            .field("http_proxy_lower", &self.http_proxy_lower.is_some())
            .field("no_proxy_upper", &self.no_proxy_upper.is_some())
            .field("no_proxy_lower", &self.no_proxy_lower.is_some())
            .field("ssl_ca_info", &self.ssl_ca_info.is_some())
            .field("ssl_ca_path", &self.ssl_ca_path.is_some())
            .field("ssl_no_verify", &self.ssl_no_verify.is_some())
            .finish()
    }
}

impl LfsHttpEnvironmentSnapshot {
    pub(crate) fn capture() -> Self {
        Self {
            https_proxy_upper: std::env::var_os("HTTPS_PROXY"),
            https_proxy_lower: std::env::var_os("https_proxy"),
            http_proxy_upper: std::env::var_os("HTTP_PROXY"),
            http_proxy_lower: std::env::var_os("http_proxy"),
            no_proxy_upper: std::env::var_os("NO_PROXY"),
            no_proxy_lower: std::env::var_os("no_proxy"),
            ssl_ca_info: std::env::var_os("GIT_SSL_CAINFO"),
            ssl_ca_path: std::env::var_os("GIT_SSL_CAPATH"),
            ssl_no_verify: std::env::var_os("GIT_SSL_NO_VERIFY"),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_proxy(
        mut self,
        https_upper: Option<&str>,
        https_lower: Option<&str>,
        http_upper: Option<&str>,
        http_lower: Option<&str>,
        no_proxy: Option<&str>,
    ) -> Self {
        self.https_proxy_upper = https_upper.map(OsString::from);
        self.https_proxy_lower = https_lower.map(OsString::from);
        self.http_proxy_upper = http_upper.map(OsString::from);
        self.http_proxy_lower = http_lower.map(OsString::from);
        self.no_proxy_upper = no_proxy.map(OsString::from);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_tls(
        mut self,
        ca_info: Option<PathBuf>,
        ca_path: Option<PathBuf>,
        no_verify: Option<&str>,
    ) -> Self {
        self.ssl_ca_info = ca_info.map(PathBuf::into_os_string);
        self.ssl_ca_path = ca_path.map(PathBuf::into_os_string);
        self.ssl_no_verify = no_verify.map(OsString::from);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsHttpPolicyError {
    InvalidTimeout,
    InvalidUrlScope,
    InvalidBoolean,
    InvalidProxy,
    UnsupportedProxyScheme,
    UnsupportedProxyPath,
    InvalidNoProxy,
    InvalidHeader,
    InvalidCertificateSource,
    CertificateSourceTooLarge,
    CertificatePolicyTooLarge,
    PolicyTooLarge,
    UnsupportedTlsConfiguration,
}

impl fmt::Display for LfsHttpPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidTimeout => "invalid or unrepresentable LFS HTTP timeout",
            Self::InvalidUrlScope => "invalid URL-scoped LFS HTTP configuration",
            Self::InvalidBoolean => "invalid LFS HTTP boolean configuration",
            Self::InvalidProxy => "invalid LFS HTTP proxy configuration",
            Self::UnsupportedProxyScheme => "unsupported LFS HTTP proxy scheme",
            Self::UnsupportedProxyPath => "LFS HTTP proxy URLs cannot contain a path",
            Self::InvalidNoProxy => "invalid LFS HTTP no-proxy configuration",
            Self::InvalidHeader => "invalid configured LFS HTTP header",
            Self::InvalidCertificateSource => "invalid LFS TLS certificate source",
            Self::CertificateSourceTooLarge => "LFS TLS certificate source is too large",
            Self::CertificatePolicyTooLarge => "LFS TLS certificate policy exceeds safe bounds",
            Self::PolicyTooLarge => "LFS HTTP policy exceeds safe bounds",
            Self::UnsupportedTlsConfiguration => {
                "configured LFS TLS client identity is not supported yet"
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for LfsHttpPolicyError {}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LfsHttpPolicy {
    proxy: ScalarRules<ProxySetting>,
    https_proxy: Option<ProxyUrl>,
    http_proxy: Option<ProxyUrl>,
    no_proxy: NoProxyRules,
    ssl_verify_global: bool,
    ssl_verify_scoped: Vec<ScopedRule<bool>>,
    ssl_no_verify: bool,
    ssl_ca_info: ScalarRules<CertificateSetting>,
    ssl_ca_path: Option<CertificateSetting>,
    env_ca_info: Option<CertificateSetting>,
    env_ca_path: Option<CertificateSetting>,
    ssl_backend: ScalarRules<TlsBackendCompatibility>,
    schannel_use_ssl_ca_info: ScalarRules<bool>,
    certificate_cache: CertificateCache,
    dial_timeout: Duration,
    tls_handshake_timeout: Duration,
    activity_timeout: ScalarRules<LfsActivityTimeout>,
    keepalive: Duration,
    headers: HeaderRules,
}

impl fmt::Debug for LfsHttpPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsHttpPolicy")
            .field("proxy_rules", &self.proxy.rule_count())
            .field("https_proxy", &self.https_proxy.is_some())
            .field("http_proxy", &self.http_proxy.is_some())
            .field("no_proxy_rules", &self.no_proxy.rules.len())
            .field("ssl_verify_scoped", &self.ssl_verify_scoped.len())
            .field("ssl_no_verify", &self.ssl_no_verify)
            .field("ssl_ca_info_rules", &self.ssl_ca_info.rule_count())
            .field("ssl_ca_path", &self.ssl_ca_path.is_some())
            .field("env_ca_info", &self.env_ca_info.is_some())
            .field("env_ca_path", &self.env_ca_path.is_some())
            .field("ssl_backend_rules", &self.ssl_backend.rule_count())
            .field(
                "schannel_use_ssl_ca_info_rules",
                &self.schannel_use_ssl_ca_info.rule_count(),
            )
            .field(
                "cached_certificate_sources",
                &self.certificate_cache.entry_count(),
            )
            .field("dial_timeout", &self.dial_timeout)
            .field("tls_handshake_timeout", &self.tls_handshake_timeout)
            .field(
                "activity_timeout_rules",
                &self.activity_timeout.rule_count(),
            )
            .field("keepalive", &self.keepalive)
            .field("header_sources", &self.headers.source_count())
            .finish()
    }
}

impl Default for LfsHttpPolicy {
    fn default() -> Self {
        Self {
            proxy: ScalarRules::default(),
            https_proxy: None,
            http_proxy: None,
            no_proxy: NoProxyRules::default(),
            ssl_verify_global: true,
            ssl_verify_scoped: Vec::new(),
            ssl_no_verify: false,
            ssl_ca_info: ScalarRules::default(),
            ssl_ca_path: None,
            env_ca_info: None,
            env_ca_path: None,
            ssl_backend: ScalarRules::default(),
            schannel_use_ssl_ca_info: ScalarRules::default(),
            certificate_cache: CertificateCache::default(),
            dial_timeout: Duration::from_secs(DEFAULT_LFS_TIMEOUT_SECONDS),
            tls_handshake_timeout: Duration::from_secs(DEFAULT_LFS_TIMEOUT_SECONDS),
            activity_timeout: ScalarRules {
                generic: Some(LfsActivityTimeout::Enabled(Duration::from_secs(
                    DEFAULT_LFS_TIMEOUT_SECONDS,
                ))),
                scoped: Vec::new(),
            },
            keepalive: Duration::from_secs(DEFAULT_KEEPALIVE_SECONDS),
            headers: HeaderRules::default(),
        }
    }
}

impl LfsHttpPolicy {
    pub(crate) fn from_entries(
        entries: &[ConfigEntry],
        environment: LfsHttpEnvironmentSnapshot,
    ) -> Result<Self, LfsHttpPolicyError> {
        enforce_policy_rule_bound(entries)?;
        enforce_timeout_rule_bound(entries)?;
        validate_relevant_url_scopes(entries)?;
        reject_unsupported_tls_settings(entries)?;
        let mut policy = Self::default();

        let https_proxy = selected_environment_text(
            environment.https_proxy_upper,
            environment.https_proxy_lower,
            LfsHttpPolicyError::InvalidProxy,
        )?;
        let http_proxy = selected_environment_text(
            environment.http_proxy_upper,
            environment.http_proxy_lower,
            LfsHttpPolicyError::InvalidProxy,
        )?;
        let no_proxy = selected_environment_text(
            environment.no_proxy_upper,
            environment.no_proxy_lower,
            LfsHttpPolicyError::InvalidNoProxy,
        )?;
        policy.https_proxy = parse_optional_proxy(https_proxy)?;
        policy.http_proxy = parse_optional_proxy(http_proxy)?;
        policy.no_proxy = NoProxyRules::parse(no_proxy)?;
        policy.ssl_no_verify = parse_optional_environment_bool(environment.ssl_no_verify)?;
        policy.env_ca_info =
            optional_certificate_source(environment.ssl_ca_info, CertificateSourceKind::File)?;
        policy.env_ca_path =
            optional_certificate_source(environment.ssl_ca_path, CertificateSourceKind::Directory)?;

        if let Some(value) = entries.iter().rev().find(|entry| {
            entry.section == "lfs" && entry.subsection.is_empty() && entry.key == "keepalive"
        }) {
            policy.keepalive = parse_keepalive(&value.value);
        }
        policy.dial_timeout = selected_global_timeout(entries, "dialtimeout")?;
        policy.tls_handshake_timeout = selected_global_timeout(entries, "tlstimeout")?;

        let mut configured_ca_path = None;
        for (order, entry) in entries.iter().enumerate() {
            if entry.section == "lfs" {
                match entry.key.as_str() {
                    "activitytimeout" => {
                        let timeout = parse_activity_timeout(&entry.value);
                        policy.activity_timeout.push(entry, order, timeout)?;
                    }
                    _ => {}
                }
            }
            if entry.section != "http" {
                continue;
            }
            match entry.key.as_str() {
                "proxy" => {
                    let setting = if entry.value.is_empty() {
                        ProxySetting::Environment
                    } else {
                        ProxySetting::Explicit(parse_proxy(&entry.value)?)
                    };
                    policy.proxy.push(entry, order, setting)?;
                }
                "sslverify" => {
                    if entry.subsection.is_empty() {
                        policy.ssl_verify_global = parse_config_bool(entry)?;
                    } else {
                        // Git LFS v3.7.1 deliberately uses a raw lexical check
                        // for URL-scoped verification, unlike the global Git
                        // boolean. Only the exact value `false` disables it.
                        let value = entry.value != "false";
                        policy
                            .ssl_verify_scoped
                            .push(ScopedRule::new(entry, order, value)?);
                    }
                }
                "sslcainfo" => {
                    let setting =
                        config_certificate_source(&entry.value, CertificateSourceKind::File)?;
                    policy.ssl_ca_info.push(entry, order, setting)?;
                }
                "sslcapath" => {
                    if !entry.subsection.is_empty() {
                        continue;
                    }
                    configured_ca_path = Some(entry.value.as_str());
                }
                "extraheader" => policy.headers.push(entry, order)?,
                "sslbackend" => policy.ssl_backend.push(
                    entry,
                    order,
                    TlsBackendCompatibility::from_git_value(&entry.value),
                )?,
                "schannelusesslcainfo" => policy.schannel_use_ssl_ca_info.push(
                    entry,
                    order,
                    parse_schannel_use_ssl_ca_info(entry),
                )?,
                _ => {}
            }
        }
        if let Some(raw) = configured_ca_path {
            policy.ssl_ca_path = Some(config_certificate_source(
                raw,
                CertificateSourceKind::Directory,
            )?);
        }
        Ok(policy)
    }

    pub(crate) fn has_configured_authorization(&self, url: &HttpUrl) -> bool {
        self.configured_authorization_provenance(url).is_some()
    }

    pub(crate) fn configured_authorization_provenance(
        &self,
        url: &HttpUrl,
    ) -> Option<ConfiguredRequestHeaderProvenance> {
        let Some(target) = LfsUrlScope::from_transport(url) else {
            return None;
        };
        self.headers
            .effective(&target)
            .filter(|source| source.has_authorization())
            .map(|source| {
                if source.scope.is_some() {
                    ConfiguredRequestHeaderProvenance::UrlScoped
                } else {
                    ConfiguredRequestHeaderProvenance::Generic
                }
            })
    }

    fn proxy_for(
        &self,
        url: &HttpUrl,
        target: &LfsUrlScope,
    ) -> Result<ProxyPolicy, TransportError> {
        let configured = self.proxy.effective(target);
        let selected = match configured {
            Some(ProxySetting::Explicit(proxy)) => Some(proxy),
            Some(ProxySetting::Environment) | None => {
                if url.origin().scheme() == "https" {
                    self.https_proxy.as_ref().or(self.http_proxy.as_ref())
                } else {
                    self.http_proxy.as_ref()
                }
            }
        };
        let Some(proxy) = selected else {
            return Ok(ProxyPolicy::Disabled);
        };
        if self
            .no_proxy
            .bypasses(url.origin().host(), url.origin().port())
        {
            Ok(ProxyPolicy::Disabled)
        } else {
            proxy.for_request_url(url).map(ProxyPolicy::Explicit)
        }
    }

    fn tls_for(&self, url: &HttpUrl) -> Result<TlsVerification, LfsHttpPolicyError> {
        if url.origin().scheme() != "https" {
            return Ok(TlsVerification::Platform);
        }
        // Git LFS v3.7.1 constructs `https://<request host[:port]>/`
        // before URLConfig lookup for sslVerify and all custom-root selectors.
        let compatibility_target = tls_host_root_scope(url)?;
        let scoped_verify = self
            .ssl_verify_scoped
            .iter()
            .filter_map(|rule| {
                rule.score(&compatibility_target)
                    .map(|score| (score, rule.order, rule.value))
            })
            .max_by_key(|(score, order, _)| (*score, *order))
            .map(|(_, _, value)| value);
        if self.ssl_no_verify || !self.ssl_verify_global || scoped_verify == Some(false) {
            return Ok(TlsVerification::Disabled);
        }
        if self.schannel_ignores_ca_sources(&compatibility_target) {
            return Ok(TlsVerification::Platform);
        }
        if let Some(setting) = &self.env_ca_info {
            return self.certificate_verification(setting);
        }
        if let Some(setting) = self.ssl_ca_info.effective(&compatibility_target) {
            return self.certificate_verification(setting);
        }
        if let Some(setting) = &self.env_ca_path {
            return self.certificate_verification(setting);
        }
        self.ssl_ca_path.as_ref().map_or_else(
            || Ok(TlsVerification::Platform),
            |setting| self.certificate_verification(setting),
        )
    }

    fn schannel_ignores_ca_sources(&self, target: &LfsUrlScope) -> bool {
        self.ssl_backend.effective(target) == Some(&TlsBackendCompatibility::Schannel)
            && !self
                .schannel_use_ssl_ca_info
                .effective(target)
                .copied()
                .unwrap_or(false)
    }

    fn timeouts_for(&self, target: &LfsUrlScope) -> Result<HttpTimeoutPolicy, TransportError> {
        Ok(HttpTimeoutPolicy {
            dial_timeout: self.dial_timeout,
            tls_handshake_timeout: self.tls_handshake_timeout,
            activity_timeout: self
                .activity_timeout
                .effective(target)
                .copied()
                .unwrap_or_default()
                .for_transport()?,
        })
    }

    fn proxy_tls_for(&self, proxy: &ProxyPolicy) -> Result<ProxyTlsPolicy, TransportError> {
        let ProxyPolicy::Explicit(proxy) = proxy else {
            return Ok(ProxyTlsPolicy::default());
        };
        let Some(proxy_origin) = proxy.https_origin_url()? else {
            return Ok(ProxyTlsPolicy::default());
        };
        Ok(ProxyTlsPolicy {
            tls: self
                .tls_for(&proxy_origin)
                .map_err(|_| TransportError::InvalidCertificate)?,
            // Client identities remain unsupported and fail closed while the
            // LFS policy is assembled. A target identity is never reused for
            // the independently authenticated outer HTTPS proxy connection.
            client_identity: None,
        })
    }

    fn certificate_verification(
        &self,
        setting: &CertificateSetting,
    ) -> Result<TlsVerification, LfsHttpPolicyError> {
        match setting {
            CertificateSetting::Platform => Ok(TlsVerification::Platform),
            CertificateSetting::Source(source) => self
                .certificate_cache
                .load(source)
                .map(TlsVerification::CustomRoots),
        }
    }

    fn headers_for(
        &self,
        target: &LfsUrlScope,
    ) -> Result<ConfiguredRequestHeaders, TransportError> {
        let Some(source) = self.headers.effective(target) else {
            return Ok(ConfiguredRequestHeaders::none());
        };
        let pairs = source
            .instructions
            .iter()
            .filter_map(HeaderInstruction::pair)
            .map(|(name, value)| (name.to_owned(), value.to_vec()))
            .collect::<Vec<_>>();
        if source.scope.is_some() {
            ConfiguredRequestHeaders::url_scoped(pairs)
        } else {
            ConfiguredRequestHeaders::generic(pairs)
        }
    }
}

impl RequestPolicyResolver for LfsHttpPolicy {
    fn resolve(&self, url: &HttpUrl) -> Result<ResolvedRequestPolicy, TransportError> {
        let target = LfsUrlScope::from_transport(url).ok_or(TransportError::InvalidUrl)?;
        let proxy = self.proxy_for(url, &target)?;
        let proxy_tls_policy = self.proxy_tls_for(&proxy)?;
        Ok(ResolvedRequestPolicy {
            connection: HttpConnectionPolicy {
                proxy,
                tls: self
                    .tls_for(url)
                    .map_err(|_| TransportError::InvalidCertificate)?,
                tcp_keepalive: TcpKeepalivePolicy::Enabled(self.keepalive),
                client_identity: None,
            },
            timeouts: self.timeouts_for(&target)?,
            proxy_tls_policy,
            configured_headers: self.headers_for(&target)?,
        })
    }
}

fn tls_host_root_scope(url: &HttpUrl) -> Result<LfsUrlScope, LfsHttpPolicyError> {
    let origin = url.origin();
    let host = origin.host().trim_matches(['[', ']']);
    let authority = if host.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let port = if origin.port() == 443 {
        String::new()
    } else {
        format!(":{}", origin.port())
    };
    LfsUrlScope::parse(&format!("https://{authority}{port}/"))
        .ok_or(LfsHttpPolicyError::InvalidUrlScope)
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum LfsActivityTimeout {
    #[default]
    Disabled,
    Enabled(Duration),
    Unrepresentable,
}

impl LfsActivityTimeout {
    fn for_transport(self) -> Result<Option<Duration>, TransportError> {
        match self {
            Self::Disabled => Ok(None),
            Self::Enabled(timeout) => Ok(Some(timeout)),
            // Git LFS disables activity deadlines when strconv.Atoi cannot
            // represent a selected value. We fail before network instead of
            // allowing a repository-controlled numeric overflow to disable a
            // configured safety boundary.
            Self::Unrepresentable => Err(TransportError::InvalidTimeout),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum ProxySetting {
    Environment,
    Explicit(ProxyUrl),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TlsBackendCompatibility {
    Schannel,
    Other,
}

impl TlsBackendCompatibility {
    fn from_git_value(value: &str) -> Self {
        if value == "schannel" {
            Self::Schannel
        } else {
            Self::Other
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum CertificateSetting {
    Platform,
    Source(CertificateSource),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CertificateSourceKind {
    File,
    Directory,
}

#[derive(Clone, PartialEq, Eq)]
struct CertificateSource {
    kind: CertificateSourceKind,
    path: PathBuf,
}

impl fmt::Debug for CertificateSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CertificateSource")
            .field(
                "kind",
                &match self.kind {
                    CertificateSourceKind::File => "file",
                    CertificateSourceKind::Directory => "directory",
                },
            )
            .field("path", &"<redacted>")
            .finish()
    }
}

impl CertificateSource {
    fn new(path: PathBuf, kind: CertificateSourceKind) -> Result<Self, LfsHttpPolicyError> {
        if path.as_os_str().is_empty() || encoded_path_bytes(path.as_os_str()) > MAX_CA_PATH_BYTES {
            return Err(LfsHttpPolicyError::InvalidCertificateSource);
        }
        #[cfg(windows)]
        validate_windows_certificate_namespace(&path)?;
        Ok(Self { kind, path })
    }

    fn load(&self) -> Result<TlsRootCertificates, LfsHttpPolicyError> {
        match self.kind {
            CertificateSourceKind::File => load_certificate_file(&self.path),
            CertificateSourceKind::Directory => load_certificate_directory(&self.path),
        }
    }
}

#[derive(Clone)]
struct CachedCertificateSource {
    source: CertificateSource,
    result: Result<TlsRootCertificates, LfsHttpPolicyError>,
}

#[derive(Clone, Default)]
struct CertificateCache {
    entries: Arc<Mutex<VecDeque<CachedCertificateSource>>>,
}

impl PartialEq for CertificateCache {
    fn eq(&self, _other: &Self) -> bool {
        // Cache population is an execution detail, not policy identity.
        true
    }
}

impl Eq for CertificateCache {}

impl fmt::Debug for CertificateCache {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CertificateCache")
            .field("entries", &self.entry_count())
            .finish()
    }
}

impl CertificateCache {
    fn entry_count(&self) -> usize {
        self.entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or_default()
    }

    fn load(&self, source: &CertificateSource) -> Result<TlsRootCertificates, LfsHttpPolicyError> {
        // Serialize first population so concurrent object workers never open
        // the same certificate source more than once.
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)?;
        if let Some(index) = entries.iter().position(|entry| &entry.source == source) {
            let entry = entries
                .remove(index)
                .expect("certificate cache index exists");
            let result = entry.result.clone();
            entries.push_back(entry);
            return result;
        }
        let result = source.load();
        if entries.len() == MAX_CERTIFICATE_CACHE_ENTRIES {
            entries.pop_front();
        }
        entries.push_back(CachedCertificateSource {
            source: source.clone(),
            result: result.clone(),
        });
        result
    }
}

#[derive(Clone, PartialEq, Eq)]
struct ScopedRule<T> {
    scope: LfsUrlScope,
    value: T,
    order: usize,
}

impl<T> ScopedRule<T> {
    fn new(entry: &ConfigEntry, order: usize, value: T) -> Result<Self, LfsHttpPolicyError> {
        let scope =
            LfsUrlScope::parse(&entry.subsection).ok_or(LfsHttpPolicyError::InvalidUrlScope)?;
        Ok(Self {
            scope,
            value,
            order,
        })
    }

    fn score(&self, target: &LfsUrlScope) -> Option<(usize, usize, usize)> {
        self.scope.match_score(target)
    }
}

#[derive(Clone, PartialEq, Eq)]
struct ScalarRules<T> {
    generic: Option<T>,
    scoped: Vec<ScopedRule<T>>,
}

impl<T> Default for ScalarRules<T> {
    fn default() -> Self {
        Self {
            generic: None,
            scoped: Vec::new(),
        }
    }
}

impl<T> ScalarRules<T> {
    fn push(
        &mut self,
        entry: &ConfigEntry,
        order: usize,
        value: T,
    ) -> Result<(), LfsHttpPolicyError> {
        if entry.subsection.is_empty() {
            self.generic = Some(value);
        } else {
            self.scoped.push(ScopedRule::new(entry, order, value)?);
        }
        Ok(())
    }

    fn effective(&self, target: &LfsUrlScope) -> Option<&T> {
        self.scoped
            .iter()
            .filter_map(|rule| {
                rule.score(target)
                    .map(|score| (score, rule.order, &rule.value))
            })
            .max_by_key(|(score, order, _)| (*score, *order))
            .map(|(_, _, value)| value)
            .or(self.generic.as_ref())
    }

    fn rule_count(&self) -> usize {
        self.scoped.len() + usize::from(self.generic.is_some())
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
struct HeaderRules {
    generic: Option<HeaderSource>,
    scoped: Vec<HeaderSource>,
}

impl HeaderRules {
    fn push(&mut self, entry: &ConfigEntry, order: usize) -> Result<(), LfsHttpPolicyError> {
        let instruction = HeaderInstruction::parse(&entry.value)?;
        let source = if entry.subsection.is_empty() {
            self.generic.get_or_insert_with(HeaderSource::generic)
        } else if let Some(index) = self
            .scoped
            .iter()
            .position(|source| source.key == entry.subsection)
        {
            &mut self.scoped[index]
        } else {
            self.scoped.push(HeaderSource::scoped(entry)?);
            self.scoped.last_mut().expect("just pushed")
        };
        source.last_order = order;
        match instruction {
            HeaderInstruction::Reset => source.instructions.clear(),
            instruction => source.instructions.push(instruction),
        }
        Ok(())
    }

    fn effective(&self, target: &LfsUrlScope) -> Option<&HeaderSource> {
        self.scoped
            .iter()
            .filter_map(|source| {
                source
                    .scope
                    .as_ref()?
                    .match_score(target)
                    .map(|score| (score, source.last_order, source))
            })
            .max_by_key(|(score, order, _)| (*score, *order))
            .map(|(_, _, source)| source)
            .or(self.generic.as_ref())
    }

    fn source_count(&self) -> usize {
        self.scoped.len() + usize::from(self.generic.is_some())
    }
}

#[derive(Clone, PartialEq, Eq)]
struct HeaderSource {
    key: String,
    scope: Option<LfsUrlScope>,
    instructions: Vec<HeaderInstruction>,
    last_order: usize,
}

impl HeaderSource {
    fn generic() -> Self {
        Self {
            key: String::new(),
            scope: None,
            instructions: Vec::new(),
            last_order: 0,
        }
    }

    fn scoped(entry: &ConfigEntry) -> Result<Self, LfsHttpPolicyError> {
        Ok(Self {
            key: entry.subsection.clone(),
            scope: Some(
                LfsUrlScope::parse(&entry.subsection).ok_or(LfsHttpPolicyError::InvalidUrlScope)?,
            ),
            instructions: Vec::new(),
            last_order: 0,
        })
    }

    fn has_authorization(&self) -> bool {
        self.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                HeaderInstruction::Pair { name, value }
                    if name.eq_ignore_ascii_case("authorization") && !value.0.is_empty()
            )
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
enum HeaderInstruction {
    Reset,
    Pair {
        name: String,
        value: SecretConfigValue,
    },
}

impl HeaderInstruction {
    fn parse(raw: &str) -> Result<Self, LfsHttpPolicyError> {
        if raw.is_empty() {
            return Ok(Self::Reset);
        }
        let (name, value) = raw
            .split_once(':')
            .ok_or(LfsHttpPolicyError::InvalidHeader)?;
        let name = name.trim();
        let value = value.trim().as_bytes().to_vec();
        // Let the transport enforce the complete reserved-name, value and size
        // contract while the temporary secret bytes are still owned here.
        ConfiguredRequestHeaders::generic([(name, value.clone())])
            .map_err(|_| LfsHttpPolicyError::InvalidHeader)?;
        Ok(Self::Pair {
            name: name.to_owned(),
            value: SecretConfigValue(value),
        })
    }

    fn pair(&self) -> Option<(&str, &[u8])> {
        match self {
            Self::Reset => None,
            Self::Pair { name, value } => Some((name, &value.0)),
        }
    }
}

#[derive(PartialEq, Eq)]
struct SecretConfigValue(Vec<u8>);

impl Clone for SecretConfigValue {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Drop for SecretConfigValue {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
struct NoProxyRules {
    rules: Vec<NoProxyRule>,
}

impl NoProxyRules {
    fn parse(value: Option<String>) -> Result<Self, LfsHttpPolicyError> {
        let Some(value) = value else {
            return Ok(Self::default());
        };
        if value.len() > MAX_NO_PROXY_BYTES || !value.is_ascii() {
            return Err(LfsHttpPolicyError::InvalidNoProxy);
        }
        let mut rules = Vec::new();
        for raw in value.split(',') {
            let raw = raw.trim().to_ascii_lowercase();
            if raw.is_empty() {
                continue;
            }
            if rules.len() >= MAX_NO_PROXY_RULES {
                return Err(LfsHttpPolicyError::InvalidNoProxy);
            }
            if raw == "*" {
                return Ok(Self {
                    rules: vec![NoProxyRule::All],
                });
            }
            rules.push(NoProxyRule::parse(&raw)?);
        }
        Ok(Self { rules })
    }

    fn bypasses(&self, host: &str, port: u16) -> bool {
        let host = host.trim_matches(['[', ']']);
        if host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
        {
            return true;
        }
        self.rules.iter().any(|rule| rule.matches(host, port))
    }
}

#[derive(Clone, PartialEq, Eq)]
enum NoProxyRule {
    All,
    Ip {
        address: IpAddr,
        port: Option<u16>,
    },
    Cidr {
        address: IpAddr,
        prefix: u8,
    },
    Domain {
        suffix: String,
        port: Option<u16>,
        match_host: bool,
    },
}

impl NoProxyRule {
    fn parse(raw: &str) -> Result<Self, LfsHttpPolicyError> {
        if let Some((address, prefix)) = raw.split_once('/') {
            let address = address
                .parse::<IpAddr>()
                .map_err(|_| LfsHttpPolicyError::InvalidNoProxy)?;
            let prefix = prefix
                .parse::<u8>()
                .map_err(|_| LfsHttpPolicyError::InvalidNoProxy)?;
            let maximum = if address.is_ipv4() { 32 } else { 128 };
            if prefix > maximum {
                return Err(LfsHttpPolicyError::InvalidNoProxy);
            }
            return Ok(Self::Cidr { address, prefix });
        }
        let (host, port) = split_optional_host_port(raw)?;
        let host = host.trim_matches(['[', ']']);
        if let Ok(address) = host.parse::<IpAddr>() {
            return Ok(Self::Ip { address, port });
        }
        if host.is_empty()
            || host
                .bytes()
                .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'*')))
        {
            return Err(LfsHttpPolicyError::InvalidNoProxy);
        }
        // Match golang.org/x/net/http/httpproxy, which Git LFS v3.7.1 uses:
        // remove only the `*`, retaining the leading dot so `*.example.com`
        // matches subdomains but not the bare `example.com` host.
        let host = host.strip_prefix('*').unwrap_or(host);
        let match_host = !host.starts_with('.');
        let suffix = if host.starts_with('.') {
            host.to_owned()
        } else {
            format!(".{host}")
        };
        Ok(Self::Domain {
            suffix,
            port,
            match_host,
        })
    }

    fn matches(&self, host: &str, port: u16) -> bool {
        match self {
            Self::All => true,
            Self::Ip {
                address,
                port: rule_port,
            } => {
                host.parse::<IpAddr>().ok().as_ref() == Some(address)
                    && rule_port.is_none_or(|rule_port| rule_port == port)
            }
            Self::Cidr { address, prefix } => host
                .parse::<IpAddr>()
                .is_ok_and(|target| cidr_contains(*address, *prefix, target)),
            Self::Domain {
                suffix,
                port: rule_port,
                match_host,
            } => {
                let host = host.to_ascii_lowercase();
                (host.ends_with(suffix) || (*match_host && host == suffix[1..]))
                    && rule_port.is_none_or(|rule_port| rule_port == port)
            }
        }
    }
}

fn split_optional_host_port(raw: &str) -> Result<(&str, Option<u16>), LfsHttpPolicyError> {
    if raw.starts_with('[') {
        let closing = raw.find(']').ok_or(LfsHttpPolicyError::InvalidNoProxy)?;
        let host = &raw[..=closing];
        let suffix = &raw[closing + 1..];
        if suffix.is_empty() {
            return Ok((host, None));
        }
        let port = suffix
            .strip_prefix(':')
            .ok_or(LfsHttpPolicyError::InvalidNoProxy)?
            .parse::<u16>()
            .map_err(|_| LfsHttpPolicyError::InvalidNoProxy)?;
        return Ok((host, Some(port)));
    }
    if raw.matches(':').count() == 1 {
        let (host, port) = raw.rsplit_once(':').expect("one colon");
        if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) {
            return Ok((
                host,
                Some(
                    port.parse::<u16>()
                        .map_err(|_| LfsHttpPolicyError::InvalidNoProxy)?,
                ),
            ));
        }
    }
    Ok((raw, None))
}

fn cidr_contains(network: IpAddr, prefix: u8, target: IpAddr) -> bool {
    match (network, target) {
        (IpAddr::V4(network), IpAddr::V4(target)) => {
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - u32::from(prefix))
            };
            u32::from(network) & mask == u32::from(target) & mask
        }
        (IpAddr::V6(network), IpAddr::V6(target)) => {
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - u32::from(prefix))
            };
            u128::from(network) & mask == u128::from(target) & mask
        }
        _ => false,
    }
}

#[derive(Default)]
struct CertificateBudget {
    bytes: usize,
    entries: usize,
}

impl CertificateBudget {
    fn charge(&mut self, bytes: usize, entries: usize) -> Result<(), LfsHttpPolicyError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(LfsHttpPolicyError::CertificatePolicyTooLarge)?;
        self.entries = self
            .entries
            .checked_add(entries)
            .ok_or(LfsHttpPolicyError::CertificatePolicyTooLarge)?;
        if self.bytes > MAX_CA_TOTAL_BYTES || self.entries > MAX_CA_ENTRIES {
            return Err(LfsHttpPolicyError::CertificatePolicyTooLarge);
        }
        Ok(())
    }
}

fn optional_certificate_source(
    path: Option<OsString>,
    kind: CertificateSourceKind,
) -> Result<Option<CertificateSetting>, LfsHttpPolicyError> {
    path.filter(|path| !path.is_empty())
        .map(|path| {
            CertificateSource::new(PathBuf::from(path), kind).map(CertificateSetting::Source)
        })
        .transpose()
}

fn config_certificate_source(
    raw: &str,
    kind: CertificateSourceKind,
) -> Result<CertificateSetting, LfsHttpPolicyError> {
    if raw.is_empty() {
        Ok(CertificateSetting::Platform)
    } else {
        CertificateSource::new(PathBuf::from(raw), kind).map(CertificateSetting::Source)
    }
}

#[cfg(unix)]
fn encoded_path_bytes(path: &OsStr) -> usize {
    path.as_bytes().len()
}

#[cfg(windows)]
fn encoded_path_bytes(path: &OsStr) -> usize {
    path.encode_wide().count().saturating_mul(2)
}

#[cfg(all(not(unix), not(windows)))]
fn encoded_path_bytes(path: &OsStr) -> usize {
    path.to_string_lossy().len()
}

fn load_certificate_file(path: &Path) -> Result<TlsRootCertificates, LfsHttpPolicyError> {
    let mut budget = CertificateBudget::default();
    let bytes = read_bounded_regular_file(path, true)?
        .ok_or(LfsHttpPolicyError::InvalidCertificateSource)?;
    let entries = certificate_count(&bytes)?;
    budget.charge(bytes.len(), entries)?;
    let certificate = parse_root_certificate(bytes)?;
    TlsRootCertificates::new(vec![certificate])
        .map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)
}

fn load_certificate_directory(path: &Path) -> Result<TlsRootCertificates, LfsHttpPolicyError> {
    let directory = open_certificate_directory(path)?;
    let mut budget = CertificateBudget::default();
    let mut names = Vec::new();
    let mut enumerated_entries = 0_usize;
    for entry in fs::read_dir(path).map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)? {
        enumerated_entries = enumerated_entries
            .checked_add(1)
            .ok_or(LfsHttpPolicyError::CertificatePolicyTooLarge)?;
        if enumerated_entries > MAX_CA_ENTRIES {
            return Err(LfsHttpPolicyError::CertificatePolicyTooLarge);
        }
        let entry = entry.map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)?;
        let file_type = entry
            .file_type()
            .map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)?;
        if file_type.is_file() || file_type.is_symlink() {
            names.push(entry.file_name());
        }
    }
    names.sort();
    let mut certificates = Vec::new();
    for name in names {
        let Some(bytes) = read_bounded_regular_file_in_directory(&directory, path, &name, false)?
        else {
            continue;
        };
        let entries = certificate_count(&bytes)?;
        budget.charge(bytes.len(), entries)?;
        certificates.push(parse_root_certificate(bytes)?);
    }
    TlsRootCertificates::new(certificates).map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)
}

fn parse_root_certificate(bytes: Vec<u8>) -> Result<TlsRootCertificate, LfsHttpPolicyError> {
    if bytes
        .windows(b"-----BEGIN CERTIFICATE-----".len())
        .any(|window| window == b"-----BEGIN CERTIFICATE-----")
    {
        TlsRootCertificate::from_pem_bundle(bytes)
    } else {
        TlsRootCertificate::from_der(bytes)
    }
    .map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)
}

fn read_bounded_regular_file(
    path: &Path,
    required: bool,
) -> Result<Option<Vec<u8>>, LfsHttpPolicyError> {
    let file = open_certificate_file(path)?;
    read_bounded_regular_handle(file, required)
}

#[cfg(unix)]
fn read_bounded_regular_file_in_directory(
    directory: &fs::File,
    _directory_path: &Path,
    name: &OsStr,
    required: bool,
) -> Result<Option<Vec<u8>>, LfsHttpPolicyError> {
    let name = std::ffi::CString::new(name.as_bytes())
        .map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)?;
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if descriptor < 0 {
        return Err(LfsHttpPolicyError::InvalidCertificateSource);
    }
    let file = unsafe { fs::File::from_raw_fd(descriptor) };
    read_bounded_regular_handle(file, required)
}

#[cfg(not(unix))]
fn read_bounded_regular_file_in_directory(
    _directory: &fs::File,
    directory_path: &Path,
    name: &OsStr,
    required: bool,
) -> Result<Option<Vec<u8>>, LfsHttpPolicyError> {
    read_bounded_regular_file(&directory_path.join(name), required)
}

fn read_bounded_regular_handle(
    file: fs::File,
    required: bool,
) -> Result<Option<Vec<u8>>, LfsHttpPolicyError> {
    let metadata = file
        .metadata()
        .map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)?;
    if !metadata.is_file() {
        return if required {
            Err(LfsHttpPolicyError::InvalidCertificateSource)
        } else {
            Ok(None)
        };
    }
    if metadata.len() > MAX_CA_INPUT_BYTES {
        return Err(LfsHttpPolicyError::CertificateSourceTooLarge);
    }
    let mut bytes = Vec::new();
    file.take(MAX_CA_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)?;
    if bytes.len() as u64 > MAX_CA_INPUT_BYTES {
        bytes.fill(0);
        return Err(LfsHttpPolicyError::CertificateSourceTooLarge);
    }
    if bytes.is_empty() {
        return Err(LfsHttpPolicyError::InvalidCertificateSource);
    }
    Ok(Some(bytes))
}

#[cfg(unix)]
fn open_certificate_file(path: &Path) -> Result<fs::File, LfsHttpPolicyError> {
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        // O_NONBLOCK is required before opening an attacker-controlled path:
        // opening a FIFO for reading would otherwise wait indefinitely before
        // the same-handle regular-file check can run.
        .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK);
    options
        .open(path)
        .map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)
}

#[cfg(unix)]
fn open_certificate_directory(path: &Path) -> Result<fs::File, LfsHttpPolicyError> {
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK | libc::O_DIRECTORY);
    let directory = options
        .open(path)
        .map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)?;
    if !directory
        .metadata()
        .map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)?
        .is_dir()
    {
        return Err(LfsHttpPolicyError::InvalidCertificateSource);
    }
    Ok(directory)
}

#[cfg(windows)]
fn open_certificate_file(path: &Path) -> Result<fs::File, LfsHttpPolicyError> {
    validate_windows_certificate_namespace(path)?;
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    let file = options
        .open(path)
        .map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)?;
    let information = windows_certificate_handle_information(&file)?;
    let attributes = information.dwFileAttributes;
    if attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
        || unsafe { GetFileType(file.as_raw_handle() as _) } != FILE_TYPE_DISK
    {
        return Err(LfsHttpPolicyError::InvalidCertificateSource);
    }
    Ok(file)
}

#[cfg(windows)]
fn open_certificate_directory(path: &Path) -> Result<fs::File, LfsHttpPolicyError> {
    validate_windows_certificate_namespace(path)?;
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        // Denying FILE_SHARE_DELETE pins this directory pathname while the
        // bounded enumeration and child opens are performed.
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    let directory = options
        .open(path)
        .map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)?;
    let information = windows_certificate_handle_information(&directory)?;
    let attributes = information.dwFileAttributes;
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || unsafe { GetFileType(directory.as_raw_handle() as _) } != FILE_TYPE_DISK
    {
        return Err(LfsHttpPolicyError::InvalidCertificateSource);
    }
    Ok(directory)
}

#[cfg(windows)]
fn windows_certificate_handle_information(
    file: &fs::File,
) -> Result<BY_HANDLE_FILE_INFORMATION, LfsHttpPolicyError> {
    let mut information = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut information) } == 0 {
        return Err(LfsHttpPolicyError::InvalidCertificateSource);
    }
    Ok(information)
}

#[cfg(windows)]
fn validate_windows_certificate_namespace(path: &Path) -> Result<(), LfsHttpPolicyError> {
    let normalized = path.as_os_str().to_string_lossy().replace('/', "\\");
    let lowercase = normalized.to_ascii_lowercase();
    if lowercase.starts_with("\\\\.\\")
        || lowercase.starts_with("\\\\?\\")
        || lowercase.starts_with("\\??\\")
        || lowercase.starts_with("\\device\\")
        || lowercase.strip_prefix("\\\\").is_some_and(|unc| {
            let mut components = unc.split('\\');
            components.next().is_some_and(|server| !server.is_empty())
                && components.next() == Some("pipe")
        })
    {
        return Err(LfsHttpPolicyError::InvalidCertificateSource);
    }
    for component in path.components() {
        let std::path::Component::Normal(component) = component else {
            continue;
        };
        let component = component.to_string_lossy();
        if component.ends_with(['.', ' ']) {
            return Err(LfsHttpPolicyError::InvalidCertificateSource);
        }
        let basename = component
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let reserved = matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || basename
                .strip_prefix("COM")
                .or_else(|| basename.strip_prefix("LPT"))
                .is_some_and(|suffix| {
                    suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
                });
        if reserved {
            return Err(LfsHttpPolicyError::InvalidCertificateSource);
        }
    }
    Ok(())
}

#[cfg(all(not(unix), not(windows)))]
fn open_certificate_file(path: &Path) -> Result<fs::File, LfsHttpPolicyError> {
    fs::File::open(path).map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)
}

#[cfg(all(not(unix), not(windows)))]
fn open_certificate_directory(path: &Path) -> Result<fs::File, LfsHttpPolicyError> {
    let directory =
        fs::File::open(path).map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)?;
    if !directory
        .metadata()
        .map_err(|_| LfsHttpPolicyError::InvalidCertificateSource)?
        .is_dir()
    {
        return Err(LfsHttpPolicyError::InvalidCertificateSource);
    }
    Ok(directory)
}

fn certificate_count(bytes: &[u8]) -> Result<usize, LfsHttpPolicyError> {
    let marker = b"-----BEGIN CERTIFICATE-----";
    let count = bytes
        .windows(marker.len())
        .filter(|window| *window == marker)
        .count();
    let count = if count == 0 { 1 } else { count };
    if count > MAX_CA_ENTRIES {
        Err(LfsHttpPolicyError::CertificatePolicyTooLarge)
    } else {
        Ok(count)
    }
}

fn enforce_timeout_rule_bound(entries: &[ConfigEntry]) -> Result<(), LfsHttpPolicyError> {
    let mut rules = 0_usize;
    for entry in entries {
        if entry.section == "lfs"
            && matches!(
                entry.key.as_str(),
                "dialtimeout" | "tlstimeout" | "activitytimeout"
            )
        {
            rules = rules
                .checked_add(1)
                .ok_or(LfsHttpPolicyError::PolicyTooLarge)?;
            if rules > MAX_TIMEOUT_POLICY_RULES {
                return Err(LfsHttpPolicyError::PolicyTooLarge);
            }
        }
    }
    Ok(())
}

fn parse_global_timeout(raw: &str) -> Result<Duration, LfsHttpPolicyError> {
    match parse_activity_timeout(raw) {
        LfsActivityTimeout::Enabled(timeout) => Ok(timeout),
        LfsActivityTimeout::Disabled => Ok(Duration::from_secs(DEFAULT_LFS_TIMEOUT_SECONDS)),
        LfsActivityTimeout::Unrepresentable => Err(LfsHttpPolicyError::InvalidTimeout),
    }
}

fn selected_global_timeout(
    entries: &[ConfigEntry],
    key: &str,
) -> Result<Duration, LfsHttpPolicyError> {
    let selected = entries
        .iter()
        .rev()
        .find(|entry| entry.section == "lfs" && entry.subsection.is_empty() && entry.key == key);
    selected.map_or_else(
        || Ok(Duration::from_secs(DEFAULT_LFS_TIMEOUT_SECONDS)),
        |entry| parse_global_timeout(&entry.value),
    )
}

fn parse_activity_timeout(raw: &str) -> LfsActivityTimeout {
    match raw.parse::<u64>() {
        Ok(0) => LfsActivityTimeout::Disabled,
        Ok(seconds) => LfsActivityTimeout::Enabled(Duration::from_secs(seconds)),
        Err(_) if !is_positive_decimal(raw) => LfsActivityTimeout::Disabled,
        Err(_) => LfsActivityTimeout::Unrepresentable,
    }
}

fn is_positive_decimal(raw: &str) -> bool {
    let digits = raw.strip_prefix('+').unwrap_or(raw);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn enforce_policy_rule_bound(entries: &[ConfigEntry]) -> Result<(), LfsHttpPolicyError> {
    let mut rules = 0_usize;
    for entry in entries {
        if entry.section == "http"
            && matches!(
                entry.key.as_str(),
                "proxy"
                    | "sslverify"
                    | "sslcainfo"
                    | "sslcapath"
                    | "extraheader"
                    | "sslcert"
                    | "sslkey"
                    | "sslbackend"
                    | "schannelusesslcainfo"
            )
        {
            rules = rules
                .checked_add(1)
                .ok_or(LfsHttpPolicyError::PolicyTooLarge)?;
            if rules > MAX_HTTP_POLICY_RULES {
                return Err(LfsHttpPolicyError::PolicyTooLarge);
            }
        }
    }
    Ok(())
}

fn reject_unsupported_tls_settings(entries: &[ConfigEntry]) -> Result<(), LfsHttpPolicyError> {
    // Git's last value for the same exact key and scope wins. Empty values
    // explicitly clear sslCert/sslKey. Client identities remain unsupported
    // and are rejected rather than silently ignored. sslBackend and
    // schannelUseSSLCAInfo are modeled separately as Git LFS compatibility
    // selectors; they never switch the Rust TLS implementation.
    let mut seen = HashSet::new();
    for entry in entries.iter().rev() {
        if entry.section != "http" || !matches!(entry.key.as_str(), "sslcert" | "sslkey") {
            continue;
        }
        let scope = if entry.subsection.is_empty() {
            None
        } else {
            Some(LfsUrlScope::parse(&entry.subsection).ok_or(LfsHttpPolicyError::InvalidUrlScope)?)
        };
        if !seen.insert((scope, entry.key.as_str())) {
            continue;
        }
        match entry.key.as_str() {
            "sslcert" | "sslkey" if entry.value.is_empty() => {}
            _ => return Err(LfsHttpPolicyError::UnsupportedTlsConfiguration),
        }
    }
    Ok(())
}

fn validate_relevant_url_scopes(entries: &[ConfigEntry]) -> Result<(), LfsHttpPolicyError> {
    if entries.iter().any(|entry| {
        entry.section == "http"
            && !entry.subsection.is_empty()
            && matches!(
                entry.key.as_str(),
                "proxy"
                    | "sslverify"
                    | "sslcainfo"
                    | "extraheader"
                    | "sslcert"
                    | "sslkey"
                    | "sslbackend"
                    | "schannelusesslcainfo"
            )
            && LfsUrlScope::parse(&entry.subsection).is_none()
    }) {
        Err(LfsHttpPolicyError::InvalidUrlScope)
    } else {
        Ok(())
    }
}

fn parse_keepalive(raw: &str) -> Duration {
    let seconds = raw
        .parse::<i64>()
        .ok()
        .filter(|seconds| *seconds > 0)
        .map(|seconds| seconds as u64)
        .unwrap_or(DEFAULT_KEEPALIVE_SECONDS);
    Duration::from_secs(seconds)
}

fn parse_config_bool(entry: &ConfigEntry) -> Result<bool, LfsHttpPolicyError> {
    if entry.implicit_bool {
        Ok(true)
    } else {
        parse_strict_bool(&entry.value).ok_or(LfsHttpPolicyError::InvalidBoolean)
    }
}

fn parse_schannel_use_ssl_ca_info(entry: &ConfigEntry) -> bool {
    if entry.implicit_bool {
        true
    } else {
        // Git LFS v3.7.1 calls config.Bool(value, false), whose contract maps
        // blank and unrecognized values to false instead of rejecting them.
        parse_strict_bool(&entry.value).unwrap_or(false)
    }
}

fn parse_optional_environment_bool(value: Option<OsString>) -> Result<bool, LfsHttpPolicyError> {
    let Some(value) = value else {
        return Ok(false);
    };
    let value = value
        .into_string()
        .map_err(|_| LfsHttpPolicyError::InvalidBoolean)?;
    if value.is_empty() {
        return Ok(false);
    }
    parse_strict_bool(&value).ok_or(LfsHttpPolicyError::InvalidBoolean)
}

fn parse_strict_bool(raw: &str) -> Option<bool> {
    match raw.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" | "t" => Some(true),
        "false" | "no" | "off" | "0" | "f" => Some(false),
        _ => None,
    }
}

fn selected_environment_text(
    primary: Option<OsString>,
    fallback: Option<OsString>,
    error: LfsHttpPolicyError,
) -> Result<Option<String>, LfsHttpPolicyError> {
    primary
        .filter(|value| !value.is_empty())
        .or_else(|| fallback.filter(|value| !value.is_empty()))
        .map(|value| value.into_string().map_err(|_| error))
        .transpose()
}

fn parse_optional_proxy(raw: Option<String>) -> Result<Option<ProxyUrl>, LfsHttpPolicyError> {
    raw.map(|raw| parse_proxy(&raw)).transpose()
}

fn parse_proxy(raw: &str) -> Result<ProxyUrl, LfsHttpPolicyError> {
    if raw.is_empty() {
        return Err(LfsHttpPolicyError::InvalidProxy);
    }
    let normalized = if let Some((scheme, remainder)) = raw.split_once("://") {
        if !matches!(
            scheme.to_ascii_lowercase().as_str(),
            "http" | "https" | "socks5" | "socks5h"
        ) {
            return Err(LfsHttpPolicyError::UnsupportedProxyScheme);
        }
        if proxy_has_path(remainder) {
            return Err(LfsHttpPolicyError::UnsupportedProxyPath);
        }
        raw.to_owned()
    } else {
        if proxy_has_path(raw) {
            return Err(LfsHttpPolicyError::UnsupportedProxyPath);
        }
        format!("http://{raw}")
    };
    ProxyUrl::parse(&normalized).map_err(|_| LfsHttpPolicyError::InvalidProxy)
}

fn proxy_has_path(authority_and_path: &str) -> bool {
    authority_and_path
        .find(['/', '?', '#'])
        .is_some_and(|index| &authority_and_path[index..] != "/")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read as _, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    use super::*;
    use crate::runtime::ConfigScope;
    use zmin_http_transport::{ClientConfig, HttpClient, HttpRequest};

    const ROOT_PEM: &[u8] = br#"-----BEGIN CERTIFICATE-----
MIIBgDCCASegAwIBAgIUPHDUu9WL36yvTmFeNFZVe/qhClcwCgYIKoZIzj0EAwIw
HTEbMBkGA1UEAwwSUnVzdGxzIFJvYnVzdCBSb290MCAXDTc1MDEwMTAwMDAwMFoY
DzQwOTYwMTAxMDAwMDAwWjAdMRswGQYDVQQDDBJSdXN0bHMgUm9idXN0IFJvb3Qw
WTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAASW/VkDFs5iGDQvH8jaXYT4jMx66jo+
5CWKyMt4OlTDdBfKfnmQ9LYeK/PsYfJ8wVizuSlPzXi9je8SnyYejGP3o0MwQTAP
BgNVHQ8BAf8EBQMDB4QAMB0GA1UdDgQWBBRqY/oMENJbNo7y39iL6GW3tDs0rzAP
BgNVHRMBAf8EBTADAQH/MAoGCCqGSM49BAMCA0cAMEQCIEUbrmSUjANju9nNpFop
PAl9Wh8tBxI5IY+BPh466+aUAiA1/9+prypt6s3Doo0GDsnoFGJi1UBivUg1qdik
cy4eNw==
-----END CERTIFICATE-----
"#;

    fn entry(name: &str, value: &str) -> ConfigEntry {
        let (section, subsection, key) = if let Some(rest) = name
            .strip_prefix("http.")
            .or_else(|| name.strip_prefix("lfs."))
        {
            let section = if name.starts_with("http.") {
                "http"
            } else {
                "lfs"
            };
            if let Some((subsection, key)) = rest.rsplit_once('.') {
                if subsection.starts_with("http://") || subsection.starts_with("https://") {
                    (section, subsection, key)
                } else {
                    (section, "", rest)
                }
            } else {
                (section, "", rest)
            }
        } else {
            let (section, key) = name.split_once('.').expect("config name");
            (section, "", key)
        };
        ConfigEntry {
            section: section.to_owned(),
            raw_section: section.to_owned(),
            subsection: subsection.to_owned(),
            key: key.to_ascii_lowercase(),
            raw_key: key.to_owned(),
            value: value.to_owned(),
            comment: None,
            implicit_bool: false,
            scope: ConfigScope::Local,
            origin: "test".into(),
            line: None,
        }
    }

    fn resolve(policy: &LfsHttpPolicy, raw: &str) -> ResolvedRequestPolicy {
        policy
            .resolve(&HttpUrl::parse(raw).expect("URL"))
            .expect("policy")
    }

    fn resolve_tls(
        policy: &LfsHttpPolicy,
        raw: &str,
    ) -> Result<TlsVerification, LfsHttpPolicyError> {
        let url = HttpUrl::parse(raw).expect("URL");
        policy.tls_for(&url)
    }

    #[test]
    fn proxy_precedence_empty_fallback_https_to_http_and_no_proxy_are_exact() {
        let environment = LfsHttpEnvironmentSnapshot::default().with_proxy(
            Some("https://upper.example:8443"),
            Some("https://lower.example:8443"),
            Some("http://http.example:8080"),
            None,
            Some(".internal.example,api.example:443,10.0.0.0/8"),
        );
        let entries = vec![
            entry("http.proxy", "https://config.example:9443"),
            entry("http.https://api.example.proxy", ""),
        ];
        let policy = LfsHttpPolicy::from_entries(&entries, environment).expect("policy");
        assert!(matches!(
            resolve(&policy, "https://other.example/repo")
                .connection
                .proxy,
            ProxyPolicy::Explicit(_)
        ));
        assert_eq!(
            resolve(&policy, "https://api.example/repo")
                .connection
                .proxy,
            ProxyPolicy::Disabled
        );
        assert_eq!(
            resolve(&policy, "https://node.internal.example/repo")
                .connection
                .proxy,
            ProxyPolicy::Disabled
        );
        assert_eq!(
            resolve(&policy, "http://10.2.3.4/repo").connection.proxy,
            ProxyPolicy::Disabled
        );
    }

    #[test]
    fn resolved_explicit_proxy_is_used_by_the_transport() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("proxy listener");
        let address = listener.local_addr().expect("proxy address");
        let proxy = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("proxy request");
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).expect("proxy request headers");
                request.push(byte[0]);
                assert!(request.len() < 64 * 1024, "proxy request header bound");
            }
            let line = std::str::from_utf8(&request)
                .expect("request UTF-8")
                .lines()
                .next()
                .expect("request line");
            assert!(line.starts_with("GET http://example.invalid/lfs-object HTTP/1.1"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nproxied",
                )
                .expect("proxy response");
        });
        let policy = LfsHttpPolicy::from_entries(
            &[entry("http.proxy", &format!("http://{address}"))],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("policy");
        let mut config = ClientConfig::default();
        config.retry.max_attempts = 1;
        let resolver: Arc<dyn RequestPolicyResolver> = Arc::new(policy);
        let client = HttpClient::with_policy_resolver(config, resolver).expect("client");
        let mut response = client
            .execute(&HttpRequest::new(
                "GET".parse().expect("HTTP method"),
                HttpUrl::parse("http://example.invalid/lfs-object").expect("target URL"),
            ))
            .expect("proxied request");
        let mut body = String::new();
        response.read_to_string(&mut body).expect("response body");
        assert_eq!(body, "proxied");
        proxy.join().expect("proxy server");
    }

    #[test]
    fn no_proxy_matches_git_lfs_domain_wildcard_port_and_ip_rules() {
        let bare = NoProxyRules::parse(Some("example.com".to_owned())).expect("bare domain");
        assert!(bare.bypasses("example.com", 443));
        assert!(bare.bypasses("child.example.com", 443));

        for value in [".example.com", "*.example.com"] {
            let subdomains =
                NoProxyRules::parse(Some(value.to_owned())).expect("subdomain wildcard");
            assert!(!subdomains.bypasses("example.com", 443), "{value}");
            assert!(subdomains.bypasses("child.example.com", 443), "{value}");
            assert!(
                subdomains.bypasses("deep.child.example.com", 443),
                "{value}"
            );
            assert!(!subdomains.bypasses("notexample.com", 443), "{value}");
        }

        let port = NoProxyRules::parse(Some("*.example.com:8443".to_owned())).expect("port");
        assert!(port.bypasses("child.example.com", 8443));
        assert!(!port.bypasses("child.example.com", 443));
        assert!(!port.bypasses("example.com", 8443));

        let ipv6 = NoProxyRules::parse(Some("[2001:db8::1]:8443".to_owned())).expect("IPv6");
        assert!(ipv6.bypasses("2001:db8::1", 8443));
        assert!(!ipv6.bypasses("2001:db8::1", 443));
        assert!(!ipv6.bypasses("2001:db8::2", 8443));

        let ipv6_cidr = NoProxyRules::parse(Some("2001:db8::/32".to_owned())).expect("IPv6 CIDR");
        assert!(ipv6_cidr.bypasses("2001:db8:1::1", 443));
        assert!(!ipv6_cidr.bypasses("2001:db9::1", 443));
    }

    #[test]
    fn resolved_explicit_proxy_uses_connect_for_https() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("proxy listener");
        let address = listener.local_addr().expect("proxy address");
        let proxy = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("proxy CONNECT");
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).expect("CONNECT headers");
                request.push(byte[0]);
                assert!(request.len() < 64 * 1024, "CONNECT header bound");
            }
            let line = std::str::from_utf8(&request)
                .expect("request UTF-8")
                .lines()
                .next()
                .expect("request line");
            assert!(line.starts_with("CONNECT example.invalid:443 HTTP/1.1"));
            stream
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                .expect("proxy response");
        });
        let policy = LfsHttpPolicy::from_entries(
            &[entry("http.proxy", &format!("http://{address}"))],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("policy");
        let mut config = ClientConfig::default();
        config.retry.max_attempts = 1;
        let resolver: Arc<dyn RequestPolicyResolver> = Arc::new(policy);
        let client = HttpClient::with_policy_resolver(config, resolver).expect("client");
        let error = client
            .execute(&HttpRequest::new(
                "GET".parse().expect("HTTP method"),
                HttpUrl::parse("https://example.invalid/lfs-object").expect("target URL"),
            ))
            .expect_err("CONNECT rejected");
        assert!(matches!(
            error,
            TransportError::Connect | TransportError::Request(_)
        ));
        proxy.join().expect("proxy server");
    }

    #[test]
    fn https_proxy_tls_is_resolved_from_credential_free_proxy_origin() {
        let environment = LfsHttpEnvironmentSnapshot::default().with_proxy(
            Some("https://proxy-user:proxy-secret@proxy.example:8443"),
            None,
            None,
            None,
            None,
        );
        let policy = LfsHttpPolicy::from_entries(
            &[
                entry("http.https://target.example.sslverify", "false"),
                entry("http.https://proxy.example:8443/private.sslverify", "false"),
                entry(
                    "http.https://proxy-user@proxy.example:8443.sslverify",
                    "false",
                ),
            ],
            environment,
        )
        .expect("separate target and proxy TLS policy");
        let resolved = resolve(&policy, "https://target.example/repo/object");
        assert_eq!(resolved.connection.tls, TlsVerification::Disabled);
        assert_eq!(resolved.proxy_tls_policy.tls, TlsVerification::Platform);
        assert!(resolved.connection.client_identity.is_none());
        assert!(resolved.proxy_tls_policy.client_identity.is_none());
        let diagnostic = format!("{resolved:?}");
        assert!(!diagnostic.contains("proxy-secret"));
        assert!(!diagnostic.contains("proxy-user"));
        assert!(!diagnostic.contains("target.example"));

        let proxy_scoped = LfsHttpPolicy::from_entries(
            &[entry("http.https://proxy.example:8443.sslverify", "false")],
            LfsHttpEnvironmentSnapshot::default().with_proxy(
                Some("https://proxy.example:8443"),
                None,
                None,
                None,
                None,
            ),
        )
        .expect("proxy-origin verification policy");
        let resolved = resolve(&proxy_scoped, "https://target.example/repo/object");
        assert_eq!(resolved.connection.tls, TlsVerification::Platform);
        assert_eq!(resolved.proxy_tls_policy.tls, TlsVerification::Disabled);

        let identity_path = "/private/target-client-identity.pem";
        let error = LfsHttpPolicy::from_entries(
            &[entry("http.https://target.example.sslcert", identity_path)],
            LfsHttpEnvironmentSnapshot::default().with_proxy(
                Some("https://proxy.example:8443"),
                None,
                None,
                None,
                None,
            ),
        )
        .expect_err("target mTLS remains fail-closed before proxy setup");
        assert_eq!(error, LfsHttpPolicyError::UnsupportedTlsConfiguration);
        assert!(!format!("{error:?} {error}").contains(identity_path));
    }

    #[test]
    fn https_proxy_and_target_custom_roots_do_not_bleed_between_origins() {
        let root = std::env::temp_dir().join(format!(
            "zmin-lfs-http-proxy-ca-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::create_dir_all(&root).expect("temp root");
        let ca_info = root.join("private-root.pem");
        fs::write(&ca_info, ROOT_PEM).expect("private root");
        let environment = || {
            LfsHttpEnvironmentSnapshot::default().with_proxy(
                Some("https://proxy.example:8443"),
                None,
                None,
                None,
                None,
            )
        };

        let target_only = LfsHttpPolicy::from_entries(
            &[entry(
                "http.https://target.example.sslcainfo",
                &ca_info.to_string_lossy(),
            )],
            environment(),
        )
        .expect("target custom roots");
        let resolved = resolve(&target_only, "https://target.example/repo");
        assert!(matches!(
            resolved.connection.tls,
            TlsVerification::CustomRoots(_)
        ));
        assert_eq!(resolved.proxy_tls_policy.tls, TlsVerification::Platform);

        let proxy_only = LfsHttpPolicy::from_entries(
            &[entry(
                "http.https://proxy.example:8443.sslcainfo",
                &ca_info.to_string_lossy(),
            )],
            environment(),
        )
        .expect("proxy custom roots");
        let resolved = resolve(&proxy_only, "https://target.example/repo");
        assert_eq!(resolved.connection.tls, TlsVerification::Platform);
        assert!(matches!(
            resolved.proxy_tls_policy.tls,
            TlsVerification::CustomRoots(_)
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsupported_proxy_forms_fail_closed() {
        for value in [
            "socks4://proxy.example:1080",
            "socks5://proxy.example:1080/path",
            "socks5://proxy.example:1080?query",
            "socks5://proxy.example:1080#fragment",
            "http://proxy.example/path",
        ] {
            assert!(
                LfsHttpPolicy::from_entries(
                    &[entry("http.proxy", value)],
                    LfsHttpEnvironmentSnapshot::default(),
                )
                .is_err()
            );
        }
    }

    #[test]
    fn lfs_timeout_defaults_parsing_and_precedence_match_v3_7_1() {
        let defaults = LfsHttpPolicy::default();
        assert_eq!(
            resolve(&defaults, "https://example.test/repo").timeouts,
            HttpTimeoutPolicy {
                dial_timeout: Duration::from_secs(30),
                tls_handshake_timeout: Duration::from_secs(30),
                activity_timeout: Some(Duration::from_secs(30)),
            }
        );

        for invalid in ["", "0", "-1", "invalid"] {
            let policy = LfsHttpPolicy::from_entries(
                &[
                    entry("lfs.dialtimeout", invalid),
                    entry("lfs.tlstimeout", invalid),
                    entry("lfs.activitytimeout", invalid),
                ],
                LfsHttpEnvironmentSnapshot::default(),
            )
            .expect("Git LFS invalid-value semantics");
            assert_eq!(
                resolve(&policy, "https://example.test/repo").timeouts,
                HttpTimeoutPolicy {
                    dial_timeout: Duration::from_secs(30),
                    tls_handshake_timeout: Duration::from_secs(30),
                    activity_timeout: None,
                },
                "{invalid:?}"
            );
        }

        let policy = LfsHttpPolicy::from_entries(
            &[
                entry("lfs.dialtimeout", "7"),
                entry("lfs.dialtimeout", "17"),
                entry("lfs.https://example.test/repo.dialtimeout", "1"),
                entry("lfs.tlstimeout", "19"),
                entry("lfs.https://example.test/repo.tlstimeout", "2"),
                entry("lfs.activitytimeout", "23"),
            ],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("last ordinary value wins");
        assert_eq!(
            resolve(&policy, "https://example.test/repo").timeouts,
            HttpTimeoutPolicy {
                dial_timeout: Duration::from_secs(17),
                tls_handshake_timeout: Duration::from_secs(19),
                activity_timeout: Some(Duration::from_secs(23)),
            }
        );
    }

    #[test]
    fn activity_timeout_uses_actual_url_scope_and_last_equal_match() {
        let policy = LfsHttpPolicy::from_entries(
            &[
                entry("lfs.activitytimeout", "9"),
                entry("lfs.https://example.test/team.activitytimeout", "15"),
                entry("lfs.https://example.test/team/repo.activitytimeout", "21"),
                entry("lfs.https://example.test/team/repo.activitytimeout", "22"),
                entry(
                    "lfs.https://example.test/team/repo/private.activitytimeout",
                    "invalid",
                ),
            ],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("scoped activity policy");
        for (url, expected) in [
            ("https://example.test/team/repo/objects/one", Some(22_u64)),
            ("https://example.test:443/team/repo/two", Some(22)),
            ("https://example.test/team/other", Some(15)),
            ("https://example.test/elsewhere", Some(9)),
            ("https://example.test:8443/team/repo", Some(9)),
            ("https://example.test/team/repo/private/object", None),
        ] {
            assert_eq!(
                resolve(&policy, url).timeouts.activity_timeout,
                expected.map(Duration::from_secs),
                "{url}"
            );
        }
    }

    #[test]
    fn selected_timeout_overflow_fails_sanitized_before_network() {
        let huge = "9".repeat(1_024);
        for key in ["lfs.dialtimeout", "lfs.tlstimeout"] {
            LfsHttpPolicy::from_entries(
                &[entry(key, &huge), entry(key, "31")],
                LfsHttpEnvironmentSnapshot::default(),
            )
            .expect("shadowed global overflow is not selected");
            let error = LfsHttpPolicy::from_entries(
                &[entry(key, "31"), entry(key, &huge)],
                LfsHttpEnvironmentSnapshot::default(),
            )
            .expect_err("selected global overflow");
            assert_eq!(error, LfsHttpPolicyError::InvalidTimeout);
            assert!(!format!("{error:?} {error}").contains(&huge));
        }

        let scoped_key = "lfs.https://selected.example/repo.activitytimeout";
        let shadowed = LfsHttpPolicy::from_entries(
            &[entry(scoped_key, &huge), entry(scoped_key, "29")],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("shadowed scoped overflow is not selected");
        assert_eq!(
            resolve(&shadowed, "https://selected.example/repo/object")
                .timeouts
                .activity_timeout,
            Some(Duration::from_secs(29))
        );

        let policy = LfsHttpPolicy::from_entries(
            &[entry(scoped_key, &huge)],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("unselected overflow remains lazy");
        assert_eq!(
            resolve(&policy, "https://other.example/repo")
                .timeouts
                .activity_timeout,
            Some(Duration::from_secs(30))
        );
        let error = policy
            .resolve(&HttpUrl::parse("https://selected.example/repo/object").expect("URL"))
            .expect_err("selected activity overflow");
        assert_eq!(error, TransportError::InvalidTimeout);
        assert!(!format!("{error:?} {error}").contains(&huge));

        // These values fit the configuration integer representation but not
        // an Instant deadline on supported platforms. The transport validates
        // the selected snapshot before attempting DNS or a socket connection.
        let boundary = u64::MAX.to_string();
        for entries in [
            vec![entry("lfs.dialtimeout", &boundary)],
            vec![entry(scoped_key, &boundary)],
        ] {
            let policy =
                LfsHttpPolicy::from_entries(&entries, LfsHttpEnvironmentSnapshot::default())
                    .expect("integer timeout policy");
            let mut config = ClientConfig::default();
            config.request_timeout = None;
            config.operation_timeout = None;
            config.retry.max_attempts = 1;
            let client =
                HttpClient::with_policy_resolver(config, Arc::new(policy)).expect("HTTP client");
            let error = client
                .execute(&HttpRequest::new(
                    "GET".parse().expect("HTTP method"),
                    HttpUrl::parse("https://selected.example/repo/object").expect("target URL"),
                ))
                .expect_err("unrepresentable selected timeout");
            assert_eq!(error, TransportError::InvalidTimeout);
            assert!(!format!("{error:?} {error}").contains(&boundary));
        }
    }

    #[test]
    fn timeout_rules_have_an_independent_bound() {
        let entries = (0..=MAX_TIMEOUT_POLICY_RULES)
            .map(|index| {
                entry(
                    &format!("lfs.https://host-{index}.example.activitytimeout"),
                    "30",
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            LfsHttpPolicy::from_entries(&entries, LfsHttpEnvironmentSnapshot::default())
                .expect_err("timeout rule bound"),
            LfsHttpPolicyError::PolicyTooLarge
        );
    }

    #[test]
    fn socks_proxy_inputs_use_proxy_dns_and_preserve_no_proxy_matching() {
        let environment = LfsHttpEnvironmentSnapshot::default().with_proxy(
            Some("socks5://user:secret@proxy.example"),
            None,
            Some("socks5h://proxy.example:1080"),
            None,
            Some("bypass.example,192.0.2.9,[2001:db8::9]:8443"),
        );
        let policy = LfsHttpPolicy::from_entries(&[], environment).expect("SOCKS policy");
        for target in [
            "https://bypass.example/object",
            "http://192.0.2.9/object",
            "https://[2001:db8::9]:8443/object",
        ] {
            assert_eq!(
                resolve(&policy, target).connection.proxy,
                ProxyPolicy::Disabled,
                "{target}"
            );
        }
        for target in [
            "https://unresolvable-local.invalid/object",
            "http://unresolvable-local.invalid/object",
        ] {
            let ProxyPolicy::Explicit(proxy) = resolve(&policy, target).connection.proxy else {
                panic!("expected explicit SOCKS proxy for {target}");
            };
            assert!(proxy.as_str().starts_with("socks5h://"));
            assert!(proxy.as_str().contains(":1080"));
            assert!(!format!("{proxy:?}").contains("secret"));
        }
        let ProxyPolicy::Explicit(ipv6_proxy) = policy
            .resolve(&HttpUrl::parse("https://[2001:db8::9]/object").expect("IPv6 target"))
            .expect("IPv6 SOCKS target policy")
            .connection
            .proxy
        else {
            panic!("expected explicit SOCKS proxy for IPv6 target");
        };
        assert!(ipv6_proxy.as_str().starts_with("socks5h://"));
        assert!(!format!("{ipv6_proxy:?}").contains("secret"));
    }

    #[test]
    fn extra_headers_preserve_order_reset_provenance_and_authorization_detection() {
        let entries = vec![
            entry("http.extraheader", "X-Old: discarded"),
            entry("http.extraheader", ""),
            entry("http.extraheader", "X-Generic: one"),
            entry("http.extraheader", "X-Generic: two"),
            entry(
                "http.https://api.example/repo.extraheader",
                "Authorization: Bearer scoped",
            ),
        ];
        let policy = LfsHttpPolicy::from_entries(&entries, LfsHttpEnvironmentSnapshot::default())
            .expect("policy");
        let scoped = HttpUrl::parse("https://api.example/repo.git/info/lfs/objects/batch")
            .expect("scoped URL");
        assert!(policy.has_configured_authorization(&scoped));
        assert_eq!(
            resolve(&policy, scoped.as_str())
                .configured_headers
                .provenance(),
            zmin_http_transport::ConfiguredRequestHeaderProvenance::UrlScoped
        );
        let generic = resolve(&policy, "https://other.example/repo");
        assert_eq!(generic.configured_headers.len(), 2);
        assert_eq!(
            generic.configured_headers.provenance(),
            zmin_http_transport::ConfiguredRequestHeaderProvenance::Generic
        );
        let debug = format!("{policy:?}");
        assert!(!debug.contains("Bearer scoped"));
        assert!(!debug.contains("discarded"));
    }

    #[test]
    fn invalid_reserved_and_crlf_headers_fail_closed_without_echo() {
        for value in ["Host: evil.example", "X-Test: ok\r\ninjected: yes"] {
            let error = LfsHttpPolicy::from_entries(
                &[entry("http.extraheader", value)],
                LfsHttpEnvironmentSnapshot::default(),
            )
            .expect_err("invalid header");
            assert_eq!(error, LfsHttpPolicyError::InvalidHeader);
            assert!(!format!("{error:?} {error}").contains(value));
        }
    }

    #[test]
    fn ssl_verify_is_strict_and_environment_disable_is_sticky() {
        assert_eq!(
            LfsHttpPolicy::from_entries(
                &[entry("http.sslverify", "maybe")],
                LfsHttpEnvironmentSnapshot::default(),
            )
            .expect_err("strict boolean"),
            LfsHttpPolicyError::InvalidBoolean
        );
        let environment = LfsHttpEnvironmentSnapshot::default().with_tls(None, None, Some("true"));
        let policy = LfsHttpPolicy::from_entries(
            &[entry("http.https://api.example.sslverify", "true")],
            environment,
        )
        .expect("policy");
        assert_eq!(
            resolve(&policy, "https://api.example/repo").connection.tls,
            TlsVerification::Disabled
        );

        let exact_false = LfsHttpPolicy::from_entries(
            &[entry("http.https://api.example.sslverify", "false")],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("exact scoped false");
        assert_eq!(
            resolve(&exact_false, "https://api.example/repo")
                .connection
                .tls,
            TlsVerification::Disabled
        );
        for lexical_alias in ["FALSE", "False", "no", "off", "0", "f"] {
            let policy = LfsHttpPolicy::from_entries(
                &[entry("http.https://api.example.sslverify", lexical_alias)],
                LfsHttpEnvironmentSnapshot::default(),
            )
            .expect("Git LFS scoped sslVerify lexical behavior");
            assert_eq!(
                resolve(&policy, "https://api.example/repo").connection.tls,
                TlsVerification::Platform,
                "{lexical_alias}"
            );
        }

        // certs.go:isCertVerificationDisabledForHost performs URLConfig
        // lookup with `https://<host[:port]>`, not the request path.
        let path_only = LfsHttpPolicy::from_entries(
            &[entry("http.https://api.example/private.sslverify", "false")],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("path-specific verification policy");
        assert_eq!(
            resolve(&path_only, "https://api.example/private/object")
                .connection
                .tls,
            TlsVerification::Platform
        );
        let port_only = LfsHttpPolicy::from_entries(
            &[entry("http.https://api.example:8443.sslverify", "false")],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("port-specific verification policy");
        assert_eq!(
            resolve(&port_only, "https://api.example:8443/private")
                .connection
                .tls,
            TlsVerification::Disabled
        );
        assert_eq!(
            resolve(&port_only, "https://api.example/private")
                .connection
                .tls,
            TlsVerification::Platform
        );
    }

    #[test]
    fn certificate_files_are_bounded_and_debug_never_exposes_paths() {
        let root = std::env::temp_dir().join(format!(
            "zmin-lfs-http-policy-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::create_dir_all(&root).expect("temp root");
        let oversized = root.join("private-ca-name.pem");
        fs::write(&oversized, vec![b'x'; MAX_CA_INPUT_BYTES as usize + 1]).expect("oversized CA");
        let policy = LfsHttpPolicy::from_entries(
            &[entry("http.sslcainfo", &oversized.to_string_lossy())],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("lazy certificate policy");
        let debug = format!("{policy:?}");
        assert!(!debug.contains("private-ca-name"));
        let error = resolve_tls(&policy, "https://example.com/repo").expect_err("CA bound");
        assert_eq!(error, LfsHttpPolicyError::CertificateSourceTooLarge);
        assert!(!format!("{error:?} {error}").contains("private-ca-name"));

        let oversized_directory = root.join("too-many-ca-entries");
        fs::create_dir(&oversized_directory).expect("CA directory");
        for index in 0..=MAX_CA_ENTRIES {
            fs::create_dir(oversized_directory.join(index.to_string())).expect("CA entry");
        }
        let policy = LfsHttpPolicy::from_entries(
            &[entry(
                "http.sslcapath",
                &oversized_directory.to_string_lossy(),
            )],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("lazy certificate directory policy");
        let error = resolve_tls(&policy, "https://example.com/repo").expect_err("CA entry bound");
        assert_eq!(error, LfsHttpPolicyError::CertificatePolicyTooLarge);

        let long_path = "x".repeat(MAX_CA_PATH_BYTES + 1);
        assert_eq!(
            LfsHttpPolicy::from_entries(
                &[entry("http.sslcainfo", &long_path)],
                LfsHttpEnvironmentSnapshot::default(),
            )
            .expect_err("CA path bound"),
            LfsHttpPolicyError::InvalidCertificateSource
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn certificate_sources_are_selected_lazily_and_disabled_tls_reads_none() {
        let root = std::env::temp_dir().join(format!(
            "zmin-lfs-http-lazy-ca-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::create_dir_all(&root).expect("temp root");
        let selected = root.join("selected.pem");
        fs::write(&selected, ROOT_PEM).expect("selected CA");
        let missing = root.join("must-not-open.pem");

        let policy = LfsHttpPolicy::from_entries(
            &[
                entry("http.sslcainfo", &missing.to_string_lossy()),
                entry("http.sslcainfo", &selected.to_string_lossy()),
                entry(
                    "http.https://unmatched.example.sslcainfo",
                    &missing.to_string_lossy(),
                ),
            ],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("lazy policy");
        assert!(matches!(
            resolve_tls(&policy, "https://selected.example/repo").expect("selected root"),
            TlsVerification::CustomRoots(_)
        ));

        let disabled = LfsHttpPolicy::from_entries(
            &[
                entry("http.sslverify", "false"),
                entry("http.sslcainfo", &missing.to_string_lossy()),
            ],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("disabled verification policy");
        assert_eq!(
            resolve_tls(&disabled, "https://selected.example/repo").expect("disabled verification"),
            TlsVerification::Disabled
        );
        assert_eq!(disabled.certificate_cache.entry_count(), 0);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn selected_fifo_ca_source_fails_without_blocking_and_directory_fifo_is_skipped() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt as _;

        let root = std::env::temp_dir().join(format!(
            "zmin-lfs-http-fifo-ca-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::create_dir_all(&root).expect("temp root");
        let fifo = root.join("never-block.pem");
        let fifo_name = CString::new(fifo.as_os_str().as_bytes()).expect("FIFO path");
        assert_eq!(unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) }, 0);

        let direct = LfsHttpPolicy::from_entries(
            &[entry("http.sslcainfo", &fifo.to_string_lossy())],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("lazy FIFO policy");
        assert_eq!(
            resolve_tls(&direct, "https://example.com/repo").expect_err("FIFO rejected"),
            LfsHttpPolicyError::InvalidCertificateSource
        );

        fs::write(root.join("valid.pem"), ROOT_PEM).expect("valid directory root");
        let directory = LfsHttpPolicy::from_entries(
            &[entry("http.sslcapath", &root.to_string_lossy())],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("lazy directory policy");
        assert!(matches!(
            resolve_tls(&directory, "https://example.com/repo").expect("directory root"),
            TlsVerification::CustomRoots(_)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn certificate_cache_is_bounded_to_eight_effective_sources() {
        let root = std::env::temp_dir().join(format!(
            "zmin-lfs-http-ca-cache-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::create_dir_all(&root).expect("temp root");
        let mut entries = Vec::new();
        for index in 0..=MAX_CERTIFICATE_CACHE_ENTRIES {
            let path = root.join(format!("root-{index}.pem"));
            fs::write(&path, ROOT_PEM).expect("root CA");
            entries.push(entry(
                &format!("http.https://host-{index}.example.sslcainfo"),
                &path.to_string_lossy(),
            ));
        }
        let policy = LfsHttpPolicy::from_entries(&entries, LfsHttpEnvironmentSnapshot::default())
            .expect("certificate policy");
        for index in 0..=MAX_CERTIFICATE_CACHE_ENTRIES {
            assert!(matches!(
                resolve_tls(&policy, &format!("https://host-{index}.example/repo"))
                    .expect("root certificate"),
                TlsVerification::CustomRoots(_)
            ));
        }
        assert_eq!(
            policy.certificate_cache.entry_count(),
            MAX_CERTIFICATE_CACHE_ENTRIES
        );
        let debug = format!("{policy:?}");
        assert!(!debug.contains("root-0.pem"));
        assert!(!debug.contains(&root.to_string_lossy().into_owned()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn client_tls_keys_are_rejected_when_effective() {
        for (key, value) in [
            ("http.sslcert", "/private/client.pem"),
            ("http.sslkey", "/private/client.key"),
        ] {
            assert_eq!(
                LfsHttpPolicy::from_entries(
                    &[entry(key, value)],
                    LfsHttpEnvironmentSnapshot::default(),
                )
                .expect_err("unsupported TLS policy"),
                LfsHttpPolicyError::UnsupportedTlsConfiguration,
                "{key}"
            );
        }
        for entries in [vec![
            entry("http.sslcert", "/private/lower.pem"),
            entry("http.sslcert", ""),
        ]] {
            LfsHttpPolicy::from_entries(&entries, LfsHttpEnvironmentSnapshot::default())
                .expect("effective no-op");
        }
        LfsHttpPolicy::from_entries(
            &[
                entry("http.https://example.com.sslcert", "/private/lower.pem"),
                entry("http.https://example.com:443.sslcert", ""),
            ],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("normalized equivalent scope clears lower value");
    }

    #[test]
    fn schannel_ca_compatibility_matches_git_lfs_v3_7_1() {
        let root = std::env::temp_dir().join(format!(
            "zmin-lfs-http-schannel-ca-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::create_dir_all(&root).expect("temp root");
        let ca_info = root.join("selected.pem");
        fs::write(&ca_info, ROOT_PEM).expect("root CA");
        let missing = root.join("must-not-open.pem");

        for use_ca_info in [None, Some("false"), Some("invalid"), Some("")] {
            let mut entries = vec![
                entry("http.sslbackend", "schannel"),
                entry("http.sslcainfo", &missing.to_string_lossy()),
            ];
            if let Some(value) = use_ca_info {
                entries.push(entry("http.schannelusesslcainfo", value));
            }
            let policy =
                LfsHttpPolicy::from_entries(&entries, LfsHttpEnvironmentSnapshot::default())
                    .expect("schannel compatibility policy");
            assert_eq!(
                resolve_tls(&policy, "https://example.com/repo").expect("platform roots"),
                TlsVerification::Platform,
                "{use_ca_info:?}"
            );
            assert_eq!(policy.certificate_cache.entry_count(), 0);
        }

        let enabled = LfsHttpPolicy::from_entries(
            &[
                entry("http.sslbackend", "schannel"),
                entry("http.schannelusesslcainfo", "true"),
                entry("http.sslcainfo", &ca_info.to_string_lossy()),
            ],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("schannel CA opt-in");
        assert!(matches!(
            resolve_tls(&enabled, "https://example.com/repo").expect("custom roots"),
            TlsVerification::CustomRoots(_)
        ));

        for backend in ["openssl", "SCHANNEL", ""] {
            let policy = LfsHttpPolicy::from_entries(
                &[
                    entry("http.sslbackend", backend),
                    entry("http.sslcainfo", &ca_info.to_string_lossy()),
                ],
                LfsHttpEnvironmentSnapshot::default(),
            )
            .expect("non-schannel backend compatibility");
            assert!(matches!(
                resolve_tls(&policy, "https://example.com/repo").expect("custom roots"),
                TlsVerification::CustomRoots(_)
            ));
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn schannel_ca_compatibility_uses_host_root_scope_and_honors_ports() {
        let root = std::env::temp_dir().join(format!(
            "zmin-lfs-http-schannel-scope-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::create_dir_all(&root).expect("temp root");
        let ca_info = root.join("selected.pem");
        fs::write(&ca_info, ROOT_PEM).expect("root CA");
        let path_ignored = LfsHttpPolicy::from_entries(
            &[
                entry("http.sslbackend", "openssl"),
                entry("http.schannelusesslcainfo", "false"),
                entry("http.https://windows.example.sslbackend", "schannel"),
                entry(
                    "http.https://windows.example/allowed.schannelusesslcainfo",
                    "true",
                ),
                entry("http.sslcainfo", &ca_info.to_string_lossy()),
            ],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("scoped schannel policy");
        assert_eq!(
            resolve_tls(&path_ignored, "https://windows.example/blocked").expect("platform roots"),
            TlsVerification::Platform
        );
        assert_eq!(
            resolve_tls(&path_ignored, "https://windows.example/allowed/object")
                .expect("path-specific selector ignored"),
            TlsVerification::Platform
        );
        assert!(matches!(
            resolve_tls(&path_ignored, "https://unix.example/repo").expect("custom roots"),
            TlsVerification::CustomRoots(_)
        ));

        let missing = root.join("path-specific-must-not-open.pem");
        let port_scoped = LfsHttpPolicy::from_entries(
            &[
                entry("http.https://windows.example:8443.sslbackend", "schannel"),
                entry(
                    "http.https://windows.example:8443.schannelusesslcainfo",
                    "true",
                ),
                entry(
                    "http.https://windows.example:8443.sslcainfo",
                    &ca_info.to_string_lossy(),
                ),
                entry(
                    "http.https://windows.example:8443/private.sslcainfo",
                    &missing.to_string_lossy(),
                ),
                entry(
                    "http.https://private-user@windows.example:8443.sslcainfo",
                    &missing.to_string_lossy(),
                ),
            ],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("port-scoped schannel policy");
        assert!(matches!(
            resolve_tls(&port_scoped, "https://windows.example:8443/private/object")
                .expect("host-port roots"),
            TlsVerification::CustomRoots(_)
        ));
        assert_eq!(
            resolve_tls(&port_scoped, "https://windows.example/private/object")
                .expect("different effective port"),
            TlsVerification::Platform
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn windows_certificate_sources_reject_device_names_and_namespaces() {
        for path in [
            r"\\.\pipe\private",
            r"\\?\C:\private\root.pem",
            r"\\server\pipe\private",
            r"\Device\NamedPipe\private",
            r"C:\private\NUL.pem",
            r"C:\private\com1",
            r"C:\private\root.pem. ",
        ] {
            assert_eq!(
                CertificateSource::new(PathBuf::from(path), CertificateSourceKind::File)
                    .expect_err("device path"),
                LfsHttpPolicyError::InvalidCertificateSource,
                "{path}"
            );
        }
    }

    #[test]
    fn independent_http_policy_rule_bound_fails_before_parsing_values() {
        let entries = (0..=MAX_HTTP_POLICY_RULES)
            .map(|_| entry("http.extraheader", "not a parsed header"))
            .collect::<Vec<_>>();
        assert_eq!(
            LfsHttpPolicy::from_entries(&entries, LfsHttpEnvironmentSnapshot::default())
                .expect_err("rule bound"),
            LfsHttpPolicyError::PolicyTooLarge
        );
    }

    #[test]
    fn environment_ca_info_replaces_roots_without_opening_lower_precedence_sources() {
        let root = std::env::temp_dir().join(format!(
            "zmin-lfs-http-ca-precedence-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::create_dir_all(&root).expect("temp root");
        let ca_info = root.join("selected.pem");
        fs::write(&ca_info, ROOT_PEM).expect("root CA");
        let missing_ca_path = root.join("must-not-open-directory");
        let environment = LfsHttpEnvironmentSnapshot::default().with_tls(
            Some(ca_info),
            Some(missing_ca_path.clone()),
            None,
        );
        let policy = LfsHttpPolicy::from_entries(
            &[
                entry(
                    "http.sslcainfo",
                    &root.join("must-not-open-file").to_string_lossy(),
                ),
                entry("http.sslcapath", &missing_ca_path.to_string_lossy()),
            ],
            environment,
        )
        .expect("higher-precedence environment CA info");
        assert!(matches!(
            resolve(&policy, "https://example.com/repo").connection.tls,
            TlsVerification::CustomRoots(_)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn config_ca_info_precedes_environment_and_global_ca_paths() {
        let root = std::env::temp_dir().join(format!(
            "zmin-lfs-http-config-ca-precedence-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::create_dir_all(&root).expect("temp root");
        let ca_info = root.join("selected.pem");
        fs::write(&ca_info, ROOT_PEM).expect("root CA");
        let missing_ca_path = root.join("must-not-open-directory");
        let environment = LfsHttpEnvironmentSnapshot::default().with_tls(
            None,
            Some(missing_ca_path.clone()),
            None,
        );
        let policy = LfsHttpPolicy::from_entries(
            &[
                entry("http.sslcainfo", &ca_info.to_string_lossy()),
                entry("http.sslcapath", &missing_ca_path.to_string_lossy()),
                entry(
                    "http.https://example.com.sslcapath",
                    &root.join("ignored-url-scoped-path").to_string_lossy(),
                ),
            ],
            environment,
        )
        .expect("higher-precedence config CA info");
        assert!(matches!(
            resolve(&policy, "https://example.com/repo").connection.tls,
            TlsVerification::CustomRoots(_)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn keepalive_defaults_invalid_and_nonpositive_but_keeps_positive_exactly() {
        for value in [None, Some(""), Some("invalid"), Some("0"), Some("-1")] {
            let entries = value
                .map(|value| vec![entry("lfs.keepalive", value)])
                .unwrap_or_default();
            let policy =
                LfsHttpPolicy::from_entries(&entries, LfsHttpEnvironmentSnapshot::default())
                    .expect("policy");
            assert_eq!(
                resolve(&policy, "https://example.com/repo")
                    .connection
                    .tcp_keepalive,
                TcpKeepalivePolicy::Enabled(Duration::from_secs(1_800))
            );
        }
        let policy = LfsHttpPolicy::from_entries(
            &[entry("lfs.keepalive", "17")],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("policy");
        assert_eq!(
            resolve(&policy, "https://example.com/repo")
                .connection
                .tcp_keepalive,
            TcpKeepalivePolicy::Enabled(Duration::from_secs(17))
        );
    }
}
