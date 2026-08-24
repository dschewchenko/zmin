//! Bounded, streaming HTTP(S) transport primitives.
//!
//! The transport owns request retries and redirects, but deliberately knows
//! nothing about Git LFS, JSON, or the object store.  Request bodies are
//! supplied by a reopenable factory, so retries and 307/308 redirects never
//! buffer upload data in memory or spill it to a temporary file.

mod activity_io;
mod connector;
mod policy;
mod runtime;

pub use policy::{
    ConfiguredRequestHeaderProvenance, ConfiguredRequestHeaders, HttpConnectionPolicy,
    HttpTimeoutPolicy, ProxyPolicy, ProxyTlsPolicy, ProxyUrl, RequestPolicyResolver,
    ResolvedRequestPolicy, TcpKeepalivePolicy, TlsClientIdentity, TlsRootCertificate,
    TlsRootCertificates, TlsVerification,
};

use std::collections::{HashSet, VecDeque};
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use http::header::{HeaderMap, HeaderName, HeaderValue, LOCATION, RETRY_AFTER, USER_AGENT};
use http::{Method, Request};
use url::Url;

use connector::ExactConnector;
use runtime::{PreparedRequestBody, RuntimeBody, RuntimeBridge, RuntimeClient, RuntimeResponse};

const MAX_URL_BYTES: usize = 8 * 1024;
const MAX_REDIRECTS: usize = 10;
const DEFAULT_MAX_REQUEST_HEADERS: usize = 128;
const DEFAULT_MAX_REQUEST_HEADER_BYTES: usize = 64 * 1024;
const DEFAULT_MAX_RESPONSE_HEADERS: usize = 256;
const DEFAULT_MAX_RESPONSE_HEADER_BYTES: usize = 128 * 1024;
const DEFAULT_MAX_RESPONSE_BODY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CLIENT_CACHE_ENTRIES: usize = 8;
const BODY_BRIDGE_CHUNK_BYTES: usize = 64 * 1024;
const RESPONSE_BODY_BRIDGE_QUEUE_DEPTH: usize = 1;
const REQUEST_BODY_BRIDGE_QUEUE_DEPTH: usize = 2;
const HTTP1_READ_BUFFER_FRAMING_BYTES: usize = 8 * 1024;
const HTTP1_MIN_READ_BUFFER_BYTES: usize = 8 * 1024;
const HTTP1_MAX_READ_BUFFER_BYTES: usize = 128 * 1024;

/// A strict HTTP(S) URL retained in its original serialized form.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct HttpUrl {
    raw: String,
    parsed: Url,
    origin: HttpOrigin,
}

impl fmt::Debug for HttpUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("HttpUrl")
            .field(&"<redacted>")
            .finish()
    }
}

impl HttpUrl {
    /// Parse an HTTP(S) URL without accepting credentials, fragments, or
    /// encoded path routing delimiters.
    pub fn parse(raw: &str) -> Result<Self, TransportError> {
        validate_url_text(raw)?;
        if raw.len() > MAX_URL_BYTES {
            return Err(TransportError::InvalidUrl);
        }
        let parsed = Url::parse(raw).map_err(|_| TransportError::InvalidUrl)?;
        Self::from_parsed(raw.to_owned(), parsed)
    }

    fn from_parsed(raw: String, parsed: Url) -> Result<Self, TransportError> {
        if raw.len() > MAX_URL_BYTES
            || !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
        {
            return Err(TransportError::InvalidUrl);
        }
        let origin = HttpOrigin::from_url(&parsed)?;
        Ok(Self {
            raw,
            parsed,
            origin,
        })
    }

    /// The original URL bytes supplied by the caller or Location header.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn origin(&self) -> &HttpOrigin {
        &self.origin
    }

    fn resolve(&self, location: &str) -> Result<Self, TransportError> {
        if location.len() > MAX_URL_BYTES || has_control_or_space(location) {
            return Err(TransportError::InvalidRedirect);
        }
        validate_redirect_text(location)?;
        let parsed = self
            .parsed
            .join(location)
            .map_err(|_| TransportError::InvalidRedirect)?;
        let raw = parsed.as_str().to_owned();
        if raw.len() > MAX_URL_BYTES {
            return Err(TransportError::InvalidRedirect);
        }
        Self::from_parsed(raw, parsed).map_err(|_| TransportError::InvalidRedirect)
    }
}

/// The scheme/host/effective-port tuple used for credential and identity
/// isolation across redirects.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct HttpOrigin {
    scheme: String,
    host: String,
    port: u16,
}

impl HttpOrigin {
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    fn from_url(url: &Url) -> Result<Self, TransportError> {
        let host = url.host_str().ok_or(TransportError::InvalidUrl)?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let port = url
            .port_or_known_default()
            .ok_or(TransportError::InvalidUrl)?;
        Ok(Self {
            scheme: url.scheme().to_ascii_lowercase(),
            host: host.to_ascii_lowercase(),
            port,
        })
    }
}

/// A bounded request header.
///
/// `new` stores a public value, while [`Self::new_secret`] stores an
/// application-owned value that is wiped when each owned copy is dropped.
/// Values stay in byte storage until [`HttpClient`] is preparing one send;
/// only then are they converted into a third-party `HeaderValue`.  The
/// transport cannot promise wiping copies retained by Hyper after
/// that boundary.
#[derive(Clone, PartialEq, Eq)]
pub struct RequestHeader {
    name: HeaderName,
    value: RequestHeaderValue,
}

#[derive(Clone, PartialEq, Eq)]
enum RequestHeaderValue {
    Public(Vec<u8>),
    Secret(SecretHeaderValue),
}

#[derive(Clone, PartialEq, Eq)]
struct SecretHeaderValue(Vec<u8>);

impl SecretHeaderValue {
    fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    fn wipe(&mut self) {
        self.0.fill(0);
    }
}

impl Drop for SecretHeaderValue {
    fn drop(&mut self) {
        self.wipe();
    }
}

impl RequestHeaderValue {
    fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Public(value) => value,
            Self::Secret(value) => value.as_bytes(),
        }
    }
}

impl fmt::Debug for RequestHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestHeader")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .finish()
    }
}

impl RequestHeader {
    /// Construct a request header whose value is public application data.
    pub fn new(name: &str, value: &[u8]) -> Result<Self, TransportError> {
        if name.len() > 256 || value.len() > 16 * 1024 {
            return Err(TransportError::HeaderTooLarge);
        }
        let name =
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| TransportError::InvalidHeader)?;
        if is_forbidden_request_header(&name) {
            return Err(TransportError::InvalidHeader);
        }
        validate_request_header_value(value)?;
        Ok(Self {
            name,
            value: RequestHeaderValue::Public(value.to_vec()),
        })
    }

    /// Construct a request header whose value is owned and wiped on drop.
    ///
    /// On validation failure the supplied bytes are wiped before returning.
    pub fn new_secret(name: &str, mut value: Vec<u8>) -> Result<Self, TransportError> {
        if name.len() > 256 || value.len() > 16 * 1024 {
            value.fill(0);
            return Err(TransportError::HeaderTooLarge);
        }
        let name = match HeaderName::from_bytes(name.as_bytes()) {
            Ok(name) => name,
            Err(_) => {
                value.fill(0);
                return Err(TransportError::InvalidHeader);
            }
        };
        if is_forbidden_request_header(&name) {
            value.fill(0);
            return Err(TransportError::InvalidHeader);
        }
        if let Err(error) = validate_request_header_value(&value) {
            value.fill(0);
            return Err(error);
        }
        Ok(Self {
            name,
            value: RequestHeaderValue::Secret(SecretHeaderValue(value)),
        })
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    fn value(&self) -> &[u8] {
        self.value.as_bytes()
    }

    /// Convert into Hyper storage only while preparing the current send.
    /// The returned value may be copied or retained by Hyper.
    fn header_value(&self) -> Result<HeaderValue, TransportError> {
        HeaderValue::from_bytes(self.value()).map_err(|_| TransportError::InvalidHeader)
    }
}

/// A bounded ordered collection of request headers.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct RequestHeaders {
    entries: Vec<RequestHeader>,
    bytes: usize,
}

impl fmt::Debug for RequestHeaders {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestHeaders")
            .field("count", &self.entries.len())
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl RequestHeaders {
    pub fn new(entries: Vec<RequestHeader>) -> Result<Self, TransportError> {
        if entries.len() > DEFAULT_MAX_REQUEST_HEADERS {
            return Err(TransportError::TooManyHeaders);
        }
        let bytes = entries.iter().try_fold(0_usize, |total, entry| {
            total
                .checked_add(entry.name.as_str().len())
                .and_then(|total| total.checked_add(entry.value().len()))
                .ok_or(TransportError::HeaderTooLarge)
        })?;
        if bytes > DEFAULT_MAX_REQUEST_HEADER_BYTES {
            return Err(TransportError::HeaderTooLarge);
        }
        Ok(Self { entries, bytes })
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn push(&mut self, header: RequestHeader) -> Result<(), TransportError> {
        if self.entries.len() >= DEFAULT_MAX_REQUEST_HEADERS {
            return Err(TransportError::TooManyHeaders);
        }
        let next = self
            .bytes
            .checked_add(header.name.as_str().len())
            .and_then(|total| total.checked_add(header.value().len()))
            .ok_or(TransportError::HeaderTooLarge)?;
        if next > DEFAULT_MAX_REQUEST_HEADER_BYTES {
            return Err(TransportError::HeaderTooLarge);
        }
        self.bytes = next;
        self.entries.push(header);
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = &RequestHeader> {
        self.entries.iter()
    }

    fn for_cross_origin_redirect(&self, policy: &RedirectPolicy) -> Self {
        let entries = self
            .entries
            .iter()
            .filter(|entry| {
                matches!(entry.value, RequestHeaderValue::Public(_))
                    && policy.allows_header(&entry.name)
            })
            .cloned()
            .collect();
        Self::new(entries).expect("filtering headers cannot exceed the original bounds")
    }
}

/// Header state carried between attempts and redirect hops. Caller headers
/// and configured headers stay separate so target configuration replaces the
/// previous configured list rather than accumulating it.
struct PendingHeaders {
    caller: RequestHeaders,
    configured: ConfiguredRequestHeaders,
}

impl PendingHeaders {
    fn new(
        caller: RequestHeaders,
        configured: ConfiguredRequestHeaders,
    ) -> Result<Self, TransportError> {
        validate_pending_headers(&caller, &configured)?;
        Ok(Self { caller, configured })
    }

    fn for_redirect(
        &self,
        configured: ConfiguredRequestHeaders,
        cross_origin: bool,
        cross_origin_seen: bool,
        policy: &RedirectPolicy,
    ) -> Result<Self, TransportError> {
        let caller = if cross_origin {
            self.caller.for_cross_origin_redirect(policy)
        } else {
            self.caller.clone()
        };
        let configured = if cross_origin_seen {
            configured.for_cross_origin()
        } else {
            configured
        };
        Self::new(caller, configured)
    }

    fn iter(&self) -> impl Iterator<Item = &RequestHeader> {
        self.caller.iter().chain(self.configured.headers().iter())
    }
}

fn validate_pending_headers(
    caller: &RequestHeaders,
    configured: &ConfiguredRequestHeaders,
) -> Result<(), TransportError> {
    let configured = configured.headers();
    let count = caller
        .entries
        .len()
        .checked_add(configured.entries.len())
        .ok_or(TransportError::TooManyHeaders)?;
    if count > DEFAULT_MAX_REQUEST_HEADERS {
        return Err(TransportError::TooManyHeaders);
    }
    let bytes = caller
        .bytes
        .checked_add(configured.bytes)
        .ok_or(TransportError::HeaderTooLarge)?;
    if bytes > DEFAULT_MAX_REQUEST_HEADER_BYTES {
        return Err(TransportError::HeaderTooLarge);
    }
    if caller.entries.iter().any(|caller| {
        configured
            .entries
            .iter()
            .any(|configured| caller.name == configured.name)
    }) {
        return Err(TransportError::HeaderCollision);
    }
    Ok(())
}

/// A bounded typed request body. Only owned bytes and an already-opened,
/// same-handle verified regular file are accepted; arbitrary blocking readers
/// are deliberately excluded so cancellation and runtime shutdown stay
/// enforceable.
#[derive(Clone)]
pub struct RequestBodyFactory {
    source: Arc<RequestBodySourceFactory>,
    replayable: bool,
    opened: Arc<AtomicBool>,
}

impl fmt::Debug for RequestBodyFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RequestBodyFactory(<reopenable>)")
    }
}

enum RequestBodySourceFactory {
    Bytes(Arc<[u8]>),
    VerifiedRegularFile {
        length: u64,
        expected_sha256: [u8; 32],
        cancellation: RequestBodyCancellation,
        open: Arc<dyn Fn() -> io::Result<File> + Send + Sync>,
    },
}

pub(crate) enum OpenRequestBody {
    Bytes(Arc<[u8]>),
    VerifiedRegularFile {
        file: File,
        length: u64,
        expected_sha256: [u8; 32],
        cancellation: RequestBodyCancellation,
    },
}

/// Cooperative cancellation for an entire HTTP request, including dialing,
/// upload verification, waiting for response headers, and streaming the
/// response body. The same token may also be attached to a verified file body
/// so positional reads and channel handoffs stop with the request.
#[derive(Clone)]
pub struct RequestBodyCancellation(Arc<RequestCancellationState>);

struct RequestCancellationState {
    cancelled: AtomicBool,
    notification: tokio::sync::watch::Sender<bool>,
}

impl Default for RequestBodyCancellation {
    fn default() -> Self {
        let (notification, _) = tokio::sync::watch::channel(false);
        Self(Arc::new(RequestCancellationState {
            cancelled: AtomicBool::new(false),
            notification,
        }))
    }
}

impl fmt::Debug for RequestBodyCancellation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestBodyCancellation")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl RequestBodyCancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        if !self.0.cancelled.swap(true, Ordering::AcqRel) {
            self.0.notification.send_replace(true);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    pub(crate) async fn cancelled(&self) {
        let mut receiver = self.0.notification.subscribe();
        if *receiver.borrow() {
            return;
        }
        while receiver.changed().await.is_ok() {
            if *receiver.borrow() {
                return;
            }
        }
    }
}

