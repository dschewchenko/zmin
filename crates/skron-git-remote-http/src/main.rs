use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::{Buf, Bytes, BytesMut};
use clap::{Parser, ValueEnum};
use reqwest::blocking::{Body, Client};
use reqwest::header::{AUTHORIZATION, HeaderName, HeaderValue, PROXY_AUTHORIZATION};
use reqwest::redirect::Policy;
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime, pem::PemObject};
use rustls_platform_verifier::BuilderVerifierExt;

const AUTO_RESPONSE_FILE_THRESHOLD: u64 = 8 * 1024 * 1024;
const STREAM_BUFFER_SIZE: usize = 32 * 1024;
const STDIO_BUFFER_SIZE: usize = STREAM_BUFFER_SIZE;
const FILE_STREAM_WRITE_BUFFER_SIZE: usize = STREAM_BUFFER_SIZE;
const HTTP_LINE_BUFFER_CAPACITY: usize = 1024;
const HTTP_LINE_BUFFER_RETAIN_CAPACITY_LIMIT: usize = 16 * 1024;
const HTTP_RESPONSE_LINE_LIMIT: usize = 64 * 1024;
const HTTP_INFORMATIONAL_RESPONSE_LIMIT: usize = 8;
const HTTP_CHUNK_TRAILER_LIMIT: usize = 128;
const BATCH_REQUEST_LINE_LIMIT: usize = 64 * 1024;
const BATCH_REQUEST_HEADER_CAPACITY: usize = 4;
const BATCH_REQUEST_HEADER_LIMIT: usize = 128;
const COLLECTED_RESPONSE_HEADER_LIMIT: usize = 128;
const COLLECTED_RESPONSE_HEADER_BYTES_LIMIT: usize = 64 * 1024;
const REDIRECT_RESPONSE_HEADER_CAPACITY: usize = 1;
const HTTP_ORIGIN_CACHE_CAPACITY_HINT: usize = 4;
const HTTP_ORIGIN_MEMORY_ENTRY_LIMIT: usize = 16;
const HTTP_CONNECTION_POOL_CAPACITY_HINT: usize = 4;
const PLAIN_HTTP1_CONNECTION_POOL_ENTRY_LIMIT: usize = 16;
const AUTO_HTTP3_CONNECT_TIMEOUT: Duration = Duration::from_millis(25);
const AUTO_HTTP3_FAILURE_CACHE_TTL_SECS: u64 = 5 * 60;
const AUTO_HTTP3_FAILURE_CACHE_TIMESTAMP_BUF_LEN: usize = 32;
const HTTP3_CONNECTION_POOL_ENTRY_LIMIT: usize = 16;
const PLAIN_HTTP1_REQUEST_HEAD_INITIAL_CAPACITY: usize = 1024;
const PLAIN_HTTP1_REQUEST_HEAD_RETAIN_CAPACITY_LIMIT: usize = 64 * 1024;
const DEFAULT_TLS_VERIFICATION_IDENTITY: &str = "platform";
const NONE_IDENTITY: &str = "<none>";
const PROXY_ENV_VARS: &[&str] = &[
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
];

struct PhaseTrace {
    name: &'static str,
    start: Instant,
}

impl Drop for PhaseTrace {
    fn drop(&mut self) {
        let Some(path) = std::env::var_os("SKRON_REMOTE_HTTP_PHASE_TRACE_FILE") else {
            return;
        };
        let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
            return;
        };
        let _ = writeln!(
            file,
            "skron-remote-http-phase\t{}\tseconds={:.6}",
            self.name,
            self.start.elapsed().as_secs_f64()
        );
    }
}

fn phase_trace(name: &'static str) -> Option<PhaseTrace> {
    std::env::var_os("SKRON_REMOTE_HTTP_PHASE_TRACE_FILE").map(|_| PhaseTrace {
        name,
        start: Instant::now(),
    })
}

fn trace_event(message: impl std::fmt::Display) {
    let Some(path) = std::env::var_os("SKRON_REMOTE_HTTP_PHASE_TRACE_FILE") else {
        return;
    };
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(file, "skron-remote-http-event\t{message}");
}

#[derive(Debug, Parser)]
#[command(
    name = "skron-git-remote-http",
    about = "Rust-native Git HTTP transport helper"
)]
struct Args {
    #[arg(long, value_enum, default_value_t = HttpVersion::Auto)]
    http_version: HttpVersion,

    #[arg(long, default_value_t = 90)]
    pool_idle_timeout_secs: u64,

    #[arg(long, default_value_t = 8)]
    pool_max_idle_per_host: usize,

    #[arg(long)]
    method: Option<String>,

    #[arg(long)]
    url: Option<String>,

    #[arg(long = "header")]
    headers: Vec<String>,

    #[arg(long)]
    body_file: Option<String>,

    #[arg(long)]
    output_file: Option<String>,

    #[arg(long)]
    ca_file: Option<String>,

    #[arg(long)]
    client_cert_file: Option<String>,

    #[arg(long)]
    client_key_file: Option<String>,

    #[arg(long)]
    tls_no_verify: bool,

    #[arg(long)]
    batch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum HttpVersion {
    Auto,
    Http1,
    Http2,
    Http3,
}

struct TransportResponse {
    version: &'static str,
    status: ResponseStatus,
    headers: Vec<(String, String)>,
    reusable_connection: bool,
    body: ResponseBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResponseStatus {
    Static(&'static str),
    Owned(String),
}

impl ResponseStatus {
    fn as_str(&self) -> &str {
        match self {
            Self::Static(status) => status,
            Self::Owned(status) => status,
        }
    }
}

impl From<&'static str> for ResponseStatus {
    fn from(status: &'static str) -> Self {
        Self::Static(status)
    }
}

impl From<String> for ResponseStatus {
    fn from(status: String) -> Self {
        Self::Owned(status)
    }
}

impl PartialEq<&str> for ResponseStatus {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

enum ResponseBody {
    Memory(Vec<u8>),
    File { path: PathBuf, len: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RequestHeader {
    name: HeaderName,
    value: HeaderValue,
}

enum RequestBody {
    Empty,
    Memory(Bytes),
    File {
        file: fs::File,
        len: u64,
    },
    TempFile {
        file: fs::File,
        len: u64,
    },
    Chain {
        prefix: Bytes,
        file: fs::File,
        file_len: u64,
    },
}

struct SpoolingResponseBody {
    memory: Vec<u8>,
    file: Option<io::BufWriter<tempfile::NamedTempFile>>,
    written: u64,
}

struct ChainedRequestBodyReader {
    prefix: io::Cursor<Bytes>,
    file: fs::File,
}

type Http3SendRequest = h3::client::SendRequest<h3_quinn::OpenStreams, Bytes>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Http3Origin {
    scheme: String,
    host: String,
    port: u16,
}

struct AutoHttp3Candidate {
    origin: Http3Origin,
    uri: http::Uri,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Http3PooledOrigin {
    origin: Http3Origin,
    proxy_identity: Arc<str>,
    tls_verification_identity: Arc<str>,
    credential_identity: Option<String>,
}

impl Http3PooledOrigin {
    fn from_origin(
        origin: Http3Origin,
        proxy_identity: Arc<str>,
        tls_verification_identity: Arc<str>,
        credential_identity: Option<String>,
    ) -> Self {
        Self {
            origin,
            proxy_identity,
            tls_verification_identity,
            credential_identity,
        }
    }
}

struct Http3PooledConnection {
    _endpoint: h3_quinn::Endpoint,
    send_request: Http3SendRequest,
    driver: tokio::task::JoinHandle<()>,
}

type Http3ConnectionPool = HashMap<Http3PooledOrigin, Http3PooledConnection>;

fn http3_connection_pool() -> Http3ConnectionPool {
    HashMap::with_capacity(HTTP_CONNECTION_POOL_CAPACITY_HINT)
}

fn http3_stream_buffer() -> BytesMut {
    BytesMut::with_capacity(STREAM_BUFFER_SIZE)
}

impl Drop for Http3PooledConnection {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

impl Read for ChainedRequestBodyReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let prefix_read = self.prefix.read(buf)?;
        if prefix_read == buf.len() {
            return Ok(prefix_read);
        }
        let file_read = self.file.read(&mut buf[prefix_read..])?;
        Ok(prefix_read + file_read)
    }
}

impl RequestBody {
    fn try_clone(&self) -> io::Result<Self> {
        match self {
            Self::Empty => Ok(Self::Empty),
            Self::Memory(body) => Ok(Self::Memory(body.clone())),
            Self::File { file, len } => {
                let mut cloned = file.try_clone()?;
                cloned.seek(SeekFrom::Start(0))?;
                Ok(Self::File {
                    file: cloned,
                    len: *len,
                })
            }
            Self::TempFile { file, len } => {
                let mut cloned = file.try_clone()?;
                cloned.seek(SeekFrom::Start(0))?;
                Ok(Self::TempFile {
                    file: cloned,
                    len: *len,
                })
            }
            Self::Chain {
                prefix,
                file,
                file_len,
            } => {
                let mut cloned = file.try_clone()?;
                cloned.seek(SeekFrom::Start(0))?;
                Ok(Self::Chain {
                    prefix: prefix.clone(),
                    file: cloned,
                    file_len: *file_len,
                })
            }
        }
    }

    fn len(&self) -> io::Result<u64> {
        match self {
            Self::Empty => Ok(0),
            Self::Memory(body) => u64::try_from(body.len())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "request body too large")),
            Self::File { len, .. } => Ok(*len),
            Self::TempFile { len, .. } => Ok(*len),
            Self::Chain {
                prefix, file_len, ..
            } => {
                let prefix_len = u64::try_from(prefix.len()).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "request body too large")
                })?;
                prefix_len.checked_add(*file_len).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "request body too large")
                })
            }
        }
    }

    fn write_validated_to_with_buffer<W: Write>(
        &mut self,
        writer: &mut W,
        buffer: &mut [u8; STREAM_BUFFER_SIZE],
    ) -> io::Result<()> {
        match self {
            Self::Empty => Ok(()),
            Self::Memory(body) => writer.write_all(body),
            Self::File { file, len } => {
                file.seek(SeekFrom::Start(0))?;
                copy_request_body_file_with_buffer(file, writer, *len, buffer)?;
                Ok(())
            }
            Self::TempFile { file, len } => {
                file.seek(SeekFrom::Start(0))?;
                copy_request_body_file_with_buffer(file, writer, *len, buffer)?;
                Ok(())
            }
            Self::Chain {
                prefix,
                file,
                file_len,
            } => {
                writer.write_all(prefix)?;
                file.seek(SeekFrom::Start(0))?;
                copy_request_body_file_with_buffer(file, writer, *file_len, buffer)?;
                Ok(())
            }
        }
    }

    #[cfg(test)]
    fn write_to_with_buffer<W: Write>(
        &mut self,
        writer: &mut W,
        buffer: &mut [u8; STREAM_BUFFER_SIZE],
    ) -> io::Result<()> {
        validate_request_body_for_send(self)?;
        self.write_validated_to_with_buffer(writer, buffer)
    }
}

fn copy_request_body_file_with_buffer<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    len: u64,
    buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> io::Result<()> {
    let mut remaining = len;
    while remaining > 0 {
        let read_len = remaining.min(buffer.len() as u64) as usize;
        reader
            .read_exact(&mut buffer[..read_len])
            .map_err(|error| {
                if error.kind() == io::ErrorKind::UnexpectedEof {
                    io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "request body file ended early",
                    )
                } else {
                    error
                }
            })?;
        writer.write_all(&buffer[..read_len])?;
        remaining -= read_len as u64;
    }
    Ok(())
}

fn validate_request_body_file_len(actual_len: u64, expected_len: u64) -> io::Result<()> {
    if actual_len != expected_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("request body file length changed: expected {expected_len}, got {actual_len}"),
        ));
    }
    Ok(())
}

fn validate_request_body_for_send(body: &RequestBody) -> io::Result<()> {
    match body {
        RequestBody::Empty | RequestBody::Memory(_) => Ok(()),
        RequestBody::File { file, len } | RequestBody::TempFile { file, len } => {
            validate_request_body_file_len(file.metadata()?.len(), *len)
        }
        RequestBody::Chain { file, file_len, .. } => {
            validate_request_body_file_len(file.metadata()?.len(), *file_len)
        }
    }
}

impl SpoolingResponseBody {
    fn new(capacity: usize) -> Self {
        Self {
            memory: Vec::with_capacity(capacity),
            file: None,
            written: 0,
        }
    }

    fn finish(mut self) -> io::Result<ResponseBody> {
        if let Some(mut writer) = self.file.take() {
            writer.flush()?;
            let file = writer.into_inner().map_err(|error| error.into_error())?;
            persist_temp_response_file(file, self.written)
        } else {
            Ok(ResponseBody::Memory(self.memory))
        }
    }
}

impl Write for SpoolingResponseBody {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.file.is_none() {
            let next_len = self
                .memory
                .len()
                .checked_add(buf.len())
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "body too large"))?;
            if u64::try_from(next_len).unwrap_or(u64::MAX) > AUTO_RESPONSE_FILE_THRESHOLD {
                let file = temp_response_file()?;
                let mut file = io::BufWriter::with_capacity(FILE_STREAM_WRITE_BUFFER_SIZE, file);
                file.write_all(&self.memory)?;
                self.memory.clear();
                self.memory.shrink_to_fit();
                self.file = Some(file);
            }
        }
        if let Some(file) = self.file.as_mut() {
            file.write_all(buf)?;
        } else {
            self.memory.extend_from_slice(buf);
        }
        self.written = self
            .written
            .checked_add(
                u64::try_from(buf.len())
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "body too large"))?,
            )
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "body too large"))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.as_mut() {
            file.flush()
        } else {
            Ok(())
        }
    }
}

fn copy_stream<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> io::Result<u64> {
    let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
    copy_stream_with_buffer(reader, writer, &mut buffer)
}

fn copy_stream_with_buffer<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> io::Result<u64> {
    let mut copied = 0_u64;
    loop {
        let len = reader.read(buffer)?;
        if len == 0 {
            return Ok(copied);
        }
        let next = checked_stream_copy_len(copied, len)?;
        writer.write_all(&buffer[..len])?;
        copied = next;
    }
}

fn checked_stream_copy_len(current: u64, len: usize) -> io::Result<u64> {
    current
        .checked_add(
            u64::try_from(len)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "stream too large"))?,
        )
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "stream length overflow"))
}

struct TransportRequest {
    method: http::Method,
    url: String,
    headers: Vec<RequestHeader>,
    body: RequestBody,
    output_file: Option<PathBuf>,
}

#[derive(Clone, Copy)]
struct RequestOnceOptions {
    expected_version: HttpVersion,
    include_response_headers: bool,
    send_phase_label: Option<&'static str>,
}

struct AutoHttp3BatchState {
    ca_file: Option<String>,
    client_cert_file: Option<String>,
    client_key_file: Option<String>,
    tls_no_verify: bool,
    identities: BatchIdentityCache,
    runtime: Option<tokio::runtime::Runtime>,
    client_config: Option<quinn::ClientConfig>,
    stream_buffer: Option<BytesMut>,
    origin_cache: Option<HashMap<Http3Origin, bool>>,
    failed_origins: Option<HashSet<Http3Origin>>,
    h2_failed_origins: Option<HashSet<Http3Origin>>,
    h2_request_count: usize,
    connections: Option<Http3ConnectionPool>,
}

struct BatchIdentityCache {
    proxy_identity: Option<Arc<str>>,
    tls_verification_identity: Option<Arc<str>>,
}

impl BatchIdentityCache {
    fn new() -> Self {
        Self {
            proxy_identity: None,
            tls_verification_identity: None,
        }
    }

    fn proxy_identity(&mut self) -> Arc<str> {
        self.proxy_identity
            .get_or_insert_with(|| Arc::from(proxy_identity()))
            .clone()
    }

    fn tls_verification_identity(
        &mut self,
        ca_file: Option<&str>,
        client_cert_file: Option<&str>,
        client_key_file: Option<&str>,
        tls_no_verify: bool,
    ) -> Arc<str> {
        self.tls_verification_identity
            .get_or_insert_with(|| {
                Arc::from(tls_verification_identity(
                    ca_file,
                    client_cert_file,
                    client_key_file,
                    tls_no_verify,
                ))
            })
            .clone()
    }
}

impl AutoHttp3BatchState {
    fn new(
        ca_file: Option<&str>,
        client_cert_file: Option<&str>,
        client_key_file: Option<&str>,
        tls_no_verify: bool,
    ) -> Self {
        Self {
            ca_file: ca_file.map(str::to_owned),
            client_cert_file: client_cert_file.map(str::to_owned),
            client_key_file: client_key_file.map(str::to_owned),
            tls_no_verify,
            identities: BatchIdentityCache::new(),
            runtime: None,
            client_config: None,
            stream_buffer: None,
            origin_cache: None,
            failed_origins: None,
            h2_failed_origins: None,
            h2_request_count: 0,
            connections: None,
        }
    }

    fn auto_candidate(
        &mut self,
        url: &str,
    ) -> Result<Option<AutoHttp3Candidate>, Box<dyn std::error::Error>> {
        let Some(candidate) = auto_http3_candidate(url)? else {
            return Ok(None);
        };
        let origin = &candidate.origin;
        if !auto_http3_probe_host(&origin.host) {
            return Ok(None);
        }
        if http3_origin_set_contains(&self.failed_origins, origin) {
            return Ok(Some(candidate));
        }
        if let Some(cached) = http3_origin_cache_get(&self.origin_cache, origin) {
            return Ok(cached.then_some(candidate));
        }
        if auto_http3_failed_recently(origin) {
            insert_bounded_http3_origin_set(&mut self.failed_origins, origin.clone());
            return Ok(Some(candidate));
        }
        insert_bounded_http3_origin_cache(&mut self.origin_cache, origin.clone(), true);
        Ok(Some(candidate))
    }

    fn ensure_initialized(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.runtime.is_none() {
            self.runtime = Some(
                tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .enable_time()
                    .build()?,
            );
        }
        if self.client_config.is_none() {
            self.client_config = Some(http3_client_config(
                self.ca_file.as_deref(),
                self.client_cert_file.as_deref(),
                self.client_key_file.as_deref(),
                self.tls_no_verify,
            )?);
        }
        Ok(())
    }

    fn request(
        &mut self,
        client: &mut Option<Client>,
        http1_client: &mut Option<Client>,
        direct_http1_pool: &mut Option<PlainHttp1Pool>,
        direct_http1_identities: &mut Option<BatchIdentityCache>,
        direct_http1_proxy_free: &mut Option<bool>,
        args: &Args,
        candidate: AutoHttp3Candidate,
        request: TransportRequest,
        response_buffer: &mut [u8; STREAM_BUFFER_SIZE],
    ) -> Result<TransportResponse, Box<dyn std::error::Error>> {
        let origin = candidate.origin;
        let uri = candidate.uri;
        let h3_failed = if http3_origin_set_contains(&self.failed_origins, &origin) {
            true
        } else {
            auto_http3_failed_recently(&origin)
        };
        if h3_failed {
            insert_bounded_http3_origin_set(&mut self.failed_origins, origin.clone());
        }
        if !h3_failed {
            let _trace = phase_trace("helper.batch.auto.h3");
            self.ensure_initialized()?;
            let h3_body = request.body.try_clone()?;
            let credential_identity = request_credential_identity_from_uri(&uri, &request.headers);
            let proxy_identity = self.identities.proxy_identity();
            let tls_verification_identity = self.identities.tls_verification_identity(
                self.ca_file.as_deref(),
                self.client_cert_file.as_deref(),
                self.client_key_file.as_deref(),
                self.tls_no_verify,
            );
            let pooled_origin = Http3PooledOrigin::from_origin(
                origin.clone(),
                proxy_identity,
                tls_verification_identity,
                credential_identity,
            );
            let response = {
                let Some(runtime) = self.runtime.as_mut() else {
                    return Err("auto HTTP/3 runtime was not initialized".into());
                };
                let Some(client_config) = self.client_config.as_ref() else {
                    return Err("auto HTTP/3 client config was not initialized".into());
                };
                let stream_buffer = self.stream_buffer.get_or_insert_with(http3_stream_buffer);
                let connections = self.connections.get_or_insert_with(http3_connection_pool);
                runtime.block_on(request_http3_pooled_async(
                    connections,
                    client_config,
                    &pooled_origin,
                    Http3PooledRequestInput {
                        method: &request.method,
                        uri: &uri,
                        headers: &request.headers,
                        body: h3_body,
                        output_file: request.output_file.as_deref(),
                        connect_timeout: Some(AUTO_HTTP3_CONNECT_TIMEOUT),
                    },
                    stream_buffer,
                ))
            };
            match response {
                Ok(response) => return Ok(response),
                Err(_) => {
                    let _ = record_auto_http3_failure(&origin);
                    insert_bounded_http3_origin_set(&mut self.failed_origins, origin.clone());
                }
            }
        }

        if !http3_origin_set_contains(&self.h2_failed_origins, &origin) {
            let _trace = phase_trace("helper.batch.auto.h2");
            let h2_body = request.body.try_clone()?;
            let client = {
                let _trace = phase_trace("helper.batch.auto.h2.client_init");
                batch_client(client, args)?
            };
            match request_once_with_buffer(
                client,
                RequestOnceOptions {
                    expected_version: HttpVersion::Auto,
                    include_response_headers: false,
                    send_phase_label: Some(if self.h2_request_count == 0 {
                        "helper.request_once.send.first"
                    } else {
                        "helper.request_once.send.rest"
                    }),
                },
                RequestOnceInput {
                    method: request.method.clone(),
                    url: &request.url,
                    headers: &request.headers,
                    body: h2_body,
                    output_file: request.output_file.as_deref(),
                },
                response_buffer,
            ) {
                Ok(response) => {
                    self.h2_request_count += 1;
                    if response.version != "HTTP/2" && response.version != "2" {
                        insert_bounded_http3_origin_set(&mut self.h2_failed_origins, origin);
                    }
                    return Ok(response);
                }
                Err(_) => {
                    insert_bounded_http3_origin_set(&mut self.h2_failed_origins, origin.clone());
                }
            }
        }

        if direct_http1_batch_candidate(&request.url, direct_http1_proxy_free, args) {
            let _trace = phase_trace("helper.batch.auto.direct_http1");
            return batch_plain_http1_pool(
                direct_http1_pool,
                direct_http1_identities,
                args.ca_file.as_deref(),
                args.client_cert_file.as_deref(),
                args.client_key_file.as_deref(),
                args.tls_no_verify,
            )
            .request(request);
        }

        let _trace = phase_trace("helper.batch.auto.reqwest_http1");
        request_once_with_buffer(
            batch_http1_client(http1_client, args)?,
            RequestOnceOptions {
                expected_version: HttpVersion::Http1,
                include_response_headers: false,
                send_phase_label: None,
            },
            RequestOnceInput {
                method: request.method,
                url: &request.url,
                headers: &request.headers,
                body: request.body,
                output_file: request.output_file.as_deref(),
            },
            response_buffer,
        )
    }
}

fn http3_origin_set_contains(origins: &Option<HashSet<Http3Origin>>, origin: &Http3Origin) -> bool {
    origins
        .as_ref()
        .is_some_and(|origins| origins.contains(origin))
}

fn http3_origin_cache_get(
    origins: &Option<HashMap<Http3Origin, bool>>,
    origin: &Http3Origin,
) -> Option<bool> {
    origins
        .as_ref()
        .and_then(|origins| origins.get(origin).copied())
}

fn insert_bounded_http3_origin_set(
    origins: &mut Option<HashSet<Http3Origin>>,
    origin: Http3Origin,
) {
    let origins =
        origins.get_or_insert_with(|| HashSet::with_capacity(HTTP_ORIGIN_CACHE_CAPACITY_HINT));
    if origins.contains(&origin) {
        origins.replace(origin);
        return;
    }
    if origins.len() >= HTTP_ORIGIN_MEMORY_ENTRY_LIMIT
        && let Some(evicted) = origins.iter().next().cloned()
    {
        origins.remove(&evicted);
    }
    origins.insert(origin);
}

fn insert_bounded_http3_origin_cache(
    origins: &mut Option<HashMap<Http3Origin, bool>>,
    origin: Http3Origin,
    enabled: bool,
) {
    let origins =
        origins.get_or_insert_with(|| HashMap::with_capacity(HTTP_ORIGIN_CACHE_CAPACITY_HINT));
    if let Some(cached) = origins.get_mut(&origin) {
        *cached = enabled;
        return;
    }
    if origins.len() >= HTTP_ORIGIN_MEMORY_ENTRY_LIMIT {
        if let Some(evicted) = origins.keys().next().cloned() {
            origins.remove(&evicted);
        }
    }
    origins.insert(origin, enabled);
}

fn main() {
    if let Err(error) = run() {
        eprintln!("fatal: {error}");
        std::process::exit(128);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args = Args::parse();
    if args.batch {
        if args.http_version == HttpVersion::Http3 {
            return run_http3_batch(&args);
        }
        return run_batch(&args);
    }
    if let (Some(method), Some(url)) = (args.method.as_deref(), args.url.as_deref()) {
        let method = parse_request_method(method)?;
        let body = read_body(args.body_file.as_deref())?;
        let output_file = args.output_file.as_deref().map(Path::new);
        let headers = parse_request_headers(&args.headers)?;
        let auto_origin = if args.http_version == HttpVersion::Auto {
            auto_http3_origin(url)?
        } else {
            None
        };
        let response = match args.http_version {
            HttpVersion::Http3 => request_http3(
                method,
                url,
                &headers,
                body,
                output_file,
                args.ca_file.as_deref(),
                args.client_cert_file.as_deref(),
                args.client_key_file.as_deref(),
                args.tls_no_verify,
                true,
            )?,
            HttpVersion::Auto if auto_origin.is_some() => {
                let Some(origin) = auto_origin else {
                    return Err("auto HTTP/3 origin was not initialized".into());
                };
                let mut response = None;
                if !auto_http3_failed_recently(&origin) {
                    let h3_body = body.try_clone()?;
                    response = request_http3_with_connect_timeout(
                        Http3RequestInput {
                            method: method.clone(),
                            url,
                            headers: &headers,
                            body: h3_body,
                            output_file,
                            include_response_headers: true,
                        },
                        args.ca_file.as_deref(),
                        args.client_cert_file.as_deref(),
                        args.client_key_file.as_deref(),
                        args.tls_no_verify,
                        Some(AUTO_HTTP3_CONNECT_TIMEOUT),
                    )
                    .ok();
                    if response.is_none() {
                        let _ = record_auto_http3_failure(&origin);
                    }
                }
                if response.is_none() {
                    let client = build_client(&args)?;
                    response = Some(request_once(
                        &client,
                        RequestOnceOptions {
                            expected_version: HttpVersion::Auto,
                            include_response_headers: true,
                            send_phase_label: None,
                        },
                        method,
                        url,
                        &headers,
                        body,
                        output_file,
                    )?);
                }
                match response {
                    Some(response) => response,
                    None => return Err("HTTP fallback chain produced no response".into()),
                }
            }
            HttpVersion::Auto | HttpVersion::Http1 | HttpVersion::Http2 => {
                let mut direct_http1_proxy_free = None;
                if matches!(args.http_version, HttpVersion::Auto | HttpVersion::Http1)
                    && direct_http1_batch_candidate(url, &mut direct_http1_proxy_free, &args)
                {
                    request_plain_http1_once(&args, method, url, headers, body, output_file)?
                } else {
                    let client = build_client(&args)?;
                    request_once(
                        &client,
                        RequestOnceOptions {
                            expected_version: args.http_version,
                            include_response_headers: true,
                            send_phase_label: None,
                        },
                        method,
                        url,
                        &headers,
                        body,
                        output_file,
                    )?
                }
            }
        };
        write_response(response)?;
        return Ok(());
    }
    Err("expected --method and --url".into())
}

fn run_batch(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdin = io::BufReader::with_capacity(STDIO_BUFFER_SIZE, stdin.lock());
    let stdout = io::stdout();
    let mut stdout = io::BufWriter::with_capacity(STDIO_BUFFER_SIZE, stdout.lock());
    let mut http1_pool = None;
    let mut http1_identities = None;
    let mut direct_http1_proxy_free = None;
    let mut line = batch_line_buffer();
    let mut request_body_buffer = [0_u8; STREAM_BUFFER_SIZE];
    let mut response_body_buffer = [0_u8; STREAM_BUFFER_SIZE];
    let mut client = None;
    let mut http1_client = None;
    let mut auto_http3 = if args.http_version == HttpVersion::Auto {
        Some(AutoHttp3BatchState::new(
            args.ca_file.as_deref(),
            args.client_cert_file.as_deref(),
            args.client_key_file.as_deref(),
            args.tls_no_verify,
        ))
    } else {
        None
    };
    while let Some(request) = {
        let _trace = phase_trace("helper.batch.read_request");
        read_batch_request_with_line(&mut stdin, &mut line, &mut request_body_buffer)?
    } {
        let response = if args.http_version == HttpVersion::Http3 {
            let TransportRequest {
                method,
                url,
                headers,
                body,
                output_file,
            } = request;
            request_http3(
                method,
                &url,
                &headers,
                body,
                output_file.as_deref(),
                args.ca_file.as_deref(),
                args.client_cert_file.as_deref(),
                args.client_key_file.as_deref(),
                args.tls_no_verify,
                false,
            )?
        } else if args.http_version == HttpVersion::Auto
            && let Some(auto_http3) = auto_http3.as_mut()
            && let Some(candidate) = {
                let _trace = phase_trace("helper.batch.auto_candidate");
                auto_http3.auto_candidate(&request.url)?
            }
        {
            auto_http3.request(
                &mut client,
                &mut http1_client,
                &mut http1_pool,
                &mut http1_identities,
                &mut direct_http1_proxy_free,
                args,
                candidate,
                request,
                &mut response_body_buffer,
            )?
        } else if direct_http1_batch_candidate(&request.url, &mut direct_http1_proxy_free, args)
            && matches!(args.http_version, HttpVersion::Auto | HttpVersion::Http1)
        {
            batch_plain_http1_pool(
                &mut http1_pool,
                &mut http1_identities,
                args.ca_file.as_deref(),
                args.client_cert_file.as_deref(),
                args.client_key_file.as_deref(),
                args.tls_no_verify,
            )
            .request(request)?
        } else {
            let TransportRequest {
                method,
                url,
                headers,
                body,
                output_file,
            } = request;
            request_once_with_buffer(
                batch_client(&mut client, args)?,
                RequestOnceOptions {
                    expected_version: args.http_version,
                    include_response_headers: false,
                    send_phase_label: None,
                },
                RequestOnceInput {
                    method,
                    url: &url,
                    headers: &headers,
                    body,
                    output_file: output_file.as_deref(),
                },
                &mut response_body_buffer,
            )?
        };
        {
            let _trace = phase_trace("helper.batch.write_response");
            write_response_frame(&mut stdout, response)?;
        }
        {
            let _trace = phase_trace("helper.batch.flush");
            stdout.flush()?;
        }
    }
    Ok(())
}

fn request_plain_http1_once(
    args: &Args,
    method: http::Method,
    url: &str,
    headers: Vec<RequestHeader>,
    body: RequestBody,
    output_file: Option<&Path>,
) -> Result<TransportResponse, Box<dyn std::error::Error>> {
    let mut pool = PlainHttp1Pool::new(
        Arc::from(proxy_identity()),
        Arc::from(tls_verification_identity(
            args.ca_file.as_deref(),
            args.client_cert_file.as_deref(),
            args.client_key_file.as_deref(),
            args.tls_no_verify,
        )),
        args.ca_file.as_deref(),
        args.client_cert_file.as_deref(),
        args.client_key_file.as_deref(),
        args.tls_no_verify,
    );
    pool.request_with_response_headers(
        TransportRequest {
            method,
            url: url.to_owned(),
            headers,
            body,
            output_file: output_file.map(Path::to_path_buf),
        },
        true,
    )
}

fn batch_plain_http1_pool<'a>(
    pool: &'a mut Option<PlainHttp1Pool>,
    identities: &mut Option<BatchIdentityCache>,
    ca_file: Option<&str>,
    client_cert_file: Option<&str>,
    client_key_file: Option<&str>,
    tls_no_verify: bool,
) -> &'a mut PlainHttp1Pool {
    if pool.is_none() {
        let identities = identities.get_or_insert_with(BatchIdentityCache::new);
        let proxy_identity = identities.proxy_identity();
        let tls_verification_identity = identities.tls_verification_identity(
            ca_file,
            client_cert_file,
            client_key_file,
            tls_no_verify,
        );
        *pool = Some(PlainHttp1Pool::new(
            proxy_identity,
            tls_verification_identity,
            ca_file,
            client_cert_file,
            client_key_file,
            tls_no_verify,
        ));
    }
    pool.as_mut().expect("plain HTTP/1 pool initialized")
}

