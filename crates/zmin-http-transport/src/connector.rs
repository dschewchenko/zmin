use std::fmt;
use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bytes::Bytes;
use http::Uri;
use http::header::HeaderValue;
use http::uri::Scheme;
use hyper_util::client::legacy::connect::{Connected, Connection, HttpConnector};
use hyper_util::rt::TokioIo;
use pin_project_lite::pin_project;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use rustls_platform_verifier::BuilderVerifierExt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tower_service::Service;

use crate::activity_io::ActivityIo;
use crate::{
    HttpConnectionPolicy, HttpTimeoutPolicy, ProxyPolicy, ProxyTlsPolicy, TcpKeepalivePolicy,
    TlsVerification, TransportError,
};

const MAX_CONNECT_RESPONSE_BYTES: usize = 8 * 1024;
const MAX_CONNECT_REQUEST_BYTES: usize = 16 * 1024;

struct SecretBuffer(Vec<u8>);

impl SecretBuffer {
    fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl Deref for SecretBuffer {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SecretBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for SecretBuffer {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Clone)]
pub(crate) struct ExactConnector {
    http: HttpConnector,
    proxy: Option<ProxyRoute>,
    timeouts: HttpTimeoutPolicy,
    tls: Arc<rustls::ClientConfig>,
    proxy_tls: Arc<rustls::ClientConfig>,
    connect_user_agent: Option<HeaderValue>,
}

#[derive(Clone)]
struct ProxyRoute {
    proxy: crate::ProxyUrl,
    uri: Uri,
}

impl ProxyRoute {
    fn new(proxy: crate::ProxyUrl) -> Result<Self, TransportError> {
        let uri = proxy
            .origin_str()
            .parse()
            .map_err(|_| TransportError::InvalidProxy)?;
        Ok(Self { proxy, uri })
    }

    fn basic_authorization(&self) -> Option<HeaderValue> {
        let (username, password) = self.proxy.credentials()?;
        let mut plain = SecretBuffer::new(Vec::with_capacity(username.len() + password.len() + 1));
        plain.extend_from_slice(username);
        plain.push(b':');
        plain.extend_from_slice(password);
        let encoded_len = plain.len().div_ceil(3) * 4;
        let mut encoded = SecretBuffer::new(vec![0_u8; 6 + encoded_len]);
        encoded[..6].copy_from_slice(b"Basic ");
        let amount = BASE64_STANDARD
            .encode_slice(plain.as_slice(), &mut encoded[6..])
            .expect("bounded base64 destination");
        encoded.truncate(6 + amount);
        let mut header = HeaderValue::from_bytes(encoded.as_slice())
            .expect("base64 proxy authorization is a valid header");
        header.set_sensitive(true);
        Some(header)
    }
}

impl fmt::Debug for ExactConnector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExactConnector")
            .field("proxy", &self.proxy.is_some())
            .field("timeouts", &self.timeouts)
            .field("connect_user_agent", &self.connect_user_agent.is_some())
            .finish()
    }
}

impl ExactConnector {
    pub(crate) fn new(
        policy: &HttpConnectionPolicy,
        proxy_tls_policy: &ProxyTlsPolicy,
        timeouts: HttpTimeoutPolicy,
        user_agent: Option<&str>,
        with_identity: bool,
    ) -> Result<Self, TransportError> {
        let timeouts = timeouts.validate()?;
        let mut http = HttpConnector::new();
        http.enforce_http(false);
        http.set_nodelay(true);
        match policy.tcp_keepalive {
            TcpKeepalivePolicy::Default => {}
            TcpKeepalivePolicy::Disabled => http.set_keepalive(None),
            TcpKeepalivePolicy::Enabled(timeout) => http.set_keepalive(Some(timeout)),
        }
        let proxy = match &policy.proxy {
            ProxyPolicy::Disabled => None,
            ProxyPolicy::System => return Err(TransportError::InvalidProxy),
            ProxyPolicy::Explicit(proxy) => Some(ProxyRoute::new(proxy.clone())?),
        };
        let tls = build_tls_config(policy, with_identity, true)?;
        // Proxy TLS is a separate security boundary. Target-specific trust
        // relaxation, custom roots, and mTLS identity must never be applied
        // to an HTTPS proxy selected by the environment or Git config.
        let proxy_tls = build_proxy_tls_config(proxy_tls_policy)?;
        let connect_user_agent = user_agent
            .map(HeaderValue::from_str)
            .transpose()
            .map_err(|_| TransportError::InvalidHeader)?;
        Ok(Self {
            http,
            proxy,
            timeouts,
            tls: Arc::new(tls),
            proxy_tls: Arc::new(proxy_tls),
            connect_user_agent,
        })
    }