impl RequestBodyFactory {
    pub fn bytes(bytes: Vec<u8>) -> Self {
        Self {
            source: Arc::new(RequestBodySourceFactory::Bytes(Arc::from(bytes))),
            replayable: true,
            opened: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn single_use_bytes(bytes: Vec<u8>) -> Self {
        Self {
            source: Arc::new(RequestBodySourceFactory::Bytes(Arc::from(bytes))),
            replayable: false,
            opened: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn verified_regular_file<F>(
        length: u64,
        expected_sha256: [u8; 32],
        cancellation: RequestBodyCancellation,
        open: F,
    ) -> Self
    where
        F: Fn() -> io::Result<File> + Send + Sync + 'static,
    {
        Self {
            source: Arc::new(RequestBodySourceFactory::VerifiedRegularFile {
                length,
                expected_sha256,
                cancellation,
                open: Arc::new(open),
            }),
            replayable: true,
            opened: Arc::new(AtomicBool::new(false)),
        }
    }

    fn open(&self) -> Result<OpenRequestBody, TransportError> {
        if !self.replayable && self.opened.swap(true, Ordering::AcqRel) {
            return Err(TransportError::ReplayRequired);
        }
        match self.source.as_ref() {
            RequestBodySourceFactory::Bytes(bytes) => Ok(OpenRequestBody::Bytes(Arc::clone(bytes))),
            RequestBodySourceFactory::VerifiedRegularFile {
                length,
                expected_sha256,
                cancellation,
                open,
            } => {
                let file = open().map_err(|error| TransportError::BodyFactory(error.kind()))?;
                let metadata = file
                    .metadata()
                    .map_err(|error| TransportError::BodyFactory(error.kind()))?;
                if !metadata.is_file() || metadata.len() != *length {
                    return Err(TransportError::BodyFactory(io::ErrorKind::InvalidData));
                }
                Ok(OpenRequestBody::VerifiedRegularFile {
                    file,
                    length: *length,
                    expected_sha256: *expected_sha256,
                    cancellation: cancellation.clone(),
                })
            }
        }
    }

    fn is_replayable(&self) -> bool {
        self.replayable
    }
}

/// A request submitted to [`HttpClient`].
#[derive(Clone, Debug)]
pub struct HttpRequest {
    method: Method,
    url: HttpUrl,
    headers: RequestHeaders,
    body: Option<RequestBodyFactory>,
    cancellation: Option<RequestBodyCancellation>,
}

impl HttpRequest {
    pub fn new(method: Method, url: HttpUrl) -> Self {
        Self {
            method,
            url,
            headers: RequestHeaders::empty(),
            body: None,
            cancellation: None,
        }
    }

    pub fn method(&self) -> &Method {
        &self.method
    }

    pub fn url(&self) -> &HttpUrl {
        &self.url
    }

    pub fn headers(&self) -> &RequestHeaders {
        &self.headers
    }

    pub fn with_headers(mut self, headers: RequestHeaders) -> Self {
        self.headers = headers;
        self
    }

    pub fn with_body(mut self, body: RequestBodyFactory) -> Self {
        self.body = Some(body);
        self
    }

    pub fn with_cancellation(mut self, cancellation: RequestBodyCancellation) -> Self {
        self.cancellation = Some(cancellation);
        self
    }
}

/// Controls whether a redirect may replay a request body to another origin.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RedirectBodyPolicy {
    /// Bodies may only be replayed to the current request origin.
    #[default]
    SameOriginOnly,
    /// Bodies may also be replayed to the explicitly listed origins.
    AllowOrigins(Vec<HttpOrigin>),
}

impl RedirectBodyPolicy {
    pub fn allow_origins<I>(origins: I) -> Self
    where
        I: IntoIterator<Item = HttpOrigin>,
    {
        Self::AllowOrigins(origins.into_iter().collect())
    }

    fn allows(&self, current: &HttpOrigin, next: &HttpOrigin) -> bool {
        current == next
            || match self {
                Self::SameOriginOnly => false,
                Self::AllowOrigins(origins) => origins.iter().any(|origin| origin == next),
            }
    }
}

/// Redirect header/body policy. Cross-origin redirects forward no caller
/// headers by default; only the small standard safe-header allowlist can be
/// explicitly enabled. Credential and signed/action headers can never be
/// enabled through this API.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RedirectPolicy {
    pub body: RedirectBodyPolicy,
    safe_headers: Vec<HeaderName>,
}

impl RedirectPolicy {
    pub fn allow_cross_origin_header(mut self, name: &str) -> Result<Self, TransportError> {
        let name =
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| TransportError::InvalidHeader)?;
        if !is_safe_redirect_header(&name) {
            return Err(TransportError::InvalidHeader);
        }
        if !self.safe_headers.iter().any(|current| current == &name) {
            self.safe_headers.push(name);
        }
        Ok(self)
    }

    fn allows_header(&self, name: &HeaderName) -> bool {
        self.safe_headers.iter().any(|current| current == name)
    }
}

/// Blocking client configuration. The default uses rustls with platform root
/// verification and the system proxy environment.
#[derive(Clone)]
pub struct ClientConfig {
    pub timeout_policy: HttpTimeoutPolicy,
    pub request_timeout: Option<Duration>,
    pub operation_timeout: Option<Duration>,
    pub pool_idle_timeout: Duration,
    pub connection_policy: HttpConnectionPolicy,
    pub user_agent: Option<String>,
    pub max_response_headers: usize,
    pub max_response_header_bytes: usize,
    pub max_response_body_bytes: u64,
    pub redirect_policy: RedirectPolicy,
    pub retry: RetryPolicy,
}

impl fmt::Debug for ClientConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientConfig")
            .field("timeout_policy", &self.timeout_policy)
            .field("request_timeout", &self.request_timeout)
            .field("operation_timeout", &self.operation_timeout)
            .field("pool_idle_timeout", &self.pool_idle_timeout)
            .field("connection_policy", &self.connection_policy)
            .field("user_agent", &self.user_agent)
            .field("max_response_headers", &self.max_response_headers)
            .field("max_response_header_bytes", &self.max_response_header_bytes)
            .field("max_response_body_bytes", &self.max_response_body_bytes)
            .field("redirect_policy", &self.redirect_policy)
            .field("retry", &self.retry)
            .finish()
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            timeout_policy: HttpTimeoutPolicy::default(),
            request_timeout: Some(Duration::from_secs(120)),
            operation_timeout: Some(Duration::from_secs(120)),
            pool_idle_timeout: Duration::from_secs(90),
            connection_policy: HttpConnectionPolicy::default(),
            user_agent: Some(format!("zmin-http-transport/{}", env!("CARGO_PKG_VERSION"))),
            max_response_headers: DEFAULT_MAX_RESPONSE_HEADERS,
            max_response_header_bytes: DEFAULT_MAX_RESPONSE_HEADER_BYTES,
            max_response_body_bytes: DEFAULT_MAX_RESPONSE_BODY_BYTES,
            redirect_policy: RedirectPolicy::default(),
            retry: RetryPolicy::default(),
        }
    }
}

/// Deterministic bounded retry policy. No jitter is used so callers can
/// reproduce timing and tests can set all delays to zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: usize,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub allow_replayable_writes: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(10),
            allow_replayable_writes: false,
        }
    }
}

/// Reusable blocking HTTP client with explicit redirect/retry handling.
pub struct HttpClient {
    config: ClientConfig,
    policy_resolver: Arc<dyn RequestPolicyResolver>,
    runtime: RuntimeBridge,
    clients: Mutex<ClientCache>,
}

impl fmt::Debug for HttpClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpClient")
            .field("config", &self.config)
            .field("cached_connection_policies", &self.cached_client_count())
            .finish()
    }
}

impl HttpClient {
    pub fn new(config: ClientConfig) -> Result<Self, TransportError> {
        let policy = config.connection_policy.clone();
        let timeouts = config.timeout_policy;
        Self::from_parts(
            config,
            Arc::new(policy::StaticRequestPolicyResolver::new(policy, timeouts)?),
            true,
        )
    }

    /// Build a client whose connection policy and configured headers are
    /// resolved from each actual request URL. In this mode
    /// [`ClientConfig::connection_policy`] is not consulted; the resolver
    /// returns the complete effective connection policy.
    pub fn with_policy_resolver(
        config: ClientConfig,
        policy_resolver: Arc<dyn RequestPolicyResolver>,
    ) -> Result<Self, TransportError> {
        Self::from_parts(config, policy_resolver, false)
    }

    fn from_parts(
        config: ClientConfig,
        policy_resolver: Arc<dyn RequestPolicyResolver>,
        prime_static_policy: bool,
    ) -> Result<Self, TransportError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        config.timeout_policy.validate()?;
        validate_optional_timeout(config.request_timeout)?;
        validate_optional_timeout(config.operation_timeout)?;
        let runtime = RuntimeBridge::new()?;
        let client = Self {
            config,
            policy_resolver,
            runtime,
            clients: Mutex::new(ClientCache::default()),
        };
        if prime_static_policy {
            let mut validation_policy = client.config.connection_policy.clone();
            if matches!(validation_policy.proxy, ProxyPolicy::System) {
                // System selection is snapshotted and resolved to an explicit
                // route or disabled policy only when an actual URL is known.
                validation_policy.proxy = ProxyPolicy::Disabled;
            }
            let _ = build_client_pair(
                &client.config,
                &validation_policy,
                &ProxyTlsPolicy::default(),
                client.config.timeout_policy,
            )?;
        }
        Ok(client)
    }

    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    pub fn execute(&self, request: &HttpRequest) -> Result<StreamingResponse, TransportError> {
        ensure_not_cancelled(request.cancellation.as_ref())?;
        let started = Instant::now();
        let deadline = self
            .config
            .operation_timeout
            .map(|timeout| {
                started
                    .checked_add(timeout)
                    .ok_or(TransportError::DeadlineExceeded)
            })
            .transpose()?;
        let mut url = request.url.clone();
        let mut method = request.method.clone();
        let initial_policy = self.policy_resolver.resolve(&url)?;
        let mut headers =
            PendingHeaders::new(request.headers.clone(), initial_policy.configured_headers)?;
        let mut connection = initial_policy.connection;
        let mut timeouts = initial_policy.timeouts;
        let mut proxy_tls_policy = initial_policy.proxy_tls_policy;
        let mut body = request.body.clone();
        let mut identity_allowed = true;
        let mut cross_origin_seen = false;
        let mut redirects = 0_usize;
        let mut visited = HashSet::new();
        visited.insert(url.as_str().to_owned());

        loop {
            let mut attempt = 0_usize;
            loop {
                attempt += 1;
                let response = match self.send_once(
                    &method,
                    &url,
                    &headers,
                    body.as_ref(),
                    &connection,
                    &proxy_tls_policy,
                    timeouts,
                    identity_allowed,
                    deadline,
                    request.cancellation.as_ref(),
                ) {
                    Ok(response) => response,
                    Err(error) => {
                        if !is_retryable_error(&error)
                            || !self.retry_allowed(
                                attempt,
                                &method,
                                body.as_ref(),
                                started,
                                deadline,
                            )
                        {
                            return Err(error);
                        }
                        self.sleep_before_retry(
                            attempt,
                            None,
                            deadline,
                            request.cancellation.as_ref(),
                        )?;
                        continue;
                    }
                };

                if is_redirect(response.head.status) {
                    let Some(location) = response.head.headers.location()? else {
                        return Ok(response);
                    };
                    if redirects >= MAX_REDIRECTS {
                        return Err(TransportError::RedirectLimit);
                    }
                    let next_url = url.resolve(location)?;
                    if !visited.insert(next_url.as_str().to_owned()) {
                        return Err(TransportError::RedirectLoop);
                    }
                    let cross_origin = next_url.origin != url.origin;
                    let previous_body = body.is_some();
                    redirect_method_and_body(response.head.status, &mut method, &mut body);
                    if cross_origin
                        && previous_body
                        && body.is_some()
                        && !self
                            .config
                            .redirect_policy
                            .body
                            .allows(&url.origin, &next_url.origin)
                    {
                        return Err(TransportError::RedirectBodyNotAllowed);
                    }
                    let target_policy = self.policy_resolver.resolve(&next_url)?;
                    cross_origin_seen |= cross_origin;
                    headers = headers.for_redirect(
                        target_policy.configured_headers,
                        cross_origin,
                        cross_origin_seen,
                        &self.config.redirect_policy,
                    )?;
                    connection = target_policy.connection;
                    timeouts = target_policy.timeouts;
                    proxy_tls_policy = target_policy.proxy_tls_policy;
                    if cross_origin {
                        identity_allowed = false;
                    }
                    url = next_url;
                    redirects += 1;
                    break;
                }

                if is_retryable_status(response.head.status)
                    && self.retry_allowed(attempt, &method, body.as_ref(), started, deadline)
                {
                    let retry_after = response.head.headers.retry_after();
                    drop(response);
                    self.sleep_before_retry(
                        attempt,
                        retry_after,
                        deadline,
                        request.cancellation.as_ref(),
                    )?;
                    continue;
                }
                return Ok(response);
            }
        }
    }

    fn send_once(
        &self,
        method: &Method,
        url: &HttpUrl,
        headers: &PendingHeaders,
        body: Option<&RequestBodyFactory>,
        connection: &HttpConnectionPolicy,
        proxy_tls_policy: &ProxyTlsPolicy,
        timeouts: HttpTimeoutPolicy,
        identity_allowed: bool,
        operation_deadline: Option<Instant>,
        cancellation: Option<&RequestBodyCancellation>,
    ) -> Result<StreamingResponse, TransportError> {
        ensure_not_cancelled(cancellation)?;
        let clients = self.clients_for(url.origin(), connection, proxy_tls_policy, timeouts)?;
        let client = if identity_allowed {
            &clients.identity
        } else {
            &clients.no_identity
        };
        let deadline = self.request_deadline(operation_deadline)?;
        let uri = url
            .as_str()
            .parse::<http::Uri>()
            .map_err(|_| TransportError::InvalidUrl)?;
        let prepared_body = if let Some(factory) = body {
            let source = factory.open()?;
            self.runtime
                .request_body(source, deadline, cancellation.cloned())?
        } else {
            PreparedRequestBody::empty()
        };
        let upload_completion = prepared_body.completion;
        let mut request = Request::builder()
            .method(method.clone())
            .uri(uri.clone())
            .body(prepared_body.body)
            .map_err(|_| TransportError::Request(io::ErrorKind::InvalidInput))?;
        for header in headers.iter() {
            // This is the final application-owned boundary. hyper
            // may retain or clone this value, so their copies are outside the
            // transport's wiping guarantee; keep this conversion as late and
            // short-lived as the request builder allows.
            request
                .headers_mut()
                .append(header.name.clone(), header.header_value()?);
        }
        if !request.headers().contains_key(USER_AGENT) {
            if let Some(user_agent) = &self.config.user_agent {
                request.headers_mut().insert(
                    USER_AGENT,
                    HeaderValue::from_str(user_agent).map_err(|_| TransportError::InvalidHeader)?,
                );
            }
        }
        if !request
            .headers()
            .contains_key(http::header::PROXY_AUTHORIZATION)
        {
            if let Some(authorization) = client.forward_proxy_authorization(&uri) {
                request
                    .headers_mut()
                    .insert(http::header::PROXY_AUTHORIZATION, authorization);
            }
        }
        let response = self
            .runtime
            .execute(
                client.clone(),
                request,
                upload_completion,
                deadline,
                cancellation.cloned(),
            )
            .wait()?;
        StreamingResponse::new(
            response,
            url.clone(),
            self.config.max_response_headers,
            self.config.max_response_header_bytes,
            self.config.max_response_body_bytes,
            operation_deadline,
        )
    }

    fn clients_for(
        &self,
        origin: &HttpOrigin,
        policy: &HttpConnectionPolicy,
        proxy_tls_policy: &ProxyTlsPolicy,
        timeouts: HttpTimeoutPolicy,
    ) -> Result<ClientPair, TransportError> {
        let timeouts = timeouts.validate()?;
        let mut cache = self
            .clients
            .lock()
            .map_err(|_| TransportError::Request(io::ErrorKind::Other))?;
        if let Some(index) = cache.entries.iter().position(|entry| {
            &entry.origin == origin
                && &entry.policy == policy
                && &entry.proxy_tls_policy == proxy_tls_policy
                && entry.timeouts == timeouts
        }) {
            let entry = cache.entries.remove(index).expect("cache index exists");
            let clients = entry.clients.clone();
            cache.entries.push_back(entry);
            return Ok(clients);
        }

        let clients = build_client_pair(&self.config, policy, proxy_tls_policy, timeouts)?;
        if cache.entries.len() == MAX_CLIENT_CACHE_ENTRIES {
            cache.entries.pop_front();
        }
        cache.entries.push_back(CachedClientPair {
            origin: origin.clone(),
            policy: policy.clone(),
            proxy_tls_policy: proxy_tls_policy.clone(),
            timeouts,
            clients: clients.clone(),
        });
        Ok(clients)
    }

    fn cached_client_count(&self) -> usize {
        self.clients
            .lock()
            .map(|cache| cache.entries.len())
            .unwrap_or_default()
    }

    #[cfg(test)]
    fn runtime_thread_id(&self) -> std::thread::ThreadId {
        self.runtime.thread_id()
    }

    fn request_deadline(
        &self,
        operation_deadline: Option<Instant>,
    ) -> Result<Option<Instant>, TransportError> {
        if operation_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(TransportError::DeadlineExceeded);
        }
        let request_deadline = self
            .config
            .request_timeout
            .map(|timeout| {
                Instant::now()
                    .checked_add(timeout)
                    .ok_or(TransportError::InvalidTimeout)
            })
            .transpose()?;
        Ok(match (operation_deadline, request_deadline) {
            (Some(operation), Some(request)) => Some(operation.min(request)),
            (Some(operation), None) => Some(operation),
            (None, Some(request)) => Some(request),
            (None, None) => None,
        })
    }

    fn retry_allowed(
        &self,
        attempt: usize,
        method: &Method,
        body: Option<&RequestBodyFactory>,
        _started: Instant,
        deadline: Option<Instant>,
    ) -> bool {
        if attempt >= self.config.retry.max_attempts
            || !is_retryable_method(method, body, self.config.retry.allow_replayable_writes)
        {
            return false;
        }
        deadline.is_none_or(|deadline| Instant::now() < deadline)
    }

    fn sleep_before_retry(
        &self,
        attempt: usize,
        retry_after: Option<Duration>,
        deadline: Option<Instant>,
        cancellation: Option<&RequestBodyCancellation>,
    ) -> Result<(), TransportError> {
        ensure_not_cancelled(cancellation)?;
        let policy = &self.config.retry;
        let exponential = policy
            .base_delay
            .checked_mul(
                1_u32
                    .checked_shl((attempt.saturating_sub(1)).min(31) as u32)
                    .unwrap_or(u32::MAX),
            )
            .unwrap_or(policy.max_delay);
        let mut delay = retry_after.unwrap_or(exponential).min(policy.max_delay);
        if let Some(deadline) = deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(TransportError::DeadlineExceeded);
            }
            delay = delay.min(remaining);
        }
        let retry_deadline = Instant::now()
            .checked_add(delay)
            .ok_or(TransportError::DeadlineExceeded)?;
        while Instant::now() < retry_deadline {
            ensure_not_cancelled(cancellation)?;
            std::thread::park_timeout(
                retry_deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(5)),
            );
        }
        ensure_not_cancelled(cancellation)?;
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(TransportError::DeadlineExceeded);
        }
        Ok(())
    }
}

