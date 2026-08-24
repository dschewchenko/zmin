mod common;

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufWriter, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
#[cfg(any(unix, windows))]
use std::sync::mpsc::{Receiver, sync_channel};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
#[cfg(unix)]
use std::thread::JoinHandle;
#[cfg(windows)]
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use common::zmin_bin;
use tempfile::TempDir;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(60);
const CLIENT_READER_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CLIENT_OUTPUT_BYTES: usize = 1024 * 1024;
const CLIENT_CAPTURE_CHUNK_BYTES: usize = 8192;
const CLIENT_CAPTURE_EXIT_GRACE: Duration = Duration::from_millis(100);
const MAX_CLIENT_DIAGNOSTIC_BYTES: usize = 4096;
const CLIENT_FINAL_REAP_GRACE: Duration = Duration::from_millis(250);
const CLIENT_REGRESSION_TIMEOUT: Duration = Duration::from_millis(250);
const PACK_SIDEBAND_CHUNK: usize = 65_530;
const MAX_FIXTURE_REQUEST_HEADER_BYTES: usize = 256 * 1024;
const MAX_FIXTURE_REQUEST_BODY_BYTES: usize = 1024 * 1024;
const MAX_FIXTURE_REQUEST_RECORDS: usize = 128;
const MAX_FIXTURE_CONNECTIONS: usize = 128;
const FIXTURE_LIFECYCLE_STRESS_ITERATIONS: usize = 16;
const PINNED_HTTP_BUNDLE_PATH: &str = "/Users/dschewchenko/.cache/zmin/git-upstream/http-bundle-v2.55.0-e9019fcafe0040228b8631c30f97ae1adb61bcdc-Darwin-arm64-with-http-fetch-pinned";
const PINNED_HTTP_FETCH_SHA256: &str =
    "fc2e8b9e47cafb39140ca90f56fbc6cb09912c6d715feb020157733868f08b0e";
const PINNED_MANIFEST_SHA256: &str =
    "e70ca5308dbddac10ac951a0a16a31645f9a7f4338be026e4257d961b3eb29ad";
const PINNED_BUNDLE_SIDECAR_SHA256: &str =
    "cc295dc42051d204e2505acfab55d43767260fdd0cb016c6e5d3f507cf74bfdb";
const PINNED_GIT_SHA256: &str = "ca63eda87df1aaffa2b80710c4a9de6212eba6c84e8dfb3011a2498b36e841cb";
static HERMETIC_ENV_COUNTER: AtomicU64 = AtomicU64::new(0);

struct HermeticGitChildEnvironment;

impl HermeticGitChildEnvironment {
    fn apply(command: &mut Command, cwd: &Path) {
        let sequence = HERMETIC_ENV_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = cwd.join(format!(".git-uri-env-{}-{sequence}", std::process::id()));
        let home = root.join("h");
        let xdg = root.join("x");
        let template = root.join("t");
        fs::create_dir_all(&home).expect("create hermetic HOME");
        fs::create_dir_all(xdg.join("config")).expect("create hermetic XDG config");
        fs::create_dir_all(xdg.join("cache")).expect("create hermetic XDG cache");
        fs::create_dir_all(xdg.join("data")).expect("create hermetic XDG data");
        fs::create_dir_all(&template).expect("create empty hermetic template directory");
        fs::create_dir_all(root.join("gpg")).expect("create hermetic GnuPG directory");

        command
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .env("XDG_DATA_HOME", xdg.join("data"))
            .env("GIT_CONFIG_GLOBAL", null_device())
            .env("GIT_CONFIG_SYSTEM", null_device())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TEMPLATE_DIR", &template)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Pack URI Test")
            .env("GIT_AUTHOR_EMAIL", "pack-uri@example.test")
            .env("GIT_AUTHOR_DATE", "1700000000 +0000")
            .env("GIT_COMMITTER_NAME", "Pack URI Test")
            .env("GIT_COMMITTER_EMAIL", "pack-uri@example.test")
            .env("GIT_COMMITTER_DATE", "1700000000 +0000")
            .env("GNUPGHOME", root.join("gpg"))
            .env("PATH", hermetic_system_path());

        for key in std::env::vars_os().map(|(key, _)| key).filter(|key| {
            let key = key.to_string_lossy();
            key == "GIT_CONFIG_PARAMETERS"
                || key == "GIT_CONFIG_COUNT"
                || key.starts_with("GIT_CONFIG_KEY_")
                || key.starts_with("GIT_CONFIG_VALUE_")
                || key.starts_with("ZMIN_")
        }) {
            command.env_remove(key);
        }
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_OBJECT_DIRECTORY",
            "GIT_OBJECT_DIRECTORY_RELATIVE",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_INDEX_FILE",
            "GIT_NAMESPACE",
            "GIT_CEILING_DIRECTORIES",
            "GIT_DISCOVERY_ACROSS_FILESYSTEM",
            "GIT_NO_REPLACE_OBJECTS",
            "GIT_REPLACE_REF_BASE",
            "GIT_QUARANTINE_PATH",
            "GIT_TRACE",
            "GIT_TRACE2",
            "GIT_TRACE2_EVENT",
            "GIT_TRACE2_PERF",
        ] {
            command.env_remove(key);
        }
    }
}

fn null_device() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

fn hermetic_system_path() -> String {
    std::env::join_paths(["/usr/bin", "/bin", "/usr/sbin", "/sbin"])
        .expect("construct hermetic system PATH")
        .to_string_lossy()
        .into_owned()
}

fn pinned_stock_git_path() -> PathBuf {
    let expected = PathBuf::from(PINNED_HTTP_BUNDLE_PATH).join("git");
    let configured = PathBuf::from(
        std::env::var_os("ZMIN_STOCK_GIT")
            .expect("ZMIN_STOCK_GIT must select the fixed Git comparator"),
    );
    assert_eq!(configured, expected, "E2E must use the bundle-bound Git");
    expected
}