fn batch_client<'a>(
    client: &'a mut Option<Client>,
    args: &Args,
) -> Result<&'a Client, Box<dyn std::error::Error>> {
    if client.is_none() {
        *client = Some(build_client(args)?);
    }
    match client.as_ref() {
        Some(client) => Ok(client),
        None => Err("HTTP client was not initialized".into()),
    }
}

fn batch_http1_client<'a>(
    client: &'a mut Option<Client>,
    args: &Args,
) -> Result<&'a Client, Box<dyn std::error::Error>> {
    if client.is_none() {
        *client = Some(build_client_for_version(args, HttpVersion::Http1)?);
    }
    match client.as_ref() {
        Some(client) => Ok(client),
        None => Err("HTTP/1.1 client was not initialized".into()),
    }
}

fn run_http3_batch(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdin = io::BufReader::with_capacity(STDIO_BUFFER_SIZE, stdin.lock());
    let stdout = io::stdout();
    let mut stdout = io::BufWriter::with_capacity(STDIO_BUFFER_SIZE, stdout.lock());
    let proxy_identity = proxy_identity();
    let tls_verification_identity = tls_verification_identity(
        args.ca_file.as_deref(),
        args.client_cert_file.as_deref(),
        args.client_key_file.as_deref(),
        args.tls_no_verify,
    );
    let client_config = http3_client_config(
        args.ca_file.as_deref(),
        args.client_cert_file.as_deref(),
        args.client_key_file.as_deref(),
        args.tls_no_verify,
    )?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    runtime.block_on(run_http3_batch_async(
        &mut stdin,
        &mut stdout,
        &client_config,
        Arc::from(proxy_identity),
        Arc::from(tls_verification_identity),
    ))?;
    Ok(())
}

fn plain_http1_batch_candidate(url: &str) -> bool {
    url.as_bytes()
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(b"http://"))
}

fn direct_http1_batch_candidate(url: &str, proxy_free: &mut Option<bool>, _args: &Args) -> bool {
    if plain_http1_batch_candidate(url) {
        return true;
    }
    if !url
        .as_bytes()
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(b"https://"))
    {
        return false;
    }
    *proxy_free.get_or_insert_with(|| proxy_identity() == NONE_IDENTITY)
}

fn auto_http3_origin(url: &str) -> Result<Option<Http3Origin>, Box<dyn std::error::Error>> {
    let Some(candidate) = auto_http3_candidate(url)? else {
        return Ok(None);
    };
    if !auto_http3_probe_host(&candidate.origin.host) {
        return Ok(None);
    }
    Ok(Some(candidate.origin))
}

#[cfg(test)]
fn auto_http3_candidate_origin(
    url: &str,
) -> Result<Option<Http3Origin>, Box<dyn std::error::Error>> {
    Ok(auto_http3_candidate(url)?.map(|candidate| candidate.origin))
}

fn auto_http3_candidate(
    url: &str,
) -> Result<Option<AutoHttp3Candidate>, Box<dyn std::error::Error>> {
    if plain_http1_batch_candidate(url) {
        return Ok(None);
    }
    let uri: http::Uri = url.parse()?;
    if uri.scheme_str() != Some("https") {
        return Ok(None);
    }
    let authority = uri.authority().ok_or("URL is missing host")?;
    let host = authority.host();
    if authority_has_invalid_explicit_port(authority) {
        return Err("URL port is invalid".into());
    }
    Ok(Some(AutoHttp3Candidate {
        origin: Http3Origin {
            scheme: "https".to_owned(),
            host: normalized_authority_host(host).to_owned(),
            port: authority.port_u16().unwrap_or(443),
        },
        uri,
    }))
}

fn normalized_authority_host(host: &str) -> &str {
    host.trim_start_matches('[').trim_end_matches(']')
}

fn authority_has_invalid_explicit_port(authority: &http::uri::Authority) -> bool {
    if authority.port_u16().is_some() {
        return false;
    }
    let raw = authority.as_str();
    if raw.starts_with('[') {
        return raw
            .find(']')
            .and_then(|idx| raw.as_bytes().get(idx + 1))
            .is_some_and(|byte| *byte == b':');
    }
    let host = authority.host();
    raw.rfind(host)
        .and_then(|idx| raw.as_bytes().get(idx + host.len()))
        .is_some_and(|byte| *byte == b':')
}

fn decimal_len_u64(mut value: u64) -> usize {
    let mut len = 1;
    while value >= 10 {
        value /= 10;
        len += 1;
    }
    len
}

fn auto_http3_probe_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return false;
    }
    let Ok(ip) = host.parse::<IpAddr>() else {
        return true;
    };
    !is_local_auto_http3_probe_ip(ip)
}

fn is_local_auto_http3_probe_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unicast_link_local()
                || ip.is_unique_local()
        }
    }
}

fn build_client(args: &Args) -> Result<Client, Box<dyn std::error::Error>> {
    build_client_for_version(args, args.http_version)
}

fn build_client_for_version(
    args: &Args,
    version: HttpVersion,
) -> Result<Client, Box<dyn std::error::Error>> {
    let mut builder = Client::builder()
        .pool_idle_timeout(Duration::from_secs(args.pool_idle_timeout_secs))
        .pool_max_idle_per_host(args.pool_max_idle_per_host)
        .use_rustls_tls()
        .https_only(false)
        .redirect(Policy::none())
        .tcp_nodelay(true)
        .user_agent(concat!("skron-git-remote-http/", env!("CARGO_PKG_VERSION")));
    builder = match version {
        HttpVersion::Auto => builder,
        HttpVersion::Http1 => builder.http1_only(),
        HttpVersion::Http2 => builder.http2_prior_knowledge(),
        HttpVersion::Http3 => builder,
    };
    if let Some(ca_file) = args.ca_file.as_deref() {
        for cert in load_ca_bundle(ca_file)? {
            builder = builder.add_root_certificate(reqwest::Certificate::from_der(cert.as_ref())?);
        }
    }
    if args.tls_no_verify {
        builder = builder.danger_accept_invalid_certs(true);
    }
    if let Some(identity) = load_reqwest_client_identity(
        args.client_cert_file.as_deref(),
        args.client_key_file.as_deref(),
    )? {
        builder = builder.identity(identity);
    }
    Ok(builder.build()?)
}

fn read_body(path: Option<&str>) -> io::Result<RequestBody> {
    match path {
        Some("-") => {
            let stdin = io::stdin();
            let mut stdin = stdin.lock();
            let (file, len) = copy_stream_to_tempfile(&mut stdin)?;
            Ok(RequestBody::TempFile { file, len })
        }
        Some(path) => request_body_file(PathBuf::from(path)),
        None => Ok(RequestBody::Empty),
    }
}

fn request_body_file(path: PathBuf) -> io::Result<RequestBody> {
    let file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    Ok(RequestBody::File { file, len })
}

fn parse_request_headers(headers: &[String]) -> io::Result<Vec<RequestHeader>> {
    let mut parsed = Vec::with_capacity(request_headers_initial_capacity(headers.len()));
    for header in headers {
        parsed.push(parse_request_header(header)?);
    }
    Ok(parsed)
}

fn request_headers_initial_capacity(headers_len: usize) -> usize {
    headers_len.min(BATCH_REQUEST_HEADER_LIMIT)
}

fn reserve_batch_request_headers(headers: &mut Vec<RequestHeader>) {
    if headers.capacity() == 0 {
        headers.reserve_exact(BATCH_REQUEST_HEADER_CAPACITY);
    }
}

fn parse_request_header(header: &str) -> io::Result<RequestHeader> {
    let Some((name, value)) = header.split_once(':') else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("malformed header '{header}'"),
        ));
    };
    Ok(RequestHeader {
        name: HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        value: HeaderValue::from_str(value.trim())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
    })
}

fn read_batch_request_with_line<R: BufRead>(
    reader: &mut R,
    line: &mut String,
    body_buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> io::Result<Option<TransportRequest>> {
    loop {
        if read_limited_batch_line(reader, line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "DONE" {
            return Ok(None);
        }
        if trimmed == "REQUEST" {
            break;
        }
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected batch line: {trimmed}"),
        ));
    }

    let mut method = None;
    let mut url = None;
    let mut headers = Vec::new();
    let mut content_length = None;
    let mut body_prefix_length = None;
    let mut output_file = None;
    let mut body_file = None;
    loop {
        if read_limited_batch_line(reader, line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "batch request ended before headers",
            ));
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("METHOD ") {
            update_batch_method(&mut method, value)?;
        } else if let Some(value) = trimmed.strip_prefix("URL ") {
            update_batch_string_field(&mut url, "URL", value)?;
        } else if let Some(value) = trimmed.strip_prefix("HEADER ") {
            if headers.len() >= BATCH_REQUEST_HEADER_LIMIT {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "too many batch request headers",
                ));
            }
            reserve_batch_request_headers(&mut headers);
            headers.push(parse_request_header(value)?);
        } else if let Some(value) = trimmed.strip_prefix("CONTENT-LENGTH ") {
            let parsed = parse_decimal_usize(
                value.as_bytes(),
                "invalid batch content length",
                "batch content length too large",
            )?;
            update_batch_usize_field(&mut content_length, "CONTENT-LENGTH", parsed)?;
        } else if let Some(value) = trimmed.strip_prefix("BODY-PREFIX-LENGTH ") {
            let parsed = parse_decimal_usize(
                value.as_bytes(),
                "invalid batch body prefix length",
                "batch body prefix length too large",
            )?;
            update_batch_usize_field(&mut body_prefix_length, "BODY-PREFIX-LENGTH", parsed)?;
        } else if let Some(value) = trimmed.strip_prefix("OUTPUT-FILE ") {
            update_batch_path_field(&mut output_file, "OUTPUT-FILE", value)?;
        } else if let Some(value) = trimmed.strip_prefix("BODY-FILE ") {
            update_batch_path_field(&mut body_file, "BODY-FILE", value)?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected batch request header: {trimmed}"),
            ));
        }
    }

    let method = method.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "batch request is missing method",
        )
    })?;
    let url = url.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "batch request is missing url")
    })?;
    let content_length = content_length.unwrap_or(0);
    let body_prefix_length = body_prefix_length.unwrap_or(0);
    let body = if let Some(body_file) = body_file {
        if content_length != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "batch request cannot combine BODY-FILE and CONTENT-LENGTH",
            ));
        }
        if body_prefix_length == 0 {
            request_body_file(body_file)?
        } else {
            if body_prefix_length as u64 > AUTO_RESPONSE_FILE_THRESHOLD {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "batch body prefix length too large",
                ));
            }
            let prefix =
                read_exact_len_to_bytes_with_buffer(reader, body_prefix_length, body_buffer)?;
            let file = fs::File::open(body_file)?;
            let file_len = file.metadata()?.len();
            RequestBody::Chain {
                prefix,
                file,
                file_len,
            }
        }
    } else {
        if body_prefix_length != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "batch request cannot use BODY-PREFIX-LENGTH without BODY-FILE",
            ));
        }
        read_content_length_request_body_with_buffer(reader, content_length, body_buffer)?
    };
    trim_reusable_http_line_buffer(line);
    Ok(Some(TransportRequest {
        method,
        url,
        headers,
        body,
        output_file,
    }))
}

fn update_batch_method(method: &mut Option<http::Method>, value: &str) -> io::Result<()> {
    if method.is_some() {
        return Err(duplicate_batch_request_header("METHOD"));
    }
    *method = Some(parse_request_method(value)?);
    Ok(())
}

fn update_batch_string_field(
    field: &mut Option<String>,
    name: &'static str,
    value: &str,
) -> io::Result<()> {
    if field.is_some() {
        return Err(duplicate_batch_request_header(name));
    }
    *field = Some(value.to_owned());
    Ok(())
}

fn update_batch_usize_field(
    field: &mut Option<usize>,
    name: &'static str,
    value: usize,
) -> io::Result<()> {
    if field.is_some() {
        return Err(duplicate_batch_request_header(name));
    }
    *field = Some(value);
    Ok(())
}

fn update_batch_path_field(
    field: &mut Option<PathBuf>,
    name: &'static str,
    value: &str,
) -> io::Result<()> {
    if field.is_some() {
        return Err(duplicate_batch_request_header(name));
    }
    *field = Some(PathBuf::from(value));
    Ok(())
}

fn duplicate_batch_request_header(name: &'static str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("duplicate batch request header: {name}"),
    )
}

fn batch_line_buffer() -> String {
    String::with_capacity(HTTP_LINE_BUFFER_CAPACITY)
}

fn read_limited_batch_line<R: BufRead>(reader: &mut R, line: &mut String) -> io::Result<usize> {
    read_limited_line(
        reader,
        line,
        BATCH_REQUEST_LINE_LIMIT,
        "batch request line too long",
    )
}

fn parse_request_method(method: &str) -> io::Result<http::Method> {
    method
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PlainHttp1Origin {
    scheme: String,
    host: String,
    port: u16,
    proxy_identity: Arc<str>,
    tls_verification_identity: Arc<str>,
    credential_identity: Option<String>,
}

#[derive(Default)]
struct PlainHttp1Pool {
    connections: HashMap<PlainHttp1Origin, PlainHttp1Connection>,
    last_origin: Option<PlainHttp1Origin>,
    proxy_identity: Arc<str>,
    tls_verification_identity: Arc<str>,
    tls_config: Option<Arc<rustls::ClientConfig>>,
    ca_file: Option<String>,
    client_cert_file: Option<String>,
    client_key_file: Option<String>,
    tls_no_verify: bool,
}

impl PlainHttp1Pool {
    fn new(
        proxy_identity: Arc<str>,
        tls_verification_identity: Arc<str>,
        ca_file: Option<&str>,
        client_cert_file: Option<&str>,
        client_key_file: Option<&str>,
        tls_no_verify: bool,
    ) -> Self {
        Self {
            connections: HashMap::with_capacity(HTTP_CONNECTION_POOL_CAPACITY_HINT),
            last_origin: None,
            proxy_identity,
            tls_verification_identity,
            tls_config: None,
            ca_file: ca_file.map(str::to_owned),
            client_cert_file: client_cert_file.map(str::to_owned),
            client_key_file: client_key_file.map(str::to_owned),
            tls_no_verify,
        }
    }
}

impl PlainHttp1Pool {
    fn request(
        &mut self,
        request: TransportRequest,
    ) -> Result<TransportResponse, Box<dyn std::error::Error>> {
        self.request_with_response_headers(request, false)
    }

    fn request_with_response_headers(
        &mut self,
        mut request: TransportRequest,
        include_response_headers: bool,
    ) -> Result<TransportResponse, Box<dyn std::error::Error>> {
        validate_request_body_for_send(&request.body)?;
        let url = reqwest::Url::parse(&request.url)?;
        let tls_config = self.rustls_http1_config_for_url(&url)?;
        let proxy_identity = &self.proxy_identity;
        let tls_verification_identity = &self.tls_verification_identity;
        let mut owned_key = None;
        let key = if let Some(last_origin) = self.last_origin.as_ref().filter(|origin| {
            plain_http1_origin_matches_request(
                origin,
                &url,
                &request.headers,
                &proxy_identity,
                &tls_verification_identity,
            )
        }) {
            last_origin
        } else {
            owned_key = Some(plain_http1_origin_key(
                &url,
                &request.headers,
                Some(proxy_identity.clone()),
                Some(tls_verification_identity.clone()),
            )?);
            owned_key.as_ref().expect("plain HTTP/1 origin key")
        };

        if let Some(connection) = self.connections.get_mut(&key) {
            match connection.request(&mut request, &url, include_response_headers) {
                Ok(response) => {
                    if !plain_http1_response_allows_reuse(&response) {
                        self.connections.remove(&key);
                    }
                    if let Some(key) = owned_key {
                        self.last_origin = Some(key);
                    }
                    return Ok(response);
                }
                Err(error) => {
                    self.connections.remove(&key);
                    if !plain_http1_retryable_reused_connection_error(error.as_ref()) {
                        return Err(error);
                    }
                }
            }
        }

        let mut connection = PlainHttp1Connection::connect(
            &url,
            tls_config,
            self.ca_file.as_deref(),
            self.client_cert_file.as_deref(),
            self.client_key_file.as_deref(),
            self.tls_no_verify,
        )?;
        match connection.request(&mut request, &url, include_response_headers) {
            Ok(response) => {
                if plain_http1_response_allows_reuse(&response) {
                    let key = match owned_key {
                        Some(key) => {
                            self.last_origin = Some(key.clone());
                            key
                        }
                        None => key.clone(),
                    };
                    trim_plain_http1_connection_pool_for_insert(&mut self.connections, &key);
                    self.connections.insert(key, connection);
                }
                Ok(response)
            }
            Err(error) => Err(error),
        }
    }

    fn rustls_http1_config_for_url(
        &mut self,
        url: &reqwest::Url,
    ) -> Result<Option<Arc<rustls::ClientConfig>>, Box<dyn std::error::Error>> {
        if url.scheme() != "https" {
            return Ok(None);
        }
        if self.tls_config.is_none() {
            self.tls_config = Some(Arc::new(rustls_http1_client_config(
                self.ca_file.as_deref(),
                self.client_cert_file.as_deref(),
                self.client_key_file.as_deref(),
                self.tls_no_verify,
            )?));
        }
        Ok(self.tls_config.clone())
    }
}

fn plain_http1_origin_matches_request(
    origin: &PlainHttp1Origin,
    url: &reqwest::Url,
    headers: &[RequestHeader],
    proxy_identity: &Arc<str>,
    tls_verification_identity: &Arc<str>,
) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    origin.scheme == url.scheme()
        && origin.host == host
        && origin.port == url.port_or_known_default().unwrap_or(80)
        && origin.proxy_identity.as_ref() == proxy_identity.as_ref()
        && origin.tls_verification_identity.as_ref() == tls_verification_identity.as_ref()
        && request_credential_identity_matches_url(
            origin.credential_identity.as_deref(),
            url,
            headers,
        )
}

fn plain_http1_response_allows_reuse(response: &TransportResponse) -> bool {
    response.reusable_connection
}

fn trim_plain_http1_connection_pool_for_insert(
    connections: &mut HashMap<PlainHttp1Origin, PlainHttp1Connection>,
    incoming: &PlainHttp1Origin,
) {
    if let Some(evict) = plain_http1_connection_pool_eviction_candidate(connections, incoming) {
        connections.remove(&evict);
    }
}

fn plain_http1_connection_pool_eviction_candidate<V>(
    connections: &HashMap<PlainHttp1Origin, V>,
    incoming: &PlainHttp1Origin,
) -> Option<PlainHttp1Origin> {
    if connections.len() < PLAIN_HTTP1_CONNECTION_POOL_ENTRY_LIMIT {
        return None;
    }
    let mut evict = None;
    for origin in connections.keys() {
        if origin == incoming {
            return None;
        }
        if evict.is_none() {
            evict = Some(origin);
        }
    }
    evict.cloned()
}

fn plain_http1_retryable_reused_connection_error(
    error: &(dyn std::error::Error + 'static),
) -> bool {
    let Some(error) = error.downcast_ref::<io::Error>() else {
        return false;
    };
    matches!(
        error.kind(),
        io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::BrokenPipe
    )
}

struct PlainHttp1Connection {
    reader: io::BufReader<Http1ConnectionStream>,
    head: Vec<u8>,
    line: String,
    copy_buffer: [u8; STREAM_BUFFER_SIZE],
}

enum Http1ConnectionStream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Read for Http1ConnectionStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(buf),
            Self::Tls(stream) => stream.read(buf),
        }
    }
}

impl Write for Http1ConnectionStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.write(buf),
            Self::Tls(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush(),
            Self::Tls(stream) => stream.flush(),
        }
    }
}

impl PlainHttp1Connection {
    fn connect(
        url: &reqwest::Url,
        tls_config: Option<Arc<rustls::ClientConfig>>,
        ca_file: Option<&str>,
        client_cert_file: Option<&str>,
        client_key_file: Option<&str>,
        tls_no_verify: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let host = url
            .host_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "URL is missing host"))?;
        let port = url.port_or_known_default().unwrap_or(80);
        let stream = TcpStream::connect((host, port))?;
        stream.set_nodelay(true)?;
        let stream = if url.scheme() == "https" {
            let tls_config = match tls_config {
                Some(config) => config,
                None => Arc::new(rustls_http1_client_config(
                    ca_file,
                    client_cert_file,
                    client_key_file,
                    tls_no_verify,
                )?),
            };
            Http1ConnectionStream::Tls(Box::new(connect_rustls_http1_stream(
                stream, host, tls_config,
            )?))
        } else {
            Http1ConnectionStream::Plain(stream)
        };
        Ok(Self {
            reader: io::BufReader::with_capacity(STREAM_BUFFER_SIZE, stream),
            head: Vec::with_capacity(PLAIN_HTTP1_REQUEST_HEAD_INITIAL_CAPACITY),
            line: String::with_capacity(HTTP_LINE_BUFFER_CAPACITY),
            copy_buffer: [0_u8; STREAM_BUFFER_SIZE],
        })
    }

    fn request(
        &mut self,
        request: &mut TransportRequest,
        url: &reqwest::Url,
        include_response_headers: bool,
    ) -> Result<TransportResponse, Box<dyn std::error::Error>> {
        {
            let stream = self.reader.get_mut();
            write_plain_http1_request_head(&mut self.head, request, url)?;
            stream.write_all(&self.head)?;
            trim_plain_http1_request_head_buffer(&mut self.head);
            request
                .body
                .write_validated_to_with_buffer(stream, &mut self.copy_buffer)?;
            stream.flush()?;
        }

        let response = read_plain_http1_response_with_body_policy(
            &mut self.reader,
            request_method_allows_empty_response_body(&request.method),
            request.output_file.as_deref(),
            include_response_headers,
            &mut self.line,
            &mut self.copy_buffer,
        )?;
        trim_reusable_http_line_buffer(&mut self.line);
        Ok(response)
    }
}

fn read_plain_http1_response_with_body_policy<R: BufRead>(
    reader: &mut R,
    empty_body_request: bool,
    output_file: Option<&Path>,
    include_response_headers: bool,
    line: &mut String,
    buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> Result<TransportResponse, Box<dyn std::error::Error>> {
    let (
        version,
        status_code,
        status,
        headers,
        default_reusable_connection,
        content_length,
        chunked,
        connection_close,
        connection_keep_alive,
    ) = {
        let mut informational_responses = 0_usize;
        loop {
            if read_limited_http_response_line(reader, line)? == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unexpected EOF while reading HTTP status line",
                )
                .into());
            }
            let status_line = line.trim_end_matches(['\r', '\n']);
            let (version, status_text, default_reusable_connection) =
                if let Some(status) = status_line.strip_prefix("HTTP/1.1 ") {
                    ("1.1", status, true)
                } else if let Some(status) = status_line.strip_prefix("HTTP/1.0 ") {
                    ("1.0", status, false)
                } else {
                    return Err("invalid HTTP/1 response status line".into());
                };
            let status_code = http_status_code(status_text);
            let informational_response = status_code.is_some_and(|code| (100..200).contains(&code));
            let redirect_response = status_code.is_some_and(http_status_code_is_redirect);
            let status = response_status_text_from_code_and_str(status_code, status_text);

            let mut content_length = None;
            let mut chunked = false;
            let mut connection_close = false;
            let mut connection_keep_alive = false;
            let mut headers = redirect_response_headers_vec(redirect_response);
            loop {
                if read_limited_http_response_line(reader, line)? == 0 {
                    return Err("unexpected EOF while reading HTTP headers".into());
                }
                let trimmed = line.trim_end_matches(['\r', '\n']);
                if trimmed.is_empty() {
                    break;
                }
                if let Some((name, value)) = trimmed.split_once(':') {
                    if informational_response {
                        continue;
                    }
                    let name = name.trim();
                    let value = value.trim();
                    if name.eq_ignore_ascii_case("content-length") {
                        let parsed = parse_decimal_usize(
                            value.as_bytes(),
                            "invalid Content-Length header",
                            "Content-Length header too large",
                        )?;
                        if content_length.is_some_and(|existing| existing != parsed) {
                            return Err("conflicting Content-Length headers".into());
                        }
                        content_length = Some(parsed);
                    } else if name.eq_ignore_ascii_case("transfer-encoding") {
                        chunked |= plain_http1_transfer_encoding_is_chunked(value)?;
                    } else if name.eq_ignore_ascii_case("connection") {
                        let flags = connection_header_flags(value);
                        connection_close |= flags.close;
                        connection_keep_alive |= flags.keep_alive;
                    } else if include_response_headers {
                        headers.push((name.to_owned(), value.to_owned()));
                    } else if redirect_response
                        && headers.is_empty()
                        && name.eq_ignore_ascii_case("location")
                    {
                        headers.push(("location".to_owned(), value.to_owned()));
                    }
                }
            }
            if status_code == Some(101) {
                return Err("unsupported HTTP protocol switch".into());
            }
            if informational_response {
                informational_responses += 1;
                if informational_responses > HTTP_INFORMATIONAL_RESPONSE_LIMIT {
                    return Err("too many informational HTTP responses".into());
                }
                continue;
            }
            break (
                version,
                status_code,
                status,
                headers,
                default_reusable_connection,
                content_length,
                chunked,
                connection_close,
                connection_keep_alive,
            );
        }
    };

    if chunked && content_length.is_some() {
        return Err(
            "HTTP/1 response cannot combine Transfer-Encoding: chunked and Content-Length".into(),
        );
    }
    let empty_body_response =
        empty_body_request || status_code.is_some_and(http_status_code_allows_empty_body);
    let close_delimited = !chunked
        && content_length.is_none()
        && !empty_body_response
        && plain_http1_response_is_close_delimited(
            default_reusable_connection,
            connection_close,
            connection_keep_alive,
        );

    let body = if let Some(output_file) = output_file {
        let mut output = temp_output_file(output_file)?;
        let len = if empty_body_response {
            0
        } else if chunked {
            {
                let mut file = io::BufWriter::with_capacity(
                    FILE_STREAM_WRITE_BUFFER_SIZE,
                    output.as_file_mut(),
                );
                let len = write_chunked_body_with_buffers(reader, &mut file, line, buffer)?;
                file.flush()?;
                len
            }
        } else if let Some(content_length) = content_length {
            {
                let mut file = io::BufWriter::with_capacity(
                    FILE_STREAM_WRITE_BUFFER_SIZE,
                    output.as_file_mut(),
                );
                let len = copy_content_length_body_with_buffer(
                    reader,
                    &mut file,
                    content_length,
                    buffer,
                )?;
                file.flush()?;
                len
            }
        } else if close_delimited {
            {
                let mut file = io::BufWriter::with_capacity(
                    FILE_STREAM_WRITE_BUFFER_SIZE,
                    output.as_file_mut(),
                );
                let len = copy_stream_with_buffer(reader, &mut file, buffer)?;
                file.flush()?;
                len
            }
        } else {
            return Err("HTTP/1 response is missing Content-Length".into());
        };
        persist_output_file(output, output_file, len)?
    } else if empty_body_response {
        ResponseBody::Memory(Vec::new())
    } else if chunked {
        read_chunked_body_spooled_with_buffers(reader, line, buffer)?
    } else if let Some(content_length) = content_length {
        read_content_length_body_with_buffer(reader, content_length, buffer)?
    } else if close_delimited {
        read_close_delimited_body_spooled_with_buffer(reader, buffer)?
    } else {
        return Err("HTTP/1 response is missing Content-Length".into());
    };

    Ok(TransportResponse {
        version,
        status,
        headers,
        reusable_connection: if close_delimited || connection_close {
            false
        } else {
            default_reusable_connection || connection_keep_alive
        },
        body,
    })
}