    pub(crate) fn forward_proxy_authorization(&self, target: &Uri) -> Option<HeaderValue> {
        if target.scheme() != Some(&Scheme::HTTP) {
            return None;
        }
        let proxy = self.proxy.as_ref()?;
        if !matches!(proxy.proxy.scheme(), "http" | "https") {
            return None;
        }
        proxy.basic_authorization()
    }

    async fn connect(self, target: Uri) -> Result<TokioIo<ConnectedIo>, ConnectError> {
        let intercepted = self.proxy.as_ref();
        let dial_target = intercepted.map_or_else(|| target.clone(), |proxy| proxy.uri.clone());
        let mut http = self.http.clone();
        let tcp = tokio::time::timeout(self.timeouts.dial_timeout, http.call(dial_target))
            .await
            .map_err(|_| ConnectError::DialTimeout)?
            .map_err(|_| ConnectError::Connect)?
            .into_inner();
        let activity = ActivityIo::new(tcp, self.timeouts.activity_timeout);
        let mut proxy_stream = ProxyStream::Plain { inner: activity };
        let mut forward_proxy = false;
        let mut tunnel_prefix = Vec::new();

        if let Some(proxy) = intercepted.as_ref() {
            match proxy.proxy.scheme() {
                "socks5" | "socks5h" => {
                    socks5_handshake(&mut proxy_stream, &target, proxy).await?;
                }
                "http" | "https" => {
                    if proxy.proxy.scheme() == "https" {
                        let host = proxy.proxy.host();
                        let tls = tls_handshake(
                            proxy_stream.into_plain()?,
                            host,
                            self.proxy_tls.clone(),
                            self.timeouts.tls_handshake_timeout,
                        )
                        .await?;
                        proxy_stream = ProxyStream::Tls { inner: tls };
                    }
                    if target.scheme() == Some(&Scheme::HTTPS) {
                        let authorization = proxy.basic_authorization();
                        tunnel_prefix = connect_tunnel(
                            &mut proxy_stream,
                            &target,
                            authorization.as_ref(),
                            self.connect_user_agent.as_ref(),
                        )
                        .await?;
                    } else {
                        forward_proxy = true;
                    }
                }
                _ => return Err(ConnectError::InvalidProxy),
            }
        }

        let proxy_stream = BufferedProxyStream::new(proxy_stream, tunnel_prefix);
        let (stream, negotiated_h2) = if target.scheme() == Some(&Scheme::HTTPS) {
            let host = target.host().ok_or(ConnectError::InvalidTarget)?;
            let tls = tls_handshake(
                proxy_stream,
                host,
                self.tls,
                self.timeouts.tls_handshake_timeout,
            )
            .await?;
            let negotiated_h2 = tls.get_ref().1.alpn_protocol() == Some(b"h2");
            (FinalStream::Tls { inner: tls }, negotiated_h2)
        } else {
            (
                FinalStream::Plain {
                    inner: proxy_stream,
                },
                false,
            )
        };
        Ok(TokioIo::new(ConnectedIo {
            inner: stream,
            forward_proxy,
            negotiated_h2,
        }))
    }
}

impl Service<Uri> for ExactConnector {
    type Response = TokioIo<ConnectedIo>;
    type Error = ConnectError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, target: Uri) -> Self::Future {
        Box::pin(self.clone().connect(target))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectError {
    DialTimeout,
    TlsHandshakeTimeout,
    ActivityTimeout,
    Connect,
    InvalidProxy,
    InvalidTarget,
    ProxyHandshake,
    Tls,
}

impl fmt::Display for ConnectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DialTimeout => "HTTP dial timed out",
            Self::TlsHandshakeTimeout => "HTTP TLS handshake timed out",
            Self::ActivityTimeout => "HTTP TCP activity timed out",
            Self::Connect => "HTTP connection failed",
            Self::InvalidProxy => "invalid HTTP proxy",
            Self::InvalidTarget => "invalid HTTP target",
            Self::ProxyHandshake => "HTTP proxy handshake failed",
            Self::Tls => "HTTP TLS handshake failed",
        })
    }
}