fn hermetic_pinned_git_command(cwd: &Path) -> Command {
    let mut command = Command::new(pinned_stock_git_path());
    HermeticGitChildEnvironment::apply(&mut command, cwd);
    command.current_dir(cwd);
    command
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObjectFormat {
    Sha1,
    Sha256,
}

impl ObjectFormat {
    fn git_name(self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
        }
    }

    fn hex_len(self) -> usize {
        match self {
            Self::Sha1 => 40,
            Self::Sha256 => 64,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RefFormat {
    Files,
    Reftable,
}

impl RefFormat {
    fn git_name(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Reftable => "reftable",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UriFailure {
    None,
    NotFound,
    Truncated,
    Corrupt,
    WrongHash,
    SidebandFatal,
}

#[derive(Clone, Debug)]
struct PackSpec {
    path: String,
    hash: String,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
struct RemoteFixture {
    format: ObjectFormat,
    head: String,
    tree: String,
    blob_ids: Vec<String>,
    plain_inline: PackSpec,
    inline: PackSpec,
    uris: Vec<PackSpec>,
    filtered_inline: PackSpec,
    filtered_uris: Vec<PackSpec>,
}

#[derive(Clone, Debug)]
struct FixtureConfig {
    remote: RemoteFixture,
    advertise_packfile_uris: bool,
    failure: UriFailure,
    cross_origin_uri_base: Option<String>,
    redirect_uri_base: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RequestRecord {
    method: String,
    path: String,
    headers: String,
    body: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
struct RequestLog {
    records: Vec<RequestRecord>,
}

struct FixtureRequestProgress {
    accepted: AtomicUsize,
    recorded: AtomicUsize,
    wait_lock: Mutex<()>,
    wait_signal: Condvar,
}

impl FixtureRequestProgress {
    fn new() -> Self {
        Self {
            accepted: AtomicUsize::new(0),
            recorded: AtomicUsize::new(0),
            wait_lock: Mutex::new(()),
            wait_signal: Condvar::new(),
        }
    }

    fn note_accepted(&self) {
        self.accepted.fetch_add(1, Ordering::AcqRel);
        self.wait_signal.notify_all();
    }

    fn note_recorded(&self) {
        self.recorded.fetch_add(1, Ordering::AcqRel);
        self.wait_signal.notify_all();
    }

    fn wait_for(&self, expected: usize) {
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let mut wait_lock = self.wait_lock.lock().expect("fixture progress lock");
        while self.accepted.load(Ordering::Acquire) < expected
            || self.recorded.load(Ordering::Acquire) < expected
        {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "fixture request barrier timed out: accepted={}, recorded={}, expected={expected}",
                self.accepted.load(Ordering::Acquire),
                self.recorded.load(Ordering::Acquire)
            );
            let (next, timeout) = self
                .wait_signal
                .wait_timeout(wait_lock, remaining)
                .expect("wait for fixture request progress");
            wait_lock = next;
            assert!(
                !timeout.timed_out(),
                "fixture request barrier timed out: accepted={}, recorded={}, expected={expected}",
                self.accepted.load(Ordering::Acquire),
                self.recorded.load(Ordering::Acquire)
            );
        }
    }
}

struct FixtureWorkerTracker {
    active: AtomicUsize,
    cancelled: std::sync::atomic::AtomicBool,
    sockets: Mutex<Vec<TcpStream>>,
    done_lock: Mutex<()>,
    done_signal: Condvar,
}

impl FixtureWorkerTracker {
    fn new() -> Self {
        Self {
            active: AtomicUsize::new(0),
            cancelled: std::sync::atomic::AtomicBool::new(false),
            sockets: Mutex::new(Vec::new()),
            done_lock: Mutex::new(()),
            done_signal: Condvar::new(),
        }
    }

    fn register_socket(&self, stream: &TcpStream) -> bool {
        let mut sockets = self.sockets.lock().expect("fixture socket lock");
        if self.cancelled.load(Ordering::Acquire) {
            let _ = stream.shutdown(Shutdown::Both);
            return false;
        }
        sockets.push(stream.try_clone().expect("clone fixture worker socket"));
        true
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let sockets = self.sockets.lock().expect("fixture socket lock");
        for socket in sockets.iter() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }

    fn wait_for_quiescence(&self) {
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let mut done_lock = self.done_lock.lock().expect("fixture worker done lock");
        while self.active.load(Ordering::Acquire) != 0 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "fixture worker quiescence timed out with {} active workers",
                self.active.load(Ordering::Acquire)
            );
            let (next, timeout) = self
                .done_signal
                .wait_timeout(done_lock, remaining)
                .expect("wait for fixture worker quiescence");
            done_lock = next;
            assert!(
                !timeout.timed_out(),
                "fixture worker quiescence timed out with {} active workers",
                self.active.load(Ordering::Acquire)
            );
        }
    }
}

struct FixtureWorkerGuard {
    tracker: Arc<FixtureWorkerTracker>,
}

impl Drop for FixtureWorkerGuard {
    fn drop(&mut self) {
        self.tracker.active.fetch_sub(1, Ordering::AcqRel);
        self.tracker.done_signal.notify_all();
    }
}

struct FixtureHttpServer {
    port: u16,
    log: Arc<Mutex<RequestLog>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    connections: Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
    workers: Arc<FixtureWorkerTracker>,
    progress: Arc<FixtureRequestProgress>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl FixtureHttpServer {
    fn new(config: FixtureConfig) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind packfile URI fixture");
        listener
            .set_nonblocking(true)
            .expect("set fixture listener nonblocking");
        let port = listener
            .local_addr()
            .expect("read fixture listener address")
            .port();
        let log = Arc::new(Mutex::new(RequestLog::default()));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let connections = Arc::new(Mutex::new(Vec::new()));
        let workers = Arc::new(FixtureWorkerTracker::new());
        let progress = Arc::new(FixtureRequestProgress::new());
        let readiness = Arc::new((Mutex::new(false), Condvar::new()));
        let thread_log = log.clone();
        let thread_stop = stop.clone();
        let thread_connections = connections.clone();
        let thread_workers = workers.clone();
        let thread_progress = progress.clone();
        let thread_readiness = readiness.clone();
        let handle = std::thread::spawn(move || {
            let (ready_lock, ready_signal) = &*thread_readiness;
            *ready_lock.lock().expect("fixture readiness lock") = true;
            ready_signal.notify_all();
            while !thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        thread_progress.note_accepted();
                        let can_spawn = thread_connections
                            .lock()
                            .expect("fixture connection lock")
                            .len()
                            < MAX_FIXTURE_CONNECTIONS;
                        if !can_spawn {
                            let _ = stream.shutdown(Shutdown::Both);
                            continue;
                        }
                        if !thread_workers.register_socket(&stream) {
                            continue;
                        }
                        thread_workers.active.fetch_add(1, Ordering::AcqRel);
                        let config = config.clone();
                        let log = thread_log.clone();
                        let progress = thread_progress.clone();
                        let worker_guard = FixtureWorkerGuard {
                            tracker: thread_workers.clone(),
                        };
                        let connection = std::thread::spawn(move || {
                            let _worker_guard = worker_guard;
                            serve_fixture_connection(port, config, log, progress, &mut stream);
                        });
                        thread_connections
                            .lock()
                            .expect("fixture connection lock")
                            .push(connection);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) if thread_stop.load(std::sync::atomic::Ordering::Relaxed) => break,
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        let (ready_lock, ready_signal) = &*readiness;
        let mut ready = ready_lock.lock().expect("fixture readiness lock");
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        while !*ready {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "fixture accept loop readiness timed out"
            );
            let (next, timeout) = ready_signal
                .wait_timeout(ready, remaining)
                .expect("wait for fixture readiness");
            ready = next;
            assert!(
                !timeout.timed_out(),
                "fixture accept loop readiness timed out"
            );
        }
        Self {
            port,
            log,
            stop,
            connections,
            workers,
            progress,
            handle: Some(handle),
        }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/remote.git", self.port)
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn log(&self) -> RequestLog {
        self.workers.wait_for_quiescence();
        self.log.lock().expect("fixture request log lock").clone()
    }

    fn wait_for_requests(&self, expected: usize) {
        self.progress.wait_for(expected);
        self.workers.wait_for_quiescence();
    }
}

impl Drop for FixtureHttpServer {
    fn drop(&mut self) {
        self.workers.cancel();
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            assert!(handle.join().is_ok(), "fixture accept loop panicked");
        }
        self.workers.wait_for_quiescence();
        let connections = self
            .connections
            .lock()
            .expect("fixture connection lock")
            .drain(..)
            .collect::<Vec<_>>();
        for connection in connections {
            assert!(
                connection.join().is_ok(),
                "fixture connection worker panicked"
            );
        }
    }
}

fn serve_fixture_connection(
    port: u16,
    config: FixtureConfig,
    log: Arc<Mutex<RequestLog>>,
    progress: Arc<FixtureRequestProgress>,
    stream: &mut TcpStream,
) {
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .expect("set fixture read timeout");
    stream
        .set_write_timeout(Some(REQUEST_TIMEOUT))
        .expect("set fixture write timeout");
    let Some(request) = read_http_request(stream) else {
        return;
    };
    let mut request_log = log.lock().expect("fixture request log lock");
    if request_log.records.len() >= MAX_FIXTURE_REQUEST_RECORDS {
        let _ = stream.shutdown(Shutdown::Both);
        return;
    }
    request_log.records.push(RequestRecord {
        method: request.method.clone(),
        path: request.path.clone(),
        headers: request.headers.clone(),
        body: request.body.clone(),
    });
    progress.note_recorded();
    drop(request_log);
    let body = fixture_response(port, &config, &request);
    write_http_response(stream, body).expect("fixture response write");
}

#[derive(Clone, Debug)]
struct HttpRequest {
    method: String,
    path: String,
    headers: String,
    body: Vec<u8>,
}

fn read_http_request(stream: &mut TcpStream) -> Option<HttpRequest> {
    let mut bytes = Vec::with_capacity(MAX_FIXTURE_REQUEST_HEADER_BYTES);
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let remaining = MAX_FIXTURE_REQUEST_HEADER_BYTES.saturating_sub(bytes.len());
        if remaining == 0 {
            return None;
        }
        let read_limit = remaining.min(buffer.len());
        let read = stream.read(&mut buffer[..read_limit]).ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]).into_owned();
    let request_line = headers.lines().next()?;
    let mut request_parts = request_line.split_ascii_whitespace();
    let method = request_parts.next()?.to_owned();
    let path = request_parts.next()?.to_owned();
    let content_length = fixture_content_length(&headers)?;
    let mut body = Vec::with_capacity(content_length);
    body.extend_from_slice(&bytes[header_end + 4..]);
    while body.len() < content_length {
        let remaining = content_length - body.len();
        let read_limit = remaining.min(buffer.len());
        let read = stream.read(&mut buffer[..read_limit]).ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    body.truncate(content_length);
    Some(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn fixture_content_length(headers: &str) -> Option<usize> {
    let content_length = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find_map(|(name, value)| {
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    (content_length <= MAX_FIXTURE_REQUEST_BODY_BYTES).then_some(content_length)
}

fn fixture_response(port: u16, config: &FixtureConfig, request: &HttpRequest) -> HttpResponse {
    let path = request
        .path
        .split_once('?')
        .map_or(request.path.as_str(), |(path, _)| path);
    if request.method == "GET" && path.ends_with("/info/refs") {
        return HttpResponse::ok(
            b"application/x-git-upload-pack-advertisement",
            capability_advertisement(config.remote.format, config.advertise_packfile_uris),
        );
    }
    if request.method == "POST" && path.ends_with("/git-upload-pack") {
        if request
            .body
            .windows(b"command=ls-refs\n".len())
            .any(|window| window == b"command=ls-refs\n")
        {
            return HttpResponse::ok(
                b"application/x-git-upload-pack-result",
                ls_refs_response(&config.remote.head),
            );
        }
        let requested_uris = request
            .body
            .windows(b"packfile-uris ".len())
            .any(|window| window == b"packfile-uris ");
        let requested_sideband = request
            .body
            .windows(b"sideband-all".len())
            .any(|window| window == b"sideband-all");
        let requested_filter = request
            .body
            .windows(b"filter blob:none".len())
            .any(|window| window == b"filter blob:none");
        let include_uris = config.advertise_packfile_uris && requested_uris && requested_sideband;
        return fetch_response(
            port,
            config,
            include_uris,
            requested_sideband,
            requested_filter,
        );
    }
    if request.method == "GET" && path.contains("/uri/") {
        let Some(pack) = config
            .remote
            .uris
            .iter()
            .chain(config.remote.filtered_uris.iter())
            .find(|pack| path.ends_with(&pack.path))
        else {
            return HttpResponse::not_found();
        };
        if let Some(base) = config.redirect_uri_base.as_deref()
            && path.starts_with("/uri/")
        {
            let base = if base == "SELF" {
                format!("http://127.0.0.1:{port}/final")
            } else {
                base.to_owned()
            };
            return HttpResponse::redirect(format!("{base}{}", pack.path));
        }
        if config.failure == UriFailure::NotFound {
            return HttpResponse::not_found();
        }
        let mut bytes = pack.bytes.clone();
        if config.failure == UriFailure::Corrupt {
            let index = bytes.len() / 2;
            if let Some(byte) = bytes.get_mut(index) {
                *byte ^= 0x5a;
            }
        }
        if config.failure == UriFailure::Truncated {
            return HttpResponse::truncated(pack.bytes.len(), &bytes[..bytes.len() / 2]);
        }
        return HttpResponse::ok(b"application/x-git-packed-objects", bytes);
    }
    HttpResponse::not_found()
}

#[derive(Clone, Debug)]
struct HttpResponse {
    status: &'static str,
    content_type: &'static [u8],
    body: Vec<u8>,
    declared_length: Option<usize>,
    location: Option<String>,
}

impl HttpResponse {
    fn ok(content_type: &'static [u8], body: Vec<u8>) -> Self {
        Self {
            status: "200 OK",
            content_type,
            declared_length: Some(body.len()),
            body,
            location: None,
        }
    }

    fn not_found() -> Self {
        Self {
            status: "404 Not Found",
            content_type: b"text/plain",
            body: Vec::new(),
            declared_length: Some(0),
            location: None,
        }
    }

    fn truncated(declared_length: usize, body: &[u8]) -> Self {
        Self {
            status: "200 OK",
            content_type: b"application/x-git-packed-objects",
            body: body.to_vec(),
            declared_length: Some(declared_length),
            location: None,
        }
    }

    fn redirect(location: String) -> Self {
        Self {
            status: "302 Found",
            content_type: b"text/plain",
            body: Vec::new(),
            declared_length: Some(0),
            location: Some(location),
        }
    }
}

fn write_http_response(stream: &mut TcpStream, response: HttpResponse) -> std::io::Result<()> {
    let length = response.declared_length.unwrap_or(response.body.len());
    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        String::from_utf8_lossy(response.content_type),
        response
            .location
            .as_deref()
            .map_or(String::new(), |location| format!(
                "Location: {location}\r\n"
            )),
        length,
    );
    let mut writer = BufWriter::new(&mut *stream);
    writer.write_all(header.as_bytes())?;
    writer.write_all(&response.body)?;
    writer.flush()?;
    drop(writer);
    stream.shutdown(Shutdown::Write)
}

fn capability_advertisement(format: ObjectFormat, packfile_uris: bool) -> Vec<u8> {
    let mut body = Vec::new();
    append_pkt(&mut body, b"# service=git-upload-pack\n");
    body.extend_from_slice(b"0000");
    append_pkt(&mut body, b"version 2\n");
    append_pkt(&mut body, b"agent=git/2.55.0\n");
    append_pkt(&mut body, b"ls-refs=unborn\n");
    let mut fetch = b"fetch=shallow wait-for-done filter sideband-all".to_vec();
    if packfile_uris {
        fetch.extend_from_slice(b" packfile-uris");
    }
    fetch.push(b'\n');
    append_pkt(&mut body, &fetch);
    append_pkt(
        &mut body,
        format!("object-format={}\n", format.git_name()).as_bytes(),
    );
    body.extend_from_slice(b"0000");
    body
}

fn ls_refs_response(head: &str) -> Vec<u8> {
    let mut body = Vec::new();
    append_pkt(
        &mut body,
        format!("{head} HEAD symref-target:refs/heads/main\n").as_bytes(),
    );
    append_pkt(&mut body, format!("{head} refs/heads/main\n").as_bytes());
    body.extend_from_slice(b"0000");
    body
}

fn fetch_response(
    port: u16,
    config: &FixtureConfig,
    include_uris: bool,
    requested_sideband: bool,
    requested_filter: bool,
) -> HttpResponse {
    let mut body = Vec::new();
    if config.failure == UriFailure::SidebandFatal {
        append_sideband_pkt(&mut body, 2, b"fixture progress\n");
        append_sideband_pkt(&mut body, 3, b"fixture fatal\n");
        body.extend_from_slice(b"0000");
        return HttpResponse::ok(b"application/x-git-upload-pack-result", body);
    }
    if include_uris {
        append_sideband_pkt(&mut body, 2, b"fixture progress\n");
        append_sideband_pkt(&mut body, 1, b"packfile-uris\n");
        let uri_packs = if requested_filter {
            &config.remote.filtered_uris
        } else {
            &config.remote.uris
        };
        for (index, pack) in uri_packs.iter().enumerate() {
            let uri = config.cross_origin_uri_base.as_deref().map_or_else(
                || format!("http://127.0.0.1:{port}{}", pack.path),
                |base| format!("{base}{}", pack.path),
            );
            let hash = if config.failure == UriFailure::WrongHash && index == 0 {
                "0".repeat(config.remote.format.hex_len())
            } else {
                pack.hash.clone()
            };
            append_sideband_pkt(&mut body, 1, format!("{hash} {uri}\n").as_bytes());
        }
        body.extend_from_slice(b"0001");
    }
    if requested_sideband {
        append_sideband_pkt(&mut body, 1, b"packfile\n");
    } else {
        append_pkt(&mut body, b"packfile\n");
    }
    let inline = if requested_filter {
        &config.remote.filtered_inline
    } else if include_uris {
        &config.remote.inline
    } else {
        &config.remote.plain_inline
    };
    for chunk in inline.bytes.chunks(PACK_SIDEBAND_CHUNK) {
        append_sideband_pkt(&mut body, 1, chunk);
    }
    body.extend_from_slice(b"0000");
    HttpResponse::ok(b"application/x-git-upload-pack-result", body)
}

fn append_pkt(out: &mut Vec<u8>, payload: &[u8]) {
    let length = payload.len() + 4;
    assert!(length <= 0xffff, "pkt-line payload too large");
    out.extend_from_slice(format!("{length:04x}").as_bytes());
    out.extend_from_slice(payload);
}

fn append_sideband_pkt(out: &mut Vec<u8>, band: u8, payload: &[u8]) {
    let mut packet = Vec::with_capacity(payload.len() + 1);
    packet.push(band);
    packet.extend_from_slice(payload);
    append_pkt(out, &packet);
}

fn prepare_remote(root: &Path, format: ObjectFormat, uri_count: usize) -> RemoteFixture {
    let _ = validated_http_bundle();
    let remote = root.join("source");
    fs::create_dir_all(&remote).expect("create source repository");
    let mut init = stock_command(&remote, ["init", "--quiet", "--initial-branch=main"]);
    if format == ObjectFormat::Sha256 {
        init = stock_command(
            &remote,
            [
                "init",
                "--quiet",
                "--object-format=sha256",
                "--initial-branch=main",
            ],
        );
    }
    assert_success(
        init.output().expect("init source repository"),
        "init source",
    );
    stock_success(&remote, &["config", "user.name", "Pack URI Test"]);
    stock_success(&remote, &["config", "user.email", "pack-uri@example.test"]);
    fs::write(remote.join("one.txt"), b"one\n").expect("write first source blob");
    fs::write(remote.join("two.txt"), b"two\n").expect("write second source blob");
    stock_success(&remote, &["add", "one.txt", "two.txt"]);
    stock_success_with_env(
        &remote,
        &["commit", "--quiet", "-m", "pack-uri"],
        &[
            ("GIT_AUTHOR_NAME", "Pack URI Test"),
            ("GIT_AUTHOR_EMAIL", "pack-uri@example.test"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Pack URI Test"),
            ("GIT_COMMITTER_EMAIL", "pack-uri@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
    );
    let head = stock_text(&remote, &["rev-parse", "HEAD"]);
    let tree = stock_text(&remote, &["rev-parse", "HEAD^{tree}"]);
    let one = stock_text(&remote, &["rev-parse", "HEAD:one.txt"]);
    let two = stock_text(&remote, &["rev-parse", "HEAD:two.txt"]);
    let plain_inline = make_pack(&remote, "plain-inline.pack", &[&head, &tree, &one, &two]);
    let inline = make_pack(&remote, "inline.pack", &[&head, &tree]);
    let uri_objects = if uri_count == 1 {
        vec![vec![&one, &two]]
    } else {
        vec![vec![&one], vec![&two]]
    };
    let uris = uri_objects
        .iter()
        .enumerate()
        .map(|(index, objects)| make_pack(&remote, &format!("uri-{index}.pack"), objects))
        .collect();
    let filtered_inline = make_pack(&remote, "filtered-inline.pack", &[&head]);
    let filtered_uris = vec![make_pack(&remote, "filtered-uri-0.pack", &[&tree])];
    RemoteFixture {
        format,
        head,
        tree,
        blob_ids: vec![one, two],
        plain_inline,
        inline,
        uris,
        filtered_inline,
        filtered_uris,
    }
}

fn make_pack(root: &Path, name: &str, object_ids: &[&String]) -> PackSpec {
    let input = object_ids
        .iter()
        .map(|id| format!("{id}\n"))
        .collect::<String>();
    let output = stock_command_with_input(root, &["pack-objects", "--stdout"], input.as_bytes());
    let output = assert_success(output, "make pack");
    let pack_path = root.join(name);
    fs::write(&pack_path, &output.stdout).expect("write fixture pack");
    let hash = stock_text(root, &["index-pack", name]);
    let bytes = fs::read(&pack_path).expect("read fixture pack");
    PackSpec {
        path: format!("/uri/{name}"),
        hash,
        bytes,
    }
}

fn stock_command<'a, const N: usize>(cwd: &Path, args: [&'a str; N]) -> Command {
    let mut command = hermetic_pinned_git_command(cwd);
    command.args(args).env("GIT_TERMINAL_PROMPT", "0");
    command
}

fn stock_command_with_input(cwd: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut command = hermetic_pinned_git_command(cwd);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0");
    let mut child = command.spawn().expect("spawn pinned Git");
    child
        .stdin
        .take()
        .expect("pinned Git stdin")
        .write_all(input)
        .expect("write pinned Git stdin");
    child.wait_with_output().expect("wait pinned Git")
}

fn stock_success(cwd: &Path, args: &[&str]) {
    assert_success(
        stock_command_with_input(cwd, args, &[]),
        "pinned Git command",
    );
}

fn stock_success_with_env(cwd: &Path, args: &[&str], env: &[(&str, &str)]) {
    let mut command = hermetic_pinned_git_command(cwd);
    command.args(args).env("GIT_TERMINAL_PROMPT", "0");
    command.envs(env.iter().copied());
    assert_success(
        command.output().expect("run pinned Git command"),
        "pinned Git command",
    );
}

fn stock_text(cwd: &Path, args: &[&str]) -> String {
    let output = stock_command_with_input(cwd, args, &[]);
    let output = assert_success(output, &format!("pinned Git text command {args:?}"));
    String::from_utf8(output.stdout)
        .expect("pinned Git output UTF-8")
        .trim()
        .to_owned()
}

fn assert_success(output: Output, label: &str) -> Output {
    assert!(
        output.status.success(),
        "{label} failed: {}",
        redact_sensitive_diagnostics(&String::from_utf8_lossy(&output.stderr))
    );
    output
}

const REDACTED_DIAGNOSTIC: &str = "<redacted>";

fn redacted_command_display(command: &Command) -> String {
    redact_sensitive_diagnostics(&format!("{command:?}"))
}

fn redact_sensitive_diagnostics(input: &str) -> String {
    let mut output = input.to_owned();
    for value in [
        "origin-user",
        "origin-pass",
        "origin-secret",
        "origin-cookie",
        "origin-proxy",
        "cdn-auth",
        "cdn-cookie",
        "cdn-proxy",
        "cdn-secret",
        "fixture-secret",
        "top-secret",
    ] {
        output = output.replace(value, REDACTED_DIAGNOSTIC);
    }
    output = redact_sensitive_header_values(&output);
    redact_url_userinfo(&output)
}

fn redact_sensitive_header_values(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let header_names = [
        "authorization:",
        "proxy-authorization:",
        "cookie:",
        "x-origin-secret:",
        "x-cdn-secret:",
    ];
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        let Some((index, header_name)) = header_names
            .iter()
            .filter_map(|header_name| {
                lower[cursor..]
                    .find(header_name)
                    .map(|offset| (cursor + offset, *header_name))
            })
            .min_by_key(|(index, _)| *index)
        else {
            output.push_str(&input[cursor..]);
            break;
        };
        let value_start = index + header_name.len();
        output.push_str(&input[cursor..value_start]);
        let value_end = input[value_start..]
            .find(['"', '\'', '\\', '\r', '\n'])
            .map_or(input.len(), |offset| value_start + offset);
        output.push_str(REDACTED_DIAGNOSTIC);
        cursor = value_end;
    }
    output
}

fn redact_url_userinfo(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative_scheme_end) = lower[cursor..].find("://") {
        let scheme_start = cursor + relative_scheme_end;
        let authority_start = scheme_start + 3;
        let authority_end = input[authority_start..]
            .find(|character: char| {
                character == '/'
                    || character.is_ascii_whitespace()
                    || character == '"'
                    || character == '\''
                    || character == '\\'
            })
            .map_or(input.len(), |offset| authority_start + offset);
        output.push_str(&input[cursor..authority_start]);
        let authority = &input[authority_start..authority_end];
        if let Some(at) = authority.rfind('@') {
            output.push_str(REDACTED_DIAGNOSTIC);
            output.push('@');
            output.push_str(&authority[at + 1..]);
        } else {
            output.push_str(authority);
        }
        cursor = authority_end;
    }
    output.push_str(&input[cursor..]);
    output
}

#[test]
fn fixture_request_body_limit_rejects_oversized_content_length() {
    let accepted = format!(
        "POST /git-upload-pack HTTP/1.1\r\nContent-Length: {MAX_FIXTURE_REQUEST_BODY_BYTES}\r\n"
    );
    let rejected = format!(
        "POST /git-upload-pack HTTP/1.1\r\nContent-Length: {}\r\n",
        MAX_FIXTURE_REQUEST_BODY_BYTES + 1
    );
    assert_eq!(
        fixture_content_length(&accepted),
        Some(MAX_FIXTURE_REQUEST_BODY_BYTES)
    );
    assert_eq!(fixture_content_length(&rejected), None);
    assert!(MAX_FIXTURE_REQUEST_RECORDS > 0);
    assert!(MAX_FIXTURE_CONNECTIONS > 0);
}

#[test]
fn fixture_server_flushes_and_joins_bounded_workers() {
    let root = TempDir::new().expect("fixture lifecycle root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha1, 1);
    for iteration in 0..FIXTURE_LIFECYCLE_STRESS_ITERATIONS {
        let server = FixtureHttpServer::new(FixtureConfig {
            remote: remote.clone(),
            advertise_packfile_uris: false,
            failure: UriFailure::None,
            cross_origin_uri_base: None,
            redirect_uri_base: None,
        });
        let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap_or_else(|error| {
            panic!("connect lifecycle fixture at iteration {iteration}: {error}")
        });
        stream
            .set_read_timeout(Some(REQUEST_TIMEOUT))
            .expect("set lifecycle fixture read timeout");
        stream
            .write_all(
                format!(
                    "GET /remote.git/info/refs?service=git-upload-pack HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
                    server.port
                )
                .as_bytes(),
            )
            .expect("write lifecycle fixture request");
        stream
            .shutdown(Shutdown::Write)
            .expect("shutdown lifecycle fixture request");
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .expect("read lifecycle fixture response");
        assert!(
            response.starts_with(b"HTTP/1.1 200 OK\r\n"),
            "unexpected lifecycle fixture response at iteration {iteration}"
        );
        server.wait_for_requests(1);
        let log = server.log();
        assert_eq!(
            log.records.len(),
            1,
            "lifecycle fixture log at iteration {iteration}"
        );
    }
}

fn panic_payload_text(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => match payload.downcast::<&str>() {
            Ok(message) => (*message).to_owned(),
            Err(_) => "non-string panic payload".to_owned(),
        },
    }
}

#[cfg(unix)]
fn assert_client_capture_failure_is_bounded(command: Command, marker: &str) {
    let started = Instant::now();
    let result = catch_unwind(AssertUnwindSafe(|| {
        run_client_command(command, "capture regression")
    }));
    let elapsed = started.elapsed();
    assert!(result.is_err(), "capture regression unexpectedly succeeded");
    assert!(
        elapsed < Duration::from_secs(10),
        "capture regression exceeded bound: {elapsed:?}"
    );
    let message = panic_payload_text(result.expect_err("capture regression panic"));
    assert!(
        message.contains(marker),
        "diagnostic missing {marker}: {message}"
    );
    for secret in ["fixture-secret", "origin-pass", "cdn-secret"] {
        assert!(
            !message.contains(secret),
            "diagnostic leaked {secret}: {message}"
        );
    }
}

#[cfg(unix)]
#[test]
fn client_output_capture_caps_infinite_stdout_and_stderr() {
    let mut stdout = Command::new("/bin/sh");
    stdout.args(["-c", "yes fixture-secret"]);
    assert_client_capture_failure_is_bounded(stdout, "exceeded");

    let mut stderr = Command::new("/bin/sh");
    stderr.args(["-c", "yes fixture-secret >&2"]);
    assert_client_capture_failure_is_bounded(stderr, "exceeded");
}

#[cfg(unix)]
#[test]
fn client_cleanup_kills_descendants_holding_output_pipes() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "(sleep 30) & exit 0"]);
    assert_client_capture_failure_is_bounded(command, "capture failed");
}

#[cfg(unix)]
#[test]
fn client_timeout_terminates_process_group_with_bounded_reap() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "sleep 30"]);
    let started = Instant::now();
    let result = catch_unwind(AssertUnwindSafe(|| {
        run_client_command_with_timeout(command, "timeout regression", CLIENT_REGRESSION_TIMEOUT)
    }));
    assert!(result.is_err(), "timeout regression unexpectedly succeeded");
    assert!(started.elapsed() < Duration::from_secs(3));
    let message = panic_payload_text(result.expect_err("timeout regression panic"));
    assert!(
        message.contains("timed out"),
        "diagnostic missing timeout: {message}"
    );
    assert!(
        !message.contains("fixture-secret"),
        "diagnostic leaked a secret"
    );
}