fn request_method_allows_empty_response_body(method: &http::Method) -> bool {
    *method == http::Method::HEAD
}

fn http_status_code(status: &str) -> Option<u16> {
    let bytes = status.as_bytes();
    if bytes.len() < 3 || !bytes[..3].iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(
        u16::from(bytes[0] - b'0') * 100
            + u16::from(bytes[1] - b'0') * 10
            + u16::from(bytes[2] - b'0'),
    )
}

fn http_status_code_is_redirect(code: u16) -> bool {
    matches!(code, 301 | 302 | 303 | 307 | 308)
}

fn http_status_code_allows_empty_body(code: u16) -> bool {
    (100..200).contains(&code) || code == 204 || code == 304
}

#[cfg(test)]
fn http_status_allows_empty_body(status: &str) -> bool {
    http_status_code(status).is_some_and(http_status_code_allows_empty_body)
}

fn response_status_text(status: http::StatusCode) -> ResponseStatus {
    response_status_text_from_known_code(status.as_u16())
        .unwrap_or_else(|| status.to_string().into())
}

#[cfg(test)]
fn response_status_text_from_str(status: &str) -> ResponseStatus {
    response_status_text_from_code_and_str(http_status_code(status), status)
}

fn response_status_text_from_code_and_str(
    status_code: Option<u16>,
    status: &str,
) -> ResponseStatus {
    if let Some(known) = status_code.and_then(response_status_text_from_known_code)
        && known.as_str() == status
    {
        return known;
    }
    status.to_owned().into()
}

fn response_status_text_from_known_code(code: u16) -> Option<ResponseStatus> {
    match code {
        100 => Some("100 Continue".into()),
        101 => Some("101 Switching Protocols".into()),
        102 => Some("102 Processing".into()),
        103 => Some("103 Early Hints".into()),
        200 => Some("200 OK".into()),
        201 => Some("201 Created".into()),
        202 => Some("202 Accepted".into()),
        203 => Some("203 Non-Authoritative Information".into()),
        204 => Some("204 No Content".into()),
        205 => Some("205 Reset Content".into()),
        206 => Some("206 Partial Content".into()),
        300 => Some("300 Multiple Choices".into()),
        301 => Some("301 Moved Permanently".into()),
        302 => Some("302 Found".into()),
        303 => Some("303 See Other".into()),
        304 => Some("304 Not Modified".into()),
        307 => Some("307 Temporary Redirect".into()),
        308 => Some("308 Permanent Redirect".into()),
        400 => Some("400 Bad Request".into()),
        401 => Some("401 Unauthorized".into()),
        403 => Some("403 Forbidden".into()),
        404 => Some("404 Not Found".into()),
        405 => Some("405 Method Not Allowed".into()),
        407 => Some("407 Proxy Authentication Required".into()),
        408 => Some("408 Request Timeout".into()),
        409 => Some("409 Conflict".into()),
        410 => Some("410 Gone".into()),
        411 => Some("411 Length Required".into()),
        412 => Some("412 Precondition Failed".into()),
        413 => Some("413 Payload Too Large".into()),
        414 => Some("414 URI Too Long".into()),
        416 => Some("416 Range Not Satisfiable".into()),
        417 => Some("417 Expectation Failed".into()),
        425 => Some("425 Too Early".into()),
        426 => Some("426 Upgrade Required".into()),
        429 => Some("429 Too Many Requests".into()),
        431 => Some("431 Request Header Fields Too Large".into()),
        500 => Some("500 Internal Server Error".into()),
        501 => Some("501 Not Implemented".into()),
        502 => Some("502 Bad Gateway".into()),
        503 => Some("503 Service Unavailable".into()),
        504 => Some("504 Gateway Timeout".into()),
        505 => Some("505 HTTP Version Not Supported".into()),
        _ => None,
    }
}

fn plain_http1_response_is_close_delimited(
    default_reusable_connection: bool,
    connection_close: bool,
    connection_keep_alive: bool,
) -> bool {
    connection_close || (!default_reusable_connection && !connection_keep_alive)
}

fn read_close_delimited_body_spooled_with_buffer<R: Read>(
    reader: &mut R,
    buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> io::Result<ResponseBody> {
    let mut body = SpoolingResponseBody::new(0);
    copy_stream_with_buffer(reader, &mut body, buffer)?;
    body.finish()
}

fn redirect_response_headers_vec(redirect_response: bool) -> Vec<(String, String)> {
    if redirect_response {
        Vec::with_capacity(REDIRECT_RESPONSE_HEADER_CAPACITY)
    } else {
        Vec::new()
    }
}

fn exact_len_vec_initial_capacity(len: usize) -> usize {
    len.min(STREAM_BUFFER_SIZE)
}

fn read_exact_len_to_bytes_with_buffer<R: Read>(
    reader: &mut R,
    len: usize,
    buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> io::Result<Bytes> {
    read_exact_len_to_vec_with_buffer(reader, len, buffer).map(Bytes::from)
}

fn read_exact_len_to_vec_with_buffer<R: Read>(
    reader: &mut R,
    len: usize,
    buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> io::Result<Vec<u8>> {
    let mut body = Vec::with_capacity(exact_len_vec_initial_capacity(len));
    read_exact_len_into_vec_with_buffer(reader, len, &mut body, buffer)?;
    Ok(body)
}

fn read_exact_len_into_vec_with_buffer<R: Read>(
    reader: &mut R,
    len: usize,
    body: &mut Vec<u8>,
    buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> io::Result<()> {
    let mut remaining = len;
    while remaining > 0 {
        let read_len = remaining.min(buffer.len());
        reader
            .read_exact(&mut buffer[..read_len])
            .map_err(|error| {
                if error.kind() == io::ErrorKind::UnexpectedEof {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "HTTP response ended early")
                } else {
                    error
                }
            })?;
        body.extend_from_slice(&buffer[..read_len]);
        remaining -= read_len;
    }
    Ok(())
}

fn read_content_length_request_body_with_buffer<R: Read>(
    reader: &mut R,
    content_length: usize,
    buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> io::Result<RequestBody> {
    if content_length == 0 {
        return Ok(RequestBody::Empty);
    }
    let content_length_u64 = u64::try_from(content_length)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "content length too large"))?;
    if content_length_u64 > AUTO_RESPONSE_FILE_THRESHOLD {
        let (file, len) =
            copy_content_length_body_to_tempfile_with_buffer(reader, content_length, buffer)?;
        return Ok(RequestBody::TempFile { file, len });
    }
    read_exact_len_to_vec_with_buffer(reader, content_length, buffer)
        .map(|body| RequestBody::Memory(Bytes::from(body)))
}

fn read_content_length_body_with_buffer<R: Read>(
    reader: &mut R,
    content_length: usize,
    buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> io::Result<ResponseBody> {
    let content_length_u64 = u64::try_from(content_length)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "content length too large"))?;
    if content_length_u64 > AUTO_RESPONSE_FILE_THRESHOLD {
        let mut file = temp_response_file()?;
        let len = {
            let mut writer =
                io::BufWriter::with_capacity(FILE_STREAM_WRITE_BUFFER_SIZE, file.as_file_mut());
            let len =
                copy_content_length_body_with_buffer(reader, &mut writer, content_length, buffer)?;
            writer.flush()?;
            len
        };
        return persist_temp_response_file(file, len);
    }
    read_exact_len_to_vec_with_buffer(reader, content_length, buffer).map(ResponseBody::Memory)
}

fn copy_stream_to_tempfile<R: Read>(reader: &mut R) -> io::Result<(fs::File, u64)> {
    let file = tempfile::tempfile()?;
    let mut writer = io::BufWriter::with_capacity(FILE_STREAM_WRITE_BUFFER_SIZE, file);
    let len = copy_stream(reader, &mut writer)?;
    let mut file = writer.into_inner().map_err(|error| error.into_error())?;
    file.seek(SeekFrom::Start(0))?;
    Ok((file, len))
}

fn copy_content_length_body_to_tempfile_with_buffer<R: Read>(
    reader: &mut R,
    content_length: usize,
    buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> io::Result<(fs::File, u64)> {
    let file = tempfile::tempfile()?;
    let mut writer = io::BufWriter::with_capacity(FILE_STREAM_WRITE_BUFFER_SIZE, file);
    let len = copy_content_length_body_with_buffer(reader, &mut writer, content_length, buffer)?;
    let mut file = writer.into_inner().map_err(|error| error.into_error())?;
    file.seek(SeekFrom::Start(0))?;
    Ok((file, len))
}

fn copy_content_length_body_with_buffer<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    content_length: usize,
    buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> io::Result<u64> {
    let mut remaining = content_length;
    let copied = u64::try_from(content_length)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "content length too large"))?;
    while remaining > 0 {
        let read_len = remaining.min(buffer.len());
        reader
            .read_exact(&mut buffer[..read_len])
            .map_err(|error| {
                if error.kind() == io::ErrorKind::UnexpectedEof {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "HTTP response ended early")
                } else {
                    error
                }
            })?;
        writer.write_all(&buffer[..read_len])?;
        remaining -= read_len;
    }
    Ok(copied)
}

fn copy_http_response_body_with_buffer<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    buffer: &mut [u8; STREAM_BUFFER_SIZE],
    expected: Option<u64>,
) -> io::Result<u64> {
    let mut copied = 0_u64;
    loop {
        let len = reader.read(buffer)?;
        if len == 0 {
            finish_http_body_length(copied, expected)?;
            return Ok(copied);
        }
        write_http_body_bytes_checked(writer, &mut copied, &buffer[..len], expected)?;
    }
}

fn copy_http_response_body_to_spooling_response_with_buffer<R: Read>(
    reader: &mut R,
    writer: &mut SpoolingResponseBody,
    buffer: &mut [u8],
) -> io::Result<u64> {
    let mut copied = 0_u64;
    let mut first_read = true;
    loop {
        let was_first_read = first_read;
        let len = {
            let _trace = phase_trace(if was_first_read {
                "helper.request_once.body.unknown_length_copy.read.first"
            } else {
                "helper.request_once.body.unknown_length_copy.read.rest"
            });
            reader.read(buffer)?
        };
        first_read = false;
        if len == 0 {
            finish_http_body_length(copied, None)?;
            return Ok(copied);
        }
        if !was_first_read {
            let _trace = phase_trace(if len == buffer.len() {
                "helper.request_once.body.unknown_length_copy.read.rest.full_buffer"
            } else {
                "helper.request_once.body.unknown_length_copy.read.rest.partial"
            });
            let _ = &_trace;
        }
        {
            let _trace = phase_trace("helper.request_once.body.unknown_length_copy.write");
            append_unknown_length_http_body_bytes(writer, &mut copied, &buffer[..len])?;
        }
    }
}

fn append_unknown_length_http_body_bytes(
    writer: &mut SpoolingResponseBody,
    written: &mut u64,
    bytes: &[u8],
) -> io::Result<()> {
    let next_written = checked_http_body_len(*written, bytes.len(), None)?;
    if let Some(file) = writer.file.as_mut() {
        file.write_all(bytes)?;
        writer.written = next_written;
        *written = next_written;
        return Ok(());
    }

    let next_len = writer
        .memory
        .len()
        .checked_add(bytes.len())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "body too large"))?;
    if u64::try_from(next_len).unwrap_or(u64::MAX) > AUTO_RESPONSE_FILE_THRESHOLD {
        let file = temp_response_file()?;
        let mut file = io::BufWriter::with_capacity(FILE_STREAM_WRITE_BUFFER_SIZE, file);
        file.write_all(&writer.memory)?;
        writer.memory.clear();
        writer.memory.shrink_to_fit();
        file.write_all(bytes)?;
        writer.file = Some(file);
    } else {
        writer.memory.extend_from_slice(bytes);
    }
    writer.written = next_written;
    *written = next_written;
    Ok(())
}

fn temp_response_file() -> io::Result<tempfile::NamedTempFile> {
    tempfile::Builder::new()
        .prefix("skron-git-remote-http-response-")
        .suffix(".body")
        .tempfile()
}

fn persist_temp_response_file(file: tempfile::NamedTempFile, len: u64) -> io::Result<ResponseBody> {
    let path = file.into_temp_path().keep().map_err(|error| error.error)?;
    Ok(ResponseBody::File { path, len })
}

fn temp_output_file(output_file: &Path) -> io::Result<tempfile::NamedTempFile> {
    let parent = output_file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    tempfile::Builder::new()
        .prefix(".skron-git-remote-http-output-")
        .tempfile_in(parent)
}

fn persist_output_file(
    file: tempfile::NamedTempFile,
    output_file: &Path,
    len: u64,
) -> io::Result<ResponseBody> {
    file.persist(output_file).map_err(|error| error.error)?;
    Ok(ResponseBody::File {
        path: output_file.to_path_buf(),
        len,
    })
}

fn read_chunked_body_spooled_with_buffers<R: BufRead>(
    reader: &mut R,
    line: &mut String,
    buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> io::Result<ResponseBody> {
    let mut body = SpoolingResponseBody::new(0);
    write_chunked_body_with_buffers(reader, &mut body, line, buffer)?;
    body.finish()
}

fn write_chunked_body_with_buffers<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    line: &mut String,
    buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> io::Result<u64> {
    let mut written = 0_u64;
    loop {
        if read_limited_http_response_line(reader, line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "HTTP chunked response ended before chunk size",
            ));
        }
        let size = parse_http_chunk_size(line.as_bytes())?;
        if size == 0 {
            let mut trailers = 0_usize;
            loop {
                if read_limited_http_response_line(reader, line)? == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "HTTP chunked response ended before trailer terminator",
                    ));
                }
                if line == "\r\n" || line == "\n" {
                    return Ok(written);
                }
                trailers += 1;
                if trailers > HTTP_CHUNK_TRAILER_LIMIT {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "too many HTTP chunk trailers",
                    ));
                }
            }
        }
        let mut remaining = size;
        while remaining > 0 {
            let read_len = remaining.min(buffer.len());
            reader.read_exact(&mut buffer[..read_len])?;
            write_http_body_bytes_checked(writer, &mut written, &buffer[..read_len], None)?;
            remaining -= read_len;
        }
        let mut crlf = [0_u8; 2];
        reader.read_exact(&mut crlf)?;
        if crlf != *b"\r\n" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid chunk terminator",
            ));
        }
    }
}

fn read_limited_http_response_line<R: BufRead>(
    reader: &mut R,
    line: &mut String,
) -> io::Result<usize> {
    read_limited_line(
        reader,
        line,
        HTTP_RESPONSE_LINE_LIMIT,
        "HTTP response line too long",
    )
}

fn read_limited_line<R: BufRead>(
    reader: &mut R,
    line: &mut String,
    limit: usize,
    error_message: &'static str,
) -> io::Result<usize> {
    line.clear();
    // SAFETY: `line` is not observed as a `String` while bytes are appended.
    // Before every return after mutation, the buffer is either validated as
    // UTF-8 or cleared back to a valid empty string.
    let bytes = unsafe { line.as_mut_vec() };
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(error) => {
                bytes.clear();
                return Err(error);
            }
        };
        if available.is_empty() {
            break;
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        if bytes.len().saturating_add(take) > limit {
            bytes.clear();
            return Err(io::Error::new(io::ErrorKind::InvalidData, error_message));
        }
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if bytes.ends_with(b"\n") {
            break;
        }
    }
    if bytes.is_empty() {
        return Ok(0);
    }
    if std::str::from_utf8(bytes).is_err() {
        bytes.clear();
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP response line is not UTF-8",
        ));
    }
    Ok(bytes.len())
}

fn trim_reusable_http_line_buffer(line: &mut String) {
    line.clear();
    if line.capacity() > HTTP_LINE_BUFFER_RETAIN_CAPACITY_LIMIT {
        *line = String::with_capacity(HTTP_LINE_BUFFER_CAPACITY);
    }
}

fn parse_http_chunk_size(line: &[u8]) -> io::Result<usize> {
    let mut size = 0_usize;
    let mut saw_digit = false;
    for byte in line {
        let value = match *byte {
            b'0'..=b'9' => usize::from(*byte - b'0'),
            b'a'..=b'f' => usize::from(*byte - b'a' + 10),
            b'A'..=b'F' => usize::from(*byte - b'A' + 10),
            b';' | b'\r' | b'\n' => break,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid chunk size",
                ));
            }
        };
        saw_digit = true;
        size = size
            .checked_mul(16)
            .and_then(|size| size.checked_add(value))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "chunk too large"))?;
    }
    if !saw_digit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid chunk size",
        ));
    }
    Ok(size)
}

fn parse_decimal_usize(
    value: &[u8],
    invalid_message: &'static str,
    overflow_message: &'static str,
) -> io::Result<usize> {
    let mut parsed = 0_usize;
    let mut saw_digit = false;
    for byte in value {
        let digit = match *byte {
            b'0'..=b'9' => usize::from(*byte - b'0'),
            _ => {
                return Err(io::Error::new(io::ErrorKind::InvalidData, invalid_message));
            }
        };
        saw_digit = true;
        parsed = parsed
            .checked_mul(10)
            .and_then(|parsed| parsed.checked_add(digit))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, overflow_message))?;
    }
    if !saw_digit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, invalid_message));
    }
    Ok(parsed)
}

fn parse_decimal_u64(value: &[u8]) -> Option<u64> {
    let mut parsed = 0_u64;
    let mut saw_digit = false;
    for byte in value {
        let digit = match *byte {
            b'0'..=b'9' => u64::from(*byte - b'0'),
            _ => return None,
        };
        saw_digit = true;
        parsed = parsed.checked_mul(10)?.checked_add(digit)?;
    }
    saw_digit.then_some(parsed)
}

fn plain_http1_origin_key(
    url: &reqwest::Url,
    headers: &[RequestHeader],
    proxy_identity_override: Option<Arc<str>>,
    tls_verification_identity_override: Option<Arc<str>>,
) -> Result<PlainHttp1Origin, Box<dyn std::error::Error>> {
    let host = url.host_str().ok_or("URL is missing host")?;
    let port = url.port_or_known_default().unwrap_or(80);
    let credential_identity = request_credential_identity(url, headers);
    Ok(PlainHttp1Origin {
        scheme: url.scheme().to_owned(),
        host: host.to_owned(),
        port,
        proxy_identity: proxy_identity_override.unwrap_or_else(|| Arc::from(proxy_identity())),
        tls_verification_identity: tls_verification_identity_override
            .unwrap_or_else(|| Arc::from(tls_verification_identity(None, None, None, false))),
        credential_identity,
    })
}

fn request_credential_identity(url: &reqwest::Url, headers: &[RequestHeader]) -> Option<String> {
    if let Some(identity) = request_header_credential_identity(headers) {
        return Some(identity);
    }
    request_url_credential_identity(url.username(), url.password())
}

fn request_credential_identity_from_authority(
    authority: &str,
    headers: &[RequestHeader],
) -> Option<String> {
    if let Some(identity) = request_header_credential_identity(headers) {
        return Some(identity);
    }
    request_url_credential_identity_from_authority(authority)
}

fn request_url_credential_identity_from_authority(authority: &str) -> Option<String> {
    let (userinfo, _) = authority.split_once('@')?;
    let (user, password) = userinfo
        .split_once(':')
        .map_or((userinfo, None), |(user, password)| (user, Some(password)));
    request_url_credential_identity(user, password)
}

fn request_credential_identity_from_uri(
    uri: &http::Uri,
    headers: &[RequestHeader],
) -> Option<String> {
    if let Some(identity) = request_header_credential_identity(headers) {
        return Some(identity);
    }
    uri.authority()
        .and_then(|authority| request_url_credential_identity_from_authority(authority.as_str()))
}

fn request_credential_identity_matches_url(
    expected: Option<&str>,
    url: &reqwest::Url,
    headers: &[RequestHeader],
) -> bool {
    if let Some(matches) = request_header_credential_identity_matches(expected, headers) {
        return matches;
    }
    request_url_credential_identity_matches(expected, url.username(), url.password())
}

fn request_credential_identity_matches_authority(
    expected: Option<&str>,
    authority: &str,
    headers: &[RequestHeader],
) -> bool {
    if let Some(matches) = request_header_credential_identity_matches(expected, headers) {
        return matches;
    }
    let Some((userinfo, _)) = authority.split_once('@') else {
        return expected.is_none();
    };
    let (user, password) = userinfo
        .split_once(':')
        .map_or((userinfo, None), |(user, password)| (user, Some(password)));
    request_url_credential_identity_matches(expected, user, password)
}

fn request_header_credential_identity_matches(
    expected: Option<&str>,
    headers: &[RequestHeader],
) -> Option<bool> {
    let mut proxy_auth = None;
    for header in headers {
        if header.name == AUTHORIZATION {
            if let Ok(auth) = header.value.to_str() {
                return Some(prefixed_identity_matches(
                    expected,
                    "authorization:",
                    auth.trim(),
                ));
            }
        } else if header.name == PROXY_AUTHORIZATION
            && proxy_auth.is_none()
            && let Ok(auth) = header.value.to_str()
        {
            proxy_auth = Some(auth.trim());
        }
    }
    proxy_auth.map(|auth| prefixed_identity_matches(expected, "proxy-authorization:", auth))
}

fn request_header_credential_identity(headers: &[RequestHeader]) -> Option<String> {
    let mut proxy_auth = None;
    for header in headers {
        if header.name == AUTHORIZATION {
            if let Ok(auth) = header.value.to_str() {
                return Some(prefixed_identity("authorization:", auth.trim()));
            }
        } else if header.name == PROXY_AUTHORIZATION
            && proxy_auth.is_none()
            && let Ok(auth) = header.value.to_str()
        {
            proxy_auth = Some(prefixed_identity("proxy-authorization:", auth.trim()));
        }
    }
    proxy_auth
}

fn prefixed_identity_matches(expected: Option<&str>, prefix: &str, value: &str) -> bool {
    expected.and_then(|identity| identity.strip_prefix(prefix)) == Some(value)
}

fn prefixed_identity(prefix: &str, value: &str) -> String {
    let mut identity = String::with_capacity(prefix.len() + value.len());
    identity.push_str(prefix);
    identity.push_str(value);
    identity
}

fn request_url_credential_identity_matches(
    expected: Option<&str>,
    user: &str,
    password: Option<&str>,
) -> bool {
    if user.is_empty() {
        return expected.is_none();
    }
    let Some(rest) = expected.and_then(|identity| identity.strip_prefix("url-user:")) else {
        return false;
    };
    if let Some(password) = password {
        rest.len() == user.len() + 1 + password.len()
            && rest.starts_with(user)
            && rest.as_bytes().get(user.len()) == Some(&b':')
            && rest[user.len() + 1..] == *password
    } else {
        rest == user
    }
}

fn request_url_credential_identity(user: &str, password: Option<&str>) -> Option<String> {
    if user.is_empty() {
        return None;
    }
    let mut credential_identity =
        String::with_capacity(url_credential_identity_len(user, password));
    credential_identity.push_str("url-user:");
    credential_identity.push_str(user);
    if let Some(password) = password.filter(|password| !password.is_empty()) {
        credential_identity.push(':');
        credential_identity.push_str(password);
    }
    Some(credential_identity)
}

fn url_credential_identity_len(user: &str, password: Option<&str>) -> usize {
    "url-user:".len()
        + user.len()
        + password
            .filter(|password| !password.is_empty())
            .map_or(0, |password| 1 + password.len())
}

fn proxy_identity() -> String {
    proxy_identity_from_values(
        PROXY_ENV_VARS
            .iter()
            .filter_map(|var_name| std::env::var(var_name).ok().map(|value| (*var_name, value))),
    )
}

fn proxy_identity_from_values<'a, I, V>(values: I) -> String
where
    I: IntoIterator<Item = (&'a str, V)>,
    V: AsRef<str>,
{
    let mut identity = String::new();
    for (var_name, value) in values {
        let value = value.as_ref().trim();
        if value.is_empty() {
            continue;
        }
        if identity.is_empty() {
            identity.reserve(var_name.len() + 1 + value.len());
        } else {
            identity.reserve(1 + var_name.len() + 1 + value.len());
            identity.push('|');
        }
        identity.push_str(var_name);
        identity.push('=');
        identity.push_str(value);
    }
    if identity.is_empty() {
        NONE_IDENTITY.to_owned()
    } else {
        identity
    }
}

fn tls_verification_identity(
    ca_file: Option<&str>,
    client_cert_file: Option<&str>,
    client_key_file: Option<&str>,
    tls_no_verify: bool,
) -> String {
    let client_identity = client_certificate_identity(client_cert_file, client_key_file);
    if tls_no_verify {
        return format_tls_identity("no-verify", client_identity.as_deref());
    }
    let Some(ca_file) = ca_file.map(str::trim).filter(|ca_file| !ca_file.is_empty()) else {
        return format_tls_identity(
            DEFAULT_TLS_VERIFICATION_IDENTITY,
            client_identity.as_deref(),
        );
    };
    format_tls_identity(&format!("ca_file:{ca_file}"), client_identity.as_deref())
}

fn client_certificate_identity(
    client_cert_file: Option<&str>,
    client_key_file: Option<&str>,
) -> Option<String> {
    let cert = client_cert_file
        .map(str::trim)
        .filter(|path| !path.is_empty())?;
    let key = client_key_file
        .map(str::trim)
        .filter(|path| !path.is_empty());
    Some(match key {
        Some(key) => format!("client_cert:{cert}|client_key:{key}"),
        None => format!("client_cert:{cert}"),
    })
}

fn format_tls_identity(verification: &str, client_identity: Option<&str>) -> String {
    match client_identity {
        Some(client_identity) => format!("{verification}|{client_identity}"),
        None => verification.to_owned(),
    }
}

fn write_plain_http1_request_head(
    head: &mut Vec<u8>,
    request: &TransportRequest,
    url: &reqwest::Url,
) -> Result<(), Box<dyn std::error::Error>> {
    head.clear();
    let body_len = request.body.len()?;
    let has_user_agent = request_headers_include_user_agent(&request.headers);
    reserve_plain_http1_request_head(
        head,
        plain_http1_request_head_capacity(request, url, body_len, has_user_agent),
    );
    head.extend_from_slice(request.method.as_str().as_bytes());
    head.extend_from_slice(b" ");
    write_plain_http1_request_target(head, url)?;
    head.extend_from_slice(b" HTTP/1.1\r\nHost: ");
    write_plain_http1_host_header(head, url)?;
    if !has_user_agent {
        head.extend_from_slice(b"\r\nUser-Agent: skron-git-remote-http/");
        head.extend_from_slice(env!("CARGO_PKG_VERSION").as_bytes());
    }
    head.extend_from_slice(b"\r\nConnection: keep-alive\r\nContent-Length: ");
    write_decimal_u64(head, body_len)?;
    head.extend_from_slice(b"\r\n");
    for header in &request.headers {
        head.extend_from_slice(header.name.as_str().as_bytes());
        head.extend_from_slice(b": ");
        head.extend_from_slice(header.value.as_bytes());
        head.extend_from_slice(b"\r\n");
    }
    head.extend_from_slice(b"\r\n");
    Ok(())
}

fn reserve_plain_http1_request_head(head: &mut Vec<u8>, needed: usize) {
    if head.capacity() < needed {
        head.reserve(needed.saturating_sub(head.len()));
    }
}

fn trim_plain_http1_request_head_buffer(head: &mut Vec<u8>) {
    head.clear();
    if head.capacity() > PLAIN_HTTP1_REQUEST_HEAD_RETAIN_CAPACITY_LIMIT {
        *head = Vec::with_capacity(PLAIN_HTTP1_REQUEST_HEAD_INITIAL_CAPACITY);
    }
}

fn write_decimal_u64<W: Write>(writer: &mut W, mut value: u64) -> io::Result<()> {
    let mut buf = [0_u8; 20];
    let mut cursor = buf.len();
    if value == 0 {
        cursor -= 1;
        buf[cursor] = b'0';
    } else {
        while value != 0 {
            cursor -= 1;
            buf[cursor] = b'0' + u8::try_from(value % 10).expect("decimal digit");
            value /= 10;
        }
    }
    writer.write_all(&buf[cursor..])
}

fn plain_http1_request_head_capacity(
    request: &TransportRequest,
    url: &reqwest::Url,
    body_len: u64,
    has_user_agent: bool,
) -> usize {
    let target_len = plain_http1_request_target_len(url);
    let host_len = url
        .host_str()
        .map(|host| plain_http1_host_header_len(host, url))
        .unwrap_or(0);
    let header_len = request
        .headers
        .iter()
        .map(|header| header.name.as_str().len() + header.value.as_bytes().len() + 4)
        .sum::<usize>();
    request.method.as_str().len()
        + 1
        + target_len
        + " HTTP/1.1\r\nHost: ".len()
        + host_len
        + plain_http1_default_user_agent_len(has_user_agent)
        + "\r\nConnection: keep-alive\r\nContent-Length: ".len()
        + decimal_len_u64(body_len)
        + "\r\n".len()
        + header_len
        + "\r\n".len()
}

fn plain_http1_default_user_agent_len(has_user_agent: bool) -> usize {
    if has_user_agent {
        0
    } else {
        "\r\nUser-Agent: skron-git-remote-http/".len() + env!("CARGO_PKG_VERSION").len()
    }
}

fn request_headers_include_user_agent(headers: &[RequestHeader]) -> bool {
    headers
        .iter()
        .any(|header| header.name.as_str().eq_ignore_ascii_case("user-agent"))
}

fn plain_http1_request_target_len(url: &reqwest::Url) -> usize {
    let path_len = if url.path().is_empty() {
        1
    } else {
        url.path().len()
    };
    path_len + url.query().map_or(0, |query| 1 + query.len())
}

fn plain_http1_port_suffix_len(url: &reqwest::Url) -> usize {
    let port = url.port_or_known_default().unwrap_or(80);
    if port == 80 {
        0
    } else {
        1 + decimal_len_u64(u64::from(port))
    }
}

fn plain_http1_host_header_len(host: &str, url: &reqwest::Url) -> usize {
    let host_len = if plain_http1_host_needs_ipv6_brackets(host) {
        host.len() + 2
    } else {
        host.len()
    };
    host_len + plain_http1_port_suffix_len(url)
}

fn plain_http1_host_needs_ipv6_brackets(host: &str) -> bool {
    host.contains(':') && !host.starts_with('[')
}

fn write_plain_http1_request_target<W: Write>(
    writer: &mut W,
    url: &reqwest::Url,
) -> io::Result<()> {
    let path = url.path();
    if path.is_empty() {
        writer.write_all(b"/")?;
    } else {
        writer.write_all(path.as_bytes())?;
    }
    if let Some(query) = url.query() {
        writer.write_all(b"?")?;
        writer.write_all(query.as_bytes())?;
    }
    Ok(())
}

fn write_plain_http1_host_header<W: Write>(
    writer: &mut W,
    url: &reqwest::Url,
) -> Result<(), Box<dyn std::error::Error>> {
    let host = url.host_str().ok_or("URL is missing host")?;
    let port = url.port_or_known_default().unwrap_or(80);
    if plain_http1_host_needs_ipv6_brackets(host) {
        writer.write_all(b"[")?;
        writer.write_all(host.as_bytes())?;
        writer.write_all(b"]")?;
    } else {
        writer.write_all(host.as_bytes())?;
    }
    if port == 80 {
    } else {
        writer.write_all(b":")?;
        write_decimal_u64(writer, u64::from(port))?;
    }
    Ok(())
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ConnectionHeaderFlags {
    close: bool,
    keep_alive: bool,
}

fn connection_header_flags(mut value: &str) -> ConnectionHeaderFlags {
    let mut flags = ConnectionHeaderFlags::default();
    loop {
        let (part, rest) = match value.split_once(',') {
            Some((part, rest)) => (part, Some(rest)),
            None => (value, None),
        };
        let token = part.trim();
        if token.eq_ignore_ascii_case("close") {
            flags.close = true;
        } else if token.eq_ignore_ascii_case("keep-alive") {
            flags.keep_alive = true;
        }
        let Some(rest) = rest else {
            return flags;
        };
        value = rest;
    }
}

fn plain_http1_transfer_encoding_is_chunked(value: &str) -> io::Result<bool> {
    let mut chunked = false;
    let mut rest = value;
    loop {
        let (part, next) = match rest.split_once(',') {
            Some((part, next)) => (part, Some(next)),
            None => (rest, None),
        };
        let coding = part.trim();
        if !coding.is_empty() && !coding.eq_ignore_ascii_case("chunked") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported Transfer-Encoding header",
            ));
        }
        chunked |= !coding.is_empty();
        let Some(next) = next else {
            return Ok(chunked);
        };
        rest = next;
    }
}