impl std::error::Error for ConnectError {}

pin_project! {
    #[project = ProxyStreamProj]
    enum ProxyStream {
        Plain { #[pin] inner: ActivityIo<TcpStream> },
        Tls { #[pin] inner: TlsStream<ActivityIo<TcpStream>> },
    }
}

impl ProxyStream {
    fn into_plain(self) -> Result<ActivityIo<TcpStream>, ConnectError> {
        match self {
            Self::Plain { inner } => Ok(inner),
            Self::Tls { .. } => Err(ConnectError::InvalidProxy),
        }
    }
}

pin_project! {
    struct BufferedProxyStream {
        #[pin]
        inner: ProxyStream,
        prefix: Bytes,
    }
}

impl BufferedProxyStream {
    fn new(inner: ProxyStream, prefix: Vec<u8>) -> Self {
        Self {
            inner,
            prefix: Bytes::from(prefix),
        }
    }
}

impl AsyncRead for BufferedProxyStream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.project();
        if !this.prefix.is_empty() && buffer.remaining() != 0 {
            let amount = this.prefix.len().min(buffer.remaining());
            buffer.put_slice(&this.prefix.split_to(amount));
            return Poll::Ready(Ok(()));
        }
        this.inner.poll_read(context, buffer)
    }
}

impl AsyncWrite for BufferedProxyStream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.project().inner.poll_write(context, buffer)
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().inner.poll_flush(context)
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().inner.poll_shutdown(context)
    }
}

pin_project! {
    #[project = FinalStreamProj]
    enum FinalStream {
        Plain { #[pin] inner: BufferedProxyStream },
        Tls { #[pin] inner: TlsStream<BufferedProxyStream> },
    }
}

pin_project! {
    pub(crate) struct ConnectedIo {
        #[pin]
        inner: FinalStream,
        forward_proxy: bool,
        negotiated_h2: bool,
    }
}

macro_rules! delegate_async_read {
    ($projection:expr, $context:expr, $buffer:expr) => {
        match $projection {
            ProxyStreamProj::Plain { inner } => inner.poll_read($context, $buffer),
            ProxyStreamProj::Tls { inner } => inner.poll_read($context, $buffer),
        }
    };
}

impl AsyncRead for ProxyStream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        delegate_async_read!(self.project(), context, buffer)
    }
}

impl AsyncWrite for ProxyStream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.project() {
            ProxyStreamProj::Plain { inner } => inner.poll_write(context, buffer),
            ProxyStreamProj::Tls { inner } => inner.poll_write(context, buffer),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.project() {
            ProxyStreamProj::Plain { inner } => inner.poll_flush(context),
            ProxyStreamProj::Tls { inner } => inner.poll_flush(context),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.project() {
            ProxyStreamProj::Plain { inner } => inner.poll_shutdown(context),
            ProxyStreamProj::Tls { inner } => inner.poll_shutdown(context),
        }
    }
}

impl AsyncRead for FinalStream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.project() {
            FinalStreamProj::Plain { inner } => inner.poll_read(context, buffer),
            FinalStreamProj::Tls { inner } => inner.poll_read(context, buffer),
        }
    }
}

impl AsyncWrite for FinalStream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.project() {
            FinalStreamProj::Plain { inner } => inner.poll_write(context, buffer),
            FinalStreamProj::Tls { inner } => inner.poll_write(context, buffer),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.project() {
            FinalStreamProj::Plain { inner } => inner.poll_flush(context),
            FinalStreamProj::Tls { inner } => inner.poll_flush(context),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.project() {
            FinalStreamProj::Plain { inner } => inner.poll_shutdown(context),
            FinalStreamProj::Tls { inner } => inner.poll_shutdown(context),
        }
    }
}

impl AsyncRead for ConnectedIo {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.project().inner.poll_read(context, buffer)
    }
}

impl AsyncWrite for ConnectedIo {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.project().inner.poll_write(context, buffer)
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().inner.poll_flush(context)
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().inner.poll_shutdown(context)
    }
}

impl Connection for ConnectedIo {
    fn connected(&self) -> Connected {
        let mut connected = Connected::new().proxy(self.forward_proxy);
        if self.negotiated_h2 {
            connected = connected.negotiated_h2();
        }
        connected
    }
}