#[cfg(unix)]
#[test]
fn client_closes_non_reader_stdin_without_waiting() {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "cat >/dev/null; printf stdin-closed"])
        .stdin(Stdio::piped());
    let started = Instant::now();
    let output = run_client_command(command, "stdin regression");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(output.stdout, b"stdin-closed");
}

#[test]
fn security_diagnostics_redactor_hides_headers_and_url_userinfo() {
    let input = concat!(
        "Command { url: https://alice:password@example.test/remote.git, ",
        "Authorization: Bearer bearer-token cdn-auth, ",
        "Proxy-Authorization: Basic proxy-token origin-proxy cdn-proxy, ",
        "Cookie: session=cookie-token origin-cookie cdn-cookie, ",
        "X-Origin-Secret: fixture-secret, X-Cdn-Secret: cdn-secret }"
    );
    let redacted = redact_sensitive_diagnostics(input);
    for secret in [
        "alice:password",
        "password",
        "bearer-token",
        "proxy-token",
        "cookie-token",
        "fixture-secret",
        "origin-cookie",
        "origin-proxy",
        "cdn-auth",
        "cdn-cookie",
        "cdn-proxy",
        "cdn-secret",
    ] {
        assert!(!redacted.contains(secret), "diagnostic leaked {secret}");
    }
    assert!(redacted.contains(REDACTED_DIAGNOSTIC));
}

#[test]
fn diagnostic_preview_redacts_before_utf8_truncation() {
    let input = format!(
        "{} https://alice:unique-adversarial-secret@example.test/{}",
        "é".repeat(MAX_CLIENT_DIAGNOSTIC_BYTES),
        "fixture-secret".repeat(1024),
    );
    let preview = redacted_capture_preview(input.as_bytes());
    assert!(preview.len() <= MAX_CLIENT_DIAGNOSTIC_BYTES);
    assert!(preview.is_char_boundary(preview.len()));
    assert!(!preview.contains("unique-adversarial-secret"));
    assert!(!preview.contains("fixture-secret"));
}

#[cfg(windows)]
#[test]
fn windows_pipe_reader_has_a_hard_output_bound() {
    let mut output = Vec::new();
    let error = append_capped_output(
        &mut output,
        &vec![b'x'; MAX_CLIENT_OUTPUT_BYTES + CLIENT_CAPTURE_CHUNK_BYTES],
    )
    .expect_err("oversized Windows pipe capture must fail");
    assert!(matches!(
        error,
        ChildCaptureError::LimitExceeded {
            limit: MAX_CLIENT_OUTPUT_BYTES
        }
    ));
    assert_eq!(output.len(), MAX_CLIENT_OUTPUT_BYTES);
}

#[cfg(windows)]
fn assert_windows_capture_failure(command: Command, marker: &str) {
    let started = Instant::now();
    let result = catch_unwind(AssertUnwindSafe(|| {
        run_client_command_with_timeout(command, "Windows lifecycle regression", CLIENT_TIMEOUT)
    }));
    assert!(
        result.is_err(),
        "Windows lifecycle command unexpectedly succeeded"
    );
    assert!(started.elapsed() < Duration::from_secs(10));
    let message = panic_payload_text(result.expect_err("Windows lifecycle panic"));
    assert!(
        message.contains(marker),
        "diagnostic missing {marker}: {message}"
    );
    for secret in ["fixture-secret", "origin-pass", "cdn-secret"] {
        assert!(!message.contains(secret), "diagnostic leaked {secret}");
    }
}

#[cfg(windows)]
#[test]
fn windows_job_kills_descendant_retaining_pipe_handles() {
    let mut command = Command::new("cmd");
    command.args([
        "/C",
        "start \"\" /B cmd /C \"ping 127.0.0.1 -n 30 >NUL\" & exit /B 0",
    ]);
    let output = run_client_command(command, "Windows descendant lifecycle regression");
    assert!(output.status.success());
}

#[cfg(windows)]
#[test]
fn windows_pipe_reader_caps_infinite_stdout_and_stderr() {
    let mut stdout = Command::new("cmd");
    stdout.args(["/C", "for /L %i in (1,0,2) do @echo fixture-secret"]);
    assert_windows_capture_failure(stdout, "exceeded");

    let mut stderr = Command::new("cmd");
    stderr.args(["/C", "for /L %i in (1,0,2) do @echo fixture-secret 1>&2"]);
    assert_windows_capture_failure(stderr, "exceeded");
}

#[cfg(windows)]
#[test]
fn windows_job_normal_exit_descendant_is_cleaned() {
    let mut command = Command::new("cmd");
    command.args([
        "/C",
        "start \"\" /B cmd /C \"ping 127.0.0.1 -n 30 >NUL\" & exit /B 0",
    ]);
    let output = run_client_command(command, "Windows normal-exit lifecycle regression");
    assert!(output.status.success());
}

#[cfg(windows)]
#[test]
fn windows_job_timeout_is_bounded() {
    let mut command = Command::new("cmd");
    command.args(["/C", "ping 127.0.0.1 -n 30 >NUL"]);
    let started = Instant::now();
    let result = catch_unwind(AssertUnwindSafe(|| {
        run_client_command_with_timeout(
            command,
            "Windows timeout regression",
            CLIENT_REGRESSION_TIMEOUT,
        )
    }));
    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(panic_payload_text(result.expect_err("Windows timeout panic")).contains("timed out"));
}

#[cfg(windows)]
#[test]
fn windows_job_assignment_failure_path_closes_job_without_child() {
    let job = windows_client_lifecycle::WindowsJob::create()
        .expect("create Windows Job Object for assignment failure harness");
    assert!(job.assign_raw(std::ptr::null_mut()).is_err());
}

#[cfg(windows)]
#[test]
fn windows_reader_abort_decision_is_pure_and_bounded() {
    assert!(!windows_client_lifecycle::reader_abort_required(
        false, false
    ));
    assert!(!windows_client_lifecycle::reader_abort_required(true, true));
    assert!(windows_client_lifecycle::reader_abort_required(false, true));
}

fn validated_http_bundle() -> PathBuf {
    static BUNDLE: OnceLock<PathBuf> = OnceLock::new();
    BUNDLE.get_or_init(validate_fixed_http_bundle).clone()
}

fn validate_fixed_http_bundle() -> PathBuf {
    // The separately committed provenance suite proves deterministic source
    // and toolchain rebuilding. This E2E intentionally validates only the
    // fixed artifact consumed by the comparator; it never rebuilds it.
    let configured = PathBuf::from(
        std::env::var_os("ZMIN_GIT_HTTP_BUNDLE")
            .expect("ZMIN_GIT_HTTP_BUNDLE must select the fixed Git HTTP bundle"),
    );
    assert_eq!(
        configured,
        PathBuf::from(PINNED_HTTP_BUNDLE_PATH),
        "E2E must use the exact pinned HTTP bundle path"
    );
    let bundle = fs::canonicalize(&configured).expect("canonicalize HTTP bundle");
    assert_eq!(
        bundle,
        PathBuf::from(PINNED_HTTP_BUNDLE_PATH),
        "pinned HTTP bundle path must already be canonical"
    );

    let configured_git = pinned_stock_git_path();
    let git = fs::canonicalize(&configured_git).expect("canonicalize pinned Git");
    assert_eq!(git, bundle.join("git"));
    assert_eq!(file_sha256(&git), PINNED_GIT_SHA256);
    assert_eq!(bundle_mode(&git), 0o555, "pinned Git mode differs");

    let manifest = bundle.join("manifest.tsv");
    let manifest_sidecar = bundle.join("manifest.tsv.sha256");
    let bundle_table = bundle.join("bundle.tsv");
    let bundle_sidecar = bundle.join("bundle.tsv.sha256");
    assert_eq!(file_sha256(&manifest), PINNED_MANIFEST_SHA256);
    assert_eq!(sidecar_sha256(&manifest_sidecar), PINNED_MANIFEST_SHA256);
    assert_eq!(file_sha256(&bundle_table), PINNED_BUNDLE_SIDECAR_SHA256);
    assert_eq!(
        sidecar_sha256(&bundle_sidecar),
        PINNED_BUNDLE_SIDECAR_SHA256
    );
    assert_eq!(bundle_mode(&manifest), 0o444);
    assert_eq!(bundle_mode(&manifest_sidecar), 0o444);
    assert_eq!(bundle_mode(&bundle_table), 0o444);
    assert_eq!(bundle_mode(&bundle_sidecar), 0o444);

    let manifest_text = fs::read_to_string(&manifest).expect("read fixed bundle manifest");
    assert!(manifest_text.contains("manifest_version\t3\n"));
    assert!(manifest_text.contains("upstream_git_tag\tv2.55.0\n"));
    assert!(
        manifest_text.contains("upstream_git_commit\te9019fcafe0040228b8631c30f97ae1adb61bcdc\n")
    );
    validate_manifest_members(&bundle, &manifest_text);
    let bundle_text = fs::read_to_string(&bundle_table).expect("read fixed bundle table");
    assert!(bundle_text.contains("schema_version\t1\n"));
    assert!(bundle_text.contains("platform\tDarwin\narch\tarm64\n"));
    assert!(bundle_text.contains("template_dir\ttemplates\n"));

    let template = bundle.join("templates");
    assert_eq!(bundle_mode(&template), 0o555);
    assert_eq!(
        fs::read_dir(&template)
            .expect("read fixed empty template directory")
            .count(),
        0
    );
    let root = TempDir::new().expect("bundle version temp root");
    let output = pinned_git_version(root.path());
    assert_eq!(output, "git version 2.55.0");
    bundle
}

fn pinned_git_version(cwd: &Path) -> String {
    let mut command = hermetic_pinned_git_command(cwd);
    command.args(["--version"]);
    let output = command.output().expect("run pinned Git version");
    let output = assert_success(output, "pinned Git version");
    String::from_utf8(output.stdout)
        .expect("pinned Git version UTF-8")
        .trim()
        .to_owned()
}

fn validate_manifest_members(bundle: &Path, manifest: &str) {
    for line in manifest.lines().filter(|line| line.starts_with("member\t")) {
        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 5, "invalid fixed bundle member line: {line}");
        let path = bundle.join(fields[1]);
        let metadata = fs::symlink_metadata(&path).expect("stat fixed bundle member");
        assert!(metadata.file_type().is_file() && !metadata.file_type().is_symlink());
        assert_eq!(fields[2], format!("{:04o}", bundle_mode(&path)));
        assert_eq!(fields[3], metadata.len().to_string());
        assert_eq!(fields[4], file_sha256(&path));
        if fields[1] == "git-http-fetch" {
            assert_eq!(fields[4], PINNED_HTTP_FETCH_SHA256);
            assert_eq!(fields[2], "0555");
        }
    }
}

fn sidecar_sha256(path: &Path) -> String {
    fs::read_to_string(path)
        .expect("read fixed bundle sidecar")
        .split_whitespace()
        .next()
        .expect("fixed bundle sidecar hash")
        .to_owned()
}

fn file_sha256(path: &Path) -> String {
    let root = TempDir::new().expect("hash command root");
    let mut command = Command::new("/usr/bin/shasum");
    HermeticGitChildEnvironment::apply(&mut command, root.path());
    let output = command
        .args(["-a", "256"])
        .arg(path)
        .output()
        .expect("run fixed shasum");
    assert!(output.status.success(), "fixed shasum failed");
    String::from_utf8(output.stdout)
        .expect("fixed shasum UTF-8")
        .split_whitespace()
        .next()
        .expect("fixed shasum hash")
        .to_owned()
}

fn bundle_mode(path: &Path) -> u32 {
    #[cfg(unix)]
    {
        return fs::symlink_metadata(path)
            .expect("stat fixed bundle path")
            .mode()
            & 0o777;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        0
    }
}

fn run_pinned_git(cwd: &Path, args: &[String], bundle: &Path) -> Output {
    let mut path = vec![bundle.to_path_buf()];
    path.extend(std::env::split_paths(&hermetic_system_path()));
    let path = std::env::join_paths(path).expect("construct pinned PATH");
    let mut command = hermetic_pinned_git_command(cwd);
    command.args(args);
    command
        .env("PATH", path)
        .env("GIT_EXEC_PATH", bundle)
        .env("GIT_TERMINAL_PROMPT", "0");
    run_client_command(command, "pinned Git client")
}

fn run_zmin(cwd: &Path, args: &[String]) -> Output {
    let helper = PathBuf::from(zmin_bin()).with_file_name("zmin-git-remote-http");
    let mut command = Command::new(zmin_bin());
    command.args(args).current_dir(cwd);
    HermeticGitChildEnvironment::apply(&mut command, cwd);
    command
        .env("ZMIN_BIN", zmin_bin())
        .env("ZMIN_GIT_HTTP_BUNDLE", PINNED_HTTP_BUNDLE_PATH)
        .env("ZMIN_STOCK_GIT", pinned_stock_git_path())
        .env("ZMIN_GIT_REMOTE_HTTP", helper)
        .env("GIT_TERMINAL_PROMPT", "0");
    run_client_command(command, "zmin client")
}

#[derive(Clone, Debug)]
enum ChildCaptureError {
    LimitExceeded { limit: usize },
    Io(String),
    Cancelled,
}

#[derive(Clone, Debug)]
enum ChildCaptureResult {
    Complete(Vec<u8>),
    Failed {
        error: ChildCaptureError,
        output: Vec<u8>,
    },
}

#[cfg(unix)]
struct ChildOutputReader {
    receiver: Receiver<ChildCaptureResult>,
    handle: JoinHandle<()>,
    cancel: Arc<AtomicBool>,
}