fn request_once(
    client: &Client,
    options: RequestOnceOptions,
    method: http::Method,
    url: &str,
    headers: &[RequestHeader],
    body: RequestBody,
    output_file: Option<&Path>,
) -> Result<TransportResponse, Box<dyn std::error::Error>> {
    let mut response_buffer = [0_u8; STREAM_BUFFER_SIZE];
    request_once_with_buffer(
        client,
        options,
        RequestOnceInput {
            method,
            url,
            headers,
            body,
            output_file,
        },
        &mut response_buffer,
    )
}

struct RequestOnceInput<'a> {
    method: http::Method,
    url: &'a str,
    headers: &'a [RequestHeader],
    body: RequestBody,
    output_file: Option<&'a Path>,
}

struct Http3RequestInput<'a> {
    method: http::Method,
    url: &'a str,
    headers: &'a [RequestHeader],
    body: RequestBody,
    output_file: Option<&'a Path>,
    include_response_headers: bool,
}

struct Http3PooledRequestInput<'a> {
    method: &'a http::Method,
    uri: &'a http::Uri,
    headers: &'a [RequestHeader],
    body: RequestBody,
    output_file: Option<&'a Path>,
    connect_timeout: Option<Duration>,
}

fn request_once_with_buffer(
    client: &Client,
    options: RequestOnceOptions,
    input: RequestOnceInput<'_>,
    response_buffer: &mut [u8; STREAM_BUFFER_SIZE],
) -> Result<TransportResponse, Box<dyn std::error::Error>> {
    let empty_body_request = request_method_allows_empty_response_body(&input.method);
    validate_request_body_for_send(&input.body)?;
    let request = {
        let _trace = phase_trace("helper.request_once.build");
        let mut request = client.request(input.method, input.url);
        for header in input.headers {
            request = request.header(header.name.clone(), header.value.clone());
        }
        match input.body {
            RequestBody::Empty => {}
            RequestBody::Memory(body) if body.is_empty() => {}
            RequestBody::Memory(body) => {
                request = request.body(body);
            }
            RequestBody::File { file, len } => {
                request = request.body(Body::sized(file, len));
            }
            RequestBody::TempFile { file, len } => {
                request = request.body(Body::sized(file, len));
            }
            RequestBody::Chain {
                prefix,
                file,
                file_len,
            } => {
                let len = u64::try_from(prefix.len())
                    .map_err(|_| "request body too large")?
                    .checked_add(file_len)
                    .ok_or("request body too large")?;
                request = request.body(Body::sized(
                    ChainedRequestBodyReader {
                        prefix: io::Cursor::new(prefix),
                        file,
                    },
                    len,
                ));
            }
        }
        request
    };
    let mut response = {
        let _trace = phase_trace("helper.request_once.send");
        let _split_trace = options.send_phase_label.and_then(phase_trace);
        request.send()?
    };
    let response_version = response.version();
    let status_code = response.status();
    enforce_response_version(options.expected_version, response_version)?;
    let version = response.version_text();
    let status = response_status_text(status_code);
    let content_encoding = response
        .headers()
        .get(reqwest::header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let transfer_encoding = response
        .headers()
        .get(reqwest::header::TRANSFER_ENCODING)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let headers = {
        let _trace = phase_trace("helper.request_once.headers");
        if options.include_response_headers {
            collect_reqwest_response_headers(&response)
        } else {
            collect_redirect_response_headers(status_code.as_u16(), response.headers())
        }
    };
    let expected_content_length = response.content_length();
    let empty_body_response =
        empty_body_request || http_status_code_allows_empty_body(status_code.as_u16());
    let body = {
        let _trace = phase_trace("helper.request_once.body");
        if let Some(output_file) = input.output_file {
            let mut output = temp_output_file(output_file)?;
            let len = if empty_body_response {
                0
            } else {
                let mut file = io::BufWriter::with_capacity(
                    FILE_STREAM_WRITE_BUFFER_SIZE,
                    output.as_file_mut(),
                );
                let len = copy_http_response_body_with_buffer(
                    &mut response,
                    &mut file,
                    response_buffer,
                    expected_content_length,
                )?;
                file.flush()?;
                len
            };
            persist_output_file(output, output_file, len)?
        } else if empty_body_response {
            ResponseBody::Memory(Vec::new())
        } else if let Some(content_length) = expected_content_length {
            if response_content_length_spills_to_file(Some(content_length)) {
                let mut file = temp_response_file()?;
                let len = {
                    let mut writer = io::BufWriter::with_capacity(
                        FILE_STREAM_WRITE_BUFFER_SIZE,
                        file.as_file_mut(),
                    );
                    let len = copy_http_response_body_with_buffer(
                        &mut response,
                        &mut writer,
                        response_buffer,
                        Some(content_length),
                    )?;
                    writer.flush()?;
                    len
                };
                persist_temp_response_file(file, len)?
            } else {
                let content_length = usize::try_from(content_length).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "content length too large")
                })?;
                ResponseBody::Memory(read_exact_len_to_vec_with_buffer(
                    &mut response,
                    content_length,
                    response_buffer,
                )?)
            }
        } else {
            let mut body = SpoolingResponseBody::new(0);
            {
                let _trace = phase_trace("helper.request_once.body.unknown_length_copy");
                copy_http_response_body_to_spooling_response_with_buffer(
                    &mut response,
                    &mut body,
                    response_buffer,
                )?;
            }
            {
                let _trace = phase_trace("helper.request_once.body.unknown_length_finish");
                body.finish()?
            }
        }
    };
    let body_kind = match &body {
        ResponseBody::Memory(body) => format!("memory:{}B", body.len()),
        ResponseBody::File { len, .. } => format!("file:{len}B"),
    };
    trace_event(format!(
        "helper.request_once.response\tversion={version}\tstatus={}\tcontent_length={}\tcontent_encoding={content_encoding}\ttransfer_encoding={transfer_encoding}\tbody={body_kind}",
        status.as_str(),
        expected_content_length
            .map(|len| len.to_string())
            .unwrap_or_else(|| "none".to_owned())
    ));
    Ok(TransportResponse {
        version,
        status,
        headers,
        reusable_connection: true,
        body,
    })
}

fn collect_reqwest_response_headers(
    response: &reqwest::blocking::Response,
) -> Vec<(String, String)> {
    collect_http_response_headers(response.headers())
}

fn collect_http_response_headers(headers: &http::HeaderMap) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(headers.len().min(COLLECTED_RESPONSE_HEADER_LIMIT));
    let mut bytes = 0_usize;
    for (name, value) in headers {
        if out.len() >= COLLECTED_RESPONSE_HEADER_LIMIT {
            break;
        }
        let Ok(value) = value.to_str() else {
            continue;
        };
        let next_bytes = name
            .as_str()
            .len()
            .checked_add(value.len())
            .and_then(|len| bytes.checked_add(len));
        let Some(next_bytes) = next_bytes else {
            break;
        };
        if next_bytes > COLLECTED_RESPONSE_HEADER_BYTES_LIMIT {
            break;
        }
        out.push((name.as_str().to_owned(), value.to_owned()));
        bytes = next_bytes;
    }
    out
}

fn collect_redirect_response_headers(
    status_code: u16,
    headers: &http::HeaderMap,
) -> Vec<(String, String)> {
    if !http_status_code_is_redirect(status_code) {
        return Vec::new();
    }
    let Some(value) = headers.get(http::header::LOCATION) else {
        return Vec::new();
    };
    let Ok(value) = value.to_str() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(REDIRECT_RESPONSE_HEADER_CAPACITY);
    out.push(("location".to_owned(), value.to_owned()));
    out
}

fn enforce_response_version(
    expected_version: HttpVersion,
    actual_version: reqwest::Version,
) -> Result<(), Box<dyn std::error::Error>> {
    match (expected_version, actual_version) {
        (HttpVersion::Http1, reqwest::Version::HTTP_10 | reqwest::Version::HTTP_11)
        | (HttpVersion::Http2, reqwest::Version::HTTP_2)
        | (HttpVersion::Auto, _)
        | (HttpVersion::Http3, reqwest::Version::HTTP_3) => Ok(()),
        (HttpVersion::Http1, actual) => {
            Err(format!("expected HTTP/1.x response, got {actual:?}").into())
        }
        (HttpVersion::Http2, actual) => {
            Err(format!("expected HTTP/2 response, got {actual:?}").into())
        }
        (HttpVersion::Http3, actual) => {
            Err(format!("expected HTTP/3 response, got {actual:?}").into())
        }
    }
}

fn request_http3(
    method: http::Method,
    url: &str,
    headers: &[RequestHeader],
    body: RequestBody,
    output_file: Option<&Path>,
    ca_file: Option<&str>,
    client_cert_file: Option<&str>,
    client_key_file: Option<&str>,
    tls_no_verify: bool,
    include_response_headers: bool,
) -> Result<TransportResponse, Box<dyn std::error::Error>> {
    request_http3_with_connect_timeout(
        Http3RequestInput {
            method,
            url,
            headers,
            body,
            output_file,
            include_response_headers,
        },
        ca_file,
        client_cert_file,
        client_key_file,
        tls_no_verify,
        None,
    )
}

fn request_http3_with_connect_timeout(
    input: Http3RequestInput<'_>,
    ca_file: Option<&str>,
    client_cert_file: Option<&str>,
    client_key_file: Option<&str>,
    tls_no_verify: bool,
    connect_timeout: Option<Duration>,
) -> Result<TransportResponse, Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    runtime.block_on(request_http3_async(
        input,
        ca_file,
        client_cert_file,
        client_key_file,
        tls_no_verify,
        connect_timeout,
    ))
}

async fn request_http3_async(
    input: Http3RequestInput<'_>,
    ca_file: Option<&str>,
    client_cert_file: Option<&str>,
    client_key_file: Option<&str>,
    tls_no_verify: bool,
    connect_timeout: Option<Duration>,
) -> Result<TransportResponse, Box<dyn std::error::Error>> {
    validate_request_body_for_send(&input.body)?;
    let uri: http::Uri = input.url.parse()?;
    if uri.scheme_str() != Some("https") {
        return Err("HTTP/3 requires an https URL".into());
    }
    let authority = uri.authority().ok_or("HTTP/3 URL is missing authority")?;
    let host = normalized_authority_host(authority.host());
    let port = authority.port_u16().unwrap_or(443);
    let client_config =
        http3_client_config(ca_file, client_cert_file, client_key_file, tls_no_verify)?;
    let mut connection = connect_http3_origin(host, port, &client_config, connect_timeout).await?;

    let empty_body_request = request_method_allows_empty_response_body(&input.method);
    let mut request = http::Request::builder().method(input.method).uri(uri);
    for header in input.headers {
        request = request.header(header.name.clone(), header.value.clone());
    }
    let mut stream = connection
        .send_request
        .send_request(request.body(())?)
        .await?;
    let mut stream_buffer = BytesMut::with_capacity(STREAM_BUFFER_SIZE);
    send_validated_http3_request_body(&mut stream, input.body, &mut stream_buffer).await?;
    stream.finish().await?;

    let response = stream.recv_response().await?;
    let status_code = response.status();
    let response_headers = response.headers();
    let status = response_status_text(status_code);
    let headers = if input.include_response_headers {
        collect_http_response_headers(response_headers)
    } else {
        collect_redirect_response_headers(status_code.as_u16(), response_headers)
    };
    let body = read_http3_response_body(
        &mut stream,
        empty_body_request,
        status_code,
        response_headers,
        input.output_file,
    )
    .await?;
    Ok(TransportResponse {
        version: "3",
        status,
        headers,
        reusable_connection: true,
        body,
    })
}

async fn run_http3_batch_async<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    client_config: &quinn::ClientConfig,
    proxy_identity: Arc<str>,
    tls_verification_identity: Arc<str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut connections = http3_connection_pool();
    let mut line = batch_line_buffer();
    let mut request_body_buffer = [0_u8; STREAM_BUFFER_SIZE];
    let mut stream_buffer = BytesMut::with_capacity(STREAM_BUFFER_SIZE);
    let mut last_origin = None::<Http3PooledOrigin>;

    while let Some(request) =
        read_batch_request_with_line(reader, &mut line, &mut request_body_buffer)?
    {
        let TransportRequest {
            method,
            url,
            headers,
            body,
            output_file,
        } = request;
        let uri: http::Uri = url.parse()?;
        if uri.scheme_str() != Some("https") {
            return Err("HTTP/3 requires an https URL".into());
        }
        let authority = uri.authority().ok_or("HTTP/3 URL is missing authority")?;
        let mut new_origin = None;
        let pooled_origin = if let Some(origin) = last_origin.as_ref().filter(|origin| {
            http3_pooled_origin_matches_request(
                origin,
                authority,
                &headers,
                &proxy_identity,
                &tls_verification_identity,
            )
        }) {
            origin
        } else {
            let origin = http3_origin_key(authority);
            let credential_identity =
                request_credential_identity_from_authority(authority.as_str(), &headers);
            new_origin = Some(Http3PooledOrigin::from_origin(
                origin,
                proxy_identity.clone(),
                tls_verification_identity.clone(),
                credential_identity,
            ));
            new_origin.as_ref().expect("HTTP/3 pooled origin")
        };
        let response = request_http3_pooled_async(
            &mut connections,
            client_config,
            &pooled_origin,
            Http3PooledRequestInput {
                method: &method,
                uri: &uri,
                headers: &headers,
                body,
                output_file: output_file.as_deref(),
                connect_timeout: None,
            },
            &mut stream_buffer,
        )
        .await?;
        if let Some(origin) = new_origin {
            last_origin = Some(origin);
        }
        write_response_frame(
            writer,
            TransportResponse {
                version: response.version,
                status: response.status,
                headers: Vec::new(),
                reusable_connection: response.reusable_connection,
                body: response.body,
            },
        )?;
        writer.flush()?;
    }

    Ok(())
}

async fn request_http3_pooled_async(
    connections: &mut Http3ConnectionPool,
    client_config: &quinn::ClientConfig,
    origin: &Http3PooledOrigin,
    input: Http3PooledRequestInput<'_>,
    stream_buffer: &mut BytesMut,
) -> Result<TransportResponse, Box<dyn std::error::Error>> {
    if input.uri.scheme_str() != Some("https") {
        return Err("HTTP/3 requires an https URL".into());
    }
    let host = origin.origin.host.as_str();
    let port = origin.origin.port;
    validate_request_body_for_send(&input.body)?;
    let result = async {
        trim_http3_connection_pool_for_insert(connections, origin);
        let connection = match connections.entry(origin.clone()) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let connection =
                    connect_http3_origin(host, port, client_config, input.connect_timeout).await?;
                entry.insert(connection)
            }
        };
        let mut builder = http::Request::builder()
            .method(input.method.clone())
            .uri(input.uri);
        for header in input.headers {
            builder = builder.header(header.name.clone(), header.value.clone());
        }
        let mut stream = connection
            .send_request
            .send_request(builder.body(())?)
            .await?;
        send_validated_http3_request_body(&mut stream, input.body, stream_buffer).await?;
        stream.finish().await?;

        let response = stream.recv_response().await?;
        let status_code = response.status();
        let response_headers = response.headers();
        let status = response_status_text(status_code);
        let headers = collect_redirect_response_headers(status_code.as_u16(), response_headers);
        let body = read_http3_response_body(
            &mut stream,
            request_method_allows_empty_response_body(input.method),
            status_code,
            response_headers,
            input.output_file,
        )
        .await?;
        Ok(TransportResponse {
            version: "3",
            status,
            headers,
            reusable_connection: true,
            body,
        })
    }
    .await;
    if result.is_err() {
        connections.remove(origin);
    }
    result
}

fn trim_http3_connection_pool_for_insert(
    connections: &mut Http3ConnectionPool,
    incoming: &Http3PooledOrigin,
) {
    if let Some(evict) = http3_connection_pool_eviction_candidate(connections, incoming) {
        connections.remove(&evict);
    }
}

fn http3_connection_pool_eviction_candidate<V>(
    connections: &HashMap<Http3PooledOrigin, V>,
    incoming: &Http3PooledOrigin,
) -> Option<Http3PooledOrigin> {
    if connections.len() < HTTP3_CONNECTION_POOL_ENTRY_LIMIT {
        return None;
    }
    let mut evict = None;
    for origin in connections.keys() {
        if origin == incoming {
            return None;
        }
        if evict.is_none() {
            evict = Some(origin);
        }
    }
    evict.cloned()
}

async fn read_http3_response_body<S>(
    stream: &mut h3::client::RequestStream<S, Bytes>,
    empty_body_request: bool,
    status: http::StatusCode,
    headers: &http::HeaderMap,
    output_file: Option<&Path>,
) -> Result<ResponseBody, Box<dyn std::error::Error>>
where
    S: h3::quic::RecvStream,
{
    let content_length = http_content_length(headers)?;
    let empty_body_status =
        empty_body_request || http_status_code_allows_empty_body(status.as_u16());
    if let Some(output_file) = output_file {
        let mut output = temp_output_file(output_file)?;
        let written = if empty_body_status {
            finish_http3_empty_response_body(stream).await?;
            0
        } else {
            let mut file =
                io::BufWriter::with_capacity(FILE_STREAM_WRITE_BUFFER_SIZE, output.as_file_mut());
            let written =
                write_http3_response_body_to_writer(stream, &mut file, content_length).await?;
            file.flush()?;
            written
        };
        return Ok(persist_output_file(output, output_file, written)?);
    }

    if empty_body_status {
        finish_http3_empty_response_body(stream).await?;
        return Ok(ResponseBody::Memory(Vec::new()));
    }

    if response_content_length_spills_to_file(content_length) {
        let mut file = temp_response_file()?;
        let written = {
            let mut writer =
                io::BufWriter::with_capacity(FILE_STREAM_WRITE_BUFFER_SIZE, file.as_file_mut());
            let len =
                write_http3_response_body_to_writer(stream, &mut writer, content_length).await?;
            writer.flush()?;
            len
        };
        return Ok(persist_temp_response_file(file, written)?);
    }
    if let Some(content_length) = content_length {
        let capacity = memory_body_initial_capacity(content_length)?;
        let mut body = Vec::with_capacity(capacity);
        let mut written = 0_u64;
        while let Some(mut chunk) = stream.recv_data().await? {
            while chunk.has_remaining() {
                let bytes = chunk.chunk();
                let len = bytes.len();
                append_known_http_body_bytes(&mut body, &mut written, bytes, content_length)?;
                chunk.advance(len);
            }
        }
        finish_http_body_length(written, Some(content_length))?;
        return Ok(ResponseBody::Memory(body));
    }
    let mut body = SpoolingResponseBody::new(0);
    let mut written = 0_u64;
    while let Some(mut chunk) = stream.recv_data().await? {
        while chunk.has_remaining() {
            let bytes = chunk.chunk();
            add_http_body_bytes(&mut written, bytes.len(), content_length)?;
            body.write_all(bytes)?;
            let len = bytes.len();
            chunk.advance(len);
        }
    }
    finish_http_body_length(written, content_length)?;
    Ok(body.finish()?)
}

async fn finish_http3_empty_response_body<S>(
    stream: &mut h3::client::RequestStream<S, Bytes>,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: h3::quic::RecvStream,
{
    while let Some(mut chunk) = stream.recv_data().await? {
        while chunk.has_remaining() {
            let len = chunk.chunk().len();
            if len != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "HTTP response body exceeded empty status",
                )
                .into());
            }
            chunk.advance(len);
        }
    }
    Ok(())
}

async fn write_http3_response_body_to_writer<S, W>(
    stream: &mut h3::client::RequestStream<S, Bytes>,
    writer: &mut W,
    expected: Option<u64>,
) -> Result<u64, Box<dyn std::error::Error>>
where
    S: h3::quic::RecvStream,
    W: Write,
{
    let mut written = 0_u64;
    while let Some(mut chunk) = stream.recv_data().await? {
        while chunk.has_remaining() {
            let bytes = chunk.chunk();
            write_http_body_bytes_checked(writer, &mut written, bytes, expected)?;
            let len = bytes.len();
            chunk.advance(len);
        }
    }
    finish_http_body_length(written, expected)?;
    Ok(written)
}

fn add_http_body_bytes(written: &mut u64, len: usize, expected: Option<u64>) -> io::Result<()> {
    *written = checked_http_body_len(*written, len, expected)?;
    Ok(())
}

fn checked_http_body_len(current: u64, len: usize, expected: Option<u64>) -> io::Result<u64> {
    let next = current
        .checked_add(
            u64::try_from(len)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "body too large"))?,
        )
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "body too large"))?;
    if expected.is_some_and(|expected| next > expected) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP response body exceeded Content-Length",
        ));
    }
    Ok(next)
}

fn write_http_body_bytes_checked<W: Write>(
    writer: &mut W,
    written: &mut u64,
    bytes: &[u8],
    expected: Option<u64>,
) -> io::Result<()> {
    let next = checked_http_body_len(*written, bytes.len(), expected)?;
    writer.write_all(bytes)?;
    *written = next;
    Ok(())
}

fn append_known_http_body_bytes(
    body: &mut Vec<u8>,
    written: &mut u64,
    bytes: &[u8],
    expected: u64,
) -> io::Result<()> {
    add_http_body_bytes(written, bytes.len(), Some(expected))?;
    let spare = body.capacity().saturating_sub(body.len());
    if spare < bytes.len() {
        body.reserve(bytes.len() - spare);
    }
    body.extend_from_slice(bytes);
    Ok(())
}

fn finish_http_body_length(written: u64, expected: Option<u64>) -> io::Result<()> {
    if expected.is_some_and(|expected| written != expected) {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "HTTP response ended early",
        ));
    }
    Ok(())
}

async fn connect_http3_origin(
    host: &str,
    port: u16,
    client_config: &quinn::ClientConfig,
    connect_timeout: Option<Duration>,
) -> Result<Http3PooledConnection, Box<dyn std::error::Error>> {
    let remote_addr = resolve_remote_addr(host, port).await?;
    let mut endpoint = h3_quinn::Endpoint::client(local_bind_addr(remote_addr))?;
    endpoint.set_default_client_config(client_config.clone());
    let connecting = endpoint.connect(remote_addr, host)?;
    let connection = if let Some(connect_timeout) = connect_timeout {
        tokio::time::timeout(connect_timeout, connecting)
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("HTTP/3 connect to {host}:{port} timed out"),
                )
            })??
    } else {
        connecting.await?
    };
    let (mut driver, send_request) = h3::client::new(h3_quinn::Connection::new(connection)).await?;
    let driver = tokio::spawn(async move {
        let _ = std::future::poll_fn(|cx| driver.poll_close(cx)).await;
    });
    Ok(Http3PooledConnection {
        _endpoint: endpoint,
        send_request,
        driver,
    })
}

async fn resolve_remote_addr(host: &str, port: u16) -> io::Result<SocketAddr> {
    let mut addrs = tokio::net::lookup_host((host, port)).await?;
    addrs
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "host did not resolve"))
}

async fn send_validated_http3_request_body<S>(
    stream: &mut h3::client::RequestStream<S, Bytes>,
    body: RequestBody,
    buffer: &mut BytesMut,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: h3::quic::SendStream<Bytes>,
{
    match body {
        RequestBody::Empty => {}
        RequestBody::Memory(body) if body.is_empty() => {}
        RequestBody::Memory(body) => {
            stream.send_data(body).await?;
        }
        RequestBody::File { mut file, len } => {
            file.seek(SeekFrom::Start(0))?;
            send_http3_exact_len_reader(stream, file, len, buffer).await?;
        }
        RequestBody::TempFile { mut file, len } => {
            file.seek(SeekFrom::Start(0))?;
            send_http3_exact_len_reader(stream, file, len, buffer).await?;
        }
        RequestBody::Chain {
            prefix,
            mut file,
            file_len,
        } => {
            if !prefix.is_empty() {
                stream.send_data(prefix).await?;
            }
            file.seek(SeekFrom::Start(0))?;
            send_http3_exact_len_reader(stream, file, file_len, buffer).await?;
        }
    }
    Ok(())
}

async fn send_http3_exact_len_reader<S, R>(
    stream: &mut h3::client::RequestStream<S, Bytes>,
    mut reader: R,
    len: u64,
    buffer: &mut BytesMut,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: h3::quic::SendStream<Bytes>,
    R: Read,
{
    let mut remaining = len;
    while let Some(chunk) = read_http3_request_body_chunk(&mut reader, &mut remaining, buffer)? {
        stream.send_data(chunk).await?;
    }
    Ok(())
}

fn read_http3_request_body_chunk<R: Read>(
    reader: &mut R,
    remaining: &mut u64,
    buffer: &mut BytesMut,
) -> io::Result<Option<Bytes>> {
    if *remaining == 0 {
        return Ok(None);
    }
    buffer.clear();
    reserve_http3_request_body_buffer(buffer);
    let len = {
        let spare = buffer.spare_capacity_mut();
        let read_len = spare
            .len()
            .min(STREAM_BUFFER_SIZE)
            .min((*remaining).min(usize::MAX as u64) as usize);
        // Safe because `read` initializes at most `read_len` bytes, and
        // `set_len` below exposes exactly the initialized prefix.
        let read_buf =
            unsafe { std::slice::from_raw_parts_mut(spare.as_mut_ptr() as *mut u8, read_len) };
        reader.read(read_buf)?
    };
    if len == 0 {
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "request body file ended early",
        ))
    } else {
        *remaining -= len as u64;
        unsafe {
            buffer.set_len(len);
        }
        Ok(Some(buffer.split().freeze()))
    }
}

fn reserve_http3_request_body_buffer(buffer: &mut BytesMut) {
    if buffer.capacity() < STREAM_BUFFER_SIZE {
        buffer.reserve(STREAM_BUFFER_SIZE);
    }
}

fn http3_origin_key(authority: &http::uri::Authority) -> Http3Origin {
    let host = normalized_authority_host(authority.host());
    let port = authority.port_u16().unwrap_or(443);
    Http3Origin {
        scheme: "https".to_owned(),
        host: host.to_owned(),
        port,
    }
}

fn http3_pooled_origin_matches_request(
    origin: &Http3PooledOrigin,
    authority: &http::uri::Authority,
    headers: &[RequestHeader],
    proxy_identity: &Arc<str>,
    tls_verification_identity: &Arc<str>,
) -> bool {
    origin.origin.scheme == "https"
        && origin.origin.host == normalized_authority_host(authority.host())
        && origin.origin.port == authority.port_u16().unwrap_or(443)
        && origin.proxy_identity.as_ref() == proxy_identity.as_ref()
        && origin.tls_verification_identity.as_ref() == tls_verification_identity.as_ref()
        && request_credential_identity_matches_authority(
            origin.credential_identity.as_deref(),
            authority.as_str(),
            headers,
        )
}