async fn tls_handshake<T>(
    stream: T,
    host: &str,
    config: Arc<rustls::ClientConfig>,
    timeout: std::time::Duration,
) -> Result<TlsStream<T>, ConnectError>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let server_name =
        ServerName::try_from(host.to_owned()).map_err(|_| ConnectError::InvalidTarget)?;
    match tokio::time::timeout(
        timeout,
        TlsConnector::from(config).connect(server_name, stream),
    )
    .await
    {
        Err(_) => Err(ConnectError::TlsHandshakeTimeout),
        Ok(Err(error)) if error.kind() == io::ErrorKind::TimedOut => {
            Err(ConnectError::ActivityTimeout)
        }
        Ok(Err(_)) => Err(ConnectError::Tls),
        Ok(Ok(stream)) => Ok(stream),
    }
}

async fn connect_tunnel<T>(
    stream: &mut T,
    target: &Uri,
    authorization: Option<&HeaderValue>,
    user_agent: Option<&HeaderValue>,
) -> Result<Vec<u8>, ConnectError>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let host = target.host().ok_or(ConnectError::InvalidTarget)?;
    let port = effective_port(target)?;
    let mut request = SecretBuffer::new(
        format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n").into_bytes(),
    );
    if let Some(authorization) = authorization {
        request.extend_from_slice(b"Proxy-Authorization: ");
        request.extend_from_slice(authorization.as_bytes());
        request.extend_from_slice(b"\r\n");
    }
    if let Some(user_agent) = user_agent {
        request.extend_from_slice(b"User-Agent: ");
        request.extend_from_slice(user_agent.as_bytes());
        request.extend_from_slice(b"\r\n");
    }
    request.extend_from_slice(b"\r\n");
    if request.len() > MAX_CONNECT_REQUEST_BYTES {
        return Err(ConnectError::ProxyHandshake);
    }
    stream
        .write_all(request.as_slice())
        .await
        .map_err(map_activity_or_proxy)?;
    stream.flush().await.map_err(map_activity_or_proxy)?;

    let mut response = [0_u8; MAX_CONNECT_RESPONSE_BYTES];
    let mut length = 0_usize;
    loop {
        if length == response.len() {
            return Err(ConnectError::ProxyHandshake);
        }
        let amount = stream
            .read(&mut response[length..])
            .await
            .map_err(map_activity_or_proxy)?;
        if amount == 0 {
            return Err(ConnectError::ProxyHandshake);
        }
        length += amount;
        let received = &response[..length];
        if let Some(index) = received.windows(4).position(|window| window == b"\r\n\r\n") {
            if received.starts_with(b"HTTP/1.1 200 ")
                || received.starts_with(b"HTTP/1.0 200 ")
                || received.starts_with(b"HTTP/1.1 200\r\n")
                || received.starts_with(b"HTTP/1.0 200\r\n")
            {
                return Ok(received[index + 4..].to_vec());
            }
            return Err(ConnectError::ProxyHandshake);
        }
    }
}