fn append_capped_output(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ChildCaptureError> {
    let remaining = MAX_CLIENT_OUTPUT_BYTES.saturating_sub(output.len());
    if bytes.len() > remaining {
        output.extend_from_slice(&bytes[..remaining]);
        return Err(ChildCaptureError::LimitExceeded {
            limit: MAX_CLIENT_OUTPUT_BYTES,
        });
    }
    output.extend_from_slice(bytes);
    Ok(())
}

#[cfg(windows)]
fn capture_stream<R>(mut stream: R, cancel: &AtomicBool) -> ChildCaptureResult
where
    R: Read,
{
    let mut output = Vec::with_capacity(CLIENT_CAPTURE_CHUNK_BYTES);
    let mut buffer = [0_u8; CLIENT_CAPTURE_CHUNK_BYTES];
    loop {
        if cancel.load(Ordering::Acquire) {
            return ChildCaptureResult::Failed {
                error: ChildCaptureError::Cancelled,
                output,
            };
        }
        match stream.read(&mut buffer) {
            Ok(0) => return ChildCaptureResult::Complete(output),
            Ok(read) => {
                if let Err(error) = append_capped_output(&mut output, &buffer[..read]) {
                    return ChildCaptureResult::Failed { error, output };
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return ChildCaptureResult::Failed {
                    error: ChildCaptureError::Io(error.to_string()),
                    output,
                };
            }
        }
    }
}

#[cfg(unix)]
fn wait_for_capture_data(fd: std::os::unix::io::RawFd) -> Result<bool, ChildCaptureError> {
    let mut descriptor = libc::pollfd {
        fd,
        events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
        revents: 0,
    };
    loop {
        let result = unsafe { libc::poll(&mut descriptor, 1, 50) };
        if result >= 0 {
            return Ok(result != 0);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(ChildCaptureError::Io(error.to_string()));
    }
}

#[cfg(unix)]
fn capture_unix_stream<R>(mut stream: R, cancel: &AtomicBool) -> ChildCaptureResult
where
    R: Read + std::os::unix::io::AsRawFd,
{
    let descriptor = stream.as_raw_fd();
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags == -1
        || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
    {
        return ChildCaptureResult::Failed {
            error: ChildCaptureError::Io(std::io::Error::last_os_error().to_string()),
            output: Vec::new(),
        };
    }
    let mut output = Vec::with_capacity(CLIENT_CAPTURE_CHUNK_BYTES);
    let mut buffer = [0_u8; CLIENT_CAPTURE_CHUNK_BYTES];
    loop {
        if cancel.load(Ordering::Acquire) {
            return ChildCaptureResult::Failed {
                error: ChildCaptureError::Cancelled,
                output,
            };
        }
        match stream.read(&mut buffer) {
            Ok(0) => return ChildCaptureResult::Complete(output),
            Ok(read) => {
                if let Err(error) = append_capped_output(&mut output, &buffer[..read]) {
                    return ChildCaptureResult::Failed { error, output };
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if let Err(error) = wait_for_capture_data(descriptor) {
                    return ChildCaptureResult::Failed { error, output };
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return ChildCaptureResult::Failed {
                    error: ChildCaptureError::Io(error.to_string()),
                    output,
                };
            }
        }
    }
}

#[cfg(unix)]
fn spawn_child_output_reader<R>(stream: R) -> ChildOutputReader
where
    R: Read + Send + std::os::unix::io::AsRawFd + 'static,
{
    let (sender, receiver) = sync_channel(1);
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = cancel.clone();
    let handle = std::thread::spawn(move || {
        let _ = sender.send(capture_unix_stream(stream, &thread_cancel));
    });
    ChildOutputReader {
        receiver,
        handle,
        cancel,
    }
}

#[cfg(unix)]
fn poll_child_output(reader: &ChildOutputReader) -> Option<ChildCaptureResult> {
    reader.receiver.try_recv().ok()
}

#[cfg(unix)]
fn cancel_child_output_reader(reader: &ChildOutputReader) {
    reader.cancel.store(true, Ordering::Release);
}

#[cfg(unix)]
fn join_child_output_reader(
    reader: ChildOutputReader,
    result: Option<ChildCaptureResult>,
    label: &str,
    deadline: Instant,
) -> Result<ChildCaptureResult, String> {
    let result = result.or_else(|| {
        reader
            .receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .ok()
    });
    while !reader.handle.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    if !reader.handle.is_finished() {
        return Err(format!("{label} output reader cleanup timed out"));
    }
    if reader.handle.join().is_err() {
        return Err(format!("{label} output reader panicked"));
    }
    result.ok_or_else(|| format!("{label} output reader produced no result"))
}

fn capture_failure_message(label: &str, stream: &str, failure: &ChildCaptureResult) -> String {
    let ChildCaptureResult::Failed { error, output } = failure else {
        return format!("{label} {stream} output capture unexpectedly completed");
    };
    let reason = match error {
        ChildCaptureError::LimitExceeded { limit } => format!("output exceeded {limit} bytes"),
        ChildCaptureError::Io(error) => format!("output read failed: {error}"),
        ChildCaptureError::Cancelled => "output reader cancelled".to_owned(),
    };
    format!(
        "{label} {stream} capture failed ({reason}); output={}",
        redacted_capture_preview(output)
    )
}

fn redacted_capture_preview(output: &[u8]) -> String {
    let redacted = redact_sensitive_diagnostics(&String::from_utf8_lossy(output));
    truncate_utf8_preview(&redacted, MAX_CLIENT_DIAGNOSTIC_BYTES)
}

fn truncate_utf8_preview(input: &str, limit: usize) -> String {
    if input.len() <= limit {
        return input.to_owned();
    }
    let suffix = "...";
    let mut end = limit.saturating_sub(suffix.len());
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    let mut output = input[..end].to_owned();
    output.push_str(suffix);
    output
}

fn capture_result_output(result: &Result<ChildCaptureResult, String>) -> Vec<u8> {
    match result {
        Ok(ChildCaptureResult::Complete(output)) => output.clone(),
        Ok(ChildCaptureResult::Failed { output, .. }) => output.clone(),
        Err(_) => Vec::new(),
    }
}

#[cfg(unix)]
fn configure_client_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn configure_client_process_group(_command: &mut Command) {}

fn terminate_client_process(child: &mut Child) {
    #[cfg(unix)]
    {
        let process_group = -(child.id() as libc::pid_t);
        if unsafe { libc::kill(process_group, libc::SIGKILL) } == -1 {
            let _ = child.kill();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}

fn reap_terminated_client(child: &mut Child, label: &str) -> ExitStatus {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                terminate_client_process(child);
                let reap_deadline = Instant::now() + CLIENT_FINAL_REAP_GRACE;
                while Instant::now() < reap_deadline {
                    match child.try_wait() {
                        Ok(Some(status)) => return status,
                        Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                        Err(error) => panic!("{label} final reap failed: {error}"),
                    }
                }
                panic!(
                    "{label} direct child was not reaped within {:?} plus final termination grace",
                    REQUEST_TIMEOUT
                );
            }
            Err(error) => panic!("{label} wait failed after termination: {error}"),
        }
    }
}

fn run_client_command(command: Command, label: &str) -> Output {
    run_client_command_with_timeout(command, label, CLIENT_TIMEOUT)
}

#[cfg(unix)]
fn run_client_command_with_timeout(mut command: Command, label: &str, timeout: Duration) -> Output {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_client_process_group(&mut command);
    let command_debug = redacted_command_display(&command);
    let mut child = command.spawn().unwrap_or_else(|error| {
        panic!("{label} spawn failed: {error}; command={command_debug}");
    });
    drop(child.stdin.take());
    let stdout_reader = spawn_child_output_reader(child.stdout.take().expect("child stdout pipe"));
    let stderr_reader = spawn_child_output_reader(child.stderr.take().expect("child stderr pipe"));
    let mut stdout_result = None;
    let mut stderr_result = None;
    let deadline = Instant::now() + timeout;
    let mut capture_failure = None;
    let (status, timed_out) = loop {
        if stdout_result.is_none() {
            stdout_result = poll_child_output(&stdout_reader);
        }
        if stderr_result.is_none() {
            stderr_result = poll_child_output(&stderr_reader);
        }
        if let Some(result) = stdout_result.as_ref()
            && matches!(result, ChildCaptureResult::Failed { .. })
        {
            capture_failure = Some(("stdout", result.clone()));
        }
        if capture_failure.is_none()
            && let Some(result) = stderr_result.as_ref()
            && matches!(result, ChildCaptureResult::Failed { .. })
        {
            capture_failure = Some(("stderr", result.clone()));
        }
        if capture_failure.is_some() {
            terminate_client_process(&mut child);
            break (reap_terminated_client(&mut child, label), false);
        }
        match child.try_wait() {
            Ok(Some(status)) => break (status, false),
            Ok(None) if Instant::now() >= deadline => {
                terminate_client_process(&mut child);
                break (reap_terminated_client(&mut child, label), true);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                terminate_client_process(&mut child);
                let _ = reap_terminated_client(&mut child, label);
                panic!("{label} wait failed: {error}");
            }
        }
    };
    if !timed_out {
        if stdout_result.is_none() {
            stdout_result = stdout_reader
                .receiver
                .recv_timeout(CLIENT_CAPTURE_EXIT_GRACE)
                .ok();
        }
        if stderr_result.is_none() {
            stderr_result = stderr_reader
                .receiver
                .recv_timeout(CLIENT_CAPTURE_EXIT_GRACE)
                .ok();
        }
    }
    if timed_out || stdout_result.is_none() || stderr_result.is_none() {
        terminate_client_process(&mut child);
        cancel_child_output_reader(&stdout_reader);
        cancel_child_output_reader(&stderr_reader);
    }
    let reader_deadline = Instant::now() + CLIENT_READER_TIMEOUT;
    let stdout_capture =
        join_child_output_reader(stdout_reader, stdout_result, label, reader_deadline);
    let stderr_capture =
        join_child_output_reader(stderr_reader, stderr_result, label, reader_deadline);
    if let Some((stream, failure)) = capture_failure {
        panic!("{}", capture_failure_message(label, stream, &failure));
    }
    if timed_out {
        let stdout = capture_result_output(&stdout_capture);
        let stderr = capture_result_output(&stderr_capture);
        panic!(
            "{label} timed out after {timeout:?}; status={status:?}; stdout={}; stderr={}",
            redacted_capture_preview(&stdout),
            redacted_capture_preview(&stderr),
        );
    }
    let stdout = match stdout_capture {
        Ok(ChildCaptureResult::Complete(output)) => output,
        Ok(failure) => panic!("{}", capture_failure_message(label, "stdout", &failure)),
        Err(error) => panic!("{label} stdout cleanup failed: {error}"),
    };
    let stderr = match stderr_capture {
        Ok(ChildCaptureResult::Complete(output)) => output,
        Ok(failure) => panic!("{}", capture_failure_message(label, "stderr", &failure)),
        Err(error) => panic!("{label} stderr cleanup failed: {error}"),
    };
    Output {
        status,
        stdout,
        stderr,
    }
}

#[cfg(windows)]
mod windows_client_lifecycle {
    use super::{
        AtomicBool, CLIENT_FINAL_REAP_GRACE, CLIENT_READER_TIMEOUT, Child, ChildCaptureResult,
        ExitStatus, Instant, JoinHandle, Output, REQUEST_TIMEOUT, Stdio,
    };
    use std::ffi::c_void;
    use std::io::Read;
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use std::process::Command;
    use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
    use std::thread;
    use std::time::Duration;

    pub type WindowsHandle = *mut c_void;
    const INVALID_WINDOWS_HANDLE: WindowsHandle = -1isize as WindowsHandle;
    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    const TH32CS_SNAPTHREAD: u32 = 0x0000_0004;
    const THREAD_SUSPEND_RESUME: u32 = 0x0002;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
    const DUPLICATE_SAME_ACCESS: u32 = 0x0000_0002;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct WindowsIoCounters {
        read_operations: u64,
        write_operations: u64,
        other_operations: u64,
        read_bytes: u64,
        write_bytes: u64,
        other_bytes: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct WindowsBasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct WindowsExtendedLimitInformation {
        basic: WindowsBasicLimitInformation,
        io: WindowsIoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct WindowsThreadEntry {
        size: u32,
        usage: u32,
        thread_id: u32,
        owner_process_id: u32,
        base_priority: i32,
        delta_priority: i32,
        flags: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AssignProcessToJobObject(job: WindowsHandle, process: WindowsHandle) -> i32;
        fn CancelIoEx(file: WindowsHandle, overlapped: *const c_void) -> i32;
        fn CancelSynchronousIo(thread: WindowsHandle) -> i32;
        fn CloseHandle(handle: WindowsHandle) -> i32;
        fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> WindowsHandle;
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> WindowsHandle;
        fn DuplicateHandle(
            source_process: WindowsHandle,
            source: WindowsHandle,
            target_process: WindowsHandle,
            target: *mut WindowsHandle,
            desired_access: u32,
            inherit: i32,
            options: u32,
        ) -> i32;
        fn GetCurrentProcess() -> WindowsHandle;
        fn GetCurrentThread() -> WindowsHandle;
        fn OpenThread(access: u32, inherit: i32, thread_id: u32) -> WindowsHandle;
        fn ResumeThread(thread: WindowsHandle) -> u32;
        fn SetInformationJobObject(
            job: WindowsHandle,
            information_class: u32,
            information: *const c_void,
            information_length: u32,
        ) -> i32;
        fn TerminateJobObject(job: WindowsHandle, exit_code: u32) -> i32;
        fn Thread32First(snapshot: WindowsHandle, entry: *mut WindowsThreadEntry) -> i32;
        fn Thread32Next(snapshot: WindowsHandle, entry: *mut WindowsThreadEntry) -> i32;
        fn WaitForSingleObject(handle: WindowsHandle, milliseconds: u32) -> u32;
    }

    pub struct WindowsOwnedHandle(WindowsHandle);

    unsafe impl Send for WindowsOwnedHandle {}

    impl WindowsOwnedHandle {
        fn new(handle: WindowsHandle) -> Result<Self, String> {
            if handle.is_null() || handle == INVALID_WINDOWS_HANDLE {
                Err("Windows handle acquisition failed".to_owned())
            } else {
                Ok(Self(handle))
            }
        }

        fn raw(&self) -> WindowsHandle {
            self.0
        }
    }

    impl Drop for WindowsOwnedHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub struct WindowsJob {
        handle: WindowsOwnedHandle,
    }

    impl WindowsJob {
        pub fn create() -> Result<Self, String> {
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            let handle = WindowsOwnedHandle::new(handle)?;
            let information = WindowsExtendedLimitInformation {
                basic: WindowsBasicLimitInformation {
                    per_process_user_time_limit: 0,
                    per_job_user_time_limit: 0,
                    limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                    minimum_working_set_size: 0,
                    maximum_working_set_size: 0,
                    active_process_limit: 0,
                    affinity: 0,
                    priority_class: 0,
                    scheduling_class: 0,
                },
                io: WindowsIoCounters {
                    read_operations: 0,
                    write_operations: 0,
                    other_operations: 0,
                    read_bytes: 0,
                    write_bytes: 0,
                    other_bytes: 0,
                },
                process_memory_limit: 0,
                job_memory_limit: 0,
                peak_process_memory_used: 0,
                peak_job_memory_used: 0,
            };
            let configured = unsafe {
                SetInformationJobObject(
                    handle.raw(),
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                    (&information as *const WindowsExtendedLimitInformation).cast(),
                    size_of::<WindowsExtendedLimitInformation>() as u32,
                )
            };
            if configured == 0 {
                return Err("Windows Job Object limit configuration failed".to_owned());
            }
            Ok(Self { handle })
        }

        pub fn assign(&self, child: &Child) -> Result<(), String> {
            self.assign_raw(child.as_raw_handle())
        }

        pub fn assign_raw(&self, process: WindowsHandle) -> Result<(), String> {
            let assigned = unsafe { AssignProcessToJobObject(self.handle.raw(), process) };
            if assigned == 0 {
                Err("Windows Job Object process assignment failed".to_owned())
            } else {
                Ok(())
            }
        }

        pub fn terminate(&self) {
            unsafe {
                let _ = TerminateJobObject(self.handle.raw(), 1);
            }
        }
    }

    pub fn configure_suspended(command: &mut Command) {
        std::os::windows::process::CommandExt::creation_flags(command, CREATE_SUSPENDED);
    }

    pub fn suspended_main_thread(process_id: u32) -> Result<WindowsOwnedHandle, String> {
        let snapshot =
            WindowsOwnedHandle::new(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })?;
        let mut entry = WindowsThreadEntry {
            size: size_of::<WindowsThreadEntry>() as u32,
            usage: 0,
            thread_id: 0,
            owner_process_id: 0,
            base_priority: 0,
            delta_priority: 0,
            flags: 0,
        };
        let mut found = unsafe { Thread32First(snapshot.raw(), &mut entry) } != 0;
        while found {
            if entry.owner_process_id == process_id {
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.thread_id) };
                if let Ok(thread) = WindowsOwnedHandle::new(thread) {
                    return Ok(thread);
                }
            }
            found = unsafe { Thread32Next(snapshot.raw(), &mut entry) } != 0;
        }
        Err("Windows suspended child main thread was not discoverable".to_owned())
    }

    pub fn resume(thread: &WindowsOwnedHandle) -> Result<(), String> {
        if unsafe { ResumeThread(thread.raw()) } == u32::MAX {
            Err("Windows suspended child resume failed".to_owned())
        } else {
            Ok(())
        }
    }

    pub fn duplicate_handle(raw: WindowsHandle) -> Result<WindowsOwnedHandle, String> {
        let mut duplicate = std::ptr::null_mut();
        let duplicated = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                raw,
                GetCurrentProcess(),
                &mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if duplicated == 0 {
            Err("Windows handle duplication failed".to_owned())
        } else {
            WindowsOwnedHandle::new(duplicate)
        }
    }

    pub struct WindowsPipeReader {
        receiver: Receiver<ChildCaptureResult>,
        handle: JoinHandle<()>,
        read_handle: WindowsOwnedHandle,
        thread_handle: WindowsOwnedHandle,
    }

    impl WindowsPipeReader {
        fn poll(&self) -> Option<ChildCaptureResult> {
            match self.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
            }
        }

        fn cancel(&self) {
            unsafe {
                let _ = CancelIoEx(self.read_handle.raw(), std::ptr::null());
                let _ = CancelSynchronousIo(self.thread_handle.raw());
            }
        }

        fn join(
            self,
            result: Option<ChildCaptureResult>,
            label: &str,
            deadline: Instant,
        ) -> Result<ChildCaptureResult, String> {
            let result = result.or_else(|| {
                self.receiver
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .ok()
            });
            while !self.handle.is_finished() && Instant::now() < deadline {
                self.cancel();
                unsafe {
                    let _ = WaitForSingleObject(self.thread_handle.raw(), 50);
                }
            }
            if reader_abort_required(self.handle.is_finished(), true) {
                self.cancel();
                abort_after_reader_deadline();
            }
            if self.handle.join().is_err() {
                return Err(format!("{label} Windows output reader panicked"));
            }
            result.ok_or_else(|| format!("{label} Windows output reader produced no result"))
        }
    }

    fn abort_after_reader_deadline() -> ! {
        eprintln!("Windows output reader cancellation exceeded its hard deadline; aborting");
        std::process::abort();
    }

    pub fn reader_abort_required(reader_finished: bool, deadline_reached: bool) -> bool {
        deadline_reached && !reader_finished
    }

    fn duplicate_current_thread() -> Result<WindowsOwnedHandle, String> {
        duplicate_handle(unsafe { GetCurrentThread() })
    }

    pub fn spawn_pipe_reader<R>(stream: R) -> Result<WindowsPipeReader, String>
    where
        R: Read + AsRawHandle + Send + 'static,
    {
        let read_handle = duplicate_handle(stream.as_raw_handle())?;
        let (sender, receiver) = sync_channel(1);
        let (thread_sender, thread_receiver) = sync_channel(1);
        let handle = thread::spawn(move || {
            let thread_handle = match duplicate_current_thread() {
                Ok(thread_handle) => thread_handle,
                Err(error) => {
                    let _ = thread_sender.send(Err(error));
                    return;
                }
            };
            if thread_sender.send(Ok(thread_handle)).is_err() {
                return;
            }
            let _ = sender.send(super::capture_stream(stream, &AtomicBool::new(false)));
        });
        let thread_handle = match thread_receiver.recv_timeout(REQUEST_TIMEOUT) {
            Ok(Ok(thread_handle)) => thread_handle,
            Ok(Err(error)) => {
                let _ = handle.join();
                return Err(error);
            }
            Err(error) => {
                let _ = handle.join();
                return Err(format!("Windows output reader startup failed: {error}"));
            }
        };
        Ok(WindowsPipeReader {
            receiver,
            handle,
            read_handle,
            thread_handle,
        })
    }

    pub fn reap_child(child: &mut Child, label: &str) -> ExitStatus {
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Ok(None) => {
                    let _ = child.kill();
                    let reap_deadline = Instant::now() + CLIENT_FINAL_REAP_GRACE;
                    while Instant::now() < reap_deadline {
                        match child.try_wait() {
                            Ok(Some(status)) => return status,
                            Ok(None) => thread::sleep(Duration::from_millis(10)),
                            Err(error) => panic!("{label} final reap failed: {error}"),
                        }
                    }
                    panic!("{label} Windows direct child was not reaped within bounded grace");
                }
                Err(error) => panic!("{label} Windows wait failed after termination: {error}"),
            }
        }
    }

    pub fn abort_suspended_child(child: &mut Child, job: &WindowsJob, label: &str) {
        job.terminate();
        let _ = child.kill();
        let _ = reap_child(child, label);
    }

    pub fn run_client_command_with_timeout(
        mut command: Command,
        label: &str,
        timeout: Duration,
    ) -> Output {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_suspended(&mut command);
        let command_debug = super::redacted_command_display(&command);
        let job = WindowsJob::create()
            .unwrap_or_else(|error| panic!("{label} Windows Job Object setup failed: {error}"));
        let mut child = command.spawn().unwrap_or_else(|error| {
            panic!("{label} spawn failed: {error}; command={command_debug}");
        });
        let main_thread = match suspended_main_thread(child.id()) {
            Ok(thread) => thread,
            Err(error) => {
                abort_suspended_child(&mut child, &job, label);
                panic!("{label} suspended child setup failed: {error}");
            }
        };
        if let Err(error) = job.assign(&child) {
            abort_suspended_child(&mut child, &job, label);
            panic!("{label} Windows Job Object assignment failed: {error}");
        }
        let stdout_reader = match spawn_pipe_reader(child.stdout.take().expect("child stdout pipe"))
        {
            Ok(reader) => reader,
            Err(error) => {
                abort_suspended_child(&mut child, &job, label);
                panic!("{label} Windows stdout reader setup failed: {error}");
            }
        };
        let stderr_reader = match spawn_pipe_reader(child.stderr.take().expect("child stderr pipe"))
        {
            Ok(reader) => reader,
            Err(error) => {
                stdout_reader.cancel();
                let _ = stdout_reader.join(None, label, Instant::now() + CLIENT_READER_TIMEOUT);
                abort_suspended_child(&mut child, &job, label);
                panic!("{label} Windows stderr reader setup failed: {error}");
            }
        };
        if let Err(error) = resume(&main_thread) {
            stdout_reader.cancel();
            stderr_reader.cancel();
            let _ = stdout_reader.join(None, label, Instant::now() + CLIENT_READER_TIMEOUT);
            let _ = stderr_reader.join(None, label, Instant::now() + CLIENT_READER_TIMEOUT);
            abort_suspended_child(&mut child, &job, label);
            panic!("{label} suspended child resume failed: {error}");
        }
        drop(main_thread);

        let mut stdout_result = None;
        let mut stderr_result = None;
        let mut capture_failure = None;
        let deadline = Instant::now() + timeout;
        let (status, timed_out) = loop {
            if stdout_result.is_none() {
                stdout_result = stdout_reader.poll();
            }
            if stderr_result.is_none() {
                stderr_result = stderr_reader.poll();
            }
            if let Some(result) = stdout_result.as_ref()
                && matches!(result, ChildCaptureResult::Failed { .. })
            {
                capture_failure = Some(("stdout", result.clone()));
            }
            if capture_failure.is_none()
                && let Some(result) = stderr_result.as_ref()
                && matches!(result, ChildCaptureResult::Failed { .. })
            {
                capture_failure = Some(("stderr", result.clone()));
            }
            if capture_failure.is_some() {
                job.terminate();
                break (reap_child(&mut child, label), false);
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    job.terminate();
                    break (status, false);
                }
                Ok(None) if Instant::now() >= deadline => {
                    job.terminate();
                    break (reap_child(&mut child, label), true);
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    job.terminate();
                    let _ = reap_child(&mut child, label);
                    panic!("{label} Windows wait failed: {error}");
                }
            }
        };
        if timed_out || stdout_result.is_none() || stderr_result.is_none() {
            stdout_reader.cancel();
            stderr_reader.cancel();
        }
        let reader_deadline = Instant::now() + CLIENT_READER_TIMEOUT;
        let stdout_capture = stdout_reader.join(stdout_result, label, reader_deadline);
        let stderr_capture = stderr_reader.join(stderr_result, label, reader_deadline);
        if let Some((stream, failure)) = capture_failure {
            panic!(
                "{}",
                super::capture_failure_message(label, stream, &failure)
            );
        }
        if timed_out {
            let stdout = super::capture_result_output(&stdout_capture);
            let stderr = super::capture_result_output(&stderr_capture);
            panic!(
                "{label} timed out after {timeout:?}; status={status:?}; stdout={}; stderr={}",
                super::redacted_capture_preview(&stdout),
                super::redacted_capture_preview(&stderr),
            );
        }
        let stdout = match stdout_capture {
            Ok(ChildCaptureResult::Complete(output)) => output,
            Ok(failure) => panic!(
                "{}",
                super::capture_failure_message(label, "stdout", &failure)
            ),
            Err(error) => panic!("{label} stdout cleanup failed: {error}"),
        };
        let stderr = match stderr_capture {
            Ok(ChildCaptureResult::Complete(output)) => output,
            Ok(failure) => panic!(
                "{}",
                super::capture_failure_message(label, "stderr", &failure)
            ),
            Err(error) => panic!("{label} stderr cleanup failed: {error}"),
        };
        drop(job);
        Output {
            status,
            stdout,
            stderr,
        }
    }
}