fn http_content_length(headers: &http::HeaderMap) -> io::Result<Option<u64>> {
    let mut parsed = None;
    for value in headers.get_all(http::header::CONTENT_LENGTH) {
        let value = value.to_str().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length header")
        })?;
        let len = parse_decimal_u64(value.as_bytes()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length header")
        })?;
        if parsed.is_some_and(|existing| existing != len) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "conflicting Content-Length headers",
            ));
        }
        parsed = Some(len);
    }
    Ok(parsed)
}

fn response_content_length_spills_to_file(content_length: Option<u64>) -> bool {
    content_length.is_some_and(|len| len > AUTO_RESPONSE_FILE_THRESHOLD)
}

fn memory_body_initial_capacity(content_length: u64) -> io::Result<usize> {
    usize::try_from(content_length)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "content length too large"))
}

fn local_bind_addr(remote_addr: SocketAddr) -> SocketAddr {
    match remote_addr.ip() {
        IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    }
}

fn connect_rustls_http1_stream(
    stream: TcpStream,
    host: &str,
    config: Arc<rustls::ClientConfig>,
) -> Result<rustls::StreamOwned<rustls::ClientConnection, TcpStream>, Box<dyn std::error::Error>> {
    let server_name = ServerName::try_from(host.to_owned())?;
    let connection = rustls::ClientConnection::new(config, server_name)?;
    Ok(rustls::StreamOwned::new(connection, stream))
}

fn rustls_http1_client_config(
    ca_file: Option<&str>,
    client_cert_file: Option<&str>,
    client_key_file: Option<&str>,
    tls_no_verify: bool,
) -> Result<rustls::ClientConfig, Box<dyn std::error::Error>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])?;
    let mut tls = if tls_no_verify {
        finish_rustls_client_auth(
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoCertificateVerification)),
            client_cert_file,
            client_key_file,
        )?
    } else if let Some(ca_file) = ca_file {
        let roots = Arc::new(load_rustls_root_store(ca_file)?);
        let verifier = WebPkiServerVerifier::builder_with_provider(roots, provider)
            .build()
            .map(|verifier| {
                Arc::new(GitCompatibleCaServerVerifier { inner: verifier })
                    as Arc<dyn ServerCertVerifier>
            })?;
        finish_rustls_client_auth(
            builder
                .dangerous()
                .with_custom_certificate_verifier(verifier),
            client_cert_file,
            client_key_file,
        )?
    } else {
        finish_rustls_client_auth(
            builder.with_platform_verifier()?,
            client_cert_file,
            client_key_file,
        )?
    };
    tls.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(tls)
}

fn http3_client_config(
    ca_file: Option<&str>,
    client_cert_file: Option<&str>,
    client_key_file: Option<&str>,
    tls_no_verify: bool,
) -> Result<quinn::ClientConfig, Box<dyn std::error::Error>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?;
    let mut tls = if tls_no_verify {
        finish_rustls_client_auth(
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoCertificateVerification)),
            client_cert_file,
            client_key_file,
        )?
    } else if let Some(ca_file) = ca_file {
        let roots = Arc::new(load_rustls_root_store(ca_file)?);
        let verifier = WebPkiServerVerifier::builder_with_provider(roots, provider)
            .build()
            .map(|verifier| {
                Arc::new(GitCompatibleCaServerVerifier { inner: verifier })
                    as Arc<dyn ServerCertVerifier>
            })?;
        finish_rustls_client_auth(
            builder
                .dangerous()
                .with_custom_certificate_verifier(verifier),
            client_cert_file,
            client_key_file,
        )?
    } else {
        finish_rustls_client_auth(
            builder.with_platform_verifier()?,
            client_cert_file,
            client_key_file,
        )?
    };
    tls.alpn_protocols = vec![b"h3".to_vec()];
    Ok(quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(tls)?,
    )))
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
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}

#[derive(Debug)]
struct GitCompatibleCaServerVerifier {
    inner: Arc<WebPkiServerVerifier>,
}

impl ServerCertVerifier for GitCompatibleCaServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            Ok(verified) => Ok(verified),
            Err(error) if is_ca_used_as_end_entity_error(&error) => {
                let cert = rustls::server::ParsedCertificate::try_from(end_entity)?;
                rustls::client::verify_server_name(&cert, server_name)?;
                Ok(ServerCertVerified::assertion())
            }
            Err(error) => Err(error),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn is_ca_used_as_end_entity_error(error: &rustls::Error) -> bool {
    let rustls::Error::InvalidCertificate(rustls::CertificateError::Other(other)) = error else {
        return false;
    };
    other.0.downcast_ref::<webpki::Error>() == Some(&webpki::Error::CaUsedAsEndEntity)
}

fn finish_rustls_client_auth(
    builder: rustls::ConfigBuilder<rustls::ClientConfig, rustls::client::WantsClientCert>,
    client_cert_file: Option<&str>,
    client_key_file: Option<&str>,
) -> Result<rustls::ClientConfig, Box<dyn std::error::Error>> {
    let Some((certs, key)) = load_rustls_client_identity(client_cert_file, client_key_file)? else {
        return Ok(builder.with_no_client_auth());
    };
    Ok(builder.with_client_auth_cert(certs, key)?)
}

fn load_reqwest_client_identity(
    client_cert_file: Option<&str>,
    client_key_file: Option<&str>,
) -> Result<Option<reqwest::Identity>, Box<dyn std::error::Error>> {
    let Some(pem) = load_client_identity_pem(client_cert_file, client_key_file)? else {
        return Ok(None);
    };
    Ok(Some(reqwest::Identity::from_pem(&pem)?))
}

fn load_client_identity_pem(
    client_cert_file: Option<&str>,
    client_key_file: Option<&str>,
) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
    let Some(cert_file) = non_empty_path(client_cert_file) else {
        if non_empty_path(client_key_file).is_some() {
            return Err("client key file requires a client certificate file".into());
        }
        return Ok(None);
    };
    let mut pem = fs::read(cert_file)?;
    if let Some(key_file) =
        non_empty_path(client_key_file).filter(|key_file| *key_file != cert_file)
    {
        pem.push(b'\n');
        pem.extend_from_slice(&fs::read(key_file)?);
    }
    Ok(Some(pem))
}

fn load_rustls_client_identity(
    client_cert_file: Option<&str>,
    client_key_file: Option<&str>,
) -> Result<
    Option<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)>,
    Box<dyn std::error::Error>,
> {
    let Some(cert_file) = non_empty_path(client_cert_file) else {
        if non_empty_path(client_key_file).is_some() {
            return Err("client key file requires a client certificate file".into());
        }
        return Ok(None);
    };
    let certs = load_ca_bundle(cert_file)?;
    let key_file = non_empty_path(client_key_file).unwrap_or(cert_file);
    let key = PrivateKeyDer::from_pem_file(key_file)
        .map_err(|_| format!("client key file '{key_file}' does not contain a PEM private key"))?;
    Ok(Some((certs, key)))
}

fn non_empty_path(path: Option<&str>) -> Option<&str> {
    path.map(str::trim).filter(|path| !path.is_empty())
}

fn load_ca_bundle(
    path: &str,
) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, Box<dyn std::error::Error>> {
    let certs = CertificateDer::pem_file_iter(path)?.collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        return Err(format!("CA file '{path}' does not contain PEM certificates").into());
    }
    Ok(certs)
}

fn load_rustls_root_store(path: &str) -> Result<rustls::RootCertStore, Box<dyn std::error::Error>> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in load_ca_bundle(path)? {
        roots.add(cert)?;
    }
    Ok(roots)
}