#[derive(Clone)]
struct ClientPair {
    identity: RuntimeClient,
    no_identity: RuntimeClient,
}

struct CachedClientPair {
    origin: HttpOrigin,
    policy: HttpConnectionPolicy,
    proxy_tls_policy: ProxyTlsPolicy,
    timeouts: HttpTimeoutPolicy,
    clients: ClientPair,
}

#[derive(Default)]
struct ClientCache {
    entries: VecDeque<CachedClientPair>,
}

fn build_client_pair(
    config: &ClientConfig,
    policy: &HttpConnectionPolicy,
    proxy_tls_policy: &ProxyTlsPolicy,
    timeouts: HttpTimeoutPolicy,
) -> Result<ClientPair, TransportError> {
    let identity = build_client(config, policy, proxy_tls_policy, timeouts, true)?;
    let no_identity = if policy.client_identity.is_some() {
        build_client(config, policy, proxy_tls_policy, timeouts, false)?
    } else {
        identity.clone()
    };
    Ok(ClientPair {
        identity,
        no_identity,
    })
}

fn build_client(
    config: &ClientConfig,
    policy: &HttpConnectionPolicy,
    proxy_tls_policy: &ProxyTlsPolicy,
    timeouts: HttpTimeoutPolicy,
    with_identity: bool,
) -> Result<RuntimeClient, TransportError> {
    let connector = ExactConnector::new(
        policy,
        proxy_tls_policy,
        timeouts,
        config.user_agent.as_deref(),
        with_identity,
    )?;
    Ok(RuntimeClient::new(
        connector,
        config.pool_idle_timeout,
        http1_read_buffer_bytes(config.max_response_header_bytes),
    ))
}

fn http1_read_buffer_bytes(max_response_header_bytes: usize) -> usize {
    max_response_header_bytes
        .saturating_add(HTTP1_READ_BUFFER_FRAMING_BYTES)
        .clamp(HTTP1_MIN_READ_BUFFER_BYTES, HTTP1_MAX_READ_BUFFER_BYTES)
}

#[cfg(test)]
struct ExactLengthReader<R> {
    reader: R,
    remaining: u64,
    checked_extra: bool,
}

#[cfg(test)]
impl<R> ExactLengthReader<R> {
    fn new(reader: R, length: u64) -> Self {
        Self {
            reader,
            remaining: length,
            checked_extra: false,
        }
    }
}

#[cfg(test)]
impl<R: Read> Read for ExactLengthReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            if !self.checked_extra {
                self.checked_extra = true;
                let mut extra = [0_u8; 1];
                if self.reader.read(&mut extra)? != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "request body exceeds declared length",
                    ));
                }
            }
            return Ok(0);
        }

        let read_length = if self.remaining > usize::MAX as u64 {
            buffer.len()
        } else {
            buffer.len().min(self.remaining as usize)
        };
        let amount = self.reader.read(&mut buffer[..read_length])?;
        if amount == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request body is shorter than declared length",
            ));
        }
        self.remaining -= amount as u64;
        if self.remaining == 0 {
            self.checked_extra = false;
            self.read_extra()?;
        }
        Ok(amount)
    }
}

#[cfg(test)]
impl<R: Read> ExactLengthReader<R> {
    fn read_extra(&mut self) -> io::Result<()> {
        let mut extra = [0_u8; 1];
        if self.reader.read(&mut extra)? != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request body exceeds declared length",
            ));
        }
        self.checked_extra = true;
        Ok(())
    }
}

/// Bounded response metadata plus a body streamed through the shared runtime.
pub struct StreamingResponse {
    head: ResponseHead,
    response: RuntimeBody,
    expected_length: Option<u64>,
    read: u64,
    max_body_bytes: u64,
    deadline: Option<Instant>,
}

impl fmt::Debug for StreamingResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamingResponse")
            .field("head", &self.head)
            .field("read", &self.read)
            .finish()
    }
}

impl StreamingResponse {
    fn new(
        response: RuntimeResponse,
        url: HttpUrl,
        max_headers: usize,
        max_header_bytes: usize,
        max_body_bytes: u64,
        deadline: Option<Instant>,
    ) -> Result<Self, TransportError> {
        let status = response.status;
        let headers = ResponseHeaders::from_map(&response.headers, max_headers, max_header_bytes)?;
        let expected_length = response.expected_length;
        if expected_length.is_some_and(|length| length > max_body_bytes) {
            return Err(TransportError::BodyTooLarge);
        }
        Ok(Self {
            head: ResponseHead {
                status,
                url,
                headers,
            },
            response: response.body,
            expected_length,
            read: 0,
            max_body_bytes,
            deadline,
        })
    }

    pub fn head(&self) -> &ResponseHead {
        &self.head
    }

    pub fn status(&self) -> u16 {
        self.head.status
    }
}

impl Read for StreamingResponse {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "HTTP operation deadline exceeded",
            ));
        }
        if self.read >= self.max_body_bytes {
            let mut probe = [0_u8; 1];
            let amount = self.response.read(&mut probe)?;
            return if amount == 0 {
                self.check_truncation()?;
                Ok(0)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "HTTP response body exceeds configured limit",
                ))
            };
        }
        let remaining = self.max_body_bytes - self.read;
        let read_length = if remaining > usize::MAX as u64 {
            buffer.len()
        } else {
            buffer.len().min(remaining as usize)
        };
        let read_buffer = &mut buffer[..read_length];
        let amount = match self.response.read(read_buffer) {
            Ok(amount) => amount,
            Err(error)
                if self
                    .expected_length
                    .is_some_and(|length| self.read < length)
                    && !matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                            | io::ErrorKind::ConnectionAborted
                    ) =>
            {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, error));
            }
            Err(error) => return Err(error),
        };
        if amount == 0 {
            self.check_truncation()?;
            return Ok(0);
        }
        self.read = self
            .read
            .checked_add(amount as u64)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "HTTP body size overflow"))?;
        Ok(amount)
    }
}

impl StreamingResponse {
    fn check_truncation(&self) -> io::Result<()> {
        if self
            .expected_length
            .is_some_and(|length| self.read < length)
        {
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "HTTP response body is truncated",
            ))
        } else {
            Ok(())
        }
    }
}

/// Bounded response headers, including repeated names.
#[derive(Clone, PartialEq, Eq)]
pub struct ResponseHeader {
    name: String,
    value: Vec<u8>,
}

impl fmt::Debug for ResponseHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponseHeader")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .finish()
    }
}

impl ResponseHeader {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &[u8] {
        &self.value
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct ResponseHeaders {
    entries: Vec<ResponseHeader>,
    bytes: usize,
}

impl fmt::Debug for ResponseHeaders {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponseHeaders")
            .field("count", &self.entries.len())
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl ResponseHeaders {
    fn from_map(
        headers: &HeaderMap,
        max_headers: usize,
        max_bytes: usize,
    ) -> Result<Self, TransportError> {
        if headers.len() > max_headers {
            return Err(TransportError::TooManyResponseHeaders);
        }
        let mut entries = Vec::with_capacity(headers.len());
        let mut bytes = 0_usize;
        for (name, value) in headers {
            bytes = bytes
                .checked_add(name.as_str().len())
                .and_then(|bytes| bytes.checked_add(value.as_bytes().len()))
                .ok_or(TransportError::ResponseHeadersTooLarge)?;
            if bytes > max_bytes {
                return Err(TransportError::ResponseHeadersTooLarge);
            }
            entries.push(ResponseHeader {
                name: name.as_str().to_owned(),
                value: value.as_bytes().to_vec(),
            });
        }
        Ok(Self { entries, bytes })
    }

    pub fn iter(&self) -> impl Iterator<Item = &ResponseHeader> {
        self.entries.iter()
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.value.as_slice())
    }

    fn location(&self) -> Result<Option<&str>, TransportError> {
        self.get(LOCATION.as_str())
            .map(|value| {
                std::str::from_utf8(value)
                    .map_err(|_| TransportError::InvalidRedirect)
                    .map(Some)
            })
            .unwrap_or(Ok(None))
    }

    fn retry_after(&self) -> Option<Duration> {
        parse_retry_after(self.get(RETRY_AFTER.as_str())?)
    }
}

/// Response status, final URL, and all bounded headers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResponseHead {
    status: u16,
    url: HttpUrl,
    headers: ResponseHeaders,
}

impl ResponseHead {
    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn url(&self) -> &HttpUrl {
        &self.url
    }