#[cfg(windows)]
fn run_client_command_with_timeout(command: Command, label: &str, timeout: Duration) -> Output {
    windows_client_lifecycle::run_client_command_with_timeout(command, label, timeout)
}

#[cfg(all(not(unix), not(windows)))]
fn run_client_command_with_timeout(_command: Command, label: &str, _timeout: Duration) -> Output {
    panic!("{label}: unsupported target has no bounded child lifecycle implementation")
}

fn clone_args(
    url: &str,
    destination: &Path,
    format: RefFormat,
    uri_protocols: &str,
) -> Vec<String> {
    vec![
        "clone".into(),
        "-c".into(),
        "protocol.version=2".into(),
        "-c".into(),
        format!("fetch.uriprotocols={uri_protocols}"),
        "--quiet".into(),
        format!("--ref-format={}", format.git_name()),
        url.into(),
        destination.display().to_string(),
    ]
}

fn assert_clone_state(stock: &Path, zmin: &Path) {
    let stock_refs = stock_text(stock, &["show-ref"]);
    let zmin_refs = command_text(zmin, &["show-ref"]);
    assert_eq!(stock_refs, zmin_refs, "refs differ");
    let stock_objects = stock_text(stock, &["rev-list", "--objects", "--all"]);
    let zmin_objects = command_text(zmin, &["rev-list", "--objects", "--all"]);
    assert_eq!(stock_objects, zmin_objects, "object closure differs");
    assert_pack_layout(stock);
    assert_pack_layout(zmin);
    assert_eq!(pack_artifact_names(stock), pack_artifact_names(zmin));
}

fn assert_fetch_state(stock: &Path, zmin: &Path) {
    assert_clone_state(stock, zmin);
    let stock_fetch_head = fs::read(stock.join(".git/FETCH_HEAD")).expect("read stock FETCH_HEAD");
    let zmin_fetch_head = fs::read(zmin.join(".git/FETCH_HEAD")).expect("read zmin FETCH_HEAD");
    assert_eq!(stock_fetch_head, zmin_fetch_head, "FETCH_HEAD differs");
    assert!(!stock_fetch_head.is_empty(), "stock FETCH_HEAD is empty");
    assert!(!zmin_fetch_head.is_empty(), "zmin FETCH_HEAD is empty");
}

fn assert_filtered_refs_and_fetch_head(stock: &Path, zmin: &Path) {
    let stock_refs = stock_text(stock, &["show-ref"]);
    let zmin_refs = command_text(zmin, &["show-ref"]);
    assert_eq!(stock_refs, zmin_refs, "refs differ");
    let stock_fetch_head = fs::read(stock.join(".git/FETCH_HEAD")).expect("read stock FETCH_HEAD");
    let zmin_fetch_head = fs::read(zmin.join(".git/FETCH_HEAD")).expect("read zmin FETCH_HEAD");
    assert_eq!(stock_fetch_head, zmin_fetch_head, "FETCH_HEAD differs");
    assert!(!stock_fetch_head.is_empty(), "stock FETCH_HEAD is empty");
    assert!(!zmin_fetch_head.is_empty(), "zmin FETCH_HEAD is empty");
}

fn assert_security_fetch_state(stock: &Path, zmin: &Path) {
    assert_clone_state(stock, zmin);
    let stock_fetch_head = fs::read_to_string(stock.join(".git/FETCH_HEAD"))
        .expect("read stock security FETCH_HEAD")
        .replace("http://origin-user:origin-pass@", "http://");
    let zmin_fetch_head = fs::read_to_string(zmin.join(".git/FETCH_HEAD"))
        .expect("read zmin security FETCH_HEAD")
        .replace("http://origin-user:origin-pass@", "http://");
    assert_eq!(stock_fetch_head, zmin_fetch_head, "FETCH_HEAD differs");
}

fn assert_fetch_backend_state(stock: &Path, zmin: &Path, ref_format: RefFormat) {
    let stock_reftable = reftable_artifacts(stock);
    let zmin_reftable = reftable_artifacts(zmin);
    match ref_format {
        RefFormat::Files => {
            assert!(
                stock_reftable.is_empty(),
                "stock unexpectedly has reftable state"
            );
            assert!(
                zmin_reftable.is_empty(),
                "zmin unexpectedly has reftable state"
            );
            assert!(
                stock.join(".git/refs").is_dir(),
                "stock refs backend missing"
            );
            assert!(zmin.join(".git/refs").is_dir(), "zmin refs backend missing");
        }
        RefFormat::Reftable => {
            assert!(!stock_reftable.is_empty(), "stock reftable state is empty");
            assert!(!zmin_reftable.is_empty(), "zmin reftable state is empty");
            assert_reftable_state_shape(&stock_reftable);
            assert_reftable_state_shape(&zmin_reftable);
            let stock_kinds = reftable_artifact_kinds(&stock_reftable);
            let zmin_kinds = reftable_artifact_kinds(&zmin_reftable);
            assert_eq!(stock_kinds, zmin_kinds, "reftable table layout differs");
        }
    }
}

fn reftable_artifact_kinds(artifacts: &BTreeMap<String, PackArtifactSnapshot>) -> Vec<String> {
    artifacts
        .keys()
        .map(|name| {
            if name == "tables.list" {
                name.clone()
            } else {
                name.rsplit_once('-')
                    .and_then(|(_, suffix)| suffix.split_once('.'))
                    .map_or_else(|| name.clone(), |(_, extension)| format!("*.{extension}"))
            }
        })
        .collect()
}

fn assert_reftable_state_shape(artifacts: &BTreeMap<String, PackArtifactSnapshot>) {
    let tables = artifacts.get("tables.list").expect("reftable tables.list");
    assert!(!tables.bytes.is_empty(), "reftable tables.list is empty");
    assert!(
        artifacts
            .keys()
            .any(|name| name.ends_with(".ref") || name.ends_with(".log")),
        "reftable stack has no table files"
    );
    for (name, artifact) in artifacts {
        assert!(
            !name.ends_with(".lock") && !name.ends_with(".tmp"),
            "reftable temporary artifact leaked: {name}"
        );
        if name.ends_with(".ref") {
            assert!(
                artifact.bytes.starts_with(b"REFT\x02"),
                "invalid reftable table: {name}"
            );
            assert!(
                artifact.bytes.len() >= 28,
                "truncated reftable table: {name}"
            );
            assert_eq!(
                &artifact.bytes[24..28],
                b"s256",
                "wrong reftable hash id: {name}"
            );
        }
    }
}

fn reftable_artifacts(repo: &Path) -> BTreeMap<String, PackArtifactSnapshot> {
    let directory = repo.join(".git/reftable");
    let Ok(entries) = fs::read_dir(&directory) else {
        return BTreeMap::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| {
            let path = entry.path();
            let metadata = fs::metadata(&path).expect("stat reftable artifact");
            let mode = {
                #[cfg(unix)]
                {
                    metadata.mode() & 0o777
                }
                #[cfg(not(unix))]
                {
                    0
                }
            };
            (
                path.file_name()
                    .expect("reftable artifact name")
                    .to_string_lossy()
                    .into_owned(),
                PackArtifactSnapshot {
                    mode,
                    bytes: fs::read(path).expect("read reftable artifact"),
                },
            )
        })
        .collect()
}

fn assert_active_uri_hashes(remote: &RemoteFixture) {
    let width = remote.format.hex_len();
    assert!(
        remote.uris.iter().all(|pack| pack.hash.len() == width
            && pack.hash.bytes().all(|byte| byte.is_ascii_hexdigit())),
        "URI content hashes must use the active object format width"
    );
}