fn auto_http3_failed_recently(origin: &Http3Origin) -> bool {
    let Ok(now_secs) = current_unix_secs() else {
        return false;
    };
    let path = auto_http3_failure_cache_path(origin);
    match read_auto_http3_failure_cache_secs(&path) {
        Ok(Some(failed_at_secs))
            if now_secs.saturating_sub(failed_at_secs) <= AUTO_HTTP3_FAILURE_CACHE_TTL_SECS =>
        {
            true
        }
        Ok(Some(_)) | Ok(None) => {
            let _ = fs::remove_file(path);
            false
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(_) => false,
    }
}

fn record_auto_http3_failure(origin: &Http3Origin) -> io::Result<()> {
    let now_secs = current_unix_secs().map_err(|error| io::Error::other(error.to_string()))?;
    write_auto_http3_failure_cache_secs(&auto_http3_failure_cache_path(origin), now_secs)
}

fn write_auto_http3_failure_cache_secs(path: &Path, now_secs: u64) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::Builder::new()
        .prefix("skron-http3-failed-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    write_decimal_u64_line(&mut temp, now_secs)?;
    temp.flush()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn read_auto_http3_failure_cache_secs(path: &Path) -> io::Result<Option<u64>> {
    let mut file = fs::File::open(path)?;
    let mut buf = [0_u8; AUTO_HTTP3_FAILURE_CACHE_TIMESTAMP_BUF_LEN];
    let len = file.read(&mut buf)?;
    if len == buf.len() {
        let mut extra = [0_u8; 1];
        if file.read(&mut extra)? != 0 {
            return Ok(None);
        }
    }
    Ok(parse_decimal_u64(trim_ascii_whitespace_bytes(&buf[..len])))
}

fn trim_ascii_whitespace_bytes(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(|byte| byte.is_ascii_whitespace()) {
        value = &value[1..];
    }
    while value.last().is_some_and(|byte| byte.is_ascii_whitespace()) {
        value = &value[..value.len() - 1];
    }
    value
}

fn write_decimal_u64_line<W: Write>(writer: &mut W, value: u64) -> io::Result<()> {
    write_decimal_u64(writer, value)?;
    writer.write_all(b"\n")
}

fn current_unix_secs() -> Result<u64, Box<dyn std::error::Error>> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

fn auto_http3_failure_cache_path(origin: &Http3Origin) -> PathBuf {
    std::env::temp_dir().join(auto_http3_failure_cache_file_name(origin))
}

fn auto_http3_failure_cache_file_name(origin: &Http3Origin) -> String {
    const PREFIX: &str = "skron-http3-failed-";
    const SUFFIX: &str = ".cache";
    let mut out = String::with_capacity(
        PREFIX.len() + origin.host.len().saturating_mul(2) + 1 + 5 + SUFFIX.len(),
    );
    out.push_str(PREFIX);
    push_hex_ascii(&mut out, origin.host.as_bytes());
    out.push('-');
    push_decimal_u16(&mut out, origin.port);
    out.push_str(SUFFIX);
    out
}

fn push_hex_ascii(out: &mut String, bytes: &[u8]) {
    for byte in bytes {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
}

fn push_decimal_u16(out: &mut String, value: u16) {
    let mut buf = [0_u8; 5];
    let mut cursor = buf.len();
    let mut value = value;
    loop {
        cursor -= 1;
        buf[cursor] = b'0' + u8::try_from(value % 10).expect("decimal digit");
        value /= 10;
        if value == 0 {
            break;
        }
    }
    out.push_str(std::str::from_utf8(&buf[cursor..]).expect("decimal digits are utf-8"));
}

fn write_response(response: TransportResponse) -> io::Result<()> {
    let stdout = io::stdout();
    let mut stdout = io::BufWriter::with_capacity(STDIO_BUFFER_SIZE, stdout.lock());
    write_response_to_writer(&mut stdout, response)?;
    stdout.flush()
}

fn write_response_to_writer<W: Write>(
    writer: &mut W,
    response: TransportResponse,
) -> io::Result<()> {
    writer.write_all(b"HTTP/")?;
    writer.write_all(response.version.as_bytes())?;
    writer.write_all(b" ")?;
    writer.write_all(response.status.as_str().as_bytes())?;
    writer.write_all(b"\n")?;
    for (name, value) in response.headers {
        writer.write_all(name.as_bytes())?;
        writer.write_all(b": ")?;
        writer.write_all(value.as_bytes())?;
        writer.write_all(b"\n")?;
    }
    match response.body {
        ResponseBody::Memory(body) => {
            writer.write_all(b"\n")?;
            writer.write_all(&body)?;
        }
        ResponseBody::File { path, len } => {
            writer.write_all(b"Body-File: ")?;
            write_path_bytes(writer, &path)?;
            writer.write_all(b"\nContent-Length: ")?;
            write_decimal_u64(writer, len)?;
            writer.write_all(b"\n\n")?;
        }
    }
    Ok(())
}

fn write_response_frame<W: Write>(writer: &mut W, response: TransportResponse) -> io::Result<()> {
    writer.write_all(b"RESPONSE\nVERSION ")?;
    writer.write_all(response.version.as_bytes())?;
    writer.write_all(b"\nSTATUS ")?;
    writer.write_all(response.status.as_str().as_bytes())?;
    writer.write_all(b"\n")?;
    for (name, value) in response.headers {
        writer.write_all(b"HEADER ")?;
        writer.write_all(name.as_bytes())?;
        writer.write_all(b": ")?;
        writer.write_all(value.as_bytes())?;
        writer.write_all(b"\n")?;
    }
    match response.body {
        ResponseBody::Memory(body) => {
            writer.write_all(b"CONTENT-LENGTH ")?;
            write_decimal_u64(
                writer,
                u64::try_from(body.len()).expect("memory body length fits u64"),
            )?;
            writer.write_all(b"\n\n")?;
            writer.write_all(&body)?;
            writer.write_all(b"\n")?;
        }
        ResponseBody::File { path, len } => {
            writer.write_all(b"BODY-FILE ")?;
            write_path_bytes(writer, &path)?;
            writer.write_all(b"\nCONTENT-LENGTH ")?;
            write_decimal_u64(writer, len)?;
            writer.write_all(b"\n\n")?;
        }
    }
    Ok(())
}

fn write_path_bytes<W: Write>(writer: &mut W, path: &Path) -> io::Result<()> {
    writer.write_all(path.as_os_str().as_encoded_bytes())
}

trait VersionText {
    fn version_text(&self) -> &'static str;
}

impl VersionText for reqwest::blocking::Response {
    fn version_text(&self) -> &'static str {
        match self.version() {
            reqwest::Version::HTTP_09 => "0.9",
            reqwest::Version::HTTP_10 => "1.0",
            reqwest::Version::HTTP_11 => "1.1",
            reqwest::Version::HTTP_2 => "2",
            reqwest::Version::HTTP_3 => "3",
            _ => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_plain_http1_response<R: BufRead>(
        reader: &mut R,
        output_file: Option<&Path>,
        line: &mut String,
        buffer: &mut [u8; STREAM_BUFFER_SIZE],
    ) -> Result<TransportResponse, Box<dyn std::error::Error>> {
        read_plain_http1_response_with_body_policy(reader, false, output_file, false, line, buffer)
    }

    #[test]
    fn inline_batch_request_body_spills_after_threshold() {
        let len = AUTO_RESPONSE_FILE_THRESHOLD as usize + 1;
        let mut reader = io::repeat(0x41).take(len as u64);
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let body =
            read_content_length_request_body_with_buffer(&mut reader, len, &mut buffer).unwrap();
        match body {
            RequestBody::TempFile { len: stored, .. } => assert_eq!(stored, len as u64),
            _ => panic!("large inline request body should spill to a temp file"),
        }
    }

    #[test]
    fn small_inline_batch_request_body_stays_in_memory() {
        let mut reader = io::Cursor::new(b"skron".to_vec());
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let body =
            read_content_length_request_body_with_buffer(&mut reader, 5, &mut buffer).unwrap();
        match body {
            RequestBody::Memory(body) => assert_eq!(body.as_ref(), b"skron"),
            _ => panic!("small inline request body should stay in memory"),
        }
    }

    #[test]
    fn batch_request_parses_headers_once() {
        let mut reader = io::Cursor::new(
            b"REQUEST\nMETHOD GET\nURL http://example.test/repo.git/info/refs\nHEADER Accept: application/x-git-upload-pack-advertisement\n\n"
                .to_vec(),
        );
        let mut line = String::new();
        let mut body_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let request = read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer)
            .unwrap()
            .expect("request");

        assert_eq!(
            request.headers,
            vec![
                parse_request_header("Accept: application/x-git-upload-pack-advertisement")
                    .unwrap()
            ]
        );
        assert_eq!(request.headers.capacity(), BATCH_REQUEST_HEADER_CAPACITY);
    }

    #[test]
    fn batch_request_without_header_lines_keeps_header_vec_unallocated() {
        let mut reader =
            io::Cursor::new(b"REQUEST\nMETHOD GET\nURL http://example.test/repo.git/info/refs\n\n");
        let mut line = String::new();
        let mut body_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let request = read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer)
            .unwrap()
            .expect("request");

        assert!(request.headers.is_empty());
        assert_eq!(request.headers.capacity(), 0);
    }

    #[test]
    fn request_headers_initial_capacity_is_bounded() {
        assert_eq!(request_headers_initial_capacity(0), 0);
        assert_eq!(request_headers_initial_capacity(2), 2);
        assert_eq!(
            request_headers_initial_capacity(usize::MAX),
            BATCH_REQUEST_HEADER_LIMIT
        );
    }

    #[test]
    fn http_batch_state_uses_origin_and_connection_capacity_hints() {
        let auto = AutoHttp3BatchState::new(None, None, None, false);
        assert!(auto.stream_buffer.is_none());
        assert!(auto.connections.is_none());
        assert!(auto.origin_cache.is_none());
        assert!(auto.failed_origins.is_none());
        assert!(auto.h2_failed_origins.is_none());
        assert!(auto.identities.proxy_identity.is_none());
        assert!(auto.identities.tls_verification_identity.is_none());

        let plain = PlainHttp1Pool::new(
            Arc::from(NONE_IDENTITY),
            Arc::from(NONE_IDENTITY),
            None,
            None,
            None,
            false,
        );
        assert!(plain.connections.capacity() >= HTTP_CONNECTION_POOL_CAPACITY_HINT);

        let http3 = http3_connection_pool();
        assert!(http3.capacity() >= HTTP_CONNECTION_POOL_CAPACITY_HINT);
    }

    #[test]
    fn auto_http3_batch_state_allocates_stream_buffer_lazily() {
        let mut auto = AutoHttp3BatchState::new(None, None, None, false);

        assert!(auto.stream_buffer.is_none());

        let buffer = auto.stream_buffer.get_or_insert_with(http3_stream_buffer);

        assert_eq!(buffer.capacity(), STREAM_BUFFER_SIZE);
    }

    #[test]
    fn auto_http3_batch_state_allocates_connection_pool_lazily() {
        let mut auto = AutoHttp3BatchState::new(None, None, None, false);

        assert!(auto.connections.is_none());

        let connections = auto.connections.get_or_insert_with(http3_connection_pool);

        assert!(connections.capacity() >= HTTP_CONNECTION_POOL_CAPACITY_HINT);
    }

    #[test]
    fn batch_plain_http1_pool_allocates_lazily() {
        let mut pool = None;
        let mut identities = None;

        assert!(pool.is_none());
        assert!(identities.is_none());

        let plain = batch_plain_http1_pool(&mut pool, &mut identities, None, None, None, false);

        assert!(plain.connections.capacity() >= HTTP_CONNECTION_POOL_CAPACITY_HINT);
        assert!(pool.is_some());
        let identities = identities.expect("identities");
        assert!(identities.proxy_identity.is_some());
        assert!(identities.tls_verification_identity.is_some());
    }

    #[test]
    fn batch_plain_http1_pool_does_not_reclone_identities_after_init() {
        let mut pool = None;
        let mut identities = None;

        let plain = batch_plain_http1_pool(&mut pool, &mut identities, None, None, None, false);
        let proxy_identity = plain.proxy_identity.clone();
        let tls_identity = plain.tls_verification_identity.clone();
        let proxy_count = Arc::strong_count(&proxy_identity);
        let tls_count = Arc::strong_count(&tls_identity);

        let plain = batch_plain_http1_pool(&mut pool, &mut identities, None, None, None, false);

        assert!(Arc::ptr_eq(&plain.proxy_identity, &proxy_identity));
        assert!(Arc::ptr_eq(&plain.tls_verification_identity, &tls_identity));
        assert_eq!(Arc::strong_count(&proxy_identity), proxy_count);
        assert_eq!(Arc::strong_count(&tls_identity), tls_count);
    }

    #[test]
    fn auto_http3_origin_state_allocates_lazily() {
        let mut failed = None;
        let mut cache = None;
        let origin = Http3Origin {
            scheme: "https".to_owned(),
            host: "lazy-origin-state.example.test".to_owned(),
            port: 443,
        };

        assert!(!http3_origin_set_contains(&failed, &origin));
        assert_eq!(http3_origin_cache_get(&cache, &origin), None);

        insert_bounded_http3_origin_set(&mut failed, origin.clone());
        insert_bounded_http3_origin_cache(&mut cache, origin.clone(), true);

        assert!(http3_origin_set_contains(&failed, &origin));
        assert_eq!(http3_origin_cache_get(&cache, &origin), Some(true));
        assert!(
            failed.as_ref().expect("failed origins").capacity() >= HTTP_ORIGIN_CACHE_CAPACITY_HINT
        );
        assert!(
            cache.as_ref().expect("origin cache").capacity() >= HTTP_ORIGIN_CACHE_CAPACITY_HINT
        );
    }

    #[test]
    fn auto_http3_origin_memory_is_bounded() {
        let latest = Http3Origin {
            scheme: "https".to_owned(),
            host: format!("origin-{HTTP_ORIGIN_MEMORY_ENTRY_LIMIT}.example.test"),
            port: 443,
        };
        let mut failed = None;
        let mut cache = None;

        for index in 0..=HTTP_ORIGIN_MEMORY_ENTRY_LIMIT {
            let origin = Http3Origin {
                scheme: "https".to_owned(),
                host: format!("origin-{index}.example.test"),
                port: 443,
            };
            insert_bounded_http3_origin_set(&mut failed, origin.clone());
            insert_bounded_http3_origin_cache(&mut cache, origin, true);
        }

        assert!(failed.as_ref().expect("failed origins").len() <= HTTP_ORIGIN_MEMORY_ENTRY_LIMIT);
        assert!(cache.as_ref().expect("origin cache").len() <= HTTP_ORIGIN_MEMORY_ENTRY_LIMIT);
        assert!(http3_origin_set_contains(&failed, &latest));
        assert_eq!(http3_origin_cache_get(&cache, &latest), Some(true));
    }

    #[test]
    fn auto_http3_origin_set_update_does_not_evict_when_full() {
        let existing = Http3Origin {
            scheme: "https".to_owned(),
            host: "existing-origin.example.test".to_owned(),
            port: 443,
        };
        let mut failed = None;
        insert_bounded_http3_origin_set(&mut failed, existing.clone());
        for index in 1..HTTP_ORIGIN_MEMORY_ENTRY_LIMIT {
            insert_bounded_http3_origin_set(
                &mut failed,
                Http3Origin {
                    scheme: "https".to_owned(),
                    host: format!("origin-{index}.example.test"),
                    port: 443,
                },
            );
        }
        let len = failed.as_ref().expect("failed origins").len();

        insert_bounded_http3_origin_set(&mut failed, existing.clone());

        assert_eq!(failed.as_ref().expect("failed origins").len(), len);
        assert!(http3_origin_set_contains(&failed, &existing));
    }

    #[test]
    fn auto_http3_origin_cache_update_does_not_evict_when_full() {
        let existing = Http3Origin {
            scheme: "https".to_owned(),
            host: "existing-origin.example.test".to_owned(),
            port: 443,
        };
        let mut cache = None;
        insert_bounded_http3_origin_cache(&mut cache, existing.clone(), true);
        for index in 1..HTTP_ORIGIN_MEMORY_ENTRY_LIMIT {
            insert_bounded_http3_origin_cache(
                &mut cache,
                Http3Origin {
                    scheme: "https".to_owned(),
                    host: format!("origin-{index}.example.test"),
                    port: 443,
                },
                true,
            );
        }
        let len = cache.as_ref().expect("origin cache").len();

        insert_bounded_http3_origin_cache(&mut cache, existing.clone(), false);

        assert_eq!(cache.as_ref().expect("origin cache").len(), len);
        assert_eq!(http3_origin_cache_get(&cache, &existing), Some(false));
    }

    #[test]
    fn batch_request_reader_reuses_line_buffer() {
        let mut reader = io::Cursor::new(
            b"REQUEST\nMETHOD GET\nURL http://example.test/one\n\nREQUEST\nMETHOD POST\nURL http://example.test/two\nCONTENT-LENGTH 4\n\nbodyDONE\n"
                .to_vec(),
        );
        let mut line = String::with_capacity(128);
        let mut body_buffer = [0_u8; STREAM_BUFFER_SIZE];
        let capacity = line.capacity();

        let first = read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer)
            .unwrap()
            .expect("first request");
        let second = read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer)
            .unwrap()
            .expect("second request");
        let done = read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer).unwrap();

        assert_eq!(first.method, http::Method::GET);
        assert_eq!(first.url, "http://example.test/one");
        assert_eq!(second.method, http::Method::POST);
        assert_eq!(second.url, "http://example.test/two");
        assert!(matches!(second.body, RequestBody::Memory(ref body) if body.as_ref() == b"body"));
        assert!(done.is_none());
        assert_eq!(line.capacity(), capacity);
    }

    #[test]
    fn batch_request_reader_trims_oversized_line_buffer_after_request() {
        let mut bytes =
            b"REQUEST\nMETHOD GET\nURL http://example.test/one\nHEADER X-Fill: ".to_vec();
        bytes.extend(std::iter::repeat(b'a').take(HTTP_LINE_BUFFER_RETAIN_CAPACITY_LIMIT + 1));
        bytes.extend_from_slice(b"\n\n");
        let mut reader = io::Cursor::new(bytes);
        let mut line = String::with_capacity(HTTP_LINE_BUFFER_CAPACITY);
        let mut body_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let request = read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer)
            .unwrap()
            .expect("request");

        assert_eq!(request.headers.len(), 1);
        assert!(line.is_empty());
        assert_eq!(line.capacity(), HTTP_LINE_BUFFER_CAPACITY);
    }

    #[test]
    fn batch_request_reader_rejects_unbounded_lines() {
        let mut reader = io::Cursor::new({
            let mut bytes =
                b"REQUEST\nMETHOD GET\nURL http://example.test/repo.git/info/refs\nHEADER "
                    .to_vec();
            bytes.resize(bytes.len() + BATCH_REQUEST_LINE_LIMIT + 1, b'a');
            bytes.extend_from_slice(b"\n\n");
            bytes
        });
        let mut line = String::with_capacity(HTTP_LINE_BUFFER_CAPACITY);
        let mut body_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = match read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer) {
            Ok(_) => panic!("oversized batch header line should fail"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "batch request line too long");
    }

    #[test]
    fn batch_line_buffer_uses_http_line_capacity() {
        assert_eq!(batch_line_buffer().capacity(), HTTP_LINE_BUFFER_CAPACITY);
    }

    #[test]
    fn limited_line_reads_into_reusable_string_buffer() {
        let mut reader = io::Cursor::new(b"REQUEST\n");
        let mut line = String::with_capacity(HTTP_LINE_BUFFER_CAPACITY);
        let capacity = line.capacity();

        let len = read_limited_line(&mut reader, &mut line, BATCH_REQUEST_LINE_LIMIT, "too long")
            .unwrap();

        assert_eq!(len, "REQUEST\n".len());
        assert_eq!(line, "REQUEST\n");
        assert_eq!(line.capacity(), capacity);
    }

    #[test]
    fn limited_line_clears_string_after_invalid_utf8() {
        let mut reader = io::Cursor::new([0xff, b'\n']);
        let mut line = String::with_capacity(HTTP_LINE_BUFFER_CAPACITY);

        let error = read_limited_line(&mut reader, &mut line, BATCH_REQUEST_LINE_LIMIT, "too long")
            .expect_err("invalid utf8");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "HTTP response line is not UTF-8");
        assert!(line.is_empty());
    }

    #[test]
    fn reusable_http_line_trim_keeps_small_buffer() {
        let mut line = String::with_capacity(HTTP_LINE_BUFFER_CAPACITY);
        line.push_str("HTTP/1.1 200 OK\r\n");

        trim_reusable_http_line_buffer(&mut line);

        assert!(line.is_empty());
        assert_eq!(line.capacity(), HTTP_LINE_BUFFER_CAPACITY);
    }

    #[test]
    fn reusable_http_line_trim_drops_oversized_buffer() {
        let mut line = String::with_capacity(HTTP_LINE_BUFFER_RETAIN_CAPACITY_LIMIT + 1);
        line.push_str("HTTP/1.1 200 OK ");
        line.extend(std::iter::repeat('a').take(HTTP_LINE_BUFFER_RETAIN_CAPACITY_LIMIT));

        trim_reusable_http_line_buffer(&mut line);

        assert!(line.is_empty());
        assert_eq!(line.capacity(), HTTP_LINE_BUFFER_CAPACITY);
    }

    #[test]
    fn batch_request_reader_spools_large_inline_body_with_reusable_buffer() {
        let len = AUTO_RESPONSE_FILE_THRESHOLD as usize + 1;
        let head = format!(
            "REQUEST\nMETHOD POST\nURL http://example.test/repo.git/git-upload-pack\nCONTENT-LENGTH {len}\n\n"
        );
        let mut reader = io::BufReader::new(std::io::Read::chain(
            head.as_bytes(),
            io::repeat(0x41).take(len as u64),
        ));
        let mut line = String::new();
        let mut body_buffer = [0x55_u8; STREAM_BUFFER_SIZE];

        let request = read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer)
            .unwrap()
            .expect("request");

        assert!(
            matches!(request.body, RequestBody::TempFile { len: stored, .. } if stored == len as u64)
        );
    }

    #[test]
    fn batch_request_rejects_missing_method_before_reading_inline_body() {
        let frame =
            b"REQUEST\nURL http://example.test/repo.git/git-upload-pack\nCONTENT-LENGTH 4\n\nbody";
        let mut reader = io::Cursor::new(frame.to_vec());
        let mut line = String::new();
        let mut body_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = match read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer) {
            Ok(_) => panic!("missing method should fail"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "batch request is missing method");
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"body");
    }

    #[test]
    fn batch_request_rejects_duplicate_singleton_before_reading_body() {
        let frame =
            b"REQUEST\nMETHOD POST\nMETHOD GET\nURL http://example.test/repo.git/git-upload-pack\nCONTENT-LENGTH 4\n\nbody";
        let mut reader = io::Cursor::new(frame.to_vec());
        let mut line = String::new();
        let mut body_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = match read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer) {
            Ok(_) => panic!("duplicate singleton should fail"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "duplicate batch request header: METHOD");
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        assert_eq!(
            rest,
            b"URL http://example.test/repo.git/git-upload-pack\nCONTENT-LENGTH 4\n\nbody"
        );
    }

    #[test]
    fn batch_request_rejects_duplicate_body_file_without_opening_second_file() {
        let mut first = tempfile::NamedTempFile::new().unwrap();
        let mut second = tempfile::NamedTempFile::new().unwrap();
        first.write_all(b"first").unwrap();
        second.write_all(b"second").unwrap();
        let frame = format!(
            "REQUEST\nMETHOD POST\nURL http://example.test/repo.git/git-upload-pack\nBODY-FILE {}\nBODY-FILE {}\n\n",
            first.path().display(),
            second.path().display()
        );
        let mut reader = io::Cursor::new(frame.into_bytes());
        let mut line = String::new();
        let mut body_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = match read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer) {
            Ok(_) => panic!("duplicate body file should fail"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "duplicate batch request header: BODY-FILE"
        );
    }

    #[test]
    fn batch_request_rejects_ambiguous_body_file_with_content_length() {
        let mut body_file = tempfile::NamedTempFile::new().unwrap();
        body_file.write_all(b"body").unwrap();
        let frame = format!(
            "REQUEST\nMETHOD POST\nURL http://example.test/repo.git/git-upload-pack\nBODY-FILE {}\nCONTENT-LENGTH 4\n\n",
            body_file.path().display()
        );
        let mut reader = io::Cursor::new(frame.into_bytes());
        let mut line = String::new();
        let mut body_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = match read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer) {
            Ok(_) => panic!("ambiguous body mode should fail"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "batch request cannot combine BODY-FILE and CONTENT-LENGTH"
        );
    }

    #[test]
    fn request_body_send_validation_rejects_changed_file_length() {
        let mut body_file = tempfile::NamedTempFile::new().unwrap();
        body_file.write_all(b"body").unwrap();
        let file = fs::File::open(body_file.path()).unwrap();
        body_file.write_all(b"-changed").unwrap();

        let error = validate_request_body_for_send(&RequestBody::File { file, len: 4 })
            .expect_err("changed file length");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "request body file length changed: expected 4, got 12"
        );
    }

    #[test]
    fn request_body_send_validation_rejects_changed_chain_file_length() {
        let mut body_file = tempfile::NamedTempFile::new().unwrap();
        body_file.write_all(b"body").unwrap();
        let file = fs::File::open(body_file.path()).unwrap();
        body_file.write_all(b"-changed").unwrap();

        let error = validate_request_body_for_send(&RequestBody::Chain {
            prefix: Bytes::from_static(b"prefix"),
            file,
            file_len: 4,
        })
        .expect_err("changed chained file length");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "request body file length changed: expected 4, got 12"
        );
    }

    #[test]
    fn batch_request_rejects_oversized_body_prefix_before_reading_prefix() {
        let mut body_file = tempfile::NamedTempFile::new().unwrap();
        body_file.write_all(b"body").unwrap();
        let prefix_len = AUTO_RESPONSE_FILE_THRESHOLD + 1;
        let frame = format!(
            "REQUEST\nMETHOD POST\nURL http://example.test/repo.git/git-upload-pack\nBODY-FILE {}\nBODY-PREFIX-LENGTH {prefix_len}\n\n",
            body_file.path().display()
        );
        let mut reader = io::Cursor::new(frame.into_bytes());
        let mut line = String::new();
        let mut body_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = match read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer) {
            Ok(_) => panic!("oversized body prefix should fail"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "batch body prefix length too large");
    }

    #[test]
    fn batch_request_rejects_body_prefix_without_body_file() {
        let frame = b"REQUEST\nMETHOD POST\nURL http://example.test/repo.git/git-upload-pack\nBODY-PREFIX-LENGTH 4\n\nbody";
        let mut reader = io::Cursor::new(frame.to_vec());
        let mut line = String::new();
        let mut body_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = match read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer) {
            Ok(_) => panic!("body prefix without body file should fail"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "batch request cannot use BODY-PREFIX-LENGTH without BODY-FILE"
        );
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"body");
    }

    #[test]
    fn batch_request_reads_body_prefix_with_reusable_buffer() {
        let mut body_file = tempfile::NamedTempFile::new().unwrap();
        body_file.write_all(b"file-body").unwrap();
        let frame = format!(
            "REQUEST\nMETHOD POST\nURL http://example.test/repo.git/git-upload-pack\nBODY-FILE {}\nBODY-PREFIX-LENGTH 7\n\nprefix-DONE\n",
            body_file.path().display()
        );
        let mut reader = io::Cursor::new(frame.into_bytes());
        let mut line = String::new();
        let mut body_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let request = read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer)
            .unwrap()
            .expect("request");

        match request.body {
            RequestBody::Chain {
                prefix, file_len, ..
            } => {
                assert_eq!(prefix.as_ref(), b"prefix-");
                assert_eq!(file_len, 9);
            }
            _ => panic!("body prefix with file should create a chained request body"),
        }
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"DONE\n");
    }

    #[test]
    fn malformed_batch_header_is_rejected_while_reading_request() {
        let mut reader = io::Cursor::new(
            b"REQUEST\nMETHOD GET\nURL http://example.test/repo.git/info/refs\nHEADER Accept application/x-git-upload-pack-advertisement\n\n"
                .to_vec(),
        );
        let mut line = String::new();
        let mut body_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = match read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer) {
            Ok(_) => panic!("malformed header should fail"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "malformed header 'Accept application/x-git-upload-pack-advertisement'"
        );
    }

    #[test]
    fn batch_request_reader_rejects_too_many_headers() {
        let mut bytes =
            b"REQUEST\nMETHOD GET\nURL http://example.test/repo.git/info/refs\n".to_vec();
        for index in 0..=BATCH_REQUEST_HEADER_LIMIT {
            bytes.extend_from_slice(format!("HEADER X-Skron-{index}: value\n").as_bytes());
        }
        bytes.extend_from_slice(b"\n");
        let mut reader = io::Cursor::new(bytes);
        let mut line = String::new();
        let mut body_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = match read_batch_request_with_line(&mut reader, &mut line, &mut body_buffer) {
            Ok(_) => panic!("too many batch headers should fail"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "too many batch request headers");
    }

    #[test]
    fn exact_len_vec_read_does_not_consume_following_bytes() {
        let mut reader = io::Cursor::new(b"skron-next".to_vec());
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let body = read_exact_len_to_vec_with_buffer(&mut reader, 5, &mut buffer).unwrap();

        assert_eq!(body, b"skron");
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"-next");
    }

    #[test]
    fn exact_len_vec_read_handles_medium_body_without_staging_buffer() {
        let mut input = vec![0x41; STREAM_BUFFER_SIZE + 17];
        input.extend_from_slice(b"-next");
        let mut reader = io::Cursor::new(input);
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let body =
            read_exact_len_to_vec_with_buffer(&mut reader, STREAM_BUFFER_SIZE + 17, &mut buffer)
                .unwrap();

        assert_eq!(body.len(), STREAM_BUFFER_SIZE + 17);
        assert!(body.capacity() >= STREAM_BUFFER_SIZE + 17);
        assert!(body.iter().all(|byte| *byte == 0x41));
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"-next");
    }

    #[test]
    fn exact_len_vec_read_with_buffer_uses_caller_buffer() {
        let mut input = vec![0x41; STREAM_BUFFER_SIZE + 17];
        input.extend_from_slice(b"-next");
        let mut reader = io::Cursor::new(input);
        let mut buffer = [0x55_u8; STREAM_BUFFER_SIZE];

        let body =
            read_exact_len_to_vec_with_buffer(&mut reader, STREAM_BUFFER_SIZE + 17, &mut buffer)
                .unwrap();

        assert_eq!(body.len(), STREAM_BUFFER_SIZE + 17);
        assert!(body.iter().all(|byte| *byte == 0x41));
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"-next");
    }

    #[test]
    fn exact_len_vec_read_with_buffer_reports_early_eof() {
        let mut reader = io::Cursor::new(b"skr".to_vec());
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error =
            read_exact_len_to_vec_with_buffer(&mut reader, 5, &mut buffer).expect_err("early eof");

        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(error.to_string(), "HTTP response ended early");
    }

    #[test]
    fn exact_len_vec_initial_capacity_is_bounded() {
        assert_eq!(exact_len_vec_initial_capacity(0), 0);
        assert_eq!(exact_len_vec_initial_capacity(17), 17);
        assert_eq!(
            exact_len_vec_initial_capacity(STREAM_BUFFER_SIZE + 17),
            STREAM_BUFFER_SIZE
        );
        assert_eq!(
            exact_len_vec_initial_capacity(usize::MAX),
            STREAM_BUFFER_SIZE
        );
    }

    #[test]
    fn exact_len_bytes_read_freezes_prefix_without_consuming_following_bytes() {
        let mut reader = io::Cursor::new(b"prefix-next".to_vec());
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let body = read_exact_len_to_bytes_with_buffer(&mut reader, 6, &mut buffer).unwrap();

        assert_eq!(body.as_ref(), b"prefix");
        assert_eq!(body.len(), 6);
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"-next");
    }

    #[test]
    fn response_writer_streams_status_headers_and_memory_body() {
        let response = TransportResponse {
            version: "2",
            status: "200 OK".into(),
            headers: vec![(
                "content-type".to_owned(),
                "application/octet-stream".to_owned(),
            )],
            reusable_connection: true,
            body: ResponseBody::Memory(b"body".to_vec()),
        };
        let mut out = Vec::new();

        write_response_to_writer(&mut out, response).expect("write response");

        assert_eq!(
            out,
            b"HTTP/2 200 OK\ncontent-type: application/octet-stream\n\nbody"
        );
    }

    #[test]
    fn response_writer_streams_file_body_metadata_without_body() {
        let response = TransportResponse {
            version: "1.1",
            status: "200 OK".into(),
            headers: Vec::new(),
            reusable_connection: false,
            body: ResponseBody::File {
                path: PathBuf::from("body.pack"),
                len: 12_345,
            },
        };
        let mut out = Vec::new();

        write_response_to_writer(&mut out, response).expect("write response");

        assert_eq!(
            out,
            b"HTTP/1.1 200 OK\nBody-File: body.pack\nContent-Length: 12345\n\n"
        );
    }

    #[test]
    fn response_path_writer_uses_encoded_path_bytes() {
        let path = Path::new("body with spaces.pack");
        let mut out = Vec::new();

        write_path_bytes(&mut out, path).expect("write path");

        assert_eq!(out, b"body with spaces.pack");
    }

    #[test]
    fn plain_http1_output_file_keeps_existing_file_after_truncated_body() {
        let dir = tempfile::tempdir().expect("tempdir");
        let output = dir.path().join("pack.out");
        fs::write(&output, b"existing").expect("write existing output");
        let mut reader = io::BufReader::new(io::Cursor::new(
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nab".to_vec(),
        ));
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error =
            match read_plain_http1_response(&mut reader, Some(&output), &mut line, &mut buffer) {
                Ok(_) => panic!("truncated response should fail"),
                Err(error) => error,
            };

        assert_eq!(error.to_string(), "HTTP response ended early");
        assert_eq!(
            fs::read(&output).expect("read existing output"),
            b"existing"
        );
    }

    #[test]
    fn plain_http1_output_file_replaces_existing_file_after_complete_body() {
        let dir = tempfile::tempdir().expect("tempdir");
        let output = dir.path().join("pack.out");
        fs::write(&output, b"existing").expect("write existing output");
        let mut reader = io::BufReader::new(io::Cursor::new(
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\npack".to_vec(),
        ));
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let response =
            read_plain_http1_response(&mut reader, Some(&output), &mut line, &mut buffer)
                .expect("complete response");

        assert!(matches!(
            response.body,
            ResponseBody::File { path, len } if path == output && len == 4
        ));
        assert_eq!(fs::read(&output).expect("read replaced output"), b"pack");
    }

    #[test]
    fn response_header_collection_is_bounded() {
        let mut headers = http::HeaderMap::new();
        for index in 0..(COLLECTED_RESPONSE_HEADER_LIMIT + 8) {
            let name = http::HeaderName::from_bytes(format!("x-skron-{index}").as_bytes()).unwrap();
            headers.insert(name, "value".parse().unwrap());
        }

        let collected = collect_http_response_headers(&headers);

        assert_eq!(collected.len(), COLLECTED_RESPONSE_HEADER_LIMIT);
    }

    #[test]
    fn redirect_response_header_collection_keeps_only_location() {
        let mut headers = http::HeaderMap::new();
        headers.insert(http::header::LOCATION, "/repo.git".parse().unwrap());
        headers.insert(http::header::CONTENT_TYPE, "text/plain".parse().unwrap());

        let collected = collect_redirect_response_headers(302, &headers);

        assert_eq!(
            collected,
            vec![("location".to_owned(), "/repo.git".to_owned())]
        );
        assert!(collect_redirect_response_headers(200, &headers).is_empty());
    }

    #[test]
    fn batch_response_frame_writes_response_headers() {
        let response = TransportResponse {
            version: "2",
            status: "200 OK".into(),
            headers: vec![(
                "location".to_owned(),
                "https://example.test/repo.git".to_owned(),
            )],
            reusable_connection: true,
            body: ResponseBody::Memory(b"body".to_vec()),
        };
        let mut frame = Vec::new();

        write_response_frame(&mut frame, response).unwrap();

        assert_eq!(
            frame,
            b"RESPONSE\nVERSION 2\nSTATUS 200 OK\nHEADER location: https://example.test/repo.git\nCONTENT-LENGTH 4\n\nbody\n"
        );
    }

    #[test]
    fn batch_response_frame_writes_file_body_metadata() {
        let response = TransportResponse {
            version: "3",
            status: "200 OK".into(),
            headers: Vec::new(),
            reusable_connection: true,
            body: ResponseBody::File {
                path: PathBuf::from("body.pack"),
                len: 9,
            },
        };
        let mut frame = Vec::new();

        write_response_frame(&mut frame, response).unwrap();

        assert_eq!(
            frame,
            b"RESPONSE\nVERSION 3\nSTATUS 200 OK\nBODY-FILE body.pack\nCONTENT-LENGTH 9\n\n"
        );
    }

    #[test]
    fn large_known_http_response_length_spills_directly_to_file() {
        assert!(!response_content_length_spills_to_file(None));
        assert!(!response_content_length_spills_to_file(Some(0)));
        assert!(!response_content_length_spills_to_file(Some(
            AUTO_RESPONSE_FILE_THRESHOLD
        )));
        assert!(response_content_length_spills_to_file(Some(
            AUTO_RESPONSE_FILE_THRESHOLD + 1
        )));
    }

    #[test]
    fn memory_body_initial_capacity_matches_known_content_length() {
        assert_eq!(memory_body_initial_capacity(0).expect("capacity"), 0);
        assert_eq!(memory_body_initial_capacity(2).expect("capacity"), 2);
        assert_eq!(
            memory_body_initial_capacity(AUTO_RESPONSE_FILE_THRESHOLD).expect("capacity"),
            AUTO_RESPONSE_FILE_THRESHOLD as usize
        );
    }

    #[test]
    fn plain_http1_response_tracks_connection_reuse_without_storing_headers() {
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut keep_alive = io::Cursor::new(
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: keep-alive\r\n\r\nbody".to_vec(),
        );
        let response =
            read_plain_http1_response(&mut keep_alive, None, &mut line, &mut buffer).unwrap();
        assert!(response.headers.is_empty());
        assert!(response.reusable_connection);

        let mut close = io::Cursor::new(
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody".to_vec(),
        );
        let response = read_plain_http1_response(&mut close, None, &mut line, &mut buffer).unwrap();
        assert!(response.headers.is_empty());
        assert!(!response.reusable_connection);

        let mut http10_default_close =
            io::Cursor::new(b"HTTP/1.0 200 OK\r\nContent-Length: 4\r\n\r\nbody".to_vec());
        let response =
            read_plain_http1_response(&mut http10_default_close, None, &mut line, &mut buffer)
                .unwrap();
        assert_eq!(response.version, "1.0");
        assert!(!response.reusable_connection);

        let mut http10_keep_alive = io::Cursor::new(
            b"HTTP/1.0 200 OK\r\nContent-Length: 4\r\nConnection: keep-alive\r\n\r\nbody".to_vec(),
        );
        let response =
            read_plain_http1_response(&mut http10_keep_alive, None, &mut line, &mut buffer)
                .unwrap();
        assert_eq!(response.version, "1.0");
        assert!(response.reusable_connection);
    }

    #[test]
    fn connection_header_flags_scans_tokens_once() {
        assert_eq!(
            connection_header_flags("upgrade, keep-alive, close"),
            ConnectionHeaderFlags {
                close: true,
                keep_alive: true,
            }
        );
        assert_eq!(
            connection_header_flags("Upgrade"),
            ConnectionHeaderFlags {
                close: false,
                keep_alive: false,
            }
        );
    }

    #[test]
    fn plain_http1_response_keeps_redirect_location_header() {
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut response = io::Cursor::new(
            b"HTTP/1.1 302 Found\r\nLocation: /repo.git\r\nContent-Length: 0\r\n\r\n".to_vec(),
        );

        let response = read_plain_http1_response(&mut response, None, &mut line, &mut buffer)
            .expect("redirect response");

        assert_eq!(response.status, "302 Found");
        assert_eq!(
            response.headers,
            vec![("location".to_owned(), "/repo.git".to_owned())]
        );
        assert_eq!(
            response.headers.capacity(),
            REDIRECT_RESPONSE_HEADER_CAPACITY
        );
    }

    #[test]
    fn plain_http1_non_redirect_header_vec_stays_unallocated() {
        let headers = redirect_response_headers_vec(false);

        assert!(headers.is_empty());
        assert_eq!(headers.capacity(), 0);
    }

    #[test]
    fn plain_http1_response_reads_http10_close_delimited_body() {
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut response = io::Cursor::new(b"HTTP/1.0 200 OK\r\n\r\nbody".to_vec());

        let response = read_plain_http1_response(&mut response, None, &mut line, &mut buffer)
            .expect("close-delimited response");

        assert_eq!(response.version, "1.0");
        assert!(!response.reusable_connection);
        match response.body {
            ResponseBody::Memory(body) => assert_eq!(body, b"body"),
            ResponseBody::File { .. } => panic!("small body should stay in memory"),
        }
    }

    #[test]
    fn plain_http1_response_reads_connection_close_without_content_length() {
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut response =
            io::Cursor::new(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nbody".to_vec());

        let response = read_plain_http1_response(&mut response, None, &mut line, &mut buffer)
            .expect("connection-close response");

        assert_eq!(response.version, "1.1");
        assert!(!response.reusable_connection);
        match response.body {
            ResponseBody::Memory(body) => assert_eq!(body, b"body"),
            ResponseBody::File { .. } => panic!("small body should stay in memory"),
        }
    }

    #[test]
    fn plain_http1_response_rejects_keep_alive_without_content_length() {
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut response =
            io::Cursor::new(b"HTTP/1.1 200 OK\r\nConnection: keep-alive\r\n\r\nbody".to_vec());

        let error = match read_plain_http1_response(&mut response, None, &mut line, &mut buffer) {
            Ok(_) => panic!("keep-alive without length should fail"),
            Err(error) => error,
        };

        assert_eq!(
            error.to_string(),
            "HTTP/1 response is missing Content-Length"
        );
    }

    #[test]
    fn plain_http1_response_accepts_empty_body_status_without_content_length() {
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut response = io::Cursor::new(
            b"HTTP/1.1 304 Not Modified\r\nConnection: keep-alive\r\n\r\n".to_vec(),
        );

        let response = read_plain_http1_response(&mut response, None, &mut line, &mut buffer)
            .expect("empty-body status");

        assert_eq!(response.status, "304 Not Modified");
        assert!(response.reusable_connection);
        match response.body {
            ResponseBody::Memory(body) => assert!(body.is_empty()),
            ResponseBody::File { .. } => panic!("empty body should stay in memory"),
        }
    }

    #[test]
    fn plain_http1_response_skips_informational_responses() {
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut response = io::Cursor::new(
            b"HTTP/1.1 100 Continue\r\nX-Interim: ignored\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: keep-alive\r\n\r\nbody"
                .to_vec(),
        );

        let response = read_plain_http1_response(&mut response, None, &mut line, &mut buffer)
            .expect("final response after interim");

        assert_eq!(response.status, "200 OK");
        assert!(response.reusable_connection);
        match response.body {
            ResponseBody::Memory(body) => assert_eq!(body, b"body"),
            ResponseBody::File { .. } => panic!("small response should stay in memory"),
        }
    }

    #[test]
    fn plain_http1_response_ignores_informational_body_headers() {
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut response = io::Cursor::new(
            b"HTTP/1.1 100 Continue\r\nTransfer-Encoding: gzip\r\nContent-Length: 7\r\nContent-Length: 9\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbody"
                .to_vec(),
        );

        let response = read_plain_http1_response(&mut response, None, &mut line, &mut buffer)
            .expect("final response after noisy interim");

        assert_eq!(response.status, "200 OK");
        match response.body {
            ResponseBody::Memory(body) => assert_eq!(body, b"body"),
            ResponseBody::File { .. } => panic!("small response should stay in memory"),
        }
    }

    #[test]
    fn plain_http1_response_limits_informational_responses() {
        let mut bytes = Vec::new();
        for _ in 0..=HTTP_INFORMATIONAL_RESPONSE_LIMIT {
            bytes.extend_from_slice(b"HTTP/1.1 100 Continue\r\n\r\n");
        }
        bytes.extend_from_slice(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbody");
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut response = io::Cursor::new(bytes);

        let error = match read_plain_http1_response(&mut response, None, &mut line, &mut buffer) {
            Ok(_) => panic!("too many informational responses should fail"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "too many informational HTTP responses");
    }

    #[test]
    fn plain_http1_response_rejects_protocol_switch() {
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut response = io::Cursor::new(
            b"HTTP/1.1 101 Switching Protocols\r\nConnection: upgrade\r\nUpgrade: websocket\r\n\r\n"
                .to_vec(),
        );

        let error = match read_plain_http1_response(&mut response, None, &mut line, &mut buffer) {
            Ok(_) => panic!("protocol switch should fail"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "unsupported HTTP protocol switch");
    }

    #[test]
    fn plain_http1_response_ignores_content_length_for_empty_body_status() {
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut response = io::Cursor::new(
            b"HTTP/1.1 304 Not Modified\r\nContent-Length: 123\r\nConnection: keep-alive\r\n\r\n"
                .to_vec(),
        );

        let response = read_plain_http1_response(&mut response, None, &mut line, &mut buffer)
            .expect("empty-body status with content length");

        assert_eq!(response.status, "304 Not Modified");
        assert!(response.reusable_connection);
        match response.body {
            ResponseBody::Memory(body) => assert!(body.is_empty()),
            ResponseBody::File { .. } => panic!("empty body should stay in memory"),
        }
    }

    #[test]
    fn plain_http1_head_response_ignores_content_length_body() {
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut response =
            io::Cursor::new(b"HTTP/1.1 200 OK\r\nContent-Length: 123\r\n\r\n".to_vec());

        let response = read_plain_http1_response_with_body_policy(
            &mut response,
            true,
            None,
            false,
            &mut line,
            &mut buffer,
        )
        .expect("HEAD response");

        assert_eq!(response.status, "200 OK");
        assert!(response.reusable_connection);
        match response.body {
            ResponseBody::Memory(body) => assert!(body.is_empty()),
            ResponseBody::File { .. } => panic!("HEAD body should stay empty"),
        }
    }

    #[test]
    fn plain_http1_response_writes_empty_body_status_output_file() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let output = dir.path().join("empty.response");
        let mut line = String::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut response = io::Cursor::new(b"HTTP/1.1 204 No Content\r\n\r\n".to_vec());

        let response =
            read_plain_http1_response(&mut response, Some(&output), &mut line, &mut buffer)
                .expect("empty output response");

        assert_eq!(response.status, "204 No Content");
        assert!(output.exists());
        assert_eq!(fs::metadata(&output).expect("output metadata").len(), 0);
        match response.body {
            ResponseBody::File { path, len } => {
                assert_eq!(path, output);
                assert_eq!(len, 0);
            }
            ResponseBody::Memory(_) => panic!("output_file should return file body"),
        }
    }

    #[test]
    fn http_status_allows_empty_body_only_for_http_defined_statuses() {
        assert!(http_status_allows_empty_body("100 Continue"));
        assert!(http_status_allows_empty_body("204 No Content"));
        assert!(http_status_allows_empty_body("304 Not Modified"));
        assert!(http_status_code_allows_empty_body(102));
        assert!(http_status_code_allows_empty_body(204));
        assert!(http_status_code_allows_empty_body(304));
        assert!(!http_status_allows_empty_body("200 OK"));
        assert!(!http_status_allows_empty_body("20 OK"));
        assert!(!http_status_allows_empty_body("ABC Nope"));
        assert!(!http_status_code_allows_empty_body(200));
        assert!(request_method_allows_empty_response_body(
            &http::Method::HEAD
        ));
        assert!(!request_method_allows_empty_response_body(
            &http::Method::GET
        ));
    }

    #[test]
    fn response_status_text_uses_static_common_statuses() {
        assert!(matches!(
            response_status_text(http::StatusCode::OK),
            ResponseStatus::Static("200 OK")
        ));
        assert!(matches!(
            response_status_text(http::StatusCode::NOT_MODIFIED),
            ResponseStatus::Static("304 Not Modified")
        ));
        assert!(matches!(
            response_status_text_from_str("200 OK"),
            ResponseStatus::Static("200 OK")
        ));
        assert!(matches!(
            response_status_text_from_code_and_str(Some(200), "200 OK"),
            ResponseStatus::Static("200 OK")
        ));
        assert!(matches!(
            response_status_text_from_str("599 Custom"),
            ResponseStatus::Owned(status) if status == "599 Custom"
        ));
    }

    #[test]
    fn plain_http1_response_parser_reuses_line_buffer() {
        let mut line = String::with_capacity(128);
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let capacity = line.capacity();
        let mut response = io::Cursor::new(
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: keep-alive\r\n\r\nbody".to_vec(),
        );

        let parsed = read_plain_http1_response_with_body_policy(
            &mut response,
            false,
            None,
            false,
            &mut line,
            &mut buffer,
        )
        .unwrap();

        assert_eq!(parsed.status, "200 OK");
        assert!(matches!(&parsed.status, ResponseStatus::Static("200 OK")));
        assert_eq!(line.capacity(), capacity);
    }

    #[test]
    fn plain_http1_response_rejects_unbounded_status_lines() {
        let mut response = io::Cursor::new({
            let mut bytes = b"HTTP/1.1 ".to_vec();
            bytes.resize(bytes.len() + HTTP_RESPONSE_LINE_LIMIT, b'2');
            bytes.extend_from_slice(b"\r\nContent-Length: 0\r\n\r\n");
            bytes
        });
        let mut line = String::with_capacity(HTTP_LINE_BUFFER_CAPACITY);
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = match read_plain_http1_response(&mut response, None, &mut line, &mut buffer) {
            Ok(_) => panic!("oversized status line should fail"),
            Err(error) => error,
        };

        let error = error.downcast::<io::Error>().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "HTTP response line too long");
    }

    #[test]
    fn plain_http1_chunk_reader_rejects_unbounded_size_lines() {
        let mut response = io::Cursor::new({
            let mut bytes = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
            bytes.resize(bytes.len() + HTTP_RESPONSE_LINE_LIMIT + 1, b'1');
            bytes.extend_from_slice(b"\r\n");
            bytes
        });
        let mut line = String::with_capacity(HTTP_LINE_BUFFER_CAPACITY);
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = match read_plain_http1_response(&mut response, None, &mut line, &mut buffer) {
            Ok(_) => panic!("oversized chunk size line should fail"),
            Err(error) => error,
        };

        let error = error.downcast::<io::Error>().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "HTTP response line too long");
    }

    #[test]
    fn plain_http1_response_rejects_chunked_with_content_length() {
        let mut response = io::Cursor::new(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 5\r\n\r\n0\r\n\r\n"
                .to_vec(),
        );
        let mut line = String::with_capacity(HTTP_LINE_BUFFER_CAPACITY);
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = match read_plain_http1_response(&mut response, None, &mut line, &mut buffer) {
            Ok(_) => panic!("ambiguous response framing should fail"),
            Err(error) => error,
        };

        assert_eq!(
            error.to_string(),
            "HTTP/1 response cannot combine Transfer-Encoding: chunked and Content-Length"
        );
        let mut rest = Vec::new();
        response.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"0\r\n\r\n");
    }

    #[test]
    fn plain_http1_response_rejects_unsupported_transfer_encoding() {
        let mut response = io::Cursor::new(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n0\r\n\r\n".to_vec(),
        );
        let mut line = String::with_capacity(HTTP_LINE_BUFFER_CAPACITY);
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = match read_plain_http1_response(&mut response, None, &mut line, &mut buffer) {
            Ok(_) => panic!("unsupported transfer encoding should fail"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "unsupported Transfer-Encoding header");
    }

    #[test]
    fn plain_http1_response_accepts_repeated_chunked_transfer_encoding_headers() {
        let mut response = io::Cursor::new(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nbody\r\n0\r\n\r\n".to_vec(),
        );
        let mut line = String::with_capacity(HTTP_LINE_BUFFER_CAPACITY);
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let actual = read_plain_http1_response(&mut response, None, &mut line, &mut buffer)
            .expect("read response");

        match actual.body {
            ResponseBody::Memory(body) => assert_eq!(body, b"body"),
            ResponseBody::File { .. } => panic!("small chunked response should stay in memory"),
        }
    }

    #[test]
    fn plain_http1_chunk_reader_rejects_too_many_trailers() {
        let mut bytes = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n".to_vec();
        for _ in 0..=HTTP_CHUNK_TRAILER_LIMIT {
            bytes.extend_from_slice(b"X-Trailer: value\r\n");
        }
        bytes.extend_from_slice(b"\r\n");
        let mut response = io::Cursor::new(bytes);
        let mut line = String::with_capacity(HTTP_LINE_BUFFER_CAPACITY);
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = match read_plain_http1_response(&mut response, None, &mut line, &mut buffer) {
            Ok(_) => panic!("unbounded chunk trailers should fail"),
            Err(error) => error,
        };

        let error = error.downcast::<io::Error>().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "too many HTTP chunk trailers");
    }

    #[test]
    fn plain_http1_connection_preallocates_reusable_line_buffer() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (_stream, _addr) = listener.accept().unwrap();
        });
        let url = reqwest::Url::parse(&format!("http://127.0.0.1:{port}/repo.git")).unwrap();

        let connection = PlainHttp1Connection::connect(&url, None, None, None, None, false)
            .expect("connect plain http1");

        assert_eq!(connection.line.capacity(), HTTP_LINE_BUFFER_CAPACITY);
        assert_eq!(connection.reader.capacity(), STREAM_BUFFER_SIZE);
        drop(connection);
        server.join().unwrap();
    }

    #[test]
    fn plain_http1_pool_retries_stale_reused_connection_once() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            for body in [b"one".as_slice(), b"two".as_slice()] {
                let (mut stream, _addr) = listener.accept().unwrap();
                let mut request = [0_u8; 1024];
                let mut request_len = 0_usize;
                loop {
                    let n = stream.read(&mut request[request_len..]).unwrap();
                    assert!(n > 0);
                    request_len += n;
                    if request[..request_len]
                        .windows(4)
                        .any(|window| window == b"\r\n\r\n")
                    {
                        break;
                    }
                }
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
                stream.flush().unwrap();
            }
        });
        let mut pool = PlainHttp1Pool::new(
            Arc::from("proxy://plain-http-test"),
            Arc::from(DEFAULT_TLS_VERIFICATION_IDENTITY),
            None,
            None,
            None,
            false,
        );
        let url = format!("http://127.0.0.1:{port}/repo.git/info/refs");
        let first = pool
            .request(TransportRequest {
                method: http::Method::GET,
                url: url.clone(),
                headers: Vec::new(),
                body: RequestBody::Empty,
                output_file: None,
            })
            .expect("first request");
        assert!(matches!(first.body, ResponseBody::Memory(ref body) if body == b"one"));

        let second = pool
            .request(TransportRequest {
                method: http::Method::GET,
                url,
                headers: Vec::new(),
                body: RequestBody::Empty,
                output_file: None,
            })
            .expect("stale pooled connection should retry once");
        assert!(matches!(second.body, ResponseBody::Memory(ref body) if body == b"two"));

        server.join().unwrap();
    }

    #[test]
    fn request_once_ignores_content_length_for_empty_body_status() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _addr) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let mut request_len = 0_usize;
            loop {
                let n = stream.read(&mut request[request_len..]).unwrap();
                assert!(n > 0);
                request_len += n;
                if request[..request_len]
                    .windows(4)
                    .any(|window| window == b"\r\n\r\n")
                {
                    break;
                }
            }
            stream
                .write_all(
                    b"HTTP/1.1 304 Not Modified\r\nContent-Length: 123\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            stream.flush().unwrap();
        });
        let client = Client::builder()
            .no_proxy()
            .http1_only()
            .build()
            .expect("client");
        let mut response_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let response = request_once_with_buffer(
            &client,
            RequestOnceOptions {
                expected_version: HttpVersion::Http1,
                include_response_headers: true,
                send_phase_label: None,
            },
            RequestOnceInput {
                method: http::Method::GET,
                url: &format!("http://127.0.0.1:{port}/repo.git/info/refs"),
                headers: &[],
                body: RequestBody::Empty,
                output_file: None,
            },
            &mut response_buffer,
        )
        .expect("response");

        assert_eq!(response.status, "304 Not Modified");
        assert!(response.headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("content-length") && value == "123"
        }));
        match response.body {
            ResponseBody::Memory(body) => assert!(body.is_empty()),
            ResponseBody::File { .. } => panic!("empty response should stay in memory"),
        }
        server.join().unwrap();
    }

    #[test]
    fn request_once_does_not_auto_follow_redirects() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicBool, Ordering};

        let _ = rustls::crypto::ring::default_provider().install_default();
        let redirect_target_hit = Arc::new(AtomicBool::new(false));
        let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let target_port = target_listener.local_addr().unwrap().port();
        target_listener.set_nonblocking(true).unwrap();
        let target_hit = Arc::clone(&redirect_target_hit);
        let target_server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_millis(250);
            loop {
                match target_listener.accept() {
                    Ok((_stream, _addr)) => {
                        target_hit.store(true, Ordering::SeqCst);
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() >= deadline {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("target accept failed: {error}"),
                }
            }
        });

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _addr) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let mut request_len = 0_usize;
            loop {
                let n = stream.read(&mut request[request_len..]).unwrap();
                assert!(n > 0);
                request_len += n;
                if request[..request_len]
                    .windows(4)
                    .any(|window| window == b"\r\n\r\n")
                {
                    break;
                }
            }
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{target_port}/repo.git\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            stream.flush().unwrap();
        });
        let args = Args {
            http_version: HttpVersion::Http1,
            pool_idle_timeout_secs: 90,
            pool_max_idle_per_host: 8,
            method: None,
            url: None,
            headers: Vec::new(),
            body_file: None,
            output_file: None,
            ca_file: None,
            client_cert_file: None,
            client_key_file: None,
            tls_no_verify: false,
            batch: false,
        };
        let client = build_client_for_version(&args, HttpVersion::Http1).expect("client");
        let mut response_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let response = request_once_with_buffer(
            &client,
            RequestOnceOptions {
                expected_version: HttpVersion::Http1,
                include_response_headers: false,
                send_phase_label: None,
            },
            RequestOnceInput {
                method: http::Method::GET,
                url: &format!("http://127.0.0.1:{port}/repo.git/info/refs"),
                headers: &[],
                body: RequestBody::Empty,
                output_file: None,
            },
            &mut response_buffer,
        )
        .expect("response");

        assert_eq!(response.status, "302 Found");
        assert_eq!(
            response.headers,
            vec![(
                "location".to_owned(),
                format!("http://127.0.0.1:{target_port}/repo.git")
            )]
        );
        match response.body {
            ResponseBody::Memory(body) => assert!(body.is_empty()),
            ResponseBody::File { .. } => panic!("empty redirect body should stay in memory"),
        }
        server.join().unwrap();
        target_server.join().unwrap();
        assert!(!redirect_target_hit.load(Ordering::SeqCst));
    }

    #[test]
    fn request_once_head_ignores_content_length_body() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _addr) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let mut request_len = 0_usize;
            loop {
                let n = stream.read(&mut request[request_len..]).unwrap();
                assert!(n > 0);
                request_len += n;
                if request[..request_len]
                    .windows(4)
                    .any(|window| window == b"\r\n\r\n")
                {
                    break;
                }
            }
            assert!(
                std::str::from_utf8(&request[..request_len])
                    .unwrap()
                    .starts_with("HEAD ")
            );
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 123\r\nConnection: close\r\n\r\n")
                .unwrap();
            stream.flush().unwrap();
        });
        let client = Client::builder()
            .no_proxy()
            .http1_only()
            .build()
            .expect("client");
        let mut response_buffer = [0_u8; STREAM_BUFFER_SIZE];

        let response = request_once_with_buffer(
            &client,
            RequestOnceOptions {
                expected_version: HttpVersion::Http1,
                include_response_headers: false,
                send_phase_label: None,
            },
            RequestOnceInput {
                method: http::Method::HEAD,
                url: &format!("http://127.0.0.1:{port}/repo.git/info/refs"),
                headers: &[],
                body: RequestBody::Empty,
                output_file: None,
            },
            &mut response_buffer,
        )
        .expect("response");

        assert_eq!(response.status, "200 OK");
        match response.body {
            ResponseBody::Memory(body) => assert!(body.is_empty()),
            ResponseBody::File { .. } => panic!("HEAD response should stay in memory"),
        }
        server.join().unwrap();
    }

    #[test]
    fn plain_http1_chunked_response_reuses_line_and_copy_buffers() {
        let mut line = String::with_capacity(64);
        let line_capacity = line.capacity();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut response = io::Cursor::new(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nbody\r\n0\r\n\r\n".to_vec(),
        );

        let parsed =
            read_plain_http1_response(&mut response, None, &mut line, &mut buffer).unwrap();

        assert_eq!(line.capacity(), line_capacity);
        match parsed.body {
            ResponseBody::Memory(body) => assert_eq!(body, b"body"),
            ResponseBody::File { .. } => panic!("small chunked body should stay in memory"),
        }
    }

    #[test]
    fn chunk_size_parser_accepts_extensions_and_detects_overflow() {
        assert_eq!(parse_http_chunk_size(b"a;name=value\r\n").unwrap(), 10);
        assert_eq!(parse_http_chunk_size(b"0\r\n").unwrap(), 0);

        let error = parse_http_chunk_size(b"ffffffffffffffffffffffffffffffff\r\n")
            .expect_err("oversized chunk");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "chunk too large");
    }

    #[test]
    fn decimal_usize_parser_detects_invalid_and_overflow_values() {
        assert_eq!(
            parse_decimal_usize(b"12345", "invalid decimal", "decimal too large").unwrap(),
            12345
        );

        let invalid =
            parse_decimal_usize(b"12x", "invalid decimal", "decimal too large").unwrap_err();
        assert_eq!(invalid.kind(), io::ErrorKind::InvalidData);
        assert_eq!(invalid.to_string(), "invalid decimal");

        let too_large = parse_decimal_usize(
            b"99999999999999999999999999999999999999999999999999",
            "invalid decimal",
            "decimal too large",
        )
        .unwrap_err();
        assert_eq!(too_large.kind(), io::ErrorKind::InvalidData);
        assert_eq!(too_large.to_string(), "decimal too large");
    }

    #[test]
    fn decimal_u64_parser_detects_invalid_and_overflow_values() {
        assert_eq!(parse_decimal_u64(b"12345"), Some(12_345));
        assert_eq!(parse_decimal_u64(b""), None);
        assert_eq!(parse_decimal_u64(b"12x"), None);
        assert_eq!(
            parse_decimal_u64(b"99999999999999999999999999999999999999999999999999"),
            None
        );
    }

    #[test]
    fn http_content_length_parser_rejects_invalid_and_overflow_values() {
        let mut headers = http::HeaderMap::new();
        headers.insert(http::header::CONTENT_LENGTH, "12345".parse().unwrap());
        assert_eq!(http_content_length(&headers).unwrap(), Some(12_345));

        headers.insert(http::header::CONTENT_LENGTH, "12x".parse().unwrap());
        let error = http_content_length(&headers).expect_err("invalid length");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "invalid Content-Length header");

        headers.insert(
            http::header::CONTENT_LENGTH,
            "99999999999999999999999999999999999999999999999999"
                .parse()
                .unwrap(),
        );
        let error = http_content_length(&headers).expect_err("overflow length");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "invalid Content-Length header");
    }

    #[test]
    fn http_content_length_parser_rejects_conflicting_values() {
        let mut headers = http::HeaderMap::new();
        headers.append(http::header::CONTENT_LENGTH, "12".parse().unwrap());
        headers.append(http::header::CONTENT_LENGTH, "34".parse().unwrap());

        let error = http_content_length(&headers).expect_err("conflicting length");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "conflicting Content-Length headers");
    }

    #[test]
    fn content_length_copy_uses_caller_buffer_and_detects_short_body() {
        let mut reader = io::Cursor::new(b"body-next".to_vec());
        let mut out = Vec::new();
        let mut buffer = [0x55_u8; STREAM_BUFFER_SIZE];

        let copied =
            copy_content_length_body_with_buffer(&mut reader, &mut out, 4, &mut buffer).unwrap();

        assert_eq!(copied, 4);
        assert_eq!(out, b"body");
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"-next");

        let mut short = io::Cursor::new(b"bo".to_vec());
        let error =
            copy_content_length_body_with_buffer(&mut short, &mut Vec::new(), 4, &mut buffer)
                .expect_err("short body");
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(error.to_string(), "HTTP response ended early");
    }

    #[test]
    fn content_length_copy_returns_declared_length_for_multichunk_body() {
        let content_length = STREAM_BUFFER_SIZE + 3;
        let mut input = vec![0x41; content_length];
        input.extend_from_slice(b"-next");
        let mut reader = io::Cursor::new(input);
        let mut out = Vec::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let copied = copy_content_length_body_with_buffer(
            &mut reader,
            &mut out,
            content_length,
            &mut buffer,
        )
        .expect("copy body");

        assert_eq!(copied, u64::try_from(content_length).expect("length"));
        assert_eq!(out.len(), content_length);
        assert!(out.iter().all(|byte| *byte == 0x41));
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).expect("read rest");
        assert_eq!(rest, b"-next");
    }

    #[test]
    fn plain_http1_request_target_and_host_are_written_without_intermediate_strings() {
        let url = reqwest::Url::parse(
            "http://example.test:8080/repo.git/info/refs?service=git-upload-pack",
        )
        .unwrap();
        let mut target = Vec::new();
        let mut host = Vec::new();

        write_plain_http1_request_target(&mut target, &url).unwrap();
        write_plain_http1_host_header(&mut host, &url).unwrap();

        assert_eq!(target, b"/repo.git/info/refs?service=git-upload-pack");
        assert_eq!(host, b"example.test:8080");
    }

    #[test]
    fn plain_http1_request_head_reuses_buffer() {
        let url = reqwest::Url::parse(
            "http://example.test:8080/repo.git/info/refs?service=git-upload-pack",
        )
        .unwrap();
        let request = TransportRequest {
            method: http::Method::GET,
            url: url.to_string(),
            headers: vec![
                parse_request_header("Accept: application/x-git-upload-pack-advertisement")
                    .unwrap(),
            ],
            body: RequestBody::Empty,
            output_file: None,
        };
        let mut head = Vec::with_capacity(1024);
        let capacity = head.capacity();

        write_plain_http1_request_head(&mut head, &request, &url).unwrap();

        assert_eq!(head.capacity(), capacity);
        assert!(
            std::str::from_utf8(&head)
                .unwrap()
                .starts_with("GET /repo.git/info/refs?service=git-upload-pack HTTP/1.1\r\n")
        );
        assert!(
            std::str::from_utf8(&head)
                .unwrap()
                .contains("accept: application/x-git-upload-pack-advertisement\r\n")
        );
        assert!(
            std::str::from_utf8(&head)
                .unwrap()
                .contains("Content-Length: 0\r\n")
        );
    }

    #[test]
    fn plain_http1_request_head_does_not_duplicate_user_agent_header() {
        let url = reqwest::Url::parse("http://example.test/repo.git/info/refs").unwrap();
        let request = TransportRequest {
            method: http::Method::GET,
            url: url.to_string(),
            headers: vec![parse_request_header("User-Agent: skron-test/1").unwrap()],
            body: RequestBody::Empty,
            output_file: None,
        };
        let mut head = Vec::new();

        write_plain_http1_request_head(&mut head, &request, &url).unwrap();

        let head = std::str::from_utf8(&head).unwrap();
        assert_eq!(head.matches("user-agent:").count(), 1);
        assert!(head.contains("\r\nuser-agent: skron-test/1\r\n"));
    }

    #[test]
    fn plain_http1_request_head_reserve_only_adds_missing_capacity() {
        let mut head = Vec::with_capacity(128);

        reserve_plain_http1_request_head(&mut head, 96);
        assert_eq!(head.capacity(), 128);

        reserve_plain_http1_request_head(&mut head, 256);
        assert!(head.capacity() >= 256);
    }

    #[test]
    fn plain_http1_request_head_trim_keeps_small_reusable_buffer() {
        let mut head = Vec::with_capacity(PLAIN_HTTP1_REQUEST_HEAD_INITIAL_CAPACITY);
        head.extend_from_slice(b"GET / HTTP/1.1\r\n\r\n");

        trim_plain_http1_request_head_buffer(&mut head);

        assert!(head.is_empty());
        assert_eq!(head.capacity(), PLAIN_HTTP1_REQUEST_HEAD_INITIAL_CAPACITY);
    }

    #[test]
    fn plain_http1_request_head_trim_drops_oversized_pooled_buffer() {
        let mut head = Vec::with_capacity(PLAIN_HTTP1_REQUEST_HEAD_RETAIN_CAPACITY_LIMIT + 1);
        head.extend_from_slice(b"GET /large HTTP/1.1\r\n\r\n");

        trim_plain_http1_request_head_buffer(&mut head);

        assert!(head.is_empty());
        assert_eq!(head.capacity(), PLAIN_HTTP1_REQUEST_HEAD_INITIAL_CAPACITY);
    }

    #[test]
    fn plain_http1_request_head_reserves_long_target_without_growth() {
        let long_path = "segment/".repeat(512);
        let url = reqwest::Url::parse(&format!(
            "http://example.test:8080/{long_path}info/refs?service=git-upload-pack"
        ))
        .unwrap();
        let request = TransportRequest {
            method: http::Method::POST,
            url: url.to_string(),
            headers: vec![
                parse_request_header("Content-Type: application/x-git-upload-pack-request")
                    .unwrap(),
            ],
            body: RequestBody::Memory(Bytes::from_static(b"body")),
            output_file: None,
        };
        let mut head = Vec::new();

        write_plain_http1_request_head(&mut head, &request, &url).unwrap();

        assert!(head.capacity() >= head.len());
        assert!(
            std::str::from_utf8(&head)
                .unwrap()
                .starts_with("POST /segment/segment/")
        );
    }

    #[test]
    fn plain_http1_request_head_brackets_ipv6_host_header() {
        let url =
            reqwest::Url::parse("http://[2001:4860:4860::8888]:8080/repo.git/info/refs").unwrap();
        let request = TransportRequest {
            method: http::Method::GET,
            url: url.to_string(),
            headers: Vec::new(),
            body: RequestBody::Empty,
            output_file: None,
        };
        let mut head = Vec::new();

        write_plain_http1_request_head(&mut head, &request, &url).unwrap();

        let head = std::str::from_utf8(&head).unwrap();
        assert!(head.contains("\r\nHost: [2001:4860:4860::8888]:8080\r\n"));
    }

    #[test]
    fn response_body_spills_after_threshold_despite_small_hint() {
        let mut body = SpoolingResponseBody::new(16);
        let chunk = [0x41; STREAM_BUFFER_SIZE];
        let mut remaining = AUTO_RESPONSE_FILE_THRESHOLD as usize + 1;
        while remaining > 0 {
            let len = remaining.min(chunk.len());
            body.write_all(&chunk[..len]).unwrap();
            remaining -= len;
        }

        match body.finish().unwrap() {
            ResponseBody::File { path, len } => {
                assert_eq!(len, AUTO_RESPONSE_FILE_THRESHOLD + 1);
                assert_eq!(
                    fs::metadata(&path).unwrap().len(),
                    AUTO_RESPONSE_FILE_THRESHOLD + 1
                );
                fs::remove_file(path).unwrap();
            }
            ResponseBody::Memory(_) => panic!("large response body should spill to a temp file"),
        }
    }

    #[test]
    fn response_body_spill_buffers_temp_file_writes() {
        let mut body = SpoolingResponseBody::new(0);
        body.write_all(&vec![0x41; AUTO_RESPONSE_FILE_THRESHOLD as usize + 1])
            .unwrap();

        let file = body.file.as_ref().expect("large body should spill");
        assert_eq!(file.capacity(), FILE_STREAM_WRITE_BUFFER_SIZE);
        assert_eq!(body.memory.capacity(), 0);

        match body.finish().unwrap() {
            ResponseBody::File { path, len } => {
                assert_eq!(len, AUTO_RESPONSE_FILE_THRESHOLD + 1);
                fs::remove_file(path).unwrap();
            }
            ResponseBody::Memory(_) => panic!("large response body should spill to a temp file"),
        }
    }

    #[test]
    fn response_body_memory_preserves_exact_known_capacity_hint() {
        let mut body = SpoolingResponseBody::new(128);

        body.write_all(b"body").unwrap();

        match body.finish().unwrap() {
            ResponseBody::Memory(body) => {
                assert_eq!(body, b"body");
                assert_eq!(body.capacity(), 128);
            }
            ResponseBody::File { .. } => panic!("small hinted body should stay in memory"),
        }
    }

    #[test]
    fn known_http_body_bytes_append_without_spooling_wrapper() {
        let mut body = Vec::with_capacity(5);
        let mut written = 0;

        append_known_http_body_bytes(&mut body, &mut written, b"ab", 5).unwrap();
        append_known_http_body_bytes(&mut body, &mut written, b"cde", 5).unwrap();

        assert_eq!(body, b"abcde");
        assert_eq!(body.capacity(), 5);
        assert_eq!(written, 5);
        let error =
            append_known_http_body_bytes(&mut body, &mut written, b"!", 5).expect_err("overrun");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn known_http_body_bytes_reuses_existing_spare_capacity() {
        let mut body = Vec::with_capacity(8);
        let mut written = 0;

        append_known_http_body_bytes(&mut body, &mut written, b"ab", 5).unwrap();
        append_known_http_body_bytes(&mut body, &mut written, b"cde", 5).unwrap();

        assert_eq!(body, b"abcde");
        assert_eq!(body.capacity(), 8);
        assert_eq!(written, 5);
    }

    #[test]
    fn known_http_body_bytes_grows_when_spare_capacity_is_insufficient() {
        let mut body = Vec::with_capacity(1);
        let mut written = 0;

        append_known_http_body_bytes(&mut body, &mut written, b"ab", 5).unwrap();
        append_known_http_body_bytes(&mut body, &mut written, b"cde", 5).unwrap();

        assert_eq!(body, b"abcde");
        assert!(body.capacity() >= 5);
        assert_eq!(written, 5);
    }

    #[test]
    fn stream_copy_length_guard_detects_overflow_before_write() {
        let error =
            checked_stream_copy_len(u64::MAX, 1).expect_err("stream length should overflow");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "stream length overflow");
    }

    #[test]
    fn chained_request_body_reader_preserves_prefix_and_file_order() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"file-body").unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut reader = ChainedRequestBodyReader {
            prefix: io::Cursor::new(Bytes::from_static(b"prefix-")),
            file,
        };
        let mut actual = Vec::new();

        reader.read_to_end(&mut actual).unwrap();

        assert_eq!(actual, b"prefix-file-body");
    }

    #[test]
    fn request_body_write_to_rewinds_file_without_descriptor_clone() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"body").unwrap();
        let mut body = RequestBody::File { file, len: 4 };
        let mut first = Vec::new();
        let mut second = Vec::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        body.write_to_with_buffer(&mut first, &mut buffer).unwrap();
        body.write_to_with_buffer(&mut second, &mut buffer).unwrap();

        assert_eq!(first, b"body");
        assert_eq!(second, b"body");
    }

    #[test]
    fn request_body_write_to_can_reuse_caller_buffer() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"file-body").unwrap();
        let mut body = RequestBody::Chain {
            prefix: Bytes::from_static(b"prefix-"),
            file,
            file_len: 9,
        };
        let mut out = Vec::new();
        let mut buffer = [0x55_u8; STREAM_BUFFER_SIZE];

        body.write_to_with_buffer(&mut out, &mut buffer).unwrap();

        assert_eq!(out, b"prefix-file-body");
    }

    #[test]
    fn request_body_write_validated_to_trusts_declared_file_len() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"file-body-extra").unwrap();
        let mut body = RequestBody::Chain {
            prefix: Bytes::from_static(b"prefix-"),
            file,
            file_len: 9,
        };
        let mut out = Vec::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        body.write_validated_to_with_buffer(&mut out, &mut buffer)
            .expect("validated writer should trust preflight length");

        assert_eq!(out, b"prefix-file-body");
    }

    #[test]
    fn request_body_file_copy_uses_exact_declared_length() {
        let mut reader = io::Cursor::new(b"body-extra".to_vec());
        let mut out = Vec::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        copy_request_body_file_with_buffer(&mut reader, &mut out, 4, &mut buffer).unwrap();

        assert_eq!(out, b"body");
        assert_eq!(reader.position(), 4);
    }

    #[test]
    fn request_body_file_copy_reports_early_eof() {
        let mut reader = io::Cursor::new(b"body".to_vec());
        let mut out = Vec::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = copy_request_body_file_with_buffer(&mut reader, &mut out, 8, &mut buffer)
            .expect_err("short request body file");

        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(error.to_string(), "request body file ended early");
    }

    #[test]
    fn request_body_write_to_rejects_changed_file_length() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"body-extra").unwrap();
        let mut body = RequestBody::File { file, len: 4 };
        let mut out = Vec::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = body
            .write_to_with_buffer(&mut out, &mut buffer)
            .expect_err("changed length");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "request body file length changed: expected 4, got 10"
        );
        assert!(out.is_empty());
    }

    #[test]
    fn chained_request_body_write_to_rejects_changed_file_length_before_prefix() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"file-body-extra").unwrap();
        let mut body = RequestBody::Chain {
            prefix: Bytes::from_static(b"prefix-"),
            file,
            file_len: 9,
        };
        let mut out = Vec::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];

        let error = body
            .write_to_with_buffer(&mut out, &mut buffer)
            .expect_err("changed length");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "request body file length changed: expected 9, got 15"
        );
        assert!(out.is_empty());
    }

    #[test]
    fn http3_request_body_chunks_read_directly_into_owned_bytes() {
        let mut reader = io::Cursor::new(b"abc".to_vec());
        let mut buffer = BytesMut::with_capacity(STREAM_BUFFER_SIZE);
        let mut remaining = 3;

        let chunk = read_http3_request_body_chunk(&mut reader, &mut remaining, &mut buffer)
            .unwrap()
            .expect("first chunk");

        assert_eq!(chunk.as_ref(), b"abc");
        assert_eq!(remaining, 0);
        assert!(
            read_http3_request_body_chunk(&mut reader, &mut remaining, &mut buffer)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn http3_request_body_chunk_restores_full_stream_buffer_capacity() {
        let input = vec![0x41; STREAM_BUFFER_SIZE];
        let mut reader = io::Cursor::new(input);
        let mut buffer = BytesMut::with_capacity(STREAM_BUFFER_SIZE / 2);
        let mut remaining = STREAM_BUFFER_SIZE as u64;

        let chunk = read_http3_request_body_chunk(&mut reader, &mut remaining, &mut buffer)
            .unwrap()
            .expect("first chunk");

        assert_eq!(chunk.len(), STREAM_BUFFER_SIZE);
        assert_eq!(remaining, 0);
    }

    #[test]
    fn http3_request_body_chunk_reads_only_declared_length() {
        let mut reader = io::Cursor::new(b"abcdef".to_vec());
        let mut buffer = BytesMut::with_capacity(STREAM_BUFFER_SIZE);
        let mut remaining = 3;

        let chunk = read_http3_request_body_chunk(&mut reader, &mut remaining, &mut buffer)
            .unwrap()
            .expect("chunk");

        assert_eq!(chunk.as_ref(), b"abc");
        assert_eq!(reader.position(), 3);
        assert_eq!(remaining, 0);
    }

    #[test]
    fn http3_request_body_chunk_reports_early_eof() {
        let mut reader = io::Cursor::new(b"abc".to_vec());
        let mut buffer = BytesMut::with_capacity(STREAM_BUFFER_SIZE);
        let mut remaining = 4;

        let first = read_http3_request_body_chunk(&mut reader, &mut remaining, &mut buffer)
            .unwrap()
            .expect("first chunk");
        let error = read_http3_request_body_chunk(&mut reader, &mut remaining, &mut buffer)
            .expect_err("short body");

        assert_eq!(first.as_ref(), b"abc");
        assert_eq!(remaining, 1);
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(error.to_string(), "request body file ended early");
    }

    #[test]
    fn http3_request_body_buffer_reserves_only_missing_capacity() {
        let mut buffer = BytesMut::with_capacity(STREAM_BUFFER_SIZE / 2);

        reserve_http3_request_body_buffer(&mut buffer);

        assert_eq!(buffer.capacity(), STREAM_BUFFER_SIZE);
    }

    #[test]
    fn http_body_length_validation_detects_short_and_long_bodies() {
        let mut written = 0;
        add_http_body_bytes(&mut written, 2, Some(4)).unwrap();
        let error = finish_http_body_length(written, Some(4)).expect_err("short body");
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(error.to_string(), "HTTP response ended early");

        let mut written = 0;
        let error = add_http_body_bytes(&mut written, 5, Some(4)).expect_err("long body");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "HTTP response body exceeded Content-Length"
        );
    }

    #[test]
    fn checked_http_body_writer_rejects_overrun_before_write() {
        let mut out = Vec::new();
        let mut written = 0;

        write_http_body_bytes_checked(&mut out, &mut written, b"ok", Some(2)).unwrap();
        let error = write_http_body_bytes_checked(&mut out, &mut written, b"!", Some(2))
            .expect_err("overrun");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "HTTP response body exceeded Content-Length"
        );
        assert_eq!(written, 2);
        assert_eq!(out, b"ok");
    }

    #[test]
    fn checked_http_body_writer_rejects_counter_overflow_before_write() {
        let mut out = Vec::new();
        let mut written = u64::MAX;

        let error = write_http_body_bytes_checked(&mut out, &mut written, b"!", None)
            .expect_err("counter overflow");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "body too large");
        assert_eq!(written, u64::MAX);
        assert!(out.is_empty());
    }

    #[test]
    fn sync_http_response_body_copy_validates_content_length() {
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut short = io::Cursor::new(b"ok".to_vec());
        let mut short_out = Vec::new();

        let error =
            copy_http_response_body_with_buffer(&mut short, &mut short_out, &mut buffer, Some(3))
                .expect_err("short response");

        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(error.to_string(), "HTTP response ended early");
        assert_eq!(short_out, b"ok");

        let mut long = io::Cursor::new(b"okay".to_vec());
        let mut long_out = Vec::new();
        let error =
            copy_http_response_body_with_buffer(&mut long, &mut long_out, &mut buffer, Some(3))
                .expect_err("long response");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "HTTP response body exceeded Content-Length"
        );
        assert!(long_out.is_empty());
    }

    #[test]
    fn plain_http1_batch_candidate_checks_scheme_without_url_parse() {
        assert!(plain_http1_batch_candidate("http://example.test/repo.git"));
        assert!(plain_http1_batch_candidate("HTTP://example.test/repo.git"));
        assert!(!plain_http1_batch_candidate(
            "https://example.test/repo.git"
        ));
        assert!(!plain_http1_batch_candidate("ssh://example.test/repo.git"));
        assert!(!plain_http1_batch_candidate("http:/example.test/repo.git"));
    }

    #[test]
    fn direct_http1_batch_candidate_allows_https_without_proxy() {
        let args = Args {
            http_version: HttpVersion::Auto,
            pool_idle_timeout_secs: 90,
            pool_max_idle_per_host: 8,
            method: None,
            url: None,
            headers: Vec::new(),
            body_file: None,
            output_file: None,
            ca_file: None,
            client_cert_file: None,
            client_key_file: None,
            tls_no_verify: false,
            batch: true,
        };
        let mut proxy_free = Some(true);
        assert!(direct_http1_batch_candidate(
            "https://example.test/repo.git",
            &mut proxy_free,
            &args
        ));

        proxy_free = Some(false);
        assert!(!direct_http1_batch_candidate(
            "https://example.test/repo.git",
            &mut proxy_free,
            &args
        ));
    }

    #[test]
    fn direct_http1_batch_candidate_allows_custom_ca_without_client_cert() {
        let args = Args {
            http_version: HttpVersion::Auto,
            pool_idle_timeout_secs: 90,
            pool_max_idle_per_host: 8,
            method: None,
            url: None,
            headers: Vec::new(),
            body_file: None,
            output_file: None,
            ca_file: Some("ca.pem".to_owned()),
            client_cert_file: None,
            client_key_file: None,
            tls_no_verify: false,
            batch: true,
        };
        let mut proxy_free = Some(true);
        assert!(direct_http1_batch_candidate(
            "https://example.test/repo.git",
            &mut proxy_free,
            &args
        ));
    }

    #[test]
    fn direct_http1_batch_candidate_allows_client_cert() {
        let args = Args {
            http_version: HttpVersion::Auto,
            pool_idle_timeout_secs: 90,
            pool_max_idle_per_host: 8,
            method: None,
            url: None,
            headers: Vec::new(),
            body_file: None,
            output_file: None,
            ca_file: Some("ca.pem".to_owned()),
            client_cert_file: Some("client.pem".to_owned()),
            client_key_file: None,
            tls_no_verify: false,
            batch: true,
        };
        let mut proxy_free = Some(true);
        assert!(direct_http1_batch_candidate(
            "https://example.test/repo.git",
            &mut proxy_free,
            &args
        ));
    }

    #[test]
    fn plain_http1_pool_reuses_rustls_client_config() {
        let mut pool = PlainHttp1Pool::new(
            Arc::from(NONE_IDENTITY),
            Arc::from("no-verify"),
            None,
            None,
            None,
            true,
        );
        let url = reqwest::Url::parse("https://example.test/repo.git").unwrap();

        let first = pool
            .rustls_http1_config_for_url(&url)
            .expect("first config")
            .expect("https config");
        let second = pool
            .rustls_http1_config_for_url(&url)
            .expect("second config")
            .expect("https config");

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn plain_http1_origin_key_avoids_formatted_origin_string() {
        let default_port = plain_http1_origin_key(
            &reqwest::Url::parse("http://example.test/repo.git").unwrap(),
            &[],
            Some(Arc::from("proxy://example")),
            Some(Arc::from("ca-file:test")),
        )
        .unwrap();
        assert_eq!(
            default_port,
            PlainHttp1Origin {
                scheme: "http".to_owned(),
                host: "example.test".to_owned(),
                port: 80,
                proxy_identity: Arc::from("proxy://example"),
                tls_verification_identity: Arc::from("ca-file:test"),
                credential_identity: None,
            }
        );

        let explicit_port = plain_http1_origin_key(
            &reqwest::Url::parse("http://example.test:8080/repo.git").unwrap(),
            &[RequestHeader {
                name: HeaderName::from_static("authorization"),
                value: HeaderValue::from_static("Basic abc"),
            }],
            Some(Arc::from("proxy://example")),
            Some(Arc::from("ca-file:test")),
        )
        .unwrap();
        assert_eq!(
            explicit_port,
            PlainHttp1Origin {
                scheme: "http".to_owned(),
                host: "example.test".to_owned(),
                port: 8080,
                proxy_identity: Arc::from("proxy://example"),
                tls_verification_identity: Arc::from("ca-file:test"),
                credential_identity: Some("authorization:Basic abc".to_owned()),
            }
        );
    }

    #[test]
    fn plain_http1_origin_match_reuses_same_origin_key_shape() {
        let proxy_identity: Arc<str> = Arc::from("proxy://example");
        let tls_identity: Arc<str> = Arc::from("ca-file:test");
        let url = reqwest::Url::parse("https://example.test/repo.git").unwrap();
        let headers = [RequestHeader {
            name: HeaderName::from_static("authorization"),
            value: HeaderValue::from_static("Basic abc"),
        }];
        let origin = plain_http1_origin_key(
            &url,
            &headers,
            Some(proxy_identity.clone()),
            Some(tls_identity.clone()),
        )
        .unwrap();

        assert!(plain_http1_origin_matches_request(
            &origin,
            &url,
            &headers,
            &proxy_identity,
            &tls_identity
        ));

        let different_headers = [RequestHeader {
            name: HeaderName::from_static("authorization"),
            value: HeaderValue::from_static("Basic other"),
        }];
        assert!(!plain_http1_origin_matches_request(
            &origin,
            &url,
            &different_headers,
            &proxy_identity,
            &tls_identity
        ));
    }

    #[test]
    fn tls_verification_identity_separates_no_verify_from_ca_and_platform() {
        assert_eq!(
            tls_verification_identity(None, None, None, false),
            "platform"
        );
        assert_eq!(
            tls_verification_identity(Some(" test-ca.pem "), None, None, false),
            "ca_file:test-ca.pem"
        );
        assert_eq!(
            tls_verification_identity(Some("test-ca.pem"), None, None, true),
            "no-verify"
        );
    }

    #[test]
    fn tls_verification_identity_includes_client_certificate_paths() {
        assert_eq!(
            tls_verification_identity(
                Some("ca.pem"),
                Some(" client.pem "),
                Some(" client.key "),
                false
            ),
            "ca_file:ca.pem|client_cert:client.pem|client_key:client.key"
        );
        assert_eq!(
            tls_verification_identity(None, Some("client.pem"), None, true),
            "no-verify|client_cert:client.pem"
        );
    }

    #[test]
    fn plain_http1_connection_pool_eviction_keeps_existing_origin() {
        let incoming = plain_http1_test_origin("incoming.example.test", None);
        let mut connections = HashMap::new();
        for index in 0..PLAIN_HTTP1_CONNECTION_POOL_ENTRY_LIMIT {
            connections.insert(
                plain_http1_test_origin(&format!("origin-{index}.example.test"), None),
                (),
            );
        }

        assert!(plain_http1_connection_pool_eviction_candidate(&connections, &incoming).is_some());
        assert_eq!(
            plain_http1_connection_pool_eviction_candidate(
                &connections,
                connections.keys().next().unwrap()
            ),
            None
        );
    }

    #[test]
    fn plain_http1_connection_pool_eviction_accounts_for_credentials() {
        let incoming = plain_http1_test_origin("example.test", Some("authorization:next"));
        let mut connections = HashMap::new();
        connections.insert(
            plain_http1_test_origin("example.test", Some("authorization:first")),
            (),
        );

        assert_eq!(
            plain_http1_connection_pool_eviction_candidate(&connections, &incoming),
            None
        );

        for index in 1..PLAIN_HTTP1_CONNECTION_POOL_ENTRY_LIMIT {
            connections.insert(
                plain_http1_test_origin("example.test", Some(&format!("authorization:{index}"))),
                (),
            );
        }

        assert!(plain_http1_connection_pool_eviction_candidate(&connections, &incoming).is_some());
    }

    fn plain_http1_test_origin(host: &str, credential_identity: Option<&str>) -> PlainHttp1Origin {
        PlainHttp1Origin {
            scheme: "http".to_owned(),
            host: host.to_owned(),
            port: 80,
            proxy_identity: Arc::from("proxy://example"),
            tls_verification_identity: Arc::from("ca-file:test"),
            credential_identity: credential_identity.map(str::to_owned),
        }
    }

    #[test]
    fn request_credential_identity_uses_headers_before_url_userinfo() {
        let headers = [RequestHeader {
            name: HeaderName::from_static("authorization"),
            value: HeaderValue::from_static("Basic abc"),
        }];
        let uri: http::Uri = "https://user:pass@example.test/repo.git".parse().unwrap();

        assert_eq!(
            request_credential_identity_from_uri(&uri, &headers),
            Some("authorization:Basic abc".to_owned())
        );
        assert_eq!(
            request_credential_identity_from_authority("user:pass@example.test", &headers),
            Some("authorization:Basic abc".to_owned())
        );
    }

    #[test]
    fn request_header_credential_identity_keeps_authorization_priority_in_one_pass() {
        let headers = [
            RequestHeader {
                name: HeaderName::from_static("proxy-authorization"),
                value: HeaderValue::from_static("Basic proxy"),
            },
            RequestHeader {
                name: HeaderName::from_static("authorization"),
                value: HeaderValue::from_static("Basic origin"),
            },
        ];

        assert_eq!(
            request_header_credential_identity(&headers),
            Some("authorization:Basic origin".to_owned())
        );
    }

    #[test]
    fn request_credential_identity_match_uses_borrowed_identity_shape() {
        let headers = [RequestHeader {
            name: HeaderName::from_static("authorization"),
            value: HeaderValue::from_static(" Basic origin "),
        }];
        let url = reqwest::Url::parse("https://user:pass@example.test/repo.git").unwrap();

        assert!(request_credential_identity_matches_url(
            Some("authorization:Basic origin"),
            &url,
            &headers
        ));
        assert!(!request_credential_identity_matches_url(
            Some("url-user:user:pass"),
            &url,
            &headers
        ));
        assert!(request_credential_identity_matches_authority(
            Some("url-user:user:pass"),
            "user:pass@example.test",
            &[]
        ));
        assert!(request_credential_identity_matches_authority(
            None,
            "example.test",
            &[]
        ));
        assert!(!request_credential_identity_matches_authority(
            Some("url-user:user:next"),
            "user:pass@example.test",
            &[]
        ));
    }

    #[test]
    fn request_credential_identity_reads_authority_userinfo_without_url_parse() {
        assert_eq!(
            request_credential_identity_from_authority("user:pass@example.test", &[]),
            Some("url-user:user:pass".to_owned())
        );
        assert_eq!(
            request_credential_identity_from_authority("user@example.test", &[]),
            Some("url-user:user".to_owned())
        );
        assert_eq!(
            request_credential_identity_from_authority("example.test", &[]),
            None
        );
    }

    #[test]
    fn request_credential_identity_reads_http_uri_authority_without_url_parse() {
        let uri: http::Uri = "https://user:pass@example.test/repo.git".parse().unwrap();
        assert_eq!(
            request_credential_identity_from_uri(&uri, &[]),
            Some("url-user:user:pass".to_owned())
        );

        let headers = [RequestHeader {
            name: HeaderName::from_static("authorization"),
            value: HeaderValue::from_static("Basic abc"),
        }];
        assert_eq!(
            request_credential_identity_from_uri(&uri, &headers),
            Some("authorization:Basic abc".to_owned())
        );

        let relative: http::Uri = "/repo.git/git-upload-pack".parse().unwrap();
        assert_eq!(
            request_credential_identity_from_uri(&relative, &headers),
            Some("authorization:Basic abc".to_owned())
        );
    }

    #[test]
    fn proxy_identity_from_values_appends_without_intermediate_vec() {
        assert_eq!(
            proxy_identity_from_values([
                ("HTTPS_PROXY", " https://proxy.example "),
                ("HTTP_PROXY", ""),
                ("NO_PROXY", " localhost,127.0.0.1 "),
            ]),
            "HTTPS_PROXY=https://proxy.example|NO_PROXY=localhost,127.0.0.1"
        );
        assert_eq!(
            proxy_identity_from_values([("HTTPS_PROXY", "  "), ("NO_PROXY", "")]),
            NONE_IDENTITY
        );
    }

    #[test]
    fn http3_origin_key_avoids_formatted_origin_string() {
        let default_port: http::Uri = "https://example.test/repo.git".parse().unwrap();
        assert_eq!(
            http3_origin_key(default_port.authority().expect("authority")),
            Http3Origin {
                scheme: "https".to_owned(),
                host: "example.test".to_owned(),
                port: 443,
            }
        );

        let explicit_port: http::Uri = "https://example.test:8443/repo.git".parse().unwrap();
        assert_eq!(
            http3_origin_key(explicit_port.authority().expect("authority")),
            Http3Origin {
                scheme: "https".to_owned(),
                host: "example.test".to_owned(),
                port: 8443,
            }
        );

        let ipv6: http::Uri = "https://[2001:4860:4860::8888]:8443/repo.git"
            .parse()
            .unwrap();
        assert_eq!(
            http3_origin_key(ipv6.authority().expect("authority")),
            Http3Origin {
                scheme: "https".to_owned(),
                host: "2001:4860:4860::8888".to_owned(),
                port: 8443,
            }
        );
    }

    #[test]
    fn http3_pooled_origin_match_reuses_same_origin_key_shape() {
        let uri: http::Uri = "https://example.test:8443/repo.git/info/refs"
            .parse()
            .unwrap();
        let authority = uri.authority().expect("authority");
        let proxy_identity = Arc::from("HTTPS_PROXY=https://proxy.example");
        let tls_verification_identity = Arc::from("ca-file:test");
        let headers = Vec::new();
        let origin = Http3PooledOrigin::from_origin(
            http3_origin_key(authority),
            Arc::clone(&proxy_identity),
            Arc::clone(&tls_verification_identity),
            request_credential_identity_from_authority(authority.as_str(), &headers),
        );

        assert!(http3_pooled_origin_matches_request(
            &origin,
            authority,
            &headers,
            &proxy_identity,
            &tls_verification_identity
        ));

        let authenticated_headers = vec![RequestHeader {
            name: HeaderName::from_static("authorization"),
            value: HeaderValue::from_static("Basic next"),
        }];
        assert!(!http3_pooled_origin_matches_request(
            &origin,
            authority,
            &authenticated_headers,
            &proxy_identity,
            &tls_verification_identity
        ));

        let changed_tls_identity = Arc::from("ca-file:next");
        assert!(!http3_pooled_origin_matches_request(
            &origin,
            authority,
            &headers,
            &proxy_identity,
            &changed_tls_identity
        ));
    }

    #[test]
    fn http3_connection_pool_eviction_keeps_existing_origin() {
        let incoming = http3_test_pooled_origin("incoming.example.test", None);
        let mut connections = HashMap::new();
        for index in 0..HTTP3_CONNECTION_POOL_ENTRY_LIMIT {
            connections.insert(
                http3_test_pooled_origin(&format!("origin-{index}.example.test"), None),
                (),
            );
        }

        assert!(http3_connection_pool_eviction_candidate(&connections, &incoming).is_some());
        assert_eq!(
            http3_connection_pool_eviction_candidate(
                &connections,
                connections.keys().next().unwrap()
            ),
            None
        );
    }

    #[test]
    fn http3_connection_pool_eviction_accounts_for_credentials() {
        let incoming = http3_test_pooled_origin("example.test", Some("authorization:next"));
        let mut connections = HashMap::new();
        connections.insert(
            http3_test_pooled_origin("example.test", Some("authorization:first")),
            (),
        );

        assert_eq!(
            http3_connection_pool_eviction_candidate(&connections, &incoming),
            None
        );

        for index in 1..HTTP3_CONNECTION_POOL_ENTRY_LIMIT {
            connections.insert(
                http3_test_pooled_origin("example.test", Some(&format!("authorization:{index}"))),
                (),
            );
        }

        assert!(http3_connection_pool_eviction_candidate(&connections, &incoming).is_some());
    }

    fn http3_test_pooled_origin(
        host: &str,
        credential_identity: Option<&str>,
    ) -> Http3PooledOrigin {
        Http3PooledOrigin::from_origin(
            Http3Origin {
                scheme: "https".to_owned(),
                host: host.to_owned(),
                port: 443,
            },
            Arc::from("proxy://example"),
            Arc::from("ca-file:test"),
            credential_identity.map(str::to_owned),
        )
    }

    #[test]
    fn auto_http3_origin_only_accepts_https_urls() {
        assert_eq!(
            auto_http3_origin("https://example.test/repo.git")
                .expect("https origin")
                .expect("https origin present"),
            Http3Origin {
                scheme: "https".to_owned(),
                host: "example.test".into(),
                port: 443,
            }
        );
        assert_eq!(
            auto_http3_origin("https://example.test:8443/repo.git")
                .expect("https origin")
                .expect("https origin present"),
            Http3Origin {
                scheme: "https".to_owned(),
                host: "example.test".into(),
                port: 8443,
            }
        );
        assert_eq!(
            auto_http3_origin("http://example.test/repo.git").expect("http origin"),
            None
        );
        assert_eq!(
            auto_http3_origin("HTTP://example.test/repo.git").expect("uppercase http origin"),
            None
        );
    }

    #[test]
    fn auto_http3_candidate_origin_rejects_invalid_ipv6_without_brackets() {
        assert!(auto_http3_candidate_origin("https://2001:4860:4860::8888/repo.git").is_err());
    }

    #[test]
    fn auto_http3_candidate_origin_rejects_ipv6_with_invalid_port() {
        assert!(
            auto_http3_candidate_origin("https://[2001:4860:4860::8888]:abc/repo.git").is_err()
        );
    }

    #[test]
    fn auto_http3_origin_skips_loopback_and_local_ip_https_hosts() {
        assert_eq!(
            auto_http3_origin("https://localhost/repo.git").expect("localhost origin"),
            None
        );
        assert_eq!(
            auto_http3_origin("https://127.0.0.1/repo.git").expect("loopback origin"),
            None
        );
        assert_eq!(
            auto_http3_origin("https://10.0.0.5/repo.git").expect("private v4 origin"),
            None
        );
        assert_eq!(
            auto_http3_origin("https://[::1]/repo.git").expect("loopback v6 origin"),
            None
        );
        assert_eq!(
            auto_http3_origin("https://[fc00::1]/repo.git").expect("unique local v6 origin"),
            None
        );
    }

    #[test]
    fn auto_http3_origin_keeps_public_ip_https_hosts_eligible() {
        assert_eq!(
            auto_http3_origin("https://93.184.216.34/repo.git")
                .expect("public v4 origin")
                .expect("public v4 eligible"),
            Http3Origin {
                scheme: "https".to_owned(),
                host: "93.184.216.34".into(),
                port: 443,
            }
        );
        assert_eq!(
            auto_http3_origin("https://[2001:4860:4860::8888]/repo.git")
                .expect("public v6 origin")
                .expect("public v6 eligible"),
            Http3Origin {
                scheme: "https".to_owned(),
                host: "2001:4860:4860::8888".into(),
                port: 443,
            }
        );
    }

    #[test]
    fn auto_http3_failure_cache_round_trips_recent_failures() {
        let origin = Http3Origin {
            scheme: "https".to_owned(),
            host: "cache-round-trip.example.test".into(),
            port: 443,
        };
        let path = auto_http3_failure_cache_path(&origin);
        let _ = fs::remove_file(&path);
        assert!(!auto_http3_failed_recently(&origin));

        record_auto_http3_failure(&origin).expect("record failure");

        assert!(auto_http3_failed_recently(&origin));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn batch_auto_origin_routes_recent_failed_https_through_fallback_state() {
        let origin = Http3Origin {
            scheme: "https".to_owned(),
            host: format!("auto-origin-precheck-{}-b.example.test", std::process::id()),
            port: 443,
        };
        let path = auto_http3_failure_cache_path(&origin);
        let url = format!("https://{}/repo.git/info/refs", origin.host);
        let _ = fs::remove_file(&path);
        record_auto_http3_failure(&origin).expect("record failure");

        let mut state = AutoHttp3BatchState::new(None, None, None, false);
        let candidate = state.auto_candidate(&url).expect("origin precheck");

        assert_eq!(
            candidate.as_ref().map(|candidate| &candidate.origin),
            Some(&origin)
        );
        assert_eq!(
            candidate.as_ref().map(|candidate| candidate.uri.path()),
            Some("/repo.git/info/refs")
        );
        assert!(http3_origin_set_contains(&state.failed_origins, &origin));
        assert!(state.origin_cache.as_ref().map_or(true, HashMap::is_empty));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn batch_auto_origin_skips_local_hosts_even_with_failure_cache() {
        let origin = Http3Origin {
            scheme: "https".to_owned(),
            host: "localhost".to_owned(),
            port: 443,
        };
        let path = auto_http3_failure_cache_path(&origin);
        let _ = fs::remove_file(&path);
        record_auto_http3_failure(&origin).expect("record failure");

        let mut state = AutoHttp3BatchState::new(None, None, None, false);
        let first = state
            .auto_candidate("https://localhost/repo.git/info/refs")
            .expect("first origin precheck");
        let _ = fs::remove_file(&path);
        let second = state
            .auto_candidate("https://localhost/repo.git/git-upload-pack")
            .expect("second origin precheck");

        assert!(first.is_none());
        assert!(second.is_none());
        assert!(!http3_origin_set_contains(&state.failed_origins, &origin));
        assert!(state.origin_cache.as_ref().map_or(true, HashMap::is_empty));
    }

    #[test]
    fn batch_auto_origin_uses_positive_memory_cache_before_failure_cache_file() {
        let origin = Http3Origin {
            scheme: "https".to_owned(),
            host: "positive-cache.example.test".to_owned(),
            port: 443,
        };
        let path = auto_http3_failure_cache_path(&origin);
        let _ = fs::remove_file(&path);

        let mut state = AutoHttp3BatchState::new(None, None, None, false);
        let first = state
            .auto_candidate("https://positive-cache.example.test/repo.git/info/refs")
            .expect("first origin precheck");
        record_auto_http3_failure(&origin).expect("record failure after positive cache");
        let second = state
            .auto_candidate("https://positive-cache.example.test/repo.git/git-upload-pack")
            .expect("second origin precheck");

        assert_eq!(
            first.as_ref().map(|candidate| &candidate.origin),
            Some(&origin)
        );
        assert_eq!(
            second.as_ref().map(|candidate| &candidate.origin),
            Some(&origin)
        );
        assert!(!http3_origin_set_contains(&state.failed_origins, &origin));
        assert_eq!(
            http3_origin_cache_get(&state.origin_cache, &origin),
            Some(true)
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn auto_http3_failure_cache_ignores_stale_and_malformed_entries() {
        let origin = Http3Origin {
            scheme: "https".to_owned(),
            host: "cache-stale.example.test".into(),
            port: 443,
        };
        let path = auto_http3_failure_cache_path(&origin);
        let _ = fs::remove_file(&path);

        let stale_secs = current_unix_secs().expect("now") - AUTO_HTTP3_FAILURE_CACHE_TTL_SECS - 1;
        fs::write(&path, format!("{stale_secs}\n")).expect("write stale");
        assert!(!auto_http3_failed_recently(&origin));
        assert!(!path.exists());

        fs::write(&path, "not-a-timestamp\n").expect("write malformed");
        assert!(!auto_http3_failed_recently(&origin));
        assert!(!path.exists());

        fs::write(&path, b"123456789012345678901234567890123\n").expect("write oversized");
        assert!(!auto_http3_failed_recently(&origin));
        assert!(!path.exists());
    }

    #[test]
    fn auto_http3_failure_cache_decimal_writer_uses_line_bytes() {
        let mut out = Vec::new();
        write_decimal_u64(&mut out, 42).expect("write decimal");
        write_decimal_u64_line(&mut out, 0).expect("write zero");
        write_decimal_u64_line(&mut out, 12_345).expect("write decimal");

        assert_eq!(out, b"420\n12345\n");
    }

    #[test]
    fn auto_http3_failure_cache_write_replaces_existing_file_atomically() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("failure.cache");
        fs::write(&path, b"stale").expect("write stale cache");

        write_auto_http3_failure_cache_secs(&path, 12_345).expect("write failure cache");

        assert_eq!(
            read_auto_http3_failure_cache_secs(&path).expect("read failure cache"),
            Some(12_345)
        );
        let leftovers = fs::read_dir(dir.path())
            .expect("read temp dir")
            .map(|entry| entry.expect("dir entry").path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("tmp"))
            .collect::<Vec<_>>();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn auto_http3_failure_cache_file_name_encodes_host_without_intermediate_string() {
        let origin = Http3Origin {
            scheme: "https".to_owned(),
            host: "azAZ09-.".into(),
            port: 443,
        };

        assert_eq!(
            auto_http3_failure_cache_file_name(&origin),
            "skron-http3-failed-617a415a30392d2e-443.cache"
        );
    }

    #[test]
    fn decimal_u16_appender_handles_port_bounds() {
        let mut out = String::new();
        push_decimal_u16(&mut out, 0);
        out.push(',');
        push_decimal_u16(&mut out, u16::MAX);

        assert_eq!(out, "0,65535");
    }

    #[test]
    fn batch_auto_request_marks_h2_failed_for_http11_fallback() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _addr) = listener.accept().unwrap();
                let mut request = [0_u8; 1024];
                let mut request_len = 0_usize;
                loop {
                    let n = stream.read(&mut request[request_len..]).unwrap();
                    request_len += n;
                    if request
                        .windows(4)
                        .take(request_len.saturating_sub(3))
                        .any(|window| window == b"\r\n\r\n")
                    {
                        break;
                    }
                }
                let _ = request;
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    )
                    .unwrap();
                stream.flush().unwrap();
            }
        });

        let mut state = AutoHttp3BatchState::new(None, None, None, false);
        let origin = Http3Origin {
            scheme: "http".to_owned(),
            host: "example-fallback-h2-fail.test".into(),
            port: 443,
        };
        let request_url = format!("http://127.0.0.1:{port}/info/refs");
        let uri: http::Uri = request_url.parse().expect("uri");
        let request = TransportRequest {
            method: http::Method::GET,
            url: request_url.clone(),
            headers: Vec::new(),
            body: RequestBody::Empty,
            output_file: None,
        };
        let mut response_body_buffer = [0_u8; STREAM_BUFFER_SIZE];
        let mut client = None;
        let mut http1_client = None;
        let mut direct_http1_pool = None;
        let mut direct_http1_identities = None;
        let mut direct_http1_proxy_free = Some(false);
        let args = Args {
            http_version: HttpVersion::Auto,
            pool_idle_timeout_secs: 90,
            pool_max_idle_per_host: 8,
            method: None,
            url: None,
            headers: Vec::new(),
            body_file: None,
            output_file: None,
            ca_file: None,
            client_cert_file: None,
            client_key_file: None,
            tls_no_verify: false,
            batch: false,
        };

        let response = state
            .request(
                &mut client,
                &mut http1_client,
                &mut direct_http1_pool,
                &mut direct_http1_identities,
                &mut direct_http1_proxy_free,
                &args,
                AutoHttp3Candidate {
                    origin: origin.clone(),
                    uri,
                },
                request,
                &mut response_body_buffer,
            )
            .expect("request should succeed");

        assert_eq!(response.version, "1.1");
        assert!(http3_origin_set_contains(&state.h2_failed_origins, &origin));
        assert!(direct_http1_pool.is_none());

        let direct_response = state
            .request(
                &mut client,
                &mut http1_client,
                &mut direct_http1_pool,
                &mut direct_http1_identities,
                &mut direct_http1_proxy_free,
                &args,
                AutoHttp3Candidate {
                    origin: origin.clone(),
                    uri: request_url.parse().expect("uri"),
                },
                TransportRequest {
                    method: http::Method::GET,
                    url: request_url,
                    headers: Vec::new(),
                    body: RequestBody::Empty,
                    output_file: None,
                },
                &mut response_body_buffer,
            )
            .expect("direct fallback request should succeed");

        assert_eq!(direct_response.version, "1.1");
        assert!(direct_http1_pool.is_some());
        assert!(
            direct_http1_pool
                .as_ref()
                .is_some_and(|pool| pool.tls_config.is_none())
        );
        assert!(http1_client.is_none());

        server.join().unwrap();
    }

    #[test]
    fn request_body_try_clone_preserves_chain_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("body.bin");
        fs::write(&path, b"world").expect("write body");
        let file = fs::File::open(&path).expect("open body");
        let prefix = Bytes::from_static(b"hello ");
        let prefix_ptr = prefix.as_ptr();
        let body = RequestBody::Chain {
            prefix,
            file,
            file_len: 5,
        };
        let mut cloned = body.try_clone().expect("clone request body");
        match &cloned {
            RequestBody::Chain { prefix, .. } => assert_eq!(prefix.as_ptr(), prefix_ptr),
            _ => panic!("expected chained body"),
        }
        let mut output = Vec::new();
        let mut buffer = [0_u8; STREAM_BUFFER_SIZE];
        cloned
            .write_to_with_buffer(&mut output, &mut buffer)
            .expect("serialize cloned body");
        assert_eq!(output, b"hello world");
    }
}