    pub fn headers(&self) -> &ResponseHeaders {
        &self.headers
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    InvalidUrl,
    InvalidRedirect,
    RedirectBodyNotAllowed,
    RedirectLimit,
    RedirectLoop,
    ReplayRequired,
    InvalidHeader,
    HeaderCollision,
    HeaderTooLarge,
    TooManyHeaders,
    BodyFactory(io::ErrorKind),
    PrematureSuccessResponse,
    BodyTooLarge,
    BodyTruncated,
    TooManyResponseHeaders,
    ResponseHeadersTooLarge,
    Connect,
    DialTimeout,
    TlsHandshakeTimeout,
    ActivityTimeout,
    Cancelled,
    Timeout,
    Request(io::ErrorKind),
    Read(io::ErrorKind),
    DeadlineExceeded,
    InvalidProxy,
    InvalidTimeout,
    InvalidCertificate,
    UnsupportedClientIdentity,
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidUrl => "invalid HTTP(S) URL",
            Self::InvalidRedirect => "invalid HTTP redirect",
            Self::RedirectBodyNotAllowed => "HTTP redirect body replay is not allowed",
            Self::RedirectLimit => "HTTP redirect limit exceeded",
            Self::RedirectLoop => "HTTP redirect loop detected",
            Self::ReplayRequired => "request body cannot be replayed",
            Self::InvalidHeader => "invalid HTTP header",
            Self::HeaderCollision => "caller and configured HTTP headers overlap",
            Self::HeaderTooLarge => "HTTP request header bounds exceeded",
            Self::TooManyHeaders => "too many HTTP request headers",
            Self::BodyFactory(_) => "request body factory failed",
            Self::PrematureSuccessResponse => {
                "HTTP success response arrived before request body completion"
            }
            Self::BodyTooLarge => "HTTP response body exceeds configured limit",
            Self::BodyTruncated => "HTTP response body is truncated",
            Self::TooManyResponseHeaders => "too many HTTP response headers",
            Self::ResponseHeadersTooLarge => "HTTP response headers exceed configured limit",
            Self::Connect => "HTTP connection failed",
            Self::DialTimeout => "HTTP dial timed out",
            Self::TlsHandshakeTimeout => "HTTP TLS handshake timed out",
            Self::ActivityTimeout => "HTTP TCP activity timed out",
            Self::Cancelled => "HTTP request cancelled",
            Self::Timeout => "HTTP request timed out",
            Self::Request(_) => "HTTP request failed",
            Self::Read(_) => "HTTP response read failed",
            Self::DeadlineExceeded => "HTTP operation deadline exceeded",
            Self::InvalidProxy => "invalid HTTP proxy",
            Self::InvalidTimeout => "invalid HTTP timeout",
            Self::InvalidCertificate => "invalid TLS certificate or identity",
            Self::UnsupportedClientIdentity => "TLS client identity is not supported",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for TransportError {}

fn is_retryable_error(error: &TransportError) -> bool {
    matches!(
        error,
        TransportError::Connect
            | TransportError::DialTimeout
            | TransportError::TlsHandshakeTimeout
            | TransportError::ActivityTimeout
            | TransportError::Timeout
    )
}

fn ensure_not_cancelled(
    cancellation: Option<&RequestBodyCancellation>,
) -> Result<(), TransportError> {
    if cancellation.is_some_and(RequestBodyCancellation::is_cancelled) {
        Err(TransportError::Cancelled)
    } else {
        Ok(())
    }
}

fn validate_optional_timeout(timeout: Option<Duration>) -> Result<(), TransportError> {
    let Some(timeout) = timeout else {
        return Ok(());
    };
    if timeout.is_zero() || Instant::now().checked_add(timeout).is_none() {
        Err(TransportError::InvalidTimeout)
    } else {
        Ok(())
    }
}

fn is_retryable_method(
    method: &Method,
    body: Option<&RequestBodyFactory>,
    allow_replayable_writes: bool,
) -> bool {
    if matches!(
        method,
        &Method::GET | &Method::HEAD | &Method::OPTIONS | &Method::DELETE | &Method::TRACE
    ) {
        return true;
    }
    allow_replayable_writes
        && matches!(method, &Method::PUT | &Method::POST)
        && body.is_none_or(RequestBodyFactory::is_replayable)
}

fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn redirect_method_and_body(
    status: u16,
    method: &mut Method,
    body: &mut Option<RequestBodyFactory>,
) {
    match status {
        303 => {
            if *method != Method::GET && *method != Method::HEAD {
                *method = Method::GET;
            }
            *body = None;
        }
        301 | 302 => {
            if *method == Method::POST {
                *method = Method::GET;
                *body = None;
            }
        }
        307 | 308 => {}
        _ => unreachable!(),
    }
}

fn is_forbidden_request_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "content-length"
            | "host"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn validate_request_header_value(value: &[u8]) -> Result<(), TransportError> {
    if value
        .iter()
        .all(|byte| (*byte >= 32 && *byte != 127) || *byte == b'\t')
    {
        Ok(())
    } else {
        Err(TransportError::InvalidHeader)
    }
}

fn is_safe_redirect_header(name: &HeaderName) -> bool {
    let name = name.as_str();
    matches!(
        name,
        "accept"
            | "accept-encoding"
            | "accept-language"
            | "cache-control"
            | "content-type"
            | "user-agent"
    )
}

fn validate_redirect_text(value: &str) -> Result<(), TransportError> {
    if value.contains(['\\', '#']) {
        return Err(TransportError::InvalidRedirect);
    }
    if value.contains("://") {
        return validate_url_text(value).map_err(|_| TransportError::InvalidRedirect);
    }
    if value.starts_with("//") {
        let authority_end = value[2..]
            .find(['/', '?'])
            .map(|offset| offset + 2)
            .unwrap_or(value.len());
        let authority = &value[2..authority_end];
        if authority.is_empty() || authority.contains('@') {
            return Err(TransportError::InvalidRedirect);
        }
        validate_percent_encoding(authority, true).map_err(|_| TransportError::InvalidRedirect)?;
        validate_percent_encoding(&value[authority_end..], false)
            .map_err(|_| TransportError::InvalidRedirect)?;
        return Ok(());
    }
    validate_percent_encoding(value, false).map_err(|_| TransportError::InvalidRedirect)
}

fn validate_url_text(value: &str) -> Result<(), TransportError> {
    if value.is_empty() || has_control_or_space(value) || value.contains(['\\', '#']) {
        return Err(TransportError::InvalidUrl);
    }
    let authority_start = value.find("://").ok_or(TransportError::InvalidUrl)? + 3;
    let authority_end = value[authority_start..]
        .find(['/', '?'])
        .map(|offset| authority_start + offset)
        .unwrap_or(value.len());
    let authority = &value[authority_start..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return Err(TransportError::InvalidUrl);
    }
    validate_percent_encoding(authority, true)?;
    validate_percent_encoding(&value[authority_end..], false)?;
    Ok(())
}

fn validate_percent_encoding(value: &str, authority: bool) -> Result<(), TransportError> {
    let bytes = value.as_bytes();
    for index in 0..bytes.len() {
        if bytes[index] != b'%' {
            continue;
        }
        if index + 2 >= bytes.len()
            || !bytes[index + 1].is_ascii_hexdigit()
            || !bytes[index + 2].is_ascii_hexdigit()
        {
            return Err(TransportError::InvalidUrl);
        }
        let decoded = hex_value(bytes[index + 1]) << 4 | hex_value(bytes[index + 2]);
        if decoded < 0x20 || decoded == 0x7f || decoded == b'\\' {
            return Err(TransportError::InvalidUrl);
        }
        if authority {
            if matches!(decoded, b'/' | b'?' | b'#' | b'@' | b'[' | b']') {
                return Err(TransportError::InvalidUrl);
            }
        } else if matches!(decoded, b'/' | b'?' | b'#') {
            // Path routing delimiters cannot be hidden in an encoded segment.
            // Query bytes remain signed and are intentionally not normalized;
            // Url parsing still validates their percent syntax.
            let path_end = value.find('?').unwrap_or(value.len());
            if index < path_end {
                return Err(TransportError::InvalidUrl);
            }
        }
    }
    Ok(())
}

fn hex_value(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        b'A'..=b'F' => value - b'A' + 10,
        _ => 0,
    }
}

fn has_control_or_space(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_control() || character.is_ascii_whitespace())
}

fn parse_retry_after(value: &[u8]) -> Option<Duration> {
    let value = std::str::from_utf8(value).ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let timestamp = parse_imf_fixdate(value)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(Duration::from_secs(timestamp.saturating_sub(now)))
}

fn parse_imf_fixdate(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let weekday = parts.next()?;
    if !matches!(
        weekday,
        "Mon," | "Tue," | "Wed," | "Thu," | "Fri," | "Sat," | "Sun,"
    ) {
        return None;
    }
    let day = parse_fixed_ascii_digits(parts.next()?, 2)?;
    let month = match parts.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year = parse_fixed_ascii_digits(parts.next()?, 4)?;
    let time = parts.next()?;
    if parts.next()? != "GMT" || parts.next().is_some() {
        return None;
    }
    let mut clock = time.split(':');
    let hour = parse_fixed_ascii_digits(clock.next()?, 2)?;
    let minute = parse_fixed_ascii_digits(clock.next()?, 2)?;
    let second = parse_fixed_ascii_digits(clock.next()?, 2)?;
    if clock.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days_in_month = days_in_month(year, month)?;
    if day == 0 || day > days_in_month {
        return None;
    }
    let days = days_before_year(year)?
        .checked_add(days_before_month(year, month)?)?
        .checked_add(u64::from(day - 1))?;
    let seconds = hour
        .checked_mul(3_600)?
        .checked_add(minute.checked_mul(60)?)?
        .checked_add(second)?;
    days.checked_mul(86_400)?.checked_add(seconds)
}

fn parse_fixed_ascii_digits(value: &str, width: usize) -> Option<u64> {
    if value.len() != width || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn days_before_year(year: u64) -> Option<u64> {
    if !(1970..=9999).contains(&year) {
        return None;
    }
    let y = year.checked_sub(1)?;
    let mut days = 365_u64.checked_mul(year.checked_sub(1970)?)?;
    days = days.checked_add(y / 4)?;
    days = days.checked_sub(y / 100)?;
    days = days.checked_add(y / 400)?;
    days.checked_sub(1969 / 4 - 1969 / 100 + 1969 / 400)
}

fn days_before_month(year: u64, month: u32) -> Option<u64> {
    const DAYS: [u64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    if !(1..=12).contains(&month) {
        return None;
    }
    let leap = month > 2 && (year % 4 == 0 && (year % 100 != 0 || year % 400 == 0));
    DAYS.get((month - 1) as usize)?.checked_add(u64::from(leap))
}

fn days_in_month(year: u64, month: u32) -> Option<u64> {
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return None,
    };
    Some(days)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest as _;
    use std::fs::{self, OpenOptions};
    use std::io::{BufRead, BufReader, Cursor, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static BODY_FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn client() -> HttpClient {
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.retry = RetryPolicy {
            max_attempts: 2,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
            allow_replayable_writes: false,
        };
        HttpClient::new(config).expect("client")
    }

    #[test]
    fn default_system_proxy_policy_constructs_before_actual_url_resolution() {
        HttpClient::new(ClientConfig::default()).expect("default client");
    }

    #[test]
    fn http1_read_buffer_tracks_bounded_header_policy() {
        assert_eq!(
            http1_read_buffer_bytes(DEFAULT_MAX_RESPONSE_HEADER_BYTES),
            HTTP1_MAX_READ_BUFFER_BYTES
        );
        assert_eq!(http1_read_buffer_bytes(0), HTTP1_MIN_READ_BUFFER_BYTES);
        assert_eq!(
            http1_read_buffer_bytes(usize::MAX),
            HTTP1_MAX_READ_BUFFER_BYTES
        );
        assert_eq!(RESPONSE_BODY_BRIDGE_QUEUE_DEPTH, 1);
        assert_eq!(REQUEST_BODY_BRIDGE_QUEUE_DEPTH, 2);
        assert_eq!(HTTP1_READ_BUFFER_FRAMING_BYTES, 8 * 1024);
    }

    fn replayable_write_client() -> HttpClient {
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.retry.max_attempts = 2;
        config.retry.base_delay = Duration::ZERO;
        config.retry.max_delay = Duration::ZERO;
        config.retry.allow_replayable_writes = true;
        HttpClient::new(config).expect("client")
    }

    fn listener() -> (TcpListener, String) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener");
        let address = format!("http://{}", listener.local_addr().expect("address"));
        (listener, address)
    }

    fn decode_hex(value: &str) -> Vec<u8> {
        assert_eq!(value.len() % 2, 0);
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|digits| hex_value(digits[0]) << 4 | hex_value(digits[1]))
            .collect()
    }

    fn test_body_file(bytes: &[u8]) -> (std::path::PathBuf, Arc<File>) {
        let id = BODY_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("zmin-http-fixture-{}-{id}", std::process::id()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .expect("body fixture");
        file.write_all(bytes).expect("body fixture bytes");
        file.sync_all().expect("body fixture sync");
        (path, Arc::new(file))
    }

    fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let mut head = Vec::new();
        reader.read_until(b'\n', &mut head).expect("request line");
        let request_line = String::from_utf8(head).expect("request line UTF-8");
        let mut content_length = 0;
        loop {
            let mut line = Vec::new();
            reader.read_until(b'\n', &mut line).expect("header");
            if line == b"\r\n" || line == b"\n" {
                break;
            }
            if let Ok(line) = std::str::from_utf8(&line) {
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        content_length = value.trim().parse().expect("length");
                    }
                }
            }
        }
        let mut body = vec![0_u8; content_length];
        reader.read_exact(&mut body).expect("request body");
        (request_line, body)
    }

    fn read_request_header_names(stream: &mut TcpStream) -> (String, Vec<String>) {
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let mut head = Vec::new();
        reader.read_until(b'\n', &mut head).expect("request line");
        let request_line = String::from_utf8(head).expect("request line UTF-8");
        let mut names = Vec::new();
        loop {
            let mut line = Vec::new();
            reader.read_until(b'\n', &mut line).expect("header");
            if line == b"\r\n" || line == b"\n" {
                break;
            }
            if let Some(index) = line.iter().position(|byte| *byte == b':') {
                names.push(String::from_utf8_lossy(&line[..index]).to_ascii_lowercase());
            }
        }
        (request_line, names)
    }

    fn read_request_headers(stream: &mut TcpStream) -> (String, Vec<(String, String)>) {
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let mut head = Vec::new();
        reader.read_until(b'\n', &mut head).expect("request line");
        let request_line = String::from_utf8(head).expect("request line UTF-8");
        let mut headers = Vec::new();
        loop {
            let mut line = Vec::new();
            reader.read_until(b'\n', &mut line).expect("header");
            if line == b"\r\n" || line == b"\n" {
                break;
            }
            let line = String::from_utf8(line).expect("header UTF-8");
            let (name, value) = line.split_once(':').expect("header delimiter");
            headers.push((
                name.to_ascii_lowercase(),
                value.trim_end_matches(['\r', '\n']).trim().to_owned(),
            ));
        }
        (request_line, headers)
    }

    struct TestPolicyResolver {
        initial_origin: HttpOrigin,
        initial_headers: ConfiguredRequestHeaders,
        resolutions: Arc<AtomicUsize>,
    }

    struct RotatingPolicyResolver {
        resolutions: Arc<AtomicUsize>,
    }

    impl RequestPolicyResolver for RotatingPolicyResolver {
        fn resolve(&self, _url: &HttpUrl) -> Result<ResolvedRequestPolicy, TransportError> {
            let generation = self.resolutions.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(resolved(
                ConfiguredRequestHeaders::generic([(
                    "X-Policy-Generation",
                    generation.to_string().into_bytes(),
                )])
                .expect("generation header"),
            ))
        }
    }

    struct ScriptedPolicyResolver {
        policies: Vec<(String, Result<ResolvedRequestPolicy, TransportError>)>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl RequestPolicyResolver for ScriptedPolicyResolver {
        fn resolve(&self, url: &HttpUrl) -> Result<ResolvedRequestPolicy, TransportError> {
            self.calls
                .lock()
                .expect("resolver calls")
                .push(url.as_str().to_owned());
            self.policies
                .iter()
                .find(|(candidate, _)| candidate == url.as_str())
                .map(|(_, policy)| policy.clone())
                .unwrap_or(Err(TransportError::InvalidUrl))
        }
    }

    fn resolved(configured_headers: ConfiguredRequestHeaders) -> ResolvedRequestPolicy {
        ResolvedRequestPolicy {
            connection: HttpConnectionPolicy {
                proxy: ProxyPolicy::Disabled,
                ..HttpConnectionPolicy::default()
            },
            timeouts: HttpTimeoutPolicy::default(),
            proxy_tls_policy: ProxyTlsPolicy::default(),
            configured_headers,
        }
    }

    fn scripted_client(
        policies: Vec<(String, Result<ResolvedRequestPolicy, TransportError>)>,
    ) -> (HttpClient, Arc<Mutex<Vec<String>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let resolver = Arc::new(ScriptedPolicyResolver {
            policies,
            calls: Arc::clone(&calls),
        });
        let mut config = ClientConfig::default();
        config.request_timeout = Some(Duration::from_secs(1));
        config.operation_timeout = Some(Duration::from_secs(2));
        config.retry.max_attempts = 1;
        (
            HttpClient::with_policy_resolver(config, resolver).expect("client"),
            calls,
        )
    }

    impl RequestPolicyResolver for TestPolicyResolver {
        fn resolve(&self, url: &HttpUrl) -> Result<ResolvedRequestPolicy, TransportError> {
            self.resolutions.fetch_add(1, Ordering::SeqCst);
            let configured_headers = if url.origin() == &self.initial_origin {
                self.initial_headers.clone()
            } else {
                ConfiguredRequestHeaders::generic([(
                    "X-Redirect-Only",
                    b"must-not-be-added".to_vec(),
                )])?
            };
            Ok(ResolvedRequestPolicy {
                connection: HttpConnectionPolicy {
                    proxy: ProxyPolicy::Disabled,
                    ..HttpConnectionPolicy::default()
                },
                timeouts: HttpTimeoutPolicy::default(),
                proxy_tls_policy: ProxyTlsPolicy::default(),
                configured_headers,
            })
        }
    }

    fn response(stream: &mut TcpStream, status: &str, headers: &str, body: &[u8]) {
        write!(
            stream,
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .expect("response");
        stream.write_all(body).expect("body");
    }

    fn header_values<'a>(headers: &'a [(String, String)], name: &str) -> Vec<&'a str> {
        headers
            .iter()
            .filter(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_str())
            .collect()
    }

    #[test]
    fn streams_large_response_without_buffering_transport_body() {
        let (listener, base) = listener();
        let body = vec![b'x'; 2 * 1024 * 1024];
        let expected = body.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = read_request(&mut stream);
            response(&mut stream, "200 OK", "", &body);
        });
        let url = HttpUrl::parse(&format!("{base}/large")).expect("URL");
        let mut response = client()
            .execute(&HttpRequest::new(Method::GET, url))
            .expect("send");
        let mut output = Vec::new();
        response.read_to_end(&mut output).expect("stream");
        assert_eq!(output, expected);
        server.join().expect("server");
    }

    #[test]
    fn upload_retry_reopens_sized_body() {
        let (listener, base) = listener();
        let server = std::thread::spawn(move || {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept");
                let (_, body) = read_request(&mut stream);
                assert_eq!(body, b"upload-data");
                if attempt == 0 {
                    response(
                        &mut stream,
                        "503 Service Unavailable",
                        "Retry-After: 0\r\n",
                        b"",
                    );
                } else {
                    response(&mut stream, "200 OK", "", b"ok");
                }
            }
        });
        let body = RequestBodyFactory::bytes(b"upload-data".to_vec());
        let url = HttpUrl::parse(&format!("{base}/upload")).expect("URL");
        let request = HttpRequest::new(Method::POST, url).with_body(body);
        let response = replayable_write_client().execute(&request).expect("send");
        assert_eq!(response.status(), 200);
        server.join().expect("server");
    }

    #[test]
    fn corrupt_same_size_regular_file_cannot_complete_put() {
        let corrupt = vec![7_u8; BODY_BRIDGE_CHUNK_BYTES + 7];
        let mut expected = corrupt.clone();
        expected[0] = 8;
        let expected_sha256: [u8; 32] = sha2::Sha256::digest(&expected).into();
        let corrupt_len = corrupt.len();
        let (path, file) = test_body_file(&corrupt);
        let (listener, base) = listener();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("read timeout");
            let mut head = Vec::new();
            let mut byte = [0_u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).expect("request head");
                head.push(byte[0]);
                assert!(head.len() <= 64 * 1024);
            }
            let head = String::from_utf8(head).expect("request head UTF-8");
            assert!(head.starts_with("PUT /corrupt HTTP/1.1\r\n"));
            assert!(head.lines().any(|line| {
                line.eq_ignore_ascii_case(&format!("content-length: {corrupt_len}"))
            }));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .expect("early success response");
            let mut received = Vec::new();
            let _ = stream.read_to_end(&mut received);
            assert!(received.len() < corrupt_len);
        });
        let open_file = Arc::clone(&file);
        let body = RequestBodyFactory::verified_regular_file(
            corrupt_len as u64,
            expected_sha256,
            RequestBodyCancellation::new(),
            move || open_file.try_clone(),
        );
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.retry.max_attempts = 1;
        let error = HttpClient::new(config)
            .expect("client")
            .execute(
                &HttpRequest::new(
                    Method::PUT,
                    HttpUrl::parse(&format!("{base}/corrupt")).expect("URL"),
                )
                .with_body(body),
            )
            .expect_err("corrupt upload");
        assert!(matches!(
            error,
            TransportError::Request(_)
                | TransportError::Read(_)
                | TransportError::PrematureSuccessResponse
        ));
        server.join().expect("server");
        drop(file);
        fs::remove_file(path).expect("remove body fixture");
    }

    #[test]
    fn early_401_is_visible_without_waiting_for_upload_completion() {
        let bytes = vec![9_u8; BODY_BRIDGE_CHUNK_BYTES * 32];
        let expected_sha256: [u8; 32] = sha2::Sha256::digest(&bytes).into();
        let length = bytes.len();
        let (path, file) = test_body_file(&bytes);
        let (listener, base) = listener();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut head = Vec::new();
            let mut byte = [0_u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).expect("request head");
                head.push(byte[0]);
            }
            stream
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .expect("early auth response");
            std::thread::sleep(Duration::from_millis(100));
        });
        let open_file = Arc::clone(&file);
        let cancellation = RequestBodyCancellation::new();
        let body = RequestBodyFactory::verified_regular_file(
            length as u64,
            expected_sha256,
            cancellation.clone(),
            move || open_file.try_clone(),
        );
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.retry.max_attempts = 1;
        let started = Instant::now();
        let response = HttpClient::new(config)
            .expect("client")
            .execute(
                &HttpRequest::new(
                    Method::PUT,
                    HttpUrl::parse(&format!("{base}/auth")).expect("URL"),
                )
                .with_body(body),
            )
            .expect("401 remains visible");
        assert_eq!(response.status(), 401);
        assert!(started.elapsed() < Duration::from_secs(1));
        cancellation.cancel();
        drop(response);
        server.join().expect("server");
        drop(file);
        fs::remove_file(path).expect("remove body fixture");
    }

    #[test]
    fn early_large_success_without_upload_consumption_fails_bounded() {
        let (listener, base) = listener();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = read_request_headers(&mut stream);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 8388608\r\nConnection: close\r\n\r\n",
                )
                .expect("early response head");
            let chunk = vec![b'x'; BODY_BRIDGE_CHUNK_BYTES];
            for _ in 0..128 {
                if stream.write_all(&chunk).is_err() {
                    break;
                }
            }
        });
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.timeout_policy.activity_timeout = None;
        config.request_timeout = None;
        config.operation_timeout = None;
        config.retry.max_attempts = 1;
        let cancellation = RequestBodyCancellation::new();
        let started = Instant::now();
        let error = HttpClient::new(config)
            .expect("client")
            .execute(
                &HttpRequest::new(
                    Method::PUT,
                    HttpUrl::parse(&format!("{base}/early-large-success")).expect("URL"),
                )
                .with_body(RequestBodyFactory::bytes(vec![b'u'; 8 * 1024 * 1024]))
                .with_cancellation(cancellation.clone()),
            )
            .expect_err("premature success");
        assert_eq!(error, TransportError::PrematureSuccessResponse);
        assert!(cancellation.is_cancelled());
        assert!(started.elapsed() < Duration::from_secs(1));
        server.join().expect("server");
    }

    #[test]
    fn request_policy_snapshot_is_reused_for_retry_attempts() {
        let (server_listener, base) = listener();
        let server = std::thread::spawn(move || {
            for attempt in 0..2 {
                let (mut stream, _) = server_listener.accept().expect("accept");
                let _ = read_request(&mut stream);
                if attempt == 0 {
                    response(
                        &mut stream,
                        "503 Service Unavailable",
                        "Retry-After: 0\r\n",
                        b"",
                    );
                } else {
                    response(&mut stream, "200 OK", "", b"ok");
                }
            }
        });
        let url = HttpUrl::parse(&format!("{base}/retry")).expect("URL");
        let resolutions = Arc::new(AtomicUsize::new(0));
        let resolver = Arc::new(TestPolicyResolver {
            initial_origin: url.origin().clone(),
            initial_headers: ConfiguredRequestHeaders::none(),
            resolutions: Arc::clone(&resolutions),
        });
        let mut config = ClientConfig::default();
        config.retry.max_attempts = 2;
        config.retry.base_delay = Duration::ZERO;
        config.retry.max_delay = Duration::ZERO;
        let response = HttpClient::with_policy_resolver(config, resolver)
            .expect("client")
            .execute(&HttpRequest::new(Method::GET, url))
            .expect("retry");
        assert_eq!(response.status(), 200);
        assert_eq!(resolutions.load(Ordering::SeqCst), 1);
        server.join().expect("server");
    }

    #[test]
    fn redirect_resolves_new_full_snapshot_and_retry_reuses_it() {
        let (server_listener, base) = listener();
        let server = std::thread::spawn(move || {
            let mut captured = Vec::new();
            for attempt in 0..3 {
                let (mut stream, _) = server_listener.accept().expect("accept");
                let (request, headers) = read_request_headers(&mut stream);
                captured.push((request, headers));
                match attempt {
                    0 => response(&mut stream, "302 Found", "Location: /retry\r\n", b""),
                    1 => response(
                        &mut stream,
                        "503 Service Unavailable",
                        "Retry-After: 0\r\n",
                        b"",
                    ),
                    _ => response(&mut stream, "200 OK", "", b"ok"),
                }
            }
            captured
        });
        let url = HttpUrl::parse(&format!("{base}/start")).expect("URL");
        let resolutions = Arc::new(AtomicUsize::new(0));
        let resolver = Arc::new(RotatingPolicyResolver {
            resolutions: Arc::clone(&resolutions),
        });
        let mut config = ClientConfig::default();
        config.retry.max_attempts = 2;
        config.retry.base_delay = Duration::ZERO;
        config.retry.max_delay = Duration::ZERO;
        let response = HttpClient::with_policy_resolver(config, resolver)
            .expect("client")
            .execute(&HttpRequest::new(Method::GET, url))
            .expect("redirect and retry");
        assert_eq!(response.status(), 200);
        assert_eq!(resolutions.load(Ordering::SeqCst), 2);
        let captured = server.join().expect("server");
        assert!(captured[0].0.contains(" /start "));
        assert_eq!(header_values(&captured[0].1, "x-policy-generation"), ["1"]);
        for (request, headers) in &captured[1..] {
            assert!(request.contains(" /retry "));
            assert_eq!(header_values(headers, "x-policy-generation"), ["2"]);
        }
    }

    #[test]
    fn single_use_body_is_not_retried_by_default() {
        let (listener, base) = listener();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let (_, body) = read_request(&mut stream);
            assert_eq!(body, b"upload-data");
            response(
                &mut stream,
                "503 Service Unavailable",
                "Retry-After: 0\r\n",
                b"",
            );
        });
        let url = HttpUrl::parse(&format!("{base}/single-use")).expect("URL");
        let body = RequestBodyFactory::single_use_bytes(b"upload-data".to_vec());
        let response = client()
            .execute(&HttpRequest::new(Method::POST, url).with_body(body))
            .expect("response remains caller-visible");
        assert_eq!(response.status(), 503);
        server.join().expect("server");
    }

    #[test]
    fn cross_origin_redirect_strips_credentials() {
        let (target_listener, target_base) = listener();
        let target = std::thread::spawn(move || {
            let (mut stream, _) = target_listener.accept().expect("target accept");
            let (line, names) = read_request_header_names(&mut stream);
            assert!(line.starts_with("GET /target HTTP/1.1"));
            assert!(!names.iter().any(|name| name == "authorization"));
            assert!(!names.iter().any(|name| name == "cookie"));
            response(&mut stream, "200 OK", "", b"ok");
        });
        let (source_listener, source_base) = listener();
        let source = std::thread::spawn(move || {
            let (mut stream, _) = source_listener.accept().expect("source accept");
            let _ = read_request(&mut stream);
            response(
                &mut stream,
                "302 Found",
                &format!("Location: {target_base}/target\r\n"),
                b"",
            );
        });
        let mut headers = RequestHeaders::empty();
        headers
            .push(
                RequestHeader::new_secret("Authorization", b"Bearer secret".to_vec())
                    .expect("header"),
            )
            .expect("header");
        headers
            .push(RequestHeader::new("Cookie", b"session=secret").expect("header"))
            .expect("header");
        headers
            .push(RequestHeader::new("Accept", b"application/json").expect("header"))
            .expect("header");
        let url = HttpUrl::parse(&format!("{source_base}/start")).expect("URL");
        let request = HttpRequest::new(Method::GET, url).with_headers(headers);
        let policy = RedirectPolicy::default()
            .allow_cross_origin_header("Accept")
            .expect("safe header");
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.redirect_policy = policy;
        let mut response = HttpClient::new(config)
            .expect("client")
            .execute(&request)
            .expect("redirect");
        let mut body = String::new();
        response.read_to_string(&mut body).expect("body");
        assert_eq!(body, "ok");
        source.join().expect("source");
        target.join().expect("target");
    }

    #[test]
    fn configured_headers_are_repeated_in_order_and_stripped_cross_origin() {
        let (target_listener, target_base) = listener();
        let target = std::thread::spawn(move || {
            let (mut stream, _) = target_listener.accept().expect("target accept");
            let (_, headers) = read_request_headers(&mut stream);
            assert!(!headers.iter().any(|(name, _)| name == "x-signed"));
            assert!(!headers.iter().any(|(name, _)| name == "authorization"));
            assert!(!headers.iter().any(|(name, _)| name == "x-redirect-only"));
            response(&mut stream, "200 OK", "", b"ok");
        });
        let (source_listener, source_base) = listener();
        let source = std::thread::spawn(move || {
            let (mut stream, _) = source_listener.accept().expect("source accept");
            let (_, headers) = read_request_headers(&mut stream);
            let signed = headers
                .iter()
                .filter(|(name, _)| name == "x-signed")
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>();
            assert_eq!(signed, ["first", "second"]);
            assert!(headers
                .iter()
                .any(|(name, value)| name == "authorization" && value == "Bearer configured"));
            response(
                &mut stream,
                "302 Found",
                &format!("Location: {target_base}/target\r\n"),
                b"",
            );
        });

        let url = HttpUrl::parse(&format!("{source_base}/start")).expect("URL");
        let configured_headers = ConfiguredRequestHeaders::generic([
            ("X-Signed", b"first".to_vec()),
            ("X-Signed", b"second".to_vec()),
            ("Authorization", b"Bearer configured".to_vec()),
        ])
        .expect("configured headers");
        let configured_debug = format!("{configured_headers:?}");
        assert!(!configured_debug.contains("first"));
        assert!(!configured_debug.contains("Bearer configured"));
        let resolutions = Arc::new(AtomicUsize::new(0));
        let resolver = Arc::new(TestPolicyResolver {
            initial_origin: url.origin().clone(),
            initial_headers: configured_headers,
            resolutions: Arc::clone(&resolutions),
        });
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        let mut result = HttpClient::with_policy_resolver(config, resolver)
            .expect("client")
            .execute(&HttpRequest::new(Method::GET, url))
            .expect("redirect");
        let mut body = String::new();
        result.read_to_string(&mut body).expect("body");
        assert_eq!(body, "ok");
        assert_eq!(resolutions.load(Ordering::SeqCst), 2);
        source.join().expect("source");
        target.join().expect("target");
    }

    #[test]
    fn configured_headers_survive_same_origin_redirect_without_duplication() {
        let (server_listener, base) = listener();
        let server = std::thread::spawn(move || {
            for response_index in 0..2 {
                let (mut stream, _) = server_listener.accept().expect("accept");
                let (_, headers) = read_request_headers(&mut stream);
                let signed = headers
                    .iter()
                    .filter(|(name, _)| name == "x-signed")
                    .map(|(_, value)| value.as_str())
                    .collect::<Vec<_>>();
                assert_eq!(signed, ["first", "second"]);
                if response_index == 0 {
                    response(&mut stream, "302 Found", "Location: /next\r\n", b"");
                } else {
                    response(&mut stream, "200 OK", "", b"ok");
                }
            }
        });
        let url = HttpUrl::parse(&format!("{base}/start")).expect("URL");
        let resolver = Arc::new(TestPolicyResolver {
            initial_origin: url.origin().clone(),
            initial_headers: ConfiguredRequestHeaders::generic([
                ("X-Signed", b"first".to_vec()),
                ("X-Signed", b"second".to_vec()),
            ])
            .expect("headers"),
            resolutions: Arc::new(AtomicUsize::new(0)),
        });
        let mut response = HttpClient::with_policy_resolver(ClientConfig::default(), resolver)
            .expect("client")
            .execute(&HttpRequest::new(Method::GET, url))
            .expect("redirect");
        let mut body = String::new();
        response.read_to_string(&mut body).expect("body");
        assert_eq!(body, "ok");
        server.join().expect("server");
    }

    #[test]
    fn configured_header_provenance_preserves_empty_resets() {
        let none = ConfiguredRequestHeaders::none();
        let generic = ConfiguredRequestHeaders::generic(std::iter::empty::<(&str, Vec<u8>)>())
            .expect("generic reset");
        let scoped = ConfiguredRequestHeaders::url_scoped(std::iter::empty::<(&str, Vec<u8>)>())
            .expect("URL-scoped reset");
        assert_eq!(none.provenance(), ConfiguredRequestHeaderProvenance::None);
        assert_eq!(
            generic.provenance(),
            ConfiguredRequestHeaderProvenance::Generic
        );
        assert_eq!(
            scoped.provenance(),
            ConfiguredRequestHeaderProvenance::UrlScoped
        );
        assert!(none.is_empty() && generic.is_empty() && scoped.is_empty());
    }

    #[test]
    fn initial_caller_configured_collision_fails_before_network() {
        let (listener, base) = listener();
        listener.set_nonblocking(true).expect("nonblocking probe");
        let url = HttpUrl::parse(&format!("{base}/collision")).expect("URL");
        let (client, _) = scripted_client(vec![(
            url.as_str().to_owned(),
            Ok(resolved(
                ConfiguredRequestHeaders::generic([("X-Collision", b"configured".to_vec())])
                    .expect("configured"),
            )),
        )]);
        let mut caller = RequestHeaders::empty();
        caller
            .push(RequestHeader::new("x-collision", b"caller").expect("caller"))
            .expect("caller");
        let error = client
            .execute(&HttpRequest::new(Method::GET, url).with_headers(caller))
            .expect_err("collision");
        assert_eq!(error, TransportError::HeaderCollision);
        assert!(matches!(
            listener.accept(),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock
        ));
    }

    #[test]
    fn same_origin_target_collision_fails_before_target_request() {
        let (listener, base) = listener();
        let probe = listener.try_clone().expect("probe listener");
        let source = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("source accept");
            let _ = read_request(&mut stream);
            response(&mut stream, "302 Found", "Location: /target\r\n", b"");
        });
        let initial = HttpUrl::parse(&format!("{base}/start")).expect("initial URL");
        let target = HttpUrl::parse(&format!("{base}/target")).expect("target URL");
        let (client, calls) = scripted_client(vec![
            (
                initial.as_str().to_owned(),
                Ok(resolved(ConfiguredRequestHeaders::none())),
            ),
            (
                target.as_str().to_owned(),
                Ok(resolved(
                    ConfiguredRequestHeaders::url_scoped([("X-Collision", b"configured".to_vec())])
                        .expect("configured"),
                )),
            ),
        ]);
        let mut caller = RequestHeaders::empty();
        caller
            .push(RequestHeader::new("x-collision", b"caller").expect("caller"))
            .expect("caller");
        let error = client
            .execute(&HttpRequest::new(Method::GET, initial).with_headers(caller))
            .expect_err("target collision");
        assert_eq!(error, TransportError::HeaderCollision);
        source.join().expect("source");
        probe.set_nonblocking(true).expect("nonblocking probe");
        assert!(matches!(
            probe.accept(),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock
        ));
        assert_eq!(calls.lock().expect("calls").len(), 2);
    }

    #[test]
    fn redirect_target_resolver_failure_prevents_target_request() {
        let (target_listener, target_base) = listener();
        let target_url = format!("{target_base}/target");
        let target_location = target_url.clone();
        target_listener.set_nonblocking(true).expect("target probe");
        let (source_listener, source_base) = listener();
        let source = std::thread::spawn(move || {
            let (mut stream, _) = source_listener.accept().expect("source accept");
            let _ = read_request(&mut stream);
            response(
                &mut stream,
                "302 Found",
                &format!("Location: {target_location}\r\n"),
                b"",
            );
        });
        let initial = HttpUrl::parse(&format!("{source_base}/start")).expect("initial URL");
        let target = HttpUrl::parse(&target_url).expect("target URL");
        let (client, calls) = scripted_client(vec![
            (
                initial.as_str().to_owned(),
                Ok(resolved(ConfiguredRequestHeaders::none())),
            ),
            (
                target.as_str().to_owned(),
                Err(TransportError::InvalidProxy),
            ),
        ]);
        let error = client
            .execute(&HttpRequest::new(Method::GET, initial))
            .expect_err("resolver failure");
        assert_eq!(error, TransportError::InvalidProxy);
        source.join().expect("source");
        assert!(matches!(
            target_listener.accept(),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock
        ));
        assert_eq!(calls.lock().expect("calls").len(), 2);
    }

    #[test]
    fn same_origin_redirect_replaces_configured_headers() {
        let (listener, base) = listener();
        let server = std::thread::spawn(move || {
            let mut captured = Vec::new();
            for index in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept");
                let (_, headers) = read_request_headers(&mut stream);
                captured.push(headers);
                if index == 0 {
                    response(&mut stream, "302 Found", "Location: /target\r\n", b"");
                } else {
                    response(&mut stream, "200 OK", "", b"ok");
                }
            }
            captured
        });
        let initial = HttpUrl::parse(&format!("{base}/start")).expect("initial URL");
        let target = HttpUrl::parse(&format!("{base}/target")).expect("target URL");
        let (client, _) = scripted_client(vec![
            (
                initial.as_str().to_owned(),
                Ok(resolved(
                    ConfiguredRequestHeaders::generic([("X-Configured", b"old".to_vec())])
                        .expect("initial configured"),
                )),
            ),
            (
                target.as_str().to_owned(),
                Ok(resolved(
                    ConfiguredRequestHeaders::generic([("X-Configured", b"new".to_vec())])
                        .expect("target configured"),
                )),
            ),
        ]);
        let response = client
            .execute(&HttpRequest::new(Method::GET, initial))
            .expect("redirect");
        assert_eq!(response.status(), 200);
        let captured = server.join().expect("server");
        assert_eq!(header_values(&captured[0], "x-configured"), ["old"]);
        assert_eq!(header_values(&captured[1], "x-configured"), ["new"]);
    }

    #[test]
    fn same_origin_empty_reset_removes_previous_configured_headers() {
        let (listener, base) = listener();
        let server = std::thread::spawn(move || {
            let mut captured = Vec::new();
            for index in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept");
                let (_, headers) = read_request_headers(&mut stream);
                captured.push(headers);
                if index == 0 {
                    response(&mut stream, "302 Found", "Location: /target\r\n", b"");
                } else {
                    response(&mut stream, "200 OK", "", b"ok");
                }
            }
            captured
        });
        let initial = HttpUrl::parse(&format!("{base}/start")).expect("initial URL");
        let target = HttpUrl::parse(&format!("{base}/target")).expect("target URL");
        let (client, _) = scripted_client(vec![
            (
                initial.as_str().to_owned(),
                Ok(resolved(
                    ConfiguredRequestHeaders::generic([("X-Configured", b"old".to_vec())])
                        .expect("initial configured"),
                )),
            ),
            (
                target.as_str().to_owned(),
                Ok(resolved(
                    ConfiguredRequestHeaders::generic(std::iter::empty::<(&str, Vec<u8>)>())
                        .expect("empty reset"),
                )),
            ),
        ]);
        let response = client
            .execute(&HttpRequest::new(Method::GET, initial))
            .expect("redirect");
        assert_eq!(response.status(), 200);
        let captured = server.join().expect("server");
        assert_eq!(header_values(&captured[0], "x-configured"), ["old"]);
        assert!(header_values(&captured[1], "x-configured").is_empty());
    }

    #[test]
    fn cross_origin_applies_only_url_scoped_target_headers() {
        let (target_listener, target_base) = listener();
        let target_url = format!("{target_base}/target");
        let target_location = target_url.clone();
        let target_server = std::thread::spawn(move || {
            let (mut stream, _) = target_listener.accept().expect("target accept");
            let (_, headers) = read_request_headers(&mut stream);
            response(&mut stream, "200 OK", "", b"ok");
            headers
        });
        let (source_listener, source_base) = listener();
        let source_server = std::thread::spawn(move || {
            let (mut stream, _) = source_listener.accept().expect("source accept");
            let _ = read_request(&mut stream);
            response(
                &mut stream,
                "302 Found",
                &format!("Location: {target_location}\r\n"),
                b"",
            );
        });
        let initial = HttpUrl::parse(&format!("{source_base}/start")).expect("initial URL");
        let target = HttpUrl::parse(&target_url).expect("target URL");
        let (client, _) = scripted_client(vec![
            (
                initial.as_str().to_owned(),
                Ok(resolved(
                    ConfiguredRequestHeaders::generic([("X-Source", b"source".to_vec())])
                        .expect("source configured"),
                )),
            ),
            (
                target.as_str().to_owned(),
                Ok(resolved(
                    ConfiguredRequestHeaders::url_scoped([
                        ("X-Target", b"first".to_vec()),
                        ("X-Target", b"second".to_vec()),
                    ])
                    .expect("target configured"),
                )),
            ),
        ]);
        let response = client
            .execute(&HttpRequest::new(Method::GET, initial))
            .expect("redirect");
        assert_eq!(response.status(), 200);
        source_server.join().expect("source");
        let headers = target_server.join().expect("target");
        assert!(header_values(&headers, "x-source").is_empty());
        assert_eq!(header_values(&headers, "x-target"), ["first", "second"]);
    }

    #[test]
    fn cross_origin_keeps_only_allowlisted_public_caller_headers() {
        let (target_listener, target_base) = listener();
        let target_url = format!("{target_base}/target");
        let target_location = target_url.clone();
        let target_server = std::thread::spawn(move || {
            let (mut stream, _) = target_listener.accept().expect("target accept");
            let (_, headers) = read_request_headers(&mut stream);
            response(&mut stream, "200 OK", "", b"ok");
            headers
        });
        let (source_listener, source_base) = listener();
        let source_server = std::thread::spawn(move || {
            let (mut stream, _) = source_listener.accept().expect("source accept");
            let _ = read_request(&mut stream);
            response(
                &mut stream,
                "302 Found",
                &format!("Location: {target_location}\r\n"),
                b"",
            );
        });
        let initial = HttpUrl::parse(&format!("{source_base}/start")).expect("initial URL");
        let target = HttpUrl::parse(&target_url).expect("target URL");
        let (mut client, _) = scripted_client(vec![
            (
                initial.as_str().to_owned(),
                Ok(resolved(ConfiguredRequestHeaders::none())),
            ),
            (
                target.as_str().to_owned(),
                Ok(resolved(ConfiguredRequestHeaders::none())),
            ),
        ]);
        let mut caller = RequestHeaders::empty();
        caller
            .push(
                RequestHeader::new_secret("Authorization", b"Bearer helper".to_vec())
                    .expect("authorization"),
            )
            .expect("authorization");
        caller
            .push(RequestHeader::new("Cookie", b"session=secret").expect("cookie"))
            .expect("cookie");
        caller
            .push(
                RequestHeader::new_secret("X-Action-Signature", b"signed".to_vec())
                    .expect("action"),
            )
            .expect("action");
        caller
            .push(RequestHeader::new("Accept", b"application/json").expect("accept"))
            .expect("accept");
        client.config.redirect_policy = RedirectPolicy::default()
            .allow_cross_origin_header("Accept")
            .expect("safe header");
        let response = client
            .execute(&HttpRequest::new(Method::GET, initial.clone()).with_headers(caller))
            .expect("redirect");
        assert_eq!(response.status(), 200);
        source_server.join().expect("source");
        let headers = target_server.join().expect("target");
        assert!(header_values(&headers, "authorization").is_empty());
        assert!(header_values(&headers, "cookie").is_empty());
        assert!(header_values(&headers, "x-action-signature").is_empty());
        assert_eq!(header_values(&headers, "accept"), ["application/json"]);
    }

    #[test]
    fn cross_origin_target_collision_fails_before_target_request() {
        let (target_listener, target_base) = listener();
        let target_url = format!("{target_base}/target");
        let target_location = target_url.clone();
        target_listener.set_nonblocking(true).expect("target probe");
        let (source_listener, source_base) = listener();
        let source_server = std::thread::spawn(move || {
            let (mut stream, _) = source_listener.accept().expect("source accept");
            let _ = read_request(&mut stream);
            response(
                &mut stream,
                "302 Found",
                &format!("Location: {target_location}\r\n"),
                b"",
            );
        });
        let initial = HttpUrl::parse(&format!("{source_base}/start")).expect("initial URL");
        let target = HttpUrl::parse(&target_url).expect("target URL");
        let (mut client, _) = scripted_client(vec![
            (
                initial.as_str().to_owned(),
                Ok(resolved(ConfiguredRequestHeaders::none())),
            ),
            (
                target.as_str().to_owned(),
                Ok(resolved(
                    ConfiguredRequestHeaders::url_scoped([("Accept", b"configured".to_vec())])
                        .expect("configured"),
                )),
            ),
        ]);
        client.config.redirect_policy = RedirectPolicy::default()
            .allow_cross_origin_header("Accept")
            .expect("safe header");
        let mut caller = RequestHeaders::empty();
        caller
            .push(RequestHeader::new("Accept", b"caller").expect("caller"))
            .expect("caller");
        let error = client
            .execute(&HttpRequest::new(Method::GET, initial).with_headers(caller))
            .expect_err("target collision");
        assert_eq!(error, TransportError::HeaderCollision);
        source_server.join().expect("source");
        assert!(matches!(
            target_listener.accept(),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock
        ));
    }

    #[test]
    fn cross_origin_round_trip_never_resurrects_caller_credentials() {
        let (a_listener, a_base) = listener();
        let (b_listener, b_base) = listener();
        let b_hop_url = format!("{b_base}/hop");
        let b_hop_location = b_hop_url.clone();
        let a_return = format!("{a_base}/return");
        let a_return_for_server = a_return.clone();
        let a_server = std::thread::spawn(move || {
            let (mut initial, _) = a_listener.accept().expect("A initial");
            let (_, initial_headers) = read_request_headers(&mut initial);
            response(
                &mut initial,
                "302 Found",
                &format!("Location: {b_hop_location}\r\n"),
                b"",
            );
            let (mut returned, _) = a_listener.accept().expect("A return");
            let (_, returned_headers) = read_request_headers(&mut returned);
            response(&mut returned, "200 OK", "", b"ok");
            (initial_headers, returned_headers)
        });
        let b_server = std::thread::spawn(move || {
            let (mut stream, _) = b_listener.accept().expect("B accept");
            let (_, headers) = read_request_headers(&mut stream);
            response(
                &mut stream,
                "302 Found",
                &format!("Location: {a_return_for_server}\r\n"),
                b"",
            );
            headers
        });
        let initial = HttpUrl::parse(&format!("{a_base}/start")).expect("initial URL");
        let b_hop = HttpUrl::parse(&b_hop_url).expect("B URL");
        let returned = HttpUrl::parse(&a_return).expect("return URL");
        let (mut client, _) = scripted_client(vec![
            (
                initial.as_str().to_owned(),
                Ok(resolved(ConfiguredRequestHeaders::none())),
            ),
            (
                b_hop.as_str().to_owned(),
                Ok(resolved(
                    ConfiguredRequestHeaders::url_scoped([("X-B-Scoped", b"b".to_vec())])
                        .expect("B scoped"),
                )),
            ),
            (
                returned.as_str().to_owned(),
                Ok(resolved(
                    ConfiguredRequestHeaders::url_scoped([("X-A-Scoped", b"a".to_vec())])
                        .expect("A scoped"),
                )),
            ),
        ]);
        client.config.redirect_policy = RedirectPolicy::default()
            .allow_cross_origin_header("Accept")
            .expect("safe header");
        let mut caller = RequestHeaders::empty();
        caller
            .push(
                RequestHeader::new_secret("Authorization", b"Bearer helper".to_vec())
                    .expect("authorization"),
            )
            .expect("authorization");
        caller
            .push(RequestHeader::new("Cookie", b"session=secret").expect("cookie"))
            .expect("cookie");
        caller
            .push(
                RequestHeader::new_secret("X-Action-Signature", b"signed".to_vec())
                    .expect("action"),
            )
            .expect("action");
        caller
            .push(RequestHeader::new("Accept", b"application/json").expect("accept"))
            .expect("accept");
        let response = client
            .execute(&HttpRequest::new(Method::GET, initial).with_headers(caller))
            .expect("round trip");
        assert_eq!(response.status(), 200);
        let (initial_headers, returned_headers) = a_server.join().expect("A server");
        let b_headers = b_server.join().expect("B server");
        assert_eq!(
            header_values(&initial_headers, "authorization"),
            ["Bearer helper"]
        );
        for headers in [&b_headers, &returned_headers] {
            assert!(header_values(headers, "authorization").is_empty());
            assert!(header_values(headers, "cookie").is_empty());
            assert!(header_values(headers, "x-action-signature").is_empty());
            assert_eq!(header_values(headers, "accept"), ["application/json"]);
        }
        assert_eq!(header_values(&b_headers, "x-b-scoped"), ["b"]);
        assert!(header_values(&returned_headers, "x-b-scoped").is_empty());
        assert_eq!(header_values(&returned_headers, "x-a-scoped"), ["a"]);
    }

    #[test]
    fn cross_origin_taint_blocks_generic_headers_on_later_hops_and_retries() {
        let (a_listener, a_base) = listener();
        let (b_listener, b_base) = listener();
        let b_hop_url = format!("{b_base}/hop");
        let b_hop_location = b_hop_url.clone();
        let b_final_url = format!("{b_base}/final");
        let a_server = std::thread::spawn(move || {
            let (mut stream, _) = a_listener.accept().expect("A accept");
            let (_, headers) = read_request_headers(&mut stream);
            response(
                &mut stream,
                "302 Found",
                &format!("Location: {b_hop_location}\r\n"),
                b"",
            );
            headers
        });
        let b_server = std::thread::spawn(move || {
            let mut captured = Vec::new();
            for index in 0..3 {
                let (mut stream, _) = b_listener.accept().expect("B accept");
                let (request, headers) = read_request_headers(&mut stream);
                captured.push((request, headers));
                match index {
                    0 => response(&mut stream, "302 Found", "Location: /final\r\n", b""),
                    1 => response(&mut stream, "500 Internal Server Error", "", b"retry"),
                    _ => response(&mut stream, "200 OK", "", b"ok"),
                }
            }
            captured
        });
        let initial = HttpUrl::parse(&format!("{a_base}/start")).expect("initial URL");
        let b_hop = HttpUrl::parse(&b_hop_url).expect("B hop URL");
        let b_final = HttpUrl::parse(&b_final_url).expect("B final URL");
        let (mut client, calls) = scripted_client(vec![
            (
                initial.as_str().to_owned(),
                Ok(resolved(ConfiguredRequestHeaders::none())),
            ),
            (
                b_hop.as_str().to_owned(),
                Ok(resolved(
                    ConfiguredRequestHeaders::url_scoped([("X-B-Scoped", b"scoped".to_vec())])
                        .expect("B scoped"),
                )),
            ),
            (
                b_final.as_str().to_owned(),
                Ok(resolved(
                    ConfiguredRequestHeaders::generic([(
                        "Authorization",
                        b"Basic global-secret".to_vec(),
                    )])
                    .expect("B generic"),
                )),
            ),
        ]);
        client.config.retry.max_attempts = 2;
        client.config.retry.base_delay = Duration::ZERO;
        client.config.retry.max_delay = Duration::ZERO;
        let mut caller = RequestHeaders::empty();
        caller
            .push(
                RequestHeader::new_secret("Authorization", b"Bearer helper".to_vec())
                    .expect("caller authorization"),
            )
            .expect("caller authorization");

        let response = client
            .execute(&HttpRequest::new(Method::GET, initial.clone()).with_headers(caller))
            .expect("redirect and retry");
        assert_eq!(response.status(), 200);
        let a_headers = a_server.join().expect("A server");
        let b_requests = b_server.join().expect("B server");
        assert_eq!(
            header_values(&a_headers, "authorization"),
            ["Bearer helper"]
        );
        assert_eq!(b_requests.len(), 3);
        assert!(b_requests[0].0.contains(" /hop "));
        assert_eq!(header_values(&b_requests[0].1, "x-b-scoped"), ["scoped"]);
        for (request, headers) in &b_requests[1..] {
            assert!(request.contains(" /final "));
            assert!(header_values(headers, "authorization").is_empty());
            assert!(header_values(headers, "x-b-scoped").is_empty());
        }
        assert_eq!(
            calls.lock().expect("resolver calls").as_slice(),
            [initial.as_str(), b_hop.as_str(), b_final.as_str(),]
        );
    }

    #[test]
    fn secret_request_header_owns_redacts_and_wipes_value() {
        let header =
            RequestHeader::new_secret("Authorization", b"Bearer secret".to_vec()).expect("header");
        assert_eq!(header.value(), b"Bearer secret");
        let debug = format!("{header:?}");
        assert!(debug.contains("authorization"));
        assert!(!debug.contains("Bearer secret"));

        let mut secret = SecretHeaderValue(b"Bearer secret".to_vec());
        secret.wipe();
        assert_eq!(secret.as_bytes(), &[0; 13]);
    }

    #[test]
    fn public_request_header_value_is_not_secret_storage() {
        let header = RequestHeader::new("Accept", b"application/json").expect("header");
        assert!(matches!(header.value, RequestHeaderValue::Public(_)));
    }

    #[test]
    fn cross_origin_redirect_policy_keeps_secret_values_stripped() {
        let mut headers = RequestHeaders::empty();
        headers
            .push(
                RequestHeader::new_secret("Content-Type", b"signed-secret".to_vec())
                    .expect("header"),
            )
            .expect("header");
        headers
            .push(RequestHeader::new("Accept", b"application/json").expect("header"))
            .expect("header");
        let policy = RedirectPolicy::default()
            .allow_cross_origin_header("Content-Type")
            .expect("safe header")
            .allow_cross_origin_header("Accept")
            .expect("safe header");

        let filtered = headers.for_cross_origin_redirect(&policy);
        let names = filtered.iter().map(RequestHeader::name).collect::<Vec<_>>();
        assert_eq!(names, vec!["accept"]);
    }

    #[test]
    fn secret_request_header_clones_are_independent() {
        let mut original =
            RequestHeader::new_secret("Authorization", b"Bearer secret".to_vec()).expect("header");
        let clone = original.clone();
        if let RequestHeaderValue::Secret(secret) = &mut original.value {
            secret.wipe();
        } else {
            panic!("expected secret storage");
        }
        assert_eq!(clone.value(), b"Bearer secret");
    }

    #[test]
    fn cross_origin_redirect_body_requires_explicit_origin() {
        let (target_listener, target_base) = listener();
        drop(target_listener);
        let (source_listener, source_base) = listener();
        let source = std::thread::spawn(move || {
            let (mut stream, _) = source_listener.accept().expect("source accept");
            let _ = read_request(&mut stream);
            response(
                &mut stream,
                "307 Temporary Redirect",
                &format!("Location: {target_base}/target\r\n"),
                b"",
            );
        });
        let body = RequestBodyFactory::bytes(b"body".to_vec());
        let url = HttpUrl::parse(&format!("{source_base}/start")).expect("URL");
        let error = client()
            .execute(&HttpRequest::new(Method::POST, url).with_body(body))
            .expect_err("cross-origin body policy");
        assert_eq!(error, TransportError::RedirectBodyNotAllowed);
        source.join().expect("source");
    }

    #[test]
    fn cross_origin_redirect_body_can_use_allowed_origin() {
        let (target_listener, target_base) = listener();
        let target_origin = HttpUrl::parse(&target_base)
            .expect("target URL")
            .origin()
            .clone();
        let target = std::thread::spawn(move || {
            let (mut stream, _) = target_listener.accept().expect("target accept");
            let (line, body) = read_request(&mut stream);
            assert!(line.starts_with("POST /target HTTP/1.1"));
            assert_eq!(body, b"body");
            response(&mut stream, "200 OK", "", b"ok");
        });
        let (source_listener, source_base) = listener();
        let source = std::thread::spawn(move || {
            let (mut stream, _) = source_listener.accept().expect("source accept");
            let _ = read_request(&mut stream);
            response(
                &mut stream,
                "307 Temporary Redirect",
                &format!("Location: {target_base}/target\r\n"),
                b"",
            );
        });
        let body = RequestBodyFactory::bytes(b"body".to_vec());
        let url = HttpUrl::parse(&format!("{source_base}/start")).expect("URL");
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.redirect_policy.body = RedirectBodyPolicy::allow_origins([target_origin]);
        let mut response = HttpClient::new(config)
            .expect("client")
            .execute(&HttpRequest::new(Method::POST, url).with_body(body))
            .expect("redirect");
        let mut output = String::new();
        response.read_to_string(&mut output).expect("body");
        assert_eq!(output, "ok");
        source.join().expect("source");
        target.join().expect("target");
    }

    #[test]
    fn redirect_loop_is_bounded() {
        let (listener, base) = listener();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept");
                let _ = read_request(&mut stream);
                response(&mut stream, "302 Found", "Location: /loop\r\n", b"");
            }
        });
        let url = HttpUrl::parse(&format!("{base}/start")).expect("URL");
        let error = client()
            .execute(&HttpRequest::new(Method::GET, url))
            .expect_err("loop");
        assert_eq!(error, TransportError::RedirectLoop);
        server.join().expect("server");
    }

    #[test]
    fn retry_after_delta_and_date_are_bounded() {
        assert_eq!(parse_retry_after(b"3"), Some(Duration::from_secs(3)));
        assert!(parse_retry_after(b"Sun, 06 Nov 1994 08:49:37 GMT").is_some());
        assert_eq!(parse_retry_after(b"not-a-date"), None);
    }

    #[test]
    fn retry_after_imf_dates_are_strict_and_checked() {
        assert!(parse_imf_fixdate("Fri, 31 Dec 9999 23:59:59 GMT").is_some());
        assert_eq!(parse_imf_fixdate("Sun, 06 Nov 10000 08:49:37 GMT"), None);
        assert_eq!(parse_imf_fixdate("Sun, 06 Nov 99999 08:49:37 GMT"), None);
        assert_eq!(parse_imf_fixdate("Sun, 06 Nov 0000 08:49:37 GMT"), None);
        assert_eq!(parse_imf_fixdate("Sun, 06 Nov 1969 08:49:37 GMT"), None);
        assert_eq!(parse_imf_fixdate("Noday, 06 Nov 1994 08:49:37 GMT"), None);
        assert_eq!(parse_imf_fixdate("Sun, 31 Apr 2024 08:49:37 GMT"), None);
        assert_eq!(parse_imf_fixdate("Sun, 29 Feb 2023 08:49:37 GMT"), None);
        assert!(parse_imf_fixdate("Thu, 29 Feb 2024 08:49:37 GMT").is_some());
        assert_eq!(parse_imf_fixdate("Sun, 06 Nov 1994 8:49:37 GMT"), None);
        assert_eq!(
            parse_imf_fixdate("Sun, 06 Nov 1994 08:49:37 GMT extra"),
            None
        );

        for width in 0..=8 {
            let year = "9".repeat(width);
            let value = format!("Sun, 06 Nov {year} 08:49:37 GMT");
            assert_eq!(
                parse_imf_fixdate(&value).is_some(),
                width == 4 && year == "9999"
            );
        }
        for day in 1..=31 {
            let value = format!("Sun, {day:02} Apr 2024 08:49:37 GMT");
            assert_eq!(parse_imf_fixdate(&value).is_some(), day <= 30);
        }
        assert_eq!(days_before_year(u64::MAX), None);
        assert_eq!(days_before_month(2024, 13), None);
        assert_eq!(days_in_month(2024, 13), None);
    }

    #[test]
    fn redirect_method_matrix_preserves_only_replayable_methods() {
        for status in [301, 302, 303, 307, 308] {
            let mut method = Method::POST;
            let mut body = Some(RequestBodyFactory::bytes(Vec::new()));
            redirect_method_and_body(status, &mut method, &mut body);
            if status == 301 || status == 302 || status == 303 {
                assert_eq!(method, Method::GET);
                assert!(body.is_none());
            } else {
                assert_eq!(method, Method::POST);
                assert!(body.is_some());
            }

            let mut method = Method::PUT;
            let mut body = Some(RequestBodyFactory::bytes(Vec::new()));
            redirect_method_and_body(status, &mut method, &mut body);
            if status == 303 {
                assert_eq!(method, Method::GET);
                assert!(body.is_none());
            } else {
                assert_eq!(method, Method::PUT);
                assert!(body.is_some());
            }
        }
    }

    #[test]
    fn retries_default_to_idempotent_methods() {
        assert!(is_retryable_method(&Method::GET, None, false));
        assert!(is_retryable_method(&Method::DELETE, None, false));
        assert!(!is_retryable_method(&Method::POST, None, false));
        assert!(!is_retryable_method(
            &Method::POST,
            Some(&RequestBodyFactory::single_use_bytes(Vec::new())),
            true
        ));
        assert!(is_retryable_method(
            &Method::POST,
            Some(&RequestBodyFactory::bytes(Vec::new())),
            true
        ));
    }

    #[test]
    fn exact_length_reader_rejects_short_and_long_sources() {
        let mut short = ExactLengthReader::new(Cursor::new(b"short".to_vec()), 10);
        let error = short
            .read_to_end(&mut Vec::new())
            .expect_err("short source");
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);

        let mut long = ExactLengthReader::new(Cursor::new(b"longer".to_vec()), 5);
        let error = long.read_to_end(&mut Vec::new()).expect_err("long source");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);

        let mut exact = ExactLengthReader::new(Cursor::new(b"exact".to_vec()), 5);
        let mut output = Vec::new();
        exact.read_to_end(&mut output).expect("exact source");
        assert_eq!(output, b"exact");
    }

    #[test]
    fn redirect_url_validation_rejects_parser_differentials() {
        let base = HttpUrl::parse("http://example.test/base/path").expect("base");
        for location in [
            r"relative\path",
            "/path/%5croute",
            "//user@host/path",
            "//user%40host/path",
            "http://user%40host/path",
        ] {
            assert_eq!(base.resolve(location), Err(TransportError::InvalidRedirect));
        }
        for raw in [
            "http://example.test/%5Croute",
            "http://user@example.test/path",
        ] {
            assert_eq!(HttpUrl::parse(raw), Err(TransportError::InvalidUrl));
        }
        assert_eq!(
            HttpUrl::parse("http://example.test/path?next=%5c"),
            Err(TransportError::InvalidUrl)
        );
    }

    #[test]
    fn zero_operation_deadline_is_rejected_before_network() {
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.operation_timeout = Some(Duration::ZERO);
        let error = HttpClient::new(config).expect_err("zero deadline");
        assert_eq!(error, TransportError::InvalidTimeout);
    }

    #[test]
    fn invalid_utf8_location_is_rejected() {
        let (listener, base) = listener();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = read_request(&mut stream);
            stream
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: /bad-\xff\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .expect("response");
        });
        let url = HttpUrl::parse(&format!("{base}/invalid-location")).expect("URL");
        let error = client()
            .execute(&HttpRequest::new(Method::GET, url))
            .expect_err("invalid location");
        assert_eq!(error, TransportError::InvalidRedirect);
        server.join().expect("server");
    }

    #[test]
    fn certificate_and_identity_sizes_are_bounded() {
        assert!(matches!(
            TlsRootCertificate::from_der(vec![0; 1024 * 1024 + 1]),
            Err(TransportError::InvalidCertificate)
        ));
        assert!(matches!(
            TlsClientIdentity::from_pem(vec![0; 1024 * 1024 + 1]),
            Err(TransportError::InvalidCertificate)
        ));
    }

    #[test]
    fn proxy_url_is_strict_and_redacted() {
        let proxy = ProxyUrl::parse("https://user:secret@proxy.example:8443").expect("proxy");
        let debug = format!("{proxy:?}");
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("proxy.example"));
        for invalid in [
            "http://proxy.example/path",
            "http://proxy.example?query",
            "http://proxy.example/%0a",
            "http://proxy.example\\route",
            "http://proxy.example#fragment",
        ] {
            assert_eq!(ProxyUrl::parse(invalid), Err(TransportError::InvalidProxy));
        }
        assert_eq!(
            ProxyUrl::parse("HTTP://PROXY.EXAMPLE:80").expect("normalized proxy"),
            ProxyUrl::parse("http://proxy.example/").expect("normalized proxy")
        );
        assert_eq!(
            ProxyUrl::parse("socks5://proxy.example").expect("default SOCKS port"),
            ProxyUrl::parse("socks5h://proxy.example:1080").expect("explicit SOCKS port")
        );
        assert_eq!(
            ConfiguredRequestHeaders::generic([("Proxy-Authorization", b"Basic secret".to_vec(),)]),
            Err(TransportError::InvalidHeader)
        );
    }

    #[test]
    fn explicit_proxy_handles_http_requests() {
        let (proxy_listener, proxy_base) = listener();
        let proxy = std::thread::spawn(move || {
            let (mut stream, _) = proxy_listener.accept().expect("proxy accept");
            let (line, _) = read_request_headers(&mut stream);
            assert!(line.starts_with("GET http://example.invalid/resource HTTP/1.1"));
            response(&mut stream, "200 OK", "", b"proxied");
        });
        let mut config = ClientConfig::default();
        config.connection_policy.proxy =
            ProxyPolicy::Explicit(ProxyUrl::parse(&proxy_base).expect("proxy URL"));
        config.retry.max_attempts = 1;
        let mut response = HttpClient::new(config)
            .expect("client")
            .execute(&HttpRequest::new(
                Method::GET,
                HttpUrl::parse("http://example.invalid/resource").expect("URL"),
            ))
            .expect("proxied request");
        let mut body = String::new();
        response.read_to_string(&mut body).expect("body");
        assert_eq!(body, "proxied");
        proxy.join().expect("proxy");
    }

    #[test]
    fn explicit_proxy_uses_connect_for_https() {
        let (proxy_listener, proxy_base) = listener();
        let proxy = std::thread::spawn(move || {
            let (mut stream, _) = proxy_listener.accept().expect("proxy accept");
            let (line, _) = read_request_headers(&mut stream);
            assert!(line.starts_with("CONNECT example.invalid:443 HTTP/1.1"));
            response(&mut stream, "502 Bad Gateway", "", b"");
        });
        let mut config = ClientConfig::default();
        config.connection_policy.proxy =
            ProxyPolicy::Explicit(ProxyUrl::parse(&proxy_base).expect("proxy URL"));
        config.retry.max_attempts = 1;
        let error = HttpClient::new(config)
            .expect("client")
            .execute(&HttpRequest::new(
                Method::GET,
                HttpUrl::parse("https://example.invalid/resource").expect("URL"),
            ))
            .expect_err("CONNECT rejected");
        assert!(matches!(
            error,
            TransportError::Connect | TransportError::Request(_)
        ));
        proxy.join().expect("proxy");
    }

    #[test]
    fn custom_root_pem_and_der_and_disabled_verification_build() {
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
        const ROOT_DER_HEX: &str = "3082018030820127a00302010202143c70d4bbd58bdfacaf4e615e3456557bfaa10a57300a06082a8648ce3d040302301d311b301906035504030c12527573746c7320526f6275737420526f6f743020170d3735303130313030303030305a180f34303936303130313030303030305a301d311b301906035504030c12527573746c7320526f6275737420526f6f743059301306072a8648ce3d020106082a8648ce3d0301070342000496fd590316ce6218342f1fc8da5d84f88ccc7aea3a3ee4258ac8cb783a54c37417ca7e7990f4b61e2bf3ec61f27cc158b3b9294fcd78bd8def129f261e8c63f7a3433041300f0603551d0f0101ff04050303078400301d0603551d0e041604146a63fa0c10d25b368ef2dfd88be865b7b43b34af300f0603551d130101ff040530030101ff300a06082a8648ce3d04030203470030440220451bae64948c0363bbd9cda45a293c097d5a1f2d071239218f813e1e3aebe694022035ffdfa9af2a6deacdc3a28d060ec9e8146262d54062bd4835a9d8a4732e1e37";

        let pem = TlsRootCertificate::from_pem_bundle(ROOT_PEM.to_vec()).expect("PEM");
        let der = TlsRootCertificate::from_der(decode_hex(ROOT_DER_HEX)).expect("DER");
        for tls in [
            TlsVerification::CustomRoots(TlsRootCertificates::new(vec![pem]).expect("PEM roots")),
            TlsVerification::CustomRoots(TlsRootCertificates::new(vec![der]).expect("DER roots")),
            TlsVerification::Disabled,
        ] {
            let mut config = ClientConfig::default();
            config.connection_policy.proxy = ProxyPolicy::Disabled;
            config.connection_policy.tls = tls;
            HttpClient::new(config).expect("TLS policy builds");
        }
    }

    #[test]
    fn client_cache_is_bounded_profiles_share_one_runtime_and_keepalive_is_a_key() {
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        let client = HttpClient::new(config).expect("client");
        let runtime_thread = client.runtime_thread_id();
        let origin = HttpUrl::parse("http://cache.example/")
            .expect("cache origin")
            .origin()
            .clone();
        for seconds in 1..=10 {
            client
                .clients_for(
                    &origin,
                    &HttpConnectionPolicy {
                        proxy: ProxyPolicy::Disabled,
                        tcp_keepalive: TcpKeepalivePolicy::Enabled(Duration::from_secs(seconds)),
                        ..HttpConnectionPolicy::default()
                    },
                    &ProxyTlsPolicy::default(),
                    HttpTimeoutPolicy::default(),
                )
                .expect("connection policy");
        }
        assert_eq!(client.cached_client_count(), MAX_CLIENT_CACHE_ENTRIES);
        assert_eq!(client.runtime_thread_id(), runtime_thread);

        client
            .clients_for(
                &origin,
                &HttpConnectionPolicy {
                    proxy: ProxyPolicy::Disabled,
                    tcp_keepalive: TcpKeepalivePolicy::Disabled,
                    ..HttpConnectionPolicy::default()
                },
                &ProxyTlsPolicy::default(),
                HttpTimeoutPolicy::default(),
            )
            .expect("disabled keepalive");
        assert_eq!(client.cached_client_count(), MAX_CLIENT_CACHE_ENTRIES);
        assert_eq!(client.runtime_thread_id(), runtime_thread);
    }

    #[test]
    fn streams_chunked_and_detects_truncation() {
        let (truncated_listener, base) = listener();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = truncated_listener.accept().expect("accept");
            let _ = read_request(&mut stream);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n")
                .expect("chunked response");
        });
        let url = HttpUrl::parse(&format!("{base}/chunked")).expect("URL");
        let mut response = client()
            .execute(&HttpRequest::new(Method::GET, url))
            .expect("send");
        let mut body = String::new();
        response.read_to_string(&mut body).expect("chunked body");
        assert_eq!(body, "hello");
        server.join().expect("server");

        let (body_listener, base) = listener();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = body_listener.accept().expect("accept");
            let _ = read_request(&mut stream);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nshort",
                )
                .expect("truncated response");
        });
        let url = HttpUrl::parse(&format!("{base}/truncated")).expect("URL");
        let mut response = client()
            .execute(&HttpRequest::new(Method::GET, url))
            .expect("send");
        let error = response
            .read_to_end(&mut Vec::new())
            .expect_err("truncation");
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        server.join().expect("server");
    }

    #[test]
    fn tls_handshake_timeout_is_phase_specific() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener");
        let address = listener.local_addr().expect("address");
        let server = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().expect("accept");
            std::thread::sleep(Duration::from_millis(100));
        });
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.connection_policy.tls = TlsVerification::Disabled;
        config.timeout_policy.tls_handshake_timeout = Duration::from_millis(30);
        config.request_timeout = Some(Duration::from_secs(1));
        config.operation_timeout = Some(Duration::from_secs(1));
        config.retry.max_attempts = 1;
        let error = HttpClient::new(config)
            .expect("client")
            .execute(&HttpRequest::new(
                Method::GET,
                HttpUrl::parse(&format!("https://{address}/stall")).expect("URL"),
            ))
            .expect_err("TLS stall");
        assert_eq!(error, TransportError::TlsHandshakeTimeout);
        server.join().expect("server");
    }

    #[test]
    fn rolling_activity_allows_healthy_response_longer_than_one_window() {
        let (listener, base) = listener();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = read_request(&mut stream);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\n")
                .expect("response head");
            for byte in b"abcdef" {
                std::thread::sleep(Duration::from_millis(20));
                stream.write_all(&[*byte]).expect("progress byte");
                stream.flush().expect("progress flush");
            }
        });
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.timeout_policy.activity_timeout = Some(Duration::from_millis(35));
        config.request_timeout = None;
        config.operation_timeout = None;
        config.retry.max_attempts = 1;
        let started = Instant::now();
        let mut response = HttpClient::new(config)
            .expect("client")
            .execute(&HttpRequest::new(
                Method::GET,
                HttpUrl::parse(&format!("{base}/progress")).expect("URL"),
            ))
            .expect("response");
        let mut body = Vec::new();
        response.read_to_end(&mut body).expect("rolling body");
        assert_eq!(body, b"abcdef");
        assert!(started.elapsed() > Duration::from_millis(100));
        server.join().expect("server");
    }

    #[test]
    fn stalled_response_body_hits_activity_timeout_without_total_deadline() {
        let (listener, base) = listener();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = read_request(&mut stream);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\n")
                .expect("response head");
            stream.flush().expect("response flush");
            std::thread::sleep(Duration::from_millis(100));
        });
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.timeout_policy.activity_timeout = Some(Duration::from_millis(30));
        config.request_timeout = None;
        config.operation_timeout = None;
        config.retry.max_attempts = 1;
        let mut response = HttpClient::new(config)
            .expect("client")
            .execute(&HttpRequest::new(
                Method::GET,
                HttpUrl::parse(&format!("{base}/stall")).expect("URL"),
            ))
            .expect("response head");
        let error = response
            .read_to_end(&mut Vec::new())
            .expect_err("activity stall");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        server.join().expect("server");
    }

    #[test]
    fn request_cancellation_wakes_stalled_response_without_any_timeout() {
        let (listener, base) = listener();
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
        let (closed_sender, closed_receiver) = std::sync::mpsc::sync_channel(1);
        let server = std::thread::spawn(move || {
            let (mut stalled, _) = listener.accept().expect("stalled accept");
            let _ = read_request(&mut stalled);
            stalled
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\n")
                .expect("stalled response head");
            stalled.flush().expect("stalled response flush");
            release_receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("release stalled response");
            drop(stalled);
            closed_sender.send(()).expect("stalled response closed");

            let (mut healthy, _) = listener.accept().expect("healthy accept");
            let _ = read_request(&mut healthy);
            response(&mut healthy, "200 OK", "", b"next");
        });
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.timeout_policy.activity_timeout = None;
        config.request_timeout = None;
        config.operation_timeout = None;
        config.retry.max_attempts = 1;
        let client = HttpClient::new(config).expect("client");
        let cancellation = RequestBodyCancellation::new();
        let mut stalled = client
            .execute(
                &HttpRequest::new(
                    Method::GET,
                    HttpUrl::parse(&format!("{base}/stalled-download")).expect("URL"),
                )
                .with_cancellation(cancellation.clone()),
            )
            .expect("response head");
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            cancellation.cancel();
        });
        let started = Instant::now();
        let error = stalled
            .read_to_end(&mut Vec::new())
            .expect_err("cancelled response");
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
        assert!(started.elapsed() < Duration::from_secs(1));
        canceller.join().expect("canceller");
        drop(stalled);
        release_sender.send(()).expect("release server");
        closed_receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("stalled server closed");

        let mut healthy = client
            .execute(&HttpRequest::new(
                Method::GET,
                HttpUrl::parse(&format!("{base}/next-wave")).expect("URL"),
            ))
            .expect("next response");
        let mut body = Vec::new();
        healthy.read_to_end(&mut body).expect("next body");
        assert_eq!(body, b"next");
        server.join().expect("server");
    }

    #[test]
    fn cancellation_terminates_a_response_pump_with_a_full_single_slot_queue() {
        let (listener, base) = listener();
        let server = std::thread::spawn(move || {
            let (mut filled, _) = listener.accept().expect("filled accept");
            let _ = read_request(&mut filled);
            let body = vec![b'x'; BODY_BRIDGE_CHUNK_BYTES * 3];
            write!(
                filled,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .expect("filled response head");
            filled.write_all(&body).expect("filled response body");
            filled.flush().expect("filled response flush");

            let (mut healthy, _) = listener.accept().expect("healthy accept");
            let _ = read_request(&mut healthy);
            response(&mut healthy, "200 OK", "", b"next");
        });
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.timeout_policy.activity_timeout = None;
        config.request_timeout = None;
        config.operation_timeout = None;
        config.retry.max_attempts = 1;
        let client = HttpClient::new(config).expect("client");
        let cancellation = RequestBodyCancellation::new();
        let mut filled = client
            .execute(
                &HttpRequest::new(
                    Method::GET,
                    HttpUrl::parse(&format!("{base}/filled")).expect("URL"),
                )
                .with_cancellation(cancellation.clone()),
            )
            .expect("filled response head");
        let queue_deadline = Instant::now() + Duration::from_secs(1);
        while filled.response.queued_messages() != RESPONSE_BODY_BRIDGE_QUEUE_DEPTH {
            assert!(
                Instant::now() < queue_deadline,
                "response bridge queue did not become full"
            );
            std::thread::yield_now();
        }
        cancellation.cancel();
        let error = filled
            .read_to_end(&mut Vec::new())
            .expect_err("cancelled full response queue");
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
        drop(filled);

        let mut healthy = client
            .execute(&HttpRequest::new(
                Method::GET,
                HttpUrl::parse(&format!("{base}/next")).expect("URL"),
            ))
            .expect("next response");
        let mut body = Vec::new();
        healthy.read_to_end(&mut body).expect("next body");
        assert_eq!(body, b"next");
        server.join().expect("server");
    }

    #[test]
    fn request_cancellation_wakes_response_wait_after_upload_without_any_timeout() {
        let (listener, base) = listener();
        let (consumed_sender, consumed_receiver) = std::sync::mpsc::sync_channel(1);
        let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
        let (closed_sender, closed_receiver) = std::sync::mpsc::sync_channel(1);
        let server = std::thread::spawn(move || {
            let (mut stalled, _) = listener.accept().expect("stalled accept");
            let (_, body) = read_request(&mut stalled);
            assert_eq!(body, b"complete upload");
            consumed_sender.send(()).expect("upload consumed");
            release_receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("release stalled upload");
            drop(stalled);
            closed_sender.send(()).expect("stalled upload closed");

            let (mut healthy, _) = listener.accept().expect("healthy accept");
            let _ = read_request(&mut healthy);
            response(&mut healthy, "200 OK", "", b"next");
        });
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.timeout_policy.activity_timeout = None;
        config.request_timeout = None;
        config.operation_timeout = None;
        config.retry.max_attempts = 1;
        let client = HttpClient::new(config).expect("client");
        let cancellation = RequestBodyCancellation::new();
        let worker_cancellation = cancellation.clone();
        let canceller = std::thread::spawn(move || {
            consumed_receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("server consumed upload");
            worker_cancellation.cancel();
        });
        let started = Instant::now();
        let error = client
            .execute(
                &HttpRequest::new(
                    Method::PUT,
                    HttpUrl::parse(&format!("{base}/stalled-upload")).expect("URL"),
                )
                .with_body(RequestBodyFactory::bytes(b"complete upload".to_vec()))
                .with_cancellation(cancellation),
            )
            .expect_err("cancelled response wait");
        assert_eq!(error, TransportError::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(1));
        canceller.join().expect("canceller");
        release_sender.send(()).expect("release server");
        closed_receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("stalled server closed");

        let mut healthy = client
            .execute(&HttpRequest::new(
                Method::GET,
                HttpUrl::parse(&format!("{base}/next-wave")).expect("URL"),
            ))
            .expect("next response");
        let mut body = Vec::new();
        healthy.read_to_end(&mut body).expect("next body");
        assert_eq!(body, b"next");
        server.join().expect("server");
    }

    #[test]
    fn zero_length_bodies_are_terminal_and_verified_before_network() {
        let empty_sha256: [u8; 32] = sha2::Sha256::digest([]).into();
        let (path, file) = test_body_file(b"");
        let (listener, base) = listener();
        let server = std::thread::spawn(move || {
            for expected_path in ["/empty-bytes", "/empty-file"] {
                let (mut stream, _) = listener.accept().expect("accept");
                let (line, body) = read_request(&mut stream);
                assert!(line.starts_with(&format!("PUT {expected_path} HTTP/1.1")));
                assert!(body.is_empty());
                response(&mut stream, "200 OK", "", b"");
            }
        });
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.retry.max_attempts = 1;
        let client = HttpClient::new(config).expect("client");
        let bytes_response = client
            .execute(
                &HttpRequest::new(
                    Method::PUT,
                    HttpUrl::parse(&format!("{base}/empty-bytes")).expect("URL"),
                )
                .with_body(RequestBodyFactory::bytes(Vec::new())),
            )
            .expect("empty bytes response");
        assert_eq!(bytes_response.status(), 200);

        let open_file = Arc::clone(&file);
        let file_response = client
            .execute(
                &HttpRequest::new(
                    Method::PUT,
                    HttpUrl::parse(&format!("{base}/empty-file")).expect("URL"),
                )
                .with_body(RequestBodyFactory::verified_regular_file(
                    0,
                    empty_sha256,
                    RequestBodyCancellation::new(),
                    move || open_file.try_clone(),
                )),
            )
            .expect("empty file response");
        assert_eq!(file_response.status(), 200);
        server.join().expect("server");
        drop(file);
        fs::remove_file(path).expect("remove body fixture");
    }

    #[test]
    fn zero_length_verified_body_with_wrong_digest_fails_before_network() {
        let (path, file) = test_body_file(b"");
        let (listener, base) = listener();
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let open_file = Arc::clone(&file);
        let body = RequestBodyFactory::verified_regular_file(
            0,
            [0_u8; 32],
            RequestBodyCancellation::new(),
            move || open_file.try_clone(),
        );
        let error = client()
            .execute(
                &HttpRequest::new(
                    Method::PUT,
                    HttpUrl::parse(&format!("{base}/wrong-empty-digest")).expect("URL"),
                )
                .with_body(body),
            )
            .expect_err("wrong empty digest");
        assert_eq!(
            error,
            TransportError::BodyFactory(io::ErrorKind::InvalidData)
        );
        assert_eq!(
            listener.accept().expect_err("no network connection").kind(),
            io::ErrorKind::WouldBlock
        );
        drop(file);
        fs::remove_file(path).expect("remove body fixture");
    }

    #[test]
    fn header_and_body_bounds_are_enforced() {
        let (header_listener, base) = listener();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = header_listener.accept().expect("accept");
            let _ = read_request(&mut stream);
            response(&mut stream, "200 OK", "X-Large: value\r\n", b"too-large");
        });
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.max_response_headers = 16;
        config.max_response_header_bytes = 4;
        let url = HttpUrl::parse(&format!("{base}/bounds")).expect("URL");
        let error = HttpClient::new(config)
            .expect("client")
            .execute(&HttpRequest::new(Method::GET, url))
            .expect_err("header bounds");
        assert_eq!(error, TransportError::ResponseHeadersTooLarge);
        server.join().expect("server");

        let (body_listener, base) = listener();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = body_listener.accept().expect("accept");
            let _ = read_request(&mut stream);
            response(&mut stream, "200 OK", "", b"0123456789");
        });
        let mut config = ClientConfig::default();
        config.connection_policy.proxy = ProxyPolicy::Disabled;
        config.retry.max_attempts = 1;
        config.max_response_body_bytes = 5;
        let url = HttpUrl::parse(&format!("{base}/body-limit")).expect("URL");
        let error = HttpClient::new(config)
            .expect("client")
            .execute(&HttpRequest::new(Method::GET, url))
            .expect_err("body bounds");
        assert_eq!(error, TransportError::BodyTooLarge);
        server.join().expect("server");
    }
}