fn command_text(cwd: &Path, args: &[&str]) -> String {
    let mut command = Command::new(zmin_bin());
    command.args(args).current_dir(cwd);
    HermeticGitChildEnvironment::apply(&mut command, cwd);
    let output = command.output().expect("run zmin text command");
    let output = assert_success(output, "zmin text command");
    String::from_utf8(output.stdout)
        .expect("zmin output UTF-8")
        .trim()
        .to_owned()
}

fn assert_pack_layout(repo: &Path) {
    let pack_dir = repo.join(".git/objects/pack");
    let entries = fs::read_dir(&pack_dir).expect("read clone pack directory");
    let mut packs = BTreeMap::new();
    for entry in entries {
        let path = entry.expect("read pack entry").path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("pack") {
            let stem = path.file_stem().expect("pack stem").to_owned();
            packs.insert(stem, path);
        }
    }
    assert!(!packs.is_empty(), "clone should retain indexed packs");
    for pack in packs.values() {
        let idx = pack.with_extension("idx");
        assert!(idx.is_file(), "missing pack index: {}", idx.display());
    }
    let leftovers = fs::read_dir(&pack_dir)
        .expect("read pack directory leftovers")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension == "tmp" || extension == "lock")
        })
        .collect::<Vec<_>>();
    assert!(leftovers.is_empty(), "pack temp/lock leaked: {leftovers:?}");
}

fn pack_artifact_names(repo: &Path) -> BTreeMap<String, usize> {
    fs::read_dir(repo.join(".git/objects/pack"))
        .expect("read pack artifact names")
        .map(|entry| {
            let path = entry.expect("read pack artifact entry").path();
            let name = path
                .file_name()
                .expect("pack artifact name")
                .to_string_lossy()
                .into_owned();
            let size = fs::metadata(&path).expect("stat pack artifact").len() as usize;
            (name, size)
        })
        .collect()
}

fn assert_uri_request_order(log: &RequestLog, expected_uri_count: usize) {
    let uri_indices = log
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.path.starts_with("/uri/"))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(uri_indices.len(), expected_uri_count);
    let last_origin_post = log
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.method == "POST")
        .map(|(index, _)| index)
        .max()
        .expect("origin upload-pack POST");
    assert!(
        uri_indices.iter().all(|index| *index > last_origin_post),
        "URI GET preceded origin POST: {log:?}"
    );
}

fn assert_filtered_fetch_request(log: &RequestLog, expected_uri_count: usize) {
    let fetch_requests = log
        .records
        .iter()
        .filter(|record| {
            record.method == "POST"
                && record
                    .body
                    .windows(b"command=fetch".len())
                    .any(|window| window == b"command=fetch")
        })
        .collect::<Vec<_>>();
    assert_eq!(fetch_requests.len(), 1, "expected one fetch POST: {log:?}");
    let body = &fetch_requests[0].body;
    assert!(
        body.windows(b"filter blob:none".len())
            .any(|window| window == b"filter blob:none"),
        "fetch request omitted exact filter: {log:?}"
    );
    assert!(
        body.windows(b"packfile-uris https".len())
            .any(|window| window == b"packfile-uris https"),
        "fetch request omitted exact packfile-uris feature: {log:?}"
    );
    assert_uri_request_order(log, expected_uri_count);
}

fn run_clone_pair(
    remote: &RemoteFixture,
    advertise_packfile_uris: bool,
    uri_protocols: &str,
    format: RefFormat,
    failure: UriFailure,
) -> (Output, Output, RequestLog, RequestLog) {
    let root = TempDir::new().expect("clone pair root");
    let bundle = validated_http_bundle();
    let config = FixtureConfig {
        remote: remote.clone(),
        advertise_packfile_uris,
        failure,
        cross_origin_uri_base: None,
        redirect_uri_base: None,
    };
    let expected_per_client = 3 + if advertise_packfile_uris
        && !uri_protocols.is_empty()
        && failure != UriFailure::SidebandFatal
    {
        remote.uris.len()
    } else {
        0
    };
    let server = FixtureHttpServer::new(config);
    let stock_destination = root.path().join("stock");
    let zmin_destination = root.path().join("zmin");
    let stock_args = clone_args(&server.url(), &stock_destination, format, uri_protocols);
    let zmin_args = clone_args(&server.url(), &zmin_destination, format, uri_protocols);
    let stock = run_pinned_git(root.path(), &stock_args, &bundle);
    server.wait_for_requests(expected_per_client);
    let stock_log = server.log();
    let stock_count = stock_log.records.len();
    let zmin = run_zmin(root.path(), &zmin_args);
    server.wait_for_requests(expected_per_client * 2);
    let combined_log = server.log();
    let zmin_log = RequestLog {
        records: combined_log.records.into_iter().skip(stock_count).collect(),
    };
    if stock.status.success() && zmin.status.success() {
        assert_clone_state(&stock_destination, &zmin_destination);
    }
    (stock, zmin, stock_log, zmin_log)
}

fn run_fetch_pair(
    remote: &RemoteFixture,
    failure: UriFailure,
) -> (
    TempDir,
    TempDir,
    TempDir,
    Output,
    Output,
    RequestLog,
    RequestLog,
) {
    run_fetch_pair_with_backend(remote, failure, ObjectFormat::Sha1, RefFormat::Files)
}

fn run_fetch_pair_with_backend(
    remote: &RemoteFixture,
    failure: UriFailure,
    object_format: ObjectFormat,
    ref_format: RefFormat,
) -> (
    TempDir,
    TempDir,
    TempDir,
    Output,
    Output,
    RequestLog,
    RequestLog,
) {
    run_fetch_pair_with_backend_and_filter(remote, failure, object_format, ref_format, None)
}

fn run_fetch_pair_with_backend_and_filter(
    remote: &RemoteFixture,
    failure: UriFailure,
    object_format: ObjectFormat,
    ref_format: RefFormat,
    filter: Option<&str>,
) -> (
    TempDir,
    TempDir,
    TempDir,
    Output,
    Output,
    RequestLog,
    RequestLog,
) {
    let root = TempDir::new().expect("fetch pair root");
    let bundle = validated_http_bundle();
    let config = FixtureConfig {
        remote: remote.clone(),
        advertise_packfile_uris: true,
        failure,
        cross_origin_uri_base: None,
        redirect_uri_base: None,
    };
    let expected_uri_count = filter.map_or(remote.uris.len(), |_| remote.filtered_uris.len());
    let expected_per_client = 3 + if failure != UriFailure::SidebandFatal {
        expected_uri_count
    } else {
        0
    };
    let server = FixtureHttpServer::new(config);
    let stock_dir = TempDir::new_in(root.path()).expect("stock fetch repo");
    let zmin_dir = TempDir::new_in(root.path()).expect("zmin fetch repo");
    let remote_url = server.url();
    init_fetch_repo(stock_dir.path(), &remote_url, object_format, ref_format);
    init_fetch_repo(zmin_dir.path(), &remote_url, object_format, ref_format);
    let args = || {
        let mut args = vec![
            "-c".into(),
            "protocol.version=2".into(),
            "-c".into(),
            "fetch.uriprotocols=https".into(),
            "fetch".into(),
        ];
        if let Some(filter) = filter {
            args.push(format!("--filter={filter}"));
            args.extend(["origin".into(), "main".into()]);
        } else {
            args.extend([
                "origin".into(),
                "refs/heads/main:refs/remotes/origin/main".into(),
            ]);
        }
        args
    };
    let stock = run_pinned_git(stock_dir.path(), &args(), &bundle);
    server.wait_for_requests(expected_per_client);
    let stock_log = server.log();
    let stock_count = stock_log.records.len();
    let zmin = run_zmin(zmin_dir.path(), &args());
    server.wait_for_requests(expected_per_client * 2);
    let combined_log = server.log();
    let zmin_log = RequestLog {
        records: combined_log.records.into_iter().skip(stock_count).collect(),
    };
    (root, stock_dir, zmin_dir, stock, zmin, stock_log, zmin_log)
}