async fn socks5_handshake<T>(
    stream: &mut T,
    target: &Uri,
    proxy: &ProxyRoute,
) -> Result<(), ConnectError>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let credentials = proxy.proxy.credentials();
    let methods: &[u8] = if credentials.is_some() {
        &[5, 2, 0, 2]
    } else {
        &[5, 1, 0]
    };
    stream
        .write_all(methods)
        .await
        .map_err(map_activity_or_proxy)?;
    let mut negotiation = [0_u8; 2];
    stream
        .read_exact(&mut negotiation)
        .await
        .map_err(map_activity_or_proxy)?;
    if negotiation[0] != 5 {
        return Err(ConnectError::ProxyHandshake);
    }
    match (negotiation[1], credentials) {
        (0, _) => {}
        (2, Some((username, password))) => {
            let username_length =
                u8::try_from(username.len()).map_err(|_| ConnectError::InvalidProxy)?;
            let password_length =
                u8::try_from(password.len()).map_err(|_| ConnectError::InvalidProxy)?;
            let mut authentication =
                SecretBuffer::new(Vec::with_capacity(username.len() + password.len() + 3));
            authentication.extend_from_slice(&[1, username_length]);
            authentication.extend_from_slice(username);
            authentication.push(password_length);
            authentication.extend_from_slice(password);
            stream
                .write_all(authentication.as_slice())
                .await
                .map_err(map_activity_or_proxy)?;
            let mut result = [0_u8; 2];
            stream
                .read_exact(&mut result)
                .await
                .map_err(map_activity_or_proxy)?;
            if result != [1, 0] {
                return Err(ConnectError::ProxyHandshake);
            }
        }
        _ => return Err(ConnectError::ProxyHandshake),
    }

    let host = target
        .host()
        .ok_or(ConnectError::InvalidTarget)?
        .trim_start_matches('[')
        .trim_end_matches(']');
    let port = effective_port(target)?;
    let mut request = Vec::with_capacity(host.len() + 22);
    request.extend_from_slice(&[5, 1, 0]);
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => {
            request.push(1);
            request.extend_from_slice(&address.octets());
        }
        Ok(IpAddr::V6(address)) => {
            request.push(4);
            request.extend_from_slice(&address.octets());
        }
        Err(_) => {
            let length = u8::try_from(host.len()).map_err(|_| ConnectError::InvalidTarget)?;
            request.extend_from_slice(&[3, length]);
            request.extend_from_slice(host.as_bytes());
        }
    }
    request.extend_from_slice(&port.to_be_bytes());
    stream
        .write_all(&request)
        .await
        .map_err(map_activity_or_proxy)?;

    let mut response = [0_u8; 4];
    stream
        .read_exact(&mut response)
        .await
        .map_err(map_activity_or_proxy)?;
    if response[0] != 5 || response[1] != 0 || response[2] != 0 {
        return Err(ConnectError::ProxyHandshake);
    }
    let remaining = match response[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut length = [0_u8; 1];
            stream
                .read_exact(&mut length)
                .await
                .map_err(map_activity_or_proxy)?;
            usize::from(length[0])
        }
        _ => return Err(ConnectError::ProxyHandshake),
    };
    let mut bound_address = vec![0_u8; remaining + 2];
    stream
        .read_exact(&mut bound_address)
        .await
        .map_err(map_activity_or_proxy)?;
    Ok(())
}

fn effective_port(uri: &Uri) -> Result<u16, ConnectError> {
    uri.port_u16()
        .or_else(|| match uri.scheme_str() {
            Some("http") => Some(80),
            Some("https") => Some(443),
            _ => None,
        })
        .ok_or(ConnectError::InvalidTarget)
}

fn map_activity_or_proxy(error: io::Error) -> ConnectError {
    if error.kind() == io::ErrorKind::TimedOut {
        ConnectError::ActivityTimeout
    } else {
        ConnectError::ProxyHandshake
    }
}

fn build_tls_config(
    policy: &HttpConnectionPolicy,
    with_identity: bool,
    target_alpn: bool,
) -> Result<rustls::ClientConfig, TransportError> {
    if with_identity && policy.client_identity.is_some() {
        return Err(TransportError::UnsupportedClientIdentity);
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|_| TransportError::InvalidCertificate)?;
    let builder = match &policy.tls {
        TlsVerification::Platform => builder
            .with_platform_verifier()
            .map_err(|_| TransportError::InvalidCertificate)?,
        TlsVerification::CustomRoots(roots) => builder.with_root_certificates(roots.to_rustls()?),
        TlsVerification::Disabled => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoCertificateVerification)),
    };
    let mut config = builder.with_no_client_auth();
    config.alpn_protocols = if target_alpn {
        vec![b"h2".to_vec(), b"http/1.1".to_vec()]
    } else {
        Vec::new()
    };
    Ok(config)
}

fn build_proxy_tls_config(policy: &ProxyTlsPolicy) -> Result<rustls::ClientConfig, TransportError> {
    if policy.client_identity.is_some() {
        return Err(TransportError::UnsupportedClientIdentity);
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|_| TransportError::InvalidCertificate)?;
    let builder = match &policy.tls {
        TlsVerification::Platform => builder
            .with_platform_verifier()
            .map_err(|_| TransportError::InvalidCertificate)?,
        TlsVerification::CustomRoots(roots) => builder.with_root_certificates(roots.to_rustls()?),
        TlsVerification::Disabled => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoCertificateVerification)),
    };
    Ok(builder.with_no_client_auth())
}

#[derive(Debug)]
struct NoCertificateVerification;

impl ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _certificate: &CertificateDer<'_>,
        _signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _certificate: &CertificateDer<'_>,
        _signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA1,
            SignatureScheme::ECDSA_SHA1_Legacy,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }
}