#[test]
fn packfile_uri_capability_and_config_matrix_is_plain_fetch() {
    let root = TempDir::new().expect("matrix root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha1, 1);
    for (advertise, protocols) in [(false, "https"), (true, "")] {
        let (stock, zmin, stock_log, zmin_log) = run_clone_pair(
            &remote,
            advertise,
            protocols,
            RefFormat::Files,
            UriFailure::None,
        );
        assert!(
            stock.status.success(),
            "pinned plain clone failed: {stock:?}"
        );
        assert!(
            zmin.status.success(),
            "zmin plain clone failed: {zmin:?}, requests={zmin_log:?}"
        );
        assert_eq!(
            stock_log
                .records
                .iter()
                .filter(|record| record.path.starts_with("/uri/"))
                .count(),
            0
        );
        assert_eq!(
            zmin_log
                .records
                .iter()
                .filter(|record| record.path.starts_with("/uri/"))
                .count(),
            0
        );
    }
}

#[test]
fn packfile_uri_one_and_two_records_match_pinned_git_sha1_files() {
    let root = TempDir::new().expect("success root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha1, 1);
    let (_root, stock_dir, zmin_dir, stock, zmin, stock_log, zmin_log) =
        run_fetch_pair(&remote, UriFailure::None);
    assert!(stock.status.success(), "pinned URI clone failed: {stock:?}");
    assert!(zmin.status.success(), "zmin URI clone failed: {zmin:?}");
    assert_uri_request_order(&stock_log, 1);
    assert_uri_request_order(&zmin_log, 1);
    assert_fetch_state(stock_dir.path(), zmin_dir.path());
    assert_success_pack_roles(stock_dir.path(), &remote);
    assert_success_pack_roles(zmin_dir.path(), &remote);
}

#[test]
fn packfile_uri_sha256_reftable_filter_promisor_matches_pinned_git() {
    let root = TempDir::new().expect("sha256 root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha256, 2);
    let (stock, zmin, stock_log, zmin_log) = run_clone_pair(
        &remote,
        true,
        "https",
        RefFormat::Reftable,
        UriFailure::None,
    );
    assert!(
        stock.status.success(),
        "pinned SHA256 clone failed: {stock:?}"
    );
    assert!(zmin.status.success(), "zmin SHA256 clone failed: {zmin:?}");
    assert_eq!(
        stock_log
            .records
            .iter()
            .filter(|record| record.path.starts_with("/uri/"))
            .count(),
        2
    );
    assert_eq!(
        zmin_log
            .records
            .iter()
            .filter(|record| record.path.starts_with("/uri/"))
            .count(),
        2
    );
}

#[test]
fn packfile_uri_filtered_sha1_files_marks_inline_and_uri_packs() {
    let root = TempDir::new().expect("filtered SHA1 root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha1, 1);
    let (_fetch_root, stock_dir, zmin_dir, stock, zmin, stock_log, zmin_log) =
        run_fetch_pair_with_backend_and_filter(
            &remote,
            UriFailure::None,
            ObjectFormat::Sha1,
            RefFormat::Files,
            Some("blob:none"),
        );
    assert!(
        stock.status.success(),
        "pinned filtered fetch failed: {stock:?}"
    );
    assert!(
        zmin.status.success(),
        "zmin filtered fetch failed: {zmin:?}"
    );
    assert_filtered_fetch_request(&stock_log, 1);
    assert_filtered_fetch_request(&zmin_log, 1);
    assert_filtered_refs_and_fetch_head(stock_dir.path(), zmin_dir.path());
    assert_filtered_object_state(stock_dir.path(), zmin_dir.path(), &remote);
    let stock_snapshot = failure_snapshot(stock_dir.path());
    let zmin_snapshot = failure_snapshot(zmin_dir.path());
    assert_success_pack_snapshots_match(&stock_snapshot, &zmin_snapshot);
    assert_filtered_success_pack_roles(stock_dir.path(), &remote);
    assert_filtered_success_pack_roles(zmin_dir.path(), &remote);
}

#[test]
fn packfile_uri_filtered_sha256_reftable_marks_every_received_pack() {
    let root = TempDir::new().expect("filtered SHA256 root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha256, 1);
    let (_fetch_root, stock_dir, zmin_dir, stock, zmin, stock_log, zmin_log) =
        run_fetch_pair_with_backend_and_filter(
            &remote,
            UriFailure::None,
            ObjectFormat::Sha256,
            RefFormat::Reftable,
            Some("blob:none"),
        );
    assert!(
        stock.status.success(),
        "pinned filtered SHA256 fetch failed: {stock:?}"
    );
    assert!(
        zmin.status.success(),
        "zmin filtered SHA256 fetch failed: {zmin:?}"
    );
    assert_filtered_fetch_request(&stock_log, 1);
    assert_filtered_fetch_request(&zmin_log, 1);
    assert_filtered_refs_and_fetch_head(stock_dir.path(), zmin_dir.path());
    assert_fetch_backend_state(stock_dir.path(), zmin_dir.path(), RefFormat::Reftable);
    assert_filtered_object_state(stock_dir.path(), zmin_dir.path(), &remote);
    let stock_snapshot = failure_snapshot(stock_dir.path());
    let zmin_snapshot = failure_snapshot(zmin_dir.path());
    assert_success_pack_snapshots_match(&stock_snapshot, &zmin_snapshot);
    assert_filtered_success_pack_roles(stock_dir.path(), &remote);
    assert_filtered_success_pack_roles(zmin_dir.path(), &remote);
}

#[test]
fn packfile_uri_filtered_wrong_hash_matches_promisor_failure_residue() {
    let root = TempDir::new().expect("filtered wrong hash root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha1, 1);
    let (_fetch_root, stock_dir, zmin_dir, stock, zmin, stock_log, zmin_log) =
        run_fetch_pair_with_backend_and_filter(
            &remote,
            UriFailure::WrongHash,
            ObjectFormat::Sha1,
            RefFormat::Files,
            Some("blob:none"),
        );
    assert_eq!(stock.status.code(), Some(128), "stock status: {stock:?}");
    assert_eq!(zmin.status.code(), Some(128), "zmin status: {zmin:?}");
    assert_filtered_fetch_request(&stock_log, 1);
    assert_filtered_fetch_request(&zmin_log, 1);
    assert_failure_stderr(&stock.stderr, &zmin.stderr, UriFailure::WrongHash);
    let stock_snapshot = failure_snapshot(stock_dir.path());
    let zmin_snapshot = failure_snapshot(zmin_dir.path());
    assert_failure_snapshots_match(&stock_snapshot, &zmin_snapshot);
    assert_filtered_failure_snapshot(
        &stock_snapshot,
        stock_dir.path(),
        &remote,
        UriFailure::WrongHash,
    );
}

#[test]
fn packfile_uri_sha256_files_fetch_uses_active_64_hex_hashes() {
    let root = TempDir::new().expect("SHA256 files root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha256, 1);
    assert_active_uri_hashes(&remote);
    let (_fetch_root, stock_dir, zmin_dir, stock, zmin, stock_log, zmin_log) =
        run_fetch_pair_with_backend(
            &remote,
            UriFailure::None,
            ObjectFormat::Sha256,
            RefFormat::Files,
        );
    assert!(
        stock.status.success(),
        "pinned SHA256 files fetch failed: {stock:?}"
    );
    assert!(
        zmin.status.success(),
        "zmin SHA256 files fetch failed: {zmin:?}"
    );
    assert_uri_request_order(&stock_log, 1);
    assert_uri_request_order(&zmin_log, 1);
    assert_fetch_state(stock_dir.path(), zmin_dir.path());
    assert_fetch_backend_state(stock_dir.path(), zmin_dir.path(), RefFormat::Files);
    assert_success_pack_roles(stock_dir.path(), &remote);
    assert_success_pack_roles(zmin_dir.path(), &remote);
}

#[test]
fn packfile_uri_sha256_reftable_fetch_matches_pinned_backend_state() {
    let root = TempDir::new().expect("SHA256 reftable root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha256, 1);
    assert_active_uri_hashes(&remote);
    let (_fetch_root, stock_dir, zmin_dir, stock, zmin, stock_log, zmin_log) =
        run_fetch_pair_with_backend(
            &remote,
            UriFailure::None,
            ObjectFormat::Sha256,
            RefFormat::Reftable,
        );
    assert!(
        stock.status.success(),
        "pinned SHA256 reftable fetch failed: {stock:?}"
    );
    assert!(
        zmin.status.success(),
        "zmin SHA256 reftable fetch failed: {zmin:?}"
    );
    assert_uri_request_order(&stock_log, 1);
    assert_uri_request_order(&zmin_log, 1);
    assert_fetch_state(stock_dir.path(), zmin_dir.path());
    assert_fetch_backend_state(stock_dir.path(), zmin_dir.path(), RefFormat::Reftable);
    assert_success_pack_roles(stock_dir.path(), &remote);
    assert_success_pack_roles(zmin_dir.path(), &remote);
}

#[test]
fn packfile_uri_sha256_wrong_64_hex_hash_matches_failure_residue() {
    let root = TempDir::new().expect("SHA256 wrong hash root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha256, 1);
    assert_active_uri_hashes(&remote);
    let (_fetch_root, stock_dir, zmin_dir, stock, zmin, stock_log, zmin_log) =
        run_fetch_pair_with_backend(
            &remote,
            UriFailure::WrongHash,
            ObjectFormat::Sha256,
            RefFormat::Files,
        );
    assert_eq!(stock.status.code(), Some(128), "stock status: {stock:?}");
    assert_eq!(zmin.status.code(), Some(128), "zmin status: {zmin:?}");
    assert_uri_request_order(&stock_log, 1);
    assert_uri_request_order(&zmin_log, 1);
    assert_failure_stderr(&stock.stderr, &zmin.stderr, UriFailure::WrongHash);
    let stock_snapshot = failure_snapshot(stock_dir.path());
    let zmin_snapshot = failure_snapshot(zmin_dir.path());
    assert_failure_snapshots_match(&stock_snapshot, &zmin_snapshot);
    assert_failure_snapshot(
        &stock_snapshot,
        stock_dir.path(),
        &remote,
        UriFailure::WrongHash,
    );
    assert_fetch_backend_state(stock_dir.path(), zmin_dir.path(), RefFormat::Files);
}

#[test]
fn packfile_uri_wrong_hash_and_404_match_pinned_failure_lifecycle() {
    for failure in [UriFailure::WrongHash, UriFailure::NotFound] {
        let root = TempDir::new().expect("failure root");
        let remote = prepare_remote(root.path(), ObjectFormat::Sha1, 1);
        let (_fetch_root, stock_dir, zmin_dir, stock, zmin, stock_log, zmin_log) =
            run_fetch_pair(&remote, failure);
        assert_eq!(stock.status.code(), Some(128), "stock status: {stock:?}");
        assert_eq!(zmin.status.code(), Some(128), "zmin status: {zmin:?}");
        assert_uri_request_order(&stock_log, 1);
        assert_uri_request_order(&zmin_log, 1);
        assert_failure_stderr(&stock.stderr, &zmin.stderr, failure);
        let stock_snapshot = failure_snapshot(stock_dir.path());
        let zmin_snapshot = failure_snapshot(zmin_dir.path());
        assert_failure_snapshots_match(&stock_snapshot, &zmin_snapshot);
        assert_failure_snapshot(&stock_snapshot, stock_dir.path(), &remote, failure);
    }
}

#[test]
fn packfile_uri_sideband_progress_and_fatal_are_handled() {
    let root = TempDir::new().expect("sideband root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha1, 1);
    let (stock, zmin, stock_log, zmin_log) =
        run_clone_pair(&remote, true, "https", RefFormat::Files, UriFailure::None);
    assert!(stock.status.success());
    assert!(zmin.status.success());
    assert!(
        stock_log
            .records
            .iter()
            .any(|record| record.path.starts_with("/uri/"))
    );
    assert!(
        zmin_log
            .records
            .iter()
            .any(|record| record.path.starts_with("/uri/"))
    );

    let config = FixtureConfig {
        remote,
        advertise_packfile_uris: true,
        failure: UriFailure::SidebandFatal,
        cross_origin_uri_base: None,
        redirect_uri_base: None,
    };
    let server = FixtureHttpServer::new(config);
    let stock_destination = root.path().join("fatal-stock");
    let zmin_destination = root.path().join("fatal-zmin");
    let stock_args = clone_args(&server.url(), &stock_destination, RefFormat::Files, "https");
    let zmin_args = clone_args(&server.url(), &zmin_destination, RefFormat::Files, "https");
    let stock = run_pinned_git(root.path(), &stock_args, &validated_http_bundle());
    server.wait_for_requests(3);
    let _stock_log = server.log();
    let zmin = run_zmin(root.path(), &zmin_args);
    server.wait_for_requests(6);
    assert!(!stock.status.success());
    assert!(!zmin.status.success());
    assert!(String::from_utf8_lossy(&zmin.stderr).contains("fixture fatal"));
}

#[test]
fn packfile_uri_same_origin_credentials_and_headers_match_pinned_git() {
    let root = TempDir::new().expect("same-origin security root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha1, 1);
    let result = run_security_fetch_pair(&remote, SecurityRoute::SameOriginRedirect);
    assert!(
        result.stock.status.success(),
        "pinned security fetch failed: {:?} {}",
        result.stock.status,
        security_output_summary(&result.stock)
    );
    assert!(
        result.zmin.status.success(),
        "zmin security fetch failed: {:?} {}",
        result.zmin.status,
        security_output_summary(&result.zmin)
    );
    assert_security_fetch_state(result.stock_dir.path(), result.zmin_dir.path());
    assert_security_logs_match(&result.origin_stock, &result.origin_zmin, "same-origin");
    assert_security_uri_order(&result.origin_stock, 2);
    assert_security_header_present(&result.origin_stock, "/git-upload-pack", "Authorization");
    assert_security_header_present(&result.origin_stock, "/git-upload-pack", "X-Origin-Secret");
    assert_security_header_present(&result.origin_stock, "/git-upload-pack", "Cookie");
    assert_security_header_present(
        &result.origin_stock,
        "/git-upload-pack",
        "Proxy-Authorization",
    );
    assert_security_header_present(&result.origin_stock, "/uri/", "X-Origin-Secret");
    assert_security_header_present(&result.origin_stock, "/uri/", "Authorization");
    assert_security_header_present(&result.origin_stock, "/uri/", "Cookie");
    assert_security_header_present(&result.origin_stock, "/uri/", "Proxy-Authorization");
    assert_security_header_present(&result.origin_zmin, "/uri/", "X-Origin-Secret");
}

#[test]
fn packfile_uri_cross_origin_credentials_do_not_leak_to_cdn() {
    let root = TempDir::new().expect("cross-origin security root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha1, 1);
    let result = run_security_fetch_pair(&remote, SecurityRoute::CrossOrigin);
    assert!(
        result.stock.status.success(),
        "pinned cross-origin fetch failed: {:?} {}",
        result.stock.status,
        security_output_summary(&result.stock)
    );
    assert!(
        result.zmin.status.success(),
        "zmin cross-origin fetch failed: {:?} {}",
        result.zmin.status,
        security_output_summary(&result.zmin)
    );
    assert_security_fetch_state(result.stock_dir.path(), result.zmin_dir.path());
    assert_security_logs_match(&result.origin_stock, &result.origin_zmin, "origin");
    let stock_cdn = result.secondary_stock.as_ref().expect("stock CDN log");
    let zmin_cdn = result.secondary_zmin.as_ref().expect("zmin CDN log");
    assert_security_logs_match(stock_cdn, zmin_cdn, "CDN");
    assert_security_header_present(&result.origin_stock, "/git-upload-pack", "Authorization");
    assert_security_header_present(&result.origin_stock, "/git-upload-pack", "X-Origin-Secret");
    assert_security_header_present(&result.origin_stock, "/git-upload-pack", "Cookie");
    assert_security_header_present(
        &result.origin_stock,
        "/git-upload-pack",
        "Proxy-Authorization",
    );
    for log in [stock_cdn, zmin_cdn] {
        for name in [
            "Authorization",
            "Cookie",
            "Proxy-Authorization",
            "X-Origin-Secret",
            "X-Cdn-Secret",
        ] {
            assert_security_header_absent(log, name);
        }
    }
}

#[test]
fn packfile_uri_cross_origin_redirect_strips_sensitive_headers() {
    let root = TempDir::new().expect("redirect security root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha1, 1);
    let result = run_security_fetch_pair(&remote, SecurityRoute::CrossOriginRedirect);
    assert!(
        result.stock.status.success(),
        "pinned cross-origin redirect fetch failed: {:?} {}",
        result.stock.status,
        security_output_summary(&result.stock)
    );
    assert!(
        result.zmin.status.success(),
        "zmin redirect fetch failed: {:?} {}",
        result.zmin.status,
        security_output_summary(&result.zmin)
    );
    assert_security_fetch_state(result.stock_dir.path(), result.zmin_dir.path());
    assert_security_logs_match(&result.origin_stock, &result.origin_zmin, "origin");
    let stock_cdn = result.secondary_stock.as_ref().expect("stock redirect log");
    let zmin_cdn = result.secondary_zmin.as_ref().expect("zmin redirect log");
    let stock_third = result.third_stock.as_ref().expect("stock third-origin log");
    let zmin_third = result.third_zmin.as_ref().expect("zmin third-origin log");
    assert_security_logs_match(stock_cdn, zmin_cdn, "redirect CDN");
    assert_security_logs_match(stock_third, zmin_third, "third origin");
    for log in [stock_cdn, zmin_cdn, stock_third, zmin_third] {
        for name in [
            "Authorization",
            "Cookie",
            "Proxy-Authorization",
            "X-Origin-Secret",
            "X-Cdn-Secret",
        ] {
            assert_security_header_absent(log, name);
        }
    }
}

#[test]
fn packfile_uri_cross_origin_redirect_cdn_header_matches_pinned_git() {
    let root = TempDir::new().expect("scoped redirect security root");
    let remote = prepare_remote(root.path(), ObjectFormat::Sha1, 1);
    let result = run_security_fetch_pair(&remote, SecurityRoute::CrossOriginRedirectWithCdnHeader);
    assert!(
        result.stock.status.success(),
        "pinned scoped redirect fetch failed: {:?} {}",
        result.stock.status,
        security_output_summary(&result.stock)
    );
    assert!(
        result.zmin.status.success(),
        "zmin scoped redirect fetch failed: {:?} {}",
        result.zmin.status,
        security_output_summary(&result.zmin)
    );
    assert_security_fetch_state(result.stock_dir.path(), result.zmin_dir.path());
    let stock_cdn = result
        .secondary_stock
        .as_ref()
        .expect("stock scoped CDN log");
    let zmin_cdn = result.secondary_zmin.as_ref().expect("zmin scoped CDN log");
    let stock_third = result.third_stock.as_ref().expect("stock scoped third log");
    let zmin_third = result.third_zmin.as_ref().expect("zmin scoped third log");
    assert_security_logs_match(stock_cdn, zmin_cdn, "scoped redirect CDN");
    assert_security_logs_match(stock_third, zmin_third, "scoped third origin");
    for name in [
        "Authorization",
        "Cookie",
        "Proxy-Authorization",
        "X-Cdn-Secret",
    ] {
        assert_security_header_present(stock_cdn, "/uri/", name);
    }
    assert_security_header_absent(stock_third, "Authorization");
    assert_security_header_absent(stock_third, "Cookie");
    assert_security_header_present(stock_third, "/uri/", "Proxy-Authorization");
    assert_security_header_present(stock_third, "/uri/", "X-Cdn-Secret");
    assert_security_header_absent(zmin_third, "Authorization");
    assert_security_header_absent(zmin_third, "Cookie");
    assert_security_header_present(zmin_third, "/uri/", "Proxy-Authorization");
    assert_security_header_present(zmin_third, "/uri/", "X-Cdn-Secret");
}

fn init_fetch_repo(
    repo: &Path,
    remote_url: &str,
    object_format: ObjectFormat,
    ref_format: RefFormat,
) {
    let mut init_args = vec!["init", "--quiet", "--initial-branch=main"];
    if object_format == ObjectFormat::Sha256 {
        init_args.push("--object-format=sha256");
    }
    if ref_format == RefFormat::Reftable {
        init_args.push("--ref-format=reftable");
    }
    let mut init = hermetic_pinned_git_command(repo);
    init.args(&init_args);
    assert_success(init.output().expect("init fetch repo"), "init fetch repo");
    assert_success(
        stock_command_with_input(repo, &["remote", "add", "origin", remote_url], &[]),
        "configure fetch remote",
    );
    fs::write(repo.join(".git/FETCH_HEAD"), b"baseline fetch head\n")
        .expect("write FETCH_HEAD baseline");
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PackArtifactSnapshot {
    mode: u32,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FailureSnapshot {
    refs: String,
    fetch_head: Vec<u8>,
    packs: BTreeMap<String, PackArtifactSnapshot>,
}

fn failure_snapshot(repo: &Path) -> FailureSnapshot {
    let refs = String::from_utf8(stock_command_with_input(repo, &["show-ref"], &[]).stdout)
        .expect("failure refs UTF-8")
        .trim()
        .to_owned();
    let fetch_head = fs::read(repo.join(".git/FETCH_HEAD")).unwrap_or_default();
    let mut packs = BTreeMap::new();
    let pack_dir = repo.join(".git/objects/pack");
    for entry in fs::read_dir(&pack_dir).expect("read failure pack directory") {
        let path = entry.expect("failure pack entry").path();
        if path.is_file() {
            let mode = {
                #[cfg(unix)]
                {
                    fs::metadata(&path)
                        .expect("stat failure pack artifact")
                        .mode()
                        & 0o777
                }
                #[cfg(not(unix))]
                {
                    0
                }
            };
            packs.insert(
                path.file_name()
                    .expect("failure pack name")
                    .to_string_lossy()
                    .into_owned(),
                PackArtifactSnapshot {
                    mode,
                    bytes: fs::read(path).expect("read failure pack artifact"),
                },
            );
        }
    }
    FailureSnapshot {
        refs,
        fetch_head,
        packs,
    }
}

fn assert_failure_stderr(stock: &[u8], zmin: &[u8], failure: UriFailure) {
    let stock = String::from_utf8_lossy(stock);
    let zmin = String::from_utf8_lossy(zmin);
    let marker = match failure {
        UriFailure::WrongHash => "does not match expected hash",
        UriFailure::NotFound => "404",
        _ => unreachable!("bounded failure slice only covers wrong hash and 404"),
    };
    assert!(
        stock.contains(marker),
        "stock stderr missing {marker}: {stock}"
    );
    assert!(
        zmin.contains(marker),
        "zmin stderr missing {marker}: {zmin}"
    );
}

fn assert_failure_snapshots_match(stock: &FailureSnapshot, zmin: &FailureSnapshot) {
    assert_eq!(stock.refs, zmin.refs, "refs differ");
    assert_eq!(stock.fetch_head, zmin.fetch_head, "FETCH_HEAD differs");
    assert_eq!(
        stock.packs.keys().collect::<Vec<_>>(),
        zmin.packs.keys().collect::<Vec<_>>()
    );
    for (name, stock_artifact) in &stock.packs {
        let zmin_artifact = zmin.packs.get(name).expect("zmin pack artifact missing");
        assert_eq!(
            stock_artifact.mode, zmin_artifact.mode,
            "mode differs for {name}"
        );
        if name.ends_with(".keep") {
            assert_keep_shape(name, stock_artifact);
            assert_keep_shape(name, zmin_artifact);
        } else {
            assert_eq!(
                stock_artifact.bytes, zmin_artifact.bytes,
                "bytes differ for {name}"
            );
        }
    }
}

fn assert_success_pack_roles(repo: &Path, remote: &RemoteFixture) {
    let snapshot = failure_snapshot(repo);
    assert_pack_family(&snapshot.packs, &remote.inline.hash, true, true);
    assert_pack_family(&snapshot.packs, &remote.uris[0].hash, true, false);
    assert_no_promisor_or_lock_temp(&snapshot.packs);
}

fn assert_filtered_success_pack_roles(repo: &Path, remote: &RemoteFixture) {
    let snapshot = failure_snapshot(repo);
    for pack in std::iter::once(&remote.filtered_inline).chain(remote.filtered_uris.iter()) {
        assert_pack_family(
            &snapshot.packs,
            &pack.hash,
            true,
            snapshot
                .packs
                .contains_key(&format!("pack-{}.keep", pack.hash)),
        );
        let marker = snapshot
            .packs
            .get(&format!("pack-{}.promisor", pack.hash))
            .unwrap_or_else(|| panic!("missing promisor marker for {}", pack.hash));
        assert_eq!(marker.mode, 0o600, "promisor marker mode differs");
    }
    assert_no_lock_temp(&snapshot.packs);
}

fn assert_no_lock_temp(artifacts: &BTreeMap<String, PackArtifactSnapshot>) {
    assert!(artifacts.keys().all(|name| {
        !name.ends_with(".lock") && !name.ends_with(".tmp") && !name.ends_with(".pack.temp")
    }));
}

fn assert_success_pack_snapshots_match(stock: &FailureSnapshot, zmin: &FailureSnapshot) {
    assert_eq!(stock.refs, zmin.refs, "refs differ");
    assert_eq!(stock.fetch_head, zmin.fetch_head, "FETCH_HEAD differs");
    assert_eq!(
        stock.packs.keys().collect::<Vec<_>>(),
        zmin.packs.keys().collect::<Vec<_>>()
    );
    for (name, stock_artifact) in &stock.packs {
        let zmin_artifact = zmin.packs.get(name).expect("zmin pack artifact missing");
        assert_eq!(
            stock_artifact.mode, zmin_artifact.mode,
            "mode differs for {name}"
        );
        if name.ends_with(".keep") {
            assert_keep_shape(name, stock_artifact);
            assert_keep_shape(name, zmin_artifact);
        } else {
            assert_eq!(
                stock_artifact.bytes, zmin_artifact.bytes,
                "bytes differ for {name}"
            );
        }
    }
}

fn assert_filtered_object_state(stock: &Path, zmin: &Path, remote: &RemoteFixture) {
    for object in [&remote.head, &remote.tree] {
        assert_object_exists(stock, object, true);
        assert_object_exists(zmin, object, false);
    }
    for blob in &remote.blob_ids {
        assert_object_missing(stock, blob, true);
        assert_object_missing(zmin, blob, false);
    }
}

fn assert_object_exists(repo: &Path, object: &str, pinned: bool) {
    let output = object_check(repo, object, pinned);
    assert!(
        output.status.success(),
        "expected object {object} in {}",
        if pinned { "pinned Git" } else { "zmin" }
    );
}

fn assert_object_missing(repo: &Path, object: &str, pinned: bool) {
    let output = object_check(repo, object, pinned);
    assert!(
        !output.status.success(),
        "expected lazy object {object} absent in {}",
        if pinned { "pinned Git" } else { "zmin" }
    );
}

fn object_check(repo: &Path, object: &str, pinned: bool) -> Output {
    let mut command = if pinned {
        hermetic_pinned_git_command(repo)
    } else {
        let mut command = Command::new(zmin_bin());
        HermeticGitChildEnvironment::apply(&mut command, repo);
        command.current_dir(repo);
        command
    };
    command
        .args(["cat-file", "-e", object])
        .env("GIT_NO_LAZY_FETCH", "1");
    command.output().expect("check filtered object")
}

fn assert_failure_snapshot(
    snapshot: &FailureSnapshot,
    repo: &Path,
    remote: &RemoteFixture,
    failure: UriFailure,
) {
    assert!(snapshot.refs.is_empty(), "refs changed: {}", snapshot.refs);
    assert!(
        snapshot.fetch_head.is_empty(),
        "FETCH_HEAD was not truncated"
    );
    let pack_dir = repo.join(".git/objects/pack");
    let inline = remote.inline.hash.as_str();
    assert_pack_family(&snapshot.packs, inline, true, true);
    let uri = remote.uris[0].hash.as_str();
    match failure {
        UriFailure::WrongHash => {
            assert_pack_family(&snapshot.packs, uri, true, true);
            assert!(
                !pack_dir
                    .join(format!(
                        "pack-{}.pack.temp",
                        "0".repeat(remote.format.hex_len())
                    ))
                    .exists()
            );
        }
        UriFailure::NotFound => {
            assert_pack_family(&snapshot.packs, uri, false, false);
            let temp = pack_dir.join(format!("pack-{uri}.pack.temp"));
            let artifact = snapshot
                .packs
                .get(
                    temp.file_name()
                        .expect("URI temp name")
                        .to_str()
                        .expect("URI temp UTF-8"),
                )
                .expect("stock URI temp residue");
            assert!(artifact.bytes.is_empty(), "URI temp is not empty");
            assert_eq!(artifact.mode, 0o644, "URI temp mode differs");
        }
        _ => unreachable!("bounded failure slice only covers wrong hash and 404"),
    }
    assert_no_promisor_or_lock_temp(&snapshot.packs);
}

fn assert_filtered_failure_snapshot(
    snapshot: &FailureSnapshot,
    repo: &Path,
    remote: &RemoteFixture,
    failure: UriFailure,
) {
    assert!(snapshot.refs.is_empty(), "refs changed: {}", snapshot.refs);
    assert!(
        snapshot.fetch_head.is_empty(),
        "FETCH_HEAD was not truncated"
    );
    assert_pack_family(&snapshot.packs, &remote.filtered_inline.hash, true, true);
    assert_promisor_marker(&snapshot.packs, &remote.filtered_inline.hash);
    let uri = remote.filtered_uris[0].hash.as_str();
    match failure {
        UriFailure::WrongHash => {
            assert_pack_family(&snapshot.packs, uri, true, true);
            assert_promisor_marker(&snapshot.packs, uri);
            assert!(
                !repo
                    .join(".git/objects/pack")
                    .join(format!(
                        "pack-{}.pack.temp",
                        "0".repeat(remote.format.hex_len())
                    ))
                    .exists()
            );
        }
        UriFailure::NotFound => {
            assert_pack_family(&snapshot.packs, uri, false, false);
            let temp = repo
                .join(".git/objects/pack")
                .join(format!("pack-{uri}.pack.temp"));
            let artifact = snapshot
                .packs
                .get(
                    temp.file_name()
                        .expect("filtered URI temp name")
                        .to_str()
                        .unwrap(),
                )
                .expect("stock filtered URI temp residue");
            assert!(artifact.bytes.is_empty(), "filtered URI temp is not empty");
            assert_eq!(artifact.mode, 0o644, "filtered URI temp mode differs");
        }
        _ => unreachable!("filtered failure slice only covers wrong hash and 404"),
    }
    assert_no_lock_temp(&snapshot.packs);
}

fn assert_pack_family(
    artifacts: &BTreeMap<String, PackArtifactSnapshot>,
    hash: &str,
    expect_pack: bool,
    expect_keep: bool,
) {
    for suffix in [".pack", ".idx", ".rev"] {
        assert_eq!(
            artifacts.contains_key(&format!("pack-{hash}{suffix}")),
            expect_pack,
            "unexpected {suffix} residue for {hash}"
        );
    }
    let keep = artifacts.get(&format!("pack-{hash}.keep"));
    assert_eq!(
        keep.is_some(),
        expect_keep,
        "unexpected keep residue for {hash}"
    );
    if let Some(keep) = keep {
        assert_keep_shape(hash, keep);
    }
}

fn assert_keep_shape(hash: &str, keep: &PackArtifactSnapshot) {
    assert_eq!(keep.mode, 0o600, "keep mode differs for {hash}");
    let content = String::from_utf8_lossy(&keep.bytes);
    assert!(
        content.starts_with("fetch-pack "),
        "invalid keep content: {content}"
    );
    assert!(content.contains(" on "), "invalid keep content: {content}");
    assert!(content.ends_with('\n'), "invalid keep content: {content}");
}

fn assert_promisor_marker(artifacts: &BTreeMap<String, PackArtifactSnapshot>, hash: &str) {
    let marker = artifacts
        .get(&format!("pack-{hash}.promisor"))
        .unwrap_or_else(|| panic!("missing promisor marker for {hash}"));
    assert_eq!(
        marker.mode, 0o600,
        "promisor marker mode differs for {hash}"
    );
}

fn assert_no_promisor_or_lock_temp(artifacts: &BTreeMap<String, PackArtifactSnapshot>) {
    assert!(
        artifacts
            .keys()
            .all(|name| { !name.ends_with(".promisor") && !name.ends_with(".lock") })
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SecurityRoute {
    SameOriginRedirect,
    CrossOrigin,
    CrossOriginRedirect,
    CrossOriginRedirectWithCdnHeader,
}

struct SecurityRequestCounts {
    origin: usize,
    secondary: Option<usize>,
    third: Option<usize>,
}

fn security_request_counts(route: SecurityRoute, uri_count: usize) -> SecurityRequestCounts {
    match route {
        SecurityRoute::SameOriginRedirect => SecurityRequestCounts {
            origin: 3 + uri_count * 2,
            secondary: None,
            third: None,
        },
        SecurityRoute::CrossOrigin => SecurityRequestCounts {
            origin: 3,
            secondary: Some(uri_count),
            third: None,
        },
        SecurityRoute::CrossOriginRedirect | SecurityRoute::CrossOriginRedirectWithCdnHeader => {
            SecurityRequestCounts {
                origin: 3,
                secondary: Some(uri_count),
                third: Some(uri_count),
            }
        }
    }
}

struct SecurityFetchPair {
    _root: TempDir,
    stock_dir: TempDir,
    zmin_dir: TempDir,
    stock: Output,
    zmin: Output,
    origin_stock: RequestLog,
    origin_zmin: RequestLog,
    secondary_stock: Option<RequestLog>,
    secondary_zmin: Option<RequestLog>,
    third_stock: Option<RequestLog>,
    third_zmin: Option<RequestLog>,
}

fn run_security_fetch_pair(remote: &RemoteFixture, route: SecurityRoute) -> SecurityFetchPair {
    let root = TempDir::new().expect("security fetch root");
    let (secondary, third, cross_origin_uri_base, redirect_uri_base) = match route {
        SecurityRoute::SameOriginRedirect => (None, None, None, Some("SELF".to_owned())),
        SecurityRoute::CrossOrigin => {
            let secondary = FixtureHttpServer::new(FixtureConfig {
                remote: remote.clone(),
                advertise_packfile_uris: false,
                failure: UriFailure::None,
                cross_origin_uri_base: None,
                redirect_uri_base: None,
            });
            let base = secondary.base_url();
            (Some(secondary), None, Some(base), None)
        }
        SecurityRoute::CrossOriginRedirect | SecurityRoute::CrossOriginRedirectWithCdnHeader => {
            let third = FixtureHttpServer::new(FixtureConfig {
                remote: remote.clone(),
                advertise_packfile_uris: false,
                failure: UriFailure::None,
                cross_origin_uri_base: None,
                redirect_uri_base: None,
            });
            let secondary = FixtureHttpServer::new(FixtureConfig {
                remote: remote.clone(),
                advertise_packfile_uris: false,
                failure: UriFailure::None,
                cross_origin_uri_base: None,
                redirect_uri_base: Some(third.base_url()),
            });
            let base = secondary.base_url();
            (Some(secondary), Some(third), Some(base), None)
        }
    };
    let origin = FixtureHttpServer::new(FixtureConfig {
        remote: remote.clone(),
        advertise_packfile_uris: true,
        failure: UriFailure::None,
        cross_origin_uri_base,
        redirect_uri_base,
    });
    let expected = security_request_counts(route, remote.uris.len());
    let remote_url = format!("http://127.0.0.1:{}/remote.git", origin.port);
    let header_config = format!(
        "http.{}.extraHeader=X-Origin-Secret: origin-secret",
        origin.base_url()
    );
    let authorization_config = format!(
        "http.{}.extraHeader=Authorization: Basic origin-user:origin-pass",
        origin.base_url()
    );
    let cookie_config = format!(
        "http.{}.extraHeader=Cookie: origin-cookie",
        origin.base_url()
    );
    let proxy_authorization_config = format!(
        "http.{}.extraHeader=Proxy-Authorization: Basic origin-proxy",
        origin.base_url()
    );
    let cdn_header_configs =
        matches!(route, SecurityRoute::CrossOriginRedirectWithCdnHeader).then(|| {
            let cdn_base = secondary
                .as_ref()
                .expect("CDN server for scoped header")
                .base_url();
            vec![
                format!("http.{cdn_base}.extraHeader=Authorization: Basic cdn-auth"),
                format!("http.{cdn_base}.extraHeader=Cookie: cdn-cookie"),
                format!("http.{cdn_base}.extraHeader=Proxy-Authorization: Basic cdn-proxy"),
                format!("http.{cdn_base}.extraHeader=X-Cdn-Secret: cdn-secret"),
            ]
        });
    let stock_dir = TempDir::new_in(root.path()).expect("security stock repo");
    let zmin_dir = TempDir::new_in(root.path()).expect("security zmin repo");
    init_fetch_repo(
        stock_dir.path(),
        &remote_url,
        ObjectFormat::Sha1,
        RefFormat::Files,
    );
    init_fetch_repo(
        zmin_dir.path(),
        &remote_url,
        ObjectFormat::Sha1,
        RefFormat::Files,
    );
    let mut args = vec![
        "-c".to_owned(),
        "protocol.version=2".to_owned(),
        "-c".to_owned(),
        "fetch.uriprotocols=https".to_owned(),
        "-c".to_owned(),
        authorization_config,
        "-c".to_owned(),
        header_config,
        "-c".to_owned(),
        cookie_config,
        "-c".to_owned(),
        proxy_authorization_config,
        "-c".to_owned(),
        "http.followRedirects=true".to_owned(),
    ];
    if let Some(cdn_header_configs) = cdn_header_configs {
        for cdn_header_config in cdn_header_configs {
            args.extend(["-c".to_owned(), cdn_header_config]);
        }
    }
    args.extend([
        "fetch".to_owned(),
        "origin".to_owned(),
        "refs/heads/main:refs/remotes/origin/main".to_owned(),
    ]);
    let stock = run_pinned_git(stock_dir.path(), &args, &validated_http_bundle());
    origin.wait_for_requests(expected.origin);
    if let (Some(server), Some(count)) = (&secondary, expected.secondary) {
        server.wait_for_requests(count);
    }
    if let (Some(server), Some(count)) = (&third, expected.third) {
        server.wait_for_requests(count);
    }
    let origin_stock_all = origin.log();
    let origin_stock_count = origin_stock_all.records.len();
    let secondary_stock_all = secondary.as_ref().map(FixtureHttpServer::log);
    let secondary_stock_counts = secondary_stock_all.as_ref().map(|log| log.records.len());
    let third_stock_all = third.as_ref().map(FixtureHttpServer::log);
    let third_stock_counts = third_stock_all.as_ref().map(|log| log.records.len());
    let zmin = run_zmin(zmin_dir.path(), &args);
    origin.wait_for_requests(expected.origin * 2);
    if let (Some(server), Some(count)) = (&secondary, expected.secondary) {
        server.wait_for_requests(count * 2);
    }
    if let (Some(server), Some(count)) = (&third, expected.third) {
        server.wait_for_requests(count * 2);
    }
    let origin_zmin_all = origin.log();
    let secondary_zmin_all = secondary.as_ref().map(FixtureHttpServer::log);
    let third_zmin_all = third.as_ref().map(FixtureHttpServer::log);
    SecurityFetchPair {
        _root: root,
        stock_dir,
        zmin_dir,
        stock,
        zmin,
        origin_stock: request_log_slice(&origin_stock_all, 0, origin_stock_count),
        origin_zmin: request_log_slice(&origin_zmin_all, origin_stock_count, usize::MAX),
        secondary_stock: secondary_stock_all
            .map(|log| request_log_slice(&log, 0, secondary_stock_counts.unwrap_or_default())),
        secondary_zmin: secondary_zmin_all.map(|log| {
            request_log_slice(&log, secondary_stock_counts.unwrap_or_default(), usize::MAX)
        }),
        third_stock: third_stock_all
            .map(|log| request_log_slice(&log, 0, third_stock_counts.unwrap_or_default())),
        third_zmin: third_zmin_all
            .map(|log| request_log_slice(&log, third_stock_counts.unwrap_or_default(), usize::MAX)),
    }
}

fn request_log_slice(log: &RequestLog, start: usize, end: usize) -> RequestLog {
    RequestLog {
        records: log
            .records
            .iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .cloned()
            .collect(),
    }
}

fn security_header(record: &RequestRecord, name: &str) -> Option<String> {
    record.headers.lines().skip(1).find_map(|line| {
        let (header_name, value) = line.split_once(':')?;
        header_name
            .trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

fn assert_security_logs_match(stock: &RequestLog, zmin: &RequestLog, label: &str) {
    assert_eq!(
        stock.records.len(),
        zmin.records.len(),
        "{label} request count differs"
    );
    for (index, (stock_record, zmin_record)) in stock.records.iter().zip(&zmin.records).enumerate()
    {
        assert_eq!(
            stock_record.method, zmin_record.method,
            "{label} method differs at {index}"
        );
        assert_eq!(
            stock_record.path, zmin_record.path,
            "{label} path differs at {index}"
        );
        for name in [
            "Authorization",
            "Cookie",
            "Proxy-Authorization",
            "X-Origin-Secret",
            "X-Cdn-Secret",
        ] {
            let stock_header = security_header(stock_record, name);
            let zmin_header = security_header(zmin_record, name);
            assert_eq!(
                stock_header.is_some(),
                zmin_header.is_some(),
                "{label} {name} presence differs at {index}"
            );
            assert!(
                stock_header == zmin_header,
                "{label} {name} value differs at {index}"
            );
        }
    }
}

fn assert_security_header_present(log: &RequestLog, path: &str, name: &str) {
    assert!(
        log.records
            .iter()
            .filter(|record| record.path.contains(path))
            .any(|record| security_header(record, name).is_some()),
        "expected {name} on {path} request"
    );
}

fn assert_security_header_absent(log: &RequestLog, name: &str) {
    assert!(
        log.records
            .iter()
            .all(|record| security_header(record, name).is_none()),
        "unexpected {name} on unrelated-origin request"
    );
}

fn security_output_summary(output: &Output) -> String {
    redact_sensitive_diagnostics(&String::from_utf8_lossy(&output.stderr))
}

fn assert_security_uri_order(log: &RequestLog, expected_uri_requests: usize) {
    let uri_indices = log
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.path.contains("/uri/"))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(uri_indices.len(), expected_uri_requests);
    let origin_post = log
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.method == "POST")
        .map(|(index, _)| index)
        .max()
        .expect("origin upload-pack POST");
    assert!(uri_indices.iter().all(|index| *index > origin_post));
}
