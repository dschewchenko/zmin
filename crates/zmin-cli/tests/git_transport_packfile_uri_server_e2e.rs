//! Pinned Git server-side packfile-URI oracle.
//!
//! This file exercises the server half of protocol v2 against both the pinned
//! Git oracle and Zmin.  The fixture either invokes an upload-pack over direct
//! stdio or puts that same stateless-rpc exchange behind a small HTTP server.
//! URI GETs are answered by the fixture; the upload-pack process never
//! dereferences a URI.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
use libc::{self, SIGKILL};

mod common;

use common::zmin_bin;
use tempfile::TempDir;

const PINNED_GIT: &str = "/Users/dschewchenko/.cache/zmin/git-upstream/http-bundle-v2.55.0-e9019fcafe0040228b8631c30f97ae1adb61bcdc-Darwin-arm64-with-http-fetch-pinned/git";
const PINNED_BUNDLE: &str = "/Users/dschewchenko/.cache/zmin/git-upstream/http-bundle-v2.55.0-e9019fcafe0040228b8631c30f97ae1adb61bcdc-Darwin-arm64-with-http-fetch-pinned";
const PINNED_DAEMON_BUNDLE: &str = "/Users/dschewchenko/.cache/zmin/git-upstream/git-daemon-v2.55.0-e9019fcafe0040228b8631c30f97ae1adb61bcdc-Darwin-arm64-pinned";
const PINNED_DAEMON: &str = "/Users/dschewchenko/.cache/zmin/git-upstream/git-daemon-v2.55.0-e9019fcafe0040228b8631c30f97ae1adb61bcdc-Darwin-arm64-pinned/git-daemon";
const PINNED_GIT_SHA256: &str = "ca63eda87df1aaffa2b80710c4a9de6212eba6c84e8dfb3011a2498b36e841cb";
const PINNED_DAEMON_SHA256: &str =
    "476cb91fe4b8f362da2136d4ecd5129c68c86973594350ced5a89d9a20c5c28b";
const PINNED_DAEMON_MANIFEST_SHA256: &str =
    "f9a00fcc8c39b3772c753b6b68bf029af50b156b27ee0efa3051aaf406a3505f";
const PINNED_DAEMON_MANIFEST_SIDECAR_SHA256: &str =
    "e92c1aba3f894e4f957c8570102c2ff63fc94615445720ff3817dafe5be89cac";
const PINNED_DAEMON_HASH_SIDECAR_SHA256: &str =
    "7adedf3bfaafebf6be5e69c09152669fd2bb5e9a41b5ffc75379a075b45231da";
const PINNED_DAEMON_UPLOAD_PACK_HASH_SIDECAR_SHA256: &str =
    "de42a0de7e3175452d1926fb8fd0cc2bb01bca61fdf787e17b44776a1c593eef";
const PINNED_HTTP_FETCH_SHA256: &str =
    "fc2e8b9e47cafb39140ca90f56fbc6cb09912c6d715feb020157733868f08b0e";
const PINNED_HTTP_BACKEND_SHA256: &str =
    "558316c4ea88b9e50327def4dc234c7404e11790ab77da1c2a6bf52879bd1b43";
const PINNED_REMOTE_HTTP_SHA256: &str =
    "6ba041e1c71c11eb3d5f66579ea29734135446a4575ec3ecf0574437b795b640";
const PINNED_MANIFEST_SHA256: &str =
    "e70ca5308dbddac10ac951a0a16a31645f9a7f4338be026e4257d961b3eb29ad";
const PINNED_BUNDLE_TABLE_SHA256: &str =
    "cc295dc42051d204e2505acfab55d43767260fdd0cb016c6e5d3f507cf74bfdb";
const PINNED_MANIFEST_SIDECAR_SHA256: &str =
    "55c0258449052cd26f3868e464b96d9fded1e479c6f10c5286bf700bf2a4f0fd";
const PINNED_BUNDLE_SIDECAR_SHA256: &str =
    "a65872f1994b02a855776fed8554710a104c2869073976b021c031e93a2c945b";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const WORKER_IO_TIMEOUT: Duration = Duration::from_millis(250);
const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(60);
const SERVER_JOIN_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_HEADER_BYTES: usize = 128 * 1024;
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_REQUESTS: usize = 64;
const MAX_CONNECTIONS: usize = 32;
const MAX_PACK_BYTES: usize = 2 * 1024 * 1024;
const REDACTED: &str = "<redacted>";

static ENV_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObjectFormat {
    Sha1,
    Sha256,
}

impl ObjectFormat {
    fn name(self) -> &'static str {
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
enum RefBackend {
    Files,
    Reftable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServerBackend {
    Stock,
    Zmin,
}

impl RefBackend {
    fn name(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Reftable => "reftable",
        }
    }
}

#[derive(Clone, Debug)]
struct PackSpec {
    object: String,
    hash: String,
    path: String,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
struct RemoteFixture {
    repo: PathBuf,
    format: ObjectFormat,
    head: String,
    tree: String,
    blobs: Vec<String>,
    uri_packs: Vec<PackSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UriGetMode {
    Serve,
    NotFound,
    Corrupt,
}

#[derive(Clone, Debug)]
struct RequestRecord {
    method: String,
    path: String,
    body: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
struct RequestLog {
    records: Vec<RequestRecord>,
}

#[derive(Clone, Debug)]
struct HttpConfig {
    remote: RemoteFixture,
    uri_get_mode: UriGetMode,
    backend: ServerBackend,
}

struct HermeticEnvironment;

impl HermeticEnvironment {
    fn apply(command: &mut Command, cwd: &Path) {
        let n = ENV_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = cwd.join(format!(".pack-uri-env-{}-{n}", std::process::id()));
        let home = root.join("home");
        let xdg = root.join("xdg");
        let template = root.join("template");
        fs::create_dir_all(&home).expect("create hermetic HOME");
        fs::create_dir_all(xdg.join("config")).expect("create hermetic XDG config");
        fs::create_dir_all(xdg.join("cache")).expect("create hermetic XDG cache");
        fs::create_dir_all(xdg.join("data")).expect("create hermetic XDG data");
        fs::create_dir_all(&template).expect("create hermetic template");
        fs::create_dir_all(root.join("gpg")).expect("create hermetic GnuPG home");

        command
            .env_clear()
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .env("XDG_DATA_HOME", xdg.join("data"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", null_device())
            .env("GIT_CONFIG_SYSTEM", null_device())
            .env("GIT_TEMPLATE_DIR", &template)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Pack URI Oracle")
            .env("GIT_AUTHOR_EMAIL", "pack-uri-oracle@example.test")
            .env("GIT_AUTHOR_DATE", "1700000000 +0000")
            .env("GIT_COMMITTER_NAME", "Pack URI Oracle")
            .env("GIT_COMMITTER_EMAIL", "pack-uri-oracle@example.test")
            .env("GIT_COMMITTER_DATE", "1700000000 +0000")
            .env("GNUPGHOME", root.join("gpg"))
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("GIT_EXEC_PATH", PINNED_BUNDLE);
    }
}

fn null_device() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

struct ChildGuard {
    child: Option<Child>,
    stderr_capture: Option<StderrCapture>,
}

impl ChildGuard {
    fn spawn(command: &mut Command, label: &str) -> Self {
        Self::spawn_inner(command, label, false)
    }

    fn spawn_with_stderr_capture(command: &mut Command, label: &str) -> Self {
        Self::spawn_inner(command, label, true)
    }

    fn spawn_inner(command: &mut Command, label: &str, capture_stderr: bool) -> Self {
        #[cfg(unix)]
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let debug = redact(&format!("{command:?}"));
        let mut child = command
            .spawn()
            .unwrap_or_else(|error| panic!("{label} spawn failed: {error}; command={debug}"));
        let stderr_capture = capture_stderr.then(|| {
            let stderr = child.stderr.take().expect("piped child stderr");
            StderrCapture::spawn(stderr)
        });
        Self {
            child: Some(child),
            stderr_capture,
        }
    }

    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("guarded child present")
    }

    fn take(&mut self) -> Child {
        self.child.take().expect("guarded child present")
    }

    fn terminate(&mut self) {
        if let Some(child) = self.child.as_mut() {
            #[cfg(unix)]
            unsafe {
                let pid = child.id() as libc::pid_t;
                let _ = libc::kill(-pid, SIGKILL);
            }
            let _ = child.kill();
        }
    }

    fn reap(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.wait();
        }
    }

    fn finish_stderr_capture(&mut self, label: &str) -> String {
        let Some(capture) = self.stderr_capture.take() else {
            return String::new();
        };
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let _ = join_child_thread(capture.handle, self, deadline, label);
        let result = capture
            .receiver
            .recv_timeout(Duration::from_millis(100))
            .unwrap_or_else(|error| panic!("{label} result missing: {error}"));
        match result {
            Ok(bytes) => redact(&String::from_utf8_lossy(&bytes)),
            Err(error) => redact(&error.to_string()),
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.terminate();
        self.reap();
        if self.stderr_capture.is_some() {
            let _ = self.finish_stderr_capture("child stderr capture");
        }
    }
}

struct StderrCapture {
    handle: JoinHandle<()>,
    receiver: Receiver<Result<Vec<u8>, CaptureError>>,
}

impl StderrCapture {
    fn spawn(stderr: impl Read + Send + 'static) -> Self {
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let _ = sender.send(capture_stream(stderr, MAX_CAPTURE_BYTES));
        });
        Self { handle, receiver }
    }
}

const MAX_CAPTURE_BYTES: usize = MAX_BODY_BYTES;

#[derive(Debug)]
enum CaptureError {
    Io(io::Error),
    Limit(usize),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "stream read failed: {error}"),
            Self::Limit(limit) => write!(formatter, "stream exceeded {limit} bytes"),
        }
    }
}

fn capture_stream<R: Read>(mut reader: R, limit: usize) -> Result<Vec<u8>, CaptureError> {
    let mut output = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0_u8; 8192];
    loop {
        let remaining = limit.saturating_sub(output.len());
        if remaining == 0 {
            let mut extra = [0_u8; 1];
            match reader.read(&mut extra).map_err(CaptureError::Io)? {
                0 => return Ok(output),
                _ => return Err(CaptureError::Limit(limit)),
            }
        }
        let read_limit = remaining.min(buffer.len());
        let count = reader
            .read(&mut buffer[..read_limit])
            .map_err(CaptureError::Io)?;
        if count == 0 {
            return Ok(output);
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

#[derive(Clone, Copy)]
enum CapturedStream {
    Stdout,
    Stderr,
}

type CaptureMessage = (CapturedStream, Result<Vec<u8>, CaptureError>);

fn spawn_capture<R: Read + Send + 'static>(
    reader: R,
    stream: CapturedStream,
    sender: mpsc::Sender<CaptureMessage>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let result = capture_stream(reader, MAX_CAPTURE_BYTES);
        let _ = sender.send((stream, result));
    })
}

fn join_child_thread<T>(
    handle: JoinHandle<T>,
    guard: &mut ChildGuard,
    deadline: Instant,
    label: &str,
) -> T {
    while !handle.is_finished() {
        if Instant::now() >= deadline {
            guard.terminate();
            guard.reap();
            panic!("{label} thread did not stop after process-group cancellation");
        }
        thread::sleep(Duration::from_millis(5));
    }
    handle
        .join()
        .unwrap_or_else(|_| panic!("{label} thread panicked"))
}

fn run_command(mut command: Command, label: &str, timeout: Duration) -> Output {
    run_process(&mut command, None, label, timeout)
}

fn run_command_input(mut command: Command, input: &[u8], label: &str, timeout: Duration) -> Output {
    assert!(input.len() <= MAX_BODY_BYTES, "{label} input exceeds bound");
    run_process(&mut command, Some(input), label, timeout)
}

fn run_process(
    command: &mut Command,
    input: Option<&[u8]>,
    label: &str,
    timeout: Duration,
) -> Output {
    let deadline = Instant::now() + timeout;
    command
        .stdin(input.map_or(Stdio::null(), |_| Stdio::piped()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut guard = ChildGuard::spawn(command, label);
    let stdin = guard.child_mut().stdin.take();
    let stdout = guard.child_mut().stdout.take().expect("child stdout");
    let stderr = guard.child_mut().stderr.take().expect("child stderr");
    let (sender, receiver) = mpsc::channel();
    let stdout_thread = spawn_capture(stdout, CapturedStream::Stdout, sender.clone());
    let stderr_thread = spawn_capture(stderr, CapturedStream::Stderr, sender);
    let stdin_thread = input.map(|bytes| bytes.to_vec()).map(|bytes| {
        thread::spawn(move || {
            if let Some(mut stdin) = stdin {
                let _ = stdin.write_all(&bytes);
            }
        })
    });
    let mut status = None;
    let mut stdout_result = None;
    let mut stderr_result = None;
    let mut capture_error = None;
    let mut fatal_error = None;
    while status.is_none() {
        drain_capture_messages(
            &receiver,
            &mut stdout_result,
            &mut stderr_result,
            &mut capture_error,
        );
        if capture_error.is_some() {
            guard.terminate();
            status = Some(guard.child_mut().wait().unwrap_or_else(|error| {
                panic!("{label} did not reap after capture limit: {error}")
            }));
            fatal_error = capture_error.clone();
            break;
        }
        match guard.child_mut().try_wait() {
            Ok(Some(exit)) => status = Some(exit),
            Ok(None) if Instant::now() >= deadline => {
                guard.terminate();
                status = Some(
                    guard
                        .child_mut()
                        .wait()
                        .unwrap_or_else(|error| panic!("{label} timeout reap failed: {error}")),
                );
                fatal_error = Some("watchdog timeout".to_owned());
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => {
                guard.terminate();
                status = Some(guard.child_mut().wait().unwrap_or_else(|wait_error| {
                    panic!("{label} wait failed: {error}; reap failed: {wait_error}")
                }));
                fatal_error = Some(format!("wait failed: {error}"));
            }
        }
    }
    // Reap the top-level process and terminate its process group before
    // joining capture threads. A remote helper can otherwise retain the
    // stdout/stderr pipes after its parent has exited successfully.
    guard.terminate();
    guard.reap();
    let join_deadline = Instant::now() + REQUEST_TIMEOUT;
    let _ = join_child_thread(stdout_thread, &mut guard, join_deadline, "stdout capture");
    let _ = join_child_thread(stderr_thread, &mut guard, join_deadline, "stderr capture");
    if let Some(stdin_thread) = stdin_thread {
        let _ = join_child_thread(stdin_thread, &mut guard, join_deadline, "stdin writer");
    }
    while stdout_result.is_none() || stderr_result.is_none() {
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok((stream, result)) => assign_capture_result(
                stream,
                result,
                &mut stdout_result,
                &mut stderr_result,
                &mut capture_error,
            ),
            Err(_) => break,
        }
    }
    if fatal_error.is_none() {
        fatal_error = capture_error.clone();
    }
    let status = status.expect("child status");
    if let Some(error) = fatal_error {
        panic!("{label} failed: {error}");
    }
    let stdout = stdout_result
        .expect("stdout capture result")
        .expect("stdout capture");
    let stderr = stderr_result
        .expect("stderr capture result")
        .expect("stderr capture");
    assert!(stdout.len() <= MAX_CAPTURE_BYTES && stderr.len() <= MAX_CAPTURE_BYTES);
    drop(guard.take());
    Output {
        status,
        stdout,
        stderr,
    }
}

fn assign_capture_result(
    stream: CapturedStream,
    result: Result<Vec<u8>, CaptureError>,
    stdout: &mut Option<Result<Vec<u8>, CaptureError>>,
    stderr: &mut Option<Result<Vec<u8>, CaptureError>>,
    capture_error: &mut Option<String>,
) {
    if let Err(error) = &result {
        *capture_error = Some(error.to_string());
    }
    match stream {
        CapturedStream::Stdout => *stdout = Some(result),
        CapturedStream::Stderr => *stderr = Some(result),
    }
}

fn drain_capture_messages(
    receiver: &Receiver<CaptureMessage>,
    stdout: &mut Option<Result<Vec<u8>, CaptureError>>,
    stderr: &mut Option<Result<Vec<u8>, CaptureError>>,
    capture_error: &mut Option<String>,
) {
    loop {
        match receiver.try_recv() {
            Ok((stream, result)) => {
                assign_capture_result(stream, result, stdout, stderr, capture_error)
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
        }
    }
}

fn generic_command(program: &Path, cwd: &Path) -> Command {
    let mut command = Command::new(program);
    HermeticEnvironment::apply(&mut command, cwd);
    command.current_dir(cwd);
    command
}

fn file_sha256(path: &Path, cwd: &Path) -> String {
    let mut command = generic_command(Path::new("/usr/bin/shasum"), cwd);
    command.args(["-a", "256", path.to_str().expect("hash path UTF-8")]);
    let output = assert_success(
        run_command(command, "sha256 provenance", PROCESS_TIMEOUT),
        "sha256 provenance",
    );
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .expect("sha256 output")
        .to_owned()
}

fn validate_pinned_bundle() -> PathBuf {
    let configured = PathBuf::from(
        std::env::var_os("ZMIN_STOCK_GIT").expect("ZMIN_STOCK_GIT must select pinned Git 2.55.0"),
    );
    assert_eq!(configured, PathBuf::from(PINNED_GIT));
    let bundle = PathBuf::from(PINNED_BUNDLE);
    let bundle_canonical = fs::canonicalize(&bundle).expect("canonicalize pinned bundle");
    assert_eq!(bundle_canonical, bundle);
    let metadata = fs::symlink_metadata(&configured).expect("stat pinned Git");
    assert!(metadata.file_type().is_file() && !metadata.file_type().is_symlink());
    let canonical = fs::canonicalize(&configured).expect("canonicalize pinned Git");
    assert_eq!(canonical, configured);
    let files = [
        ("git", PINNED_GIT_SHA256),
        ("git-http-fetch", PINNED_HTTP_FETCH_SHA256),
        ("git-http-backend", PINNED_HTTP_BACKEND_SHA256),
        ("git-remote-http", PINNED_REMOTE_HTTP_SHA256),
        ("manifest.tsv", PINNED_MANIFEST_SHA256),
        ("bundle.tsv", PINNED_BUNDLE_TABLE_SHA256),
        ("manifest.tsv.sha256", PINNED_MANIFEST_SIDECAR_SHA256),
        ("bundle.tsv.sha256", PINNED_BUNDLE_SIDECAR_SHA256),
    ];
    let environment_root = TempDir::new().expect("create pinned Git environment root");
    for (name, expected) in files {
        let path = bundle.join(name);
        let metadata = fs::symlink_metadata(&path).expect("stat pinned bundle member");
        assert!(metadata.file_type().is_file() && !metadata.file_type().is_symlink());
        assert_eq!(
            file_sha256(&path, environment_root.path()),
            expected,
            "pinned hash for {name}"
        );
    }
    let manifest = fs::read_to_string(bundle.join("manifest.tsv")).expect("read bundle manifest");
    assert!(manifest.contains("manifest_version\t3\n"));
    assert!(manifest.contains("upstream_git_tag\tv2.55.0\n"));
    assert!(manifest.contains("upstream_git_commit\te9019fcafe0040228b8631c30f97ae1adb61bcdc\n"));
    let table = fs::read_to_string(bundle.join("bundle.tsv")).expect("read bundle table");
    for marker in [
        "schema_version\t1\n",
        "platform\tDarwin\n",
        "arch\tarm64\n",
        "template_dir\ttemplates\n",
    ] {
        assert!(table.contains(marker), "bundle table missing {marker:?}");
    }
    assert_eq!(
        fs::read_to_string(bundle.join("manifest.tsv.sha256"))
            .expect("read manifest sidecar")
            .split_whitespace()
            .next(),
        Some(PINNED_MANIFEST_SHA256)
    );
    assert_eq!(
        fs::read_to_string(bundle.join("bundle.tsv.sha256"))
            .expect("read bundle sidecar")
            .split_whitespace()
            .next(),
        Some(PINNED_BUNDLE_TABLE_SHA256)
    );
    let mut command = generic_command(&canonical, environment_root.path());
    command.arg("--version");
    let output = run_command(command, "pinned Git version", PROCESS_TIMEOUT);
    let output = assert_success(output, "pinned Git version");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "git version 2.55.0"
    );
    canonical
}

fn pinned_git() -> PathBuf {
    static VALIDATED: OnceLock<PathBuf> = OnceLock::new();
    VALIDATED.get_or_init(validate_pinned_bundle).clone()
}

fn require_pinned_stock_daemon() -> PathBuf {
    // This companion is supplied by the pinned-daemon builder. It is a fixed
    // artifact/manifest pair; the daemon never falls back to PATH or a system
    // executable, and its upload-pack helper is checked against the HTTP Git.
    let _ = pinned_git();
    let bundle = PathBuf::from(PINNED_DAEMON_BUNDLE);
    let daemon = PathBuf::from(PINNED_DAEMON);
    let manifest = bundle.join("manifest.tsv");
    let bundle_canonical = fs::canonicalize(&bundle).unwrap_or_else(|error| {
        panic!(
            "missing daemon parity fixture setup: fixed stock daemon bundle {} ({error}); no PATH fallback is permitted",
            bundle.display()
        )
    });
    assert_eq!(bundle_canonical, bundle);
    for (name, expected) in [
        ("manifest.tsv", PINNED_DAEMON_MANIFEST_SHA256),
        ("manifest.tsv.sha256", PINNED_DAEMON_MANIFEST_SIDECAR_SHA256),
        ("git-daemon.sha256", PINNED_DAEMON_HASH_SIDECAR_SHA256),
        (
            "git-upload-pack.sha256",
            PINNED_DAEMON_UPLOAD_PACK_HASH_SIDECAR_SHA256,
        ),
    ] {
        let path = bundle.join(name);
        let metadata = fs::symlink_metadata(&path).unwrap_or_else(|error| {
            panic!(
                "missing daemon parity fixture setup: {} ({error})",
                path.display()
            )
        });
        assert!(metadata.file_type().is_file() && !metadata.file_type().is_symlink());
        let environment_root = TempDir::new().expect("create daemon provenance root");
        assert_eq!(
            file_sha256(&path, environment_root.path()),
            expected,
            "fixed daemon companion hash for {name}"
        );
    }
    assert_eq!(
        fs::read_to_string(bundle.join("manifest.tsv.sha256"))
            .expect("read daemon manifest sidecar")
            .split_whitespace()
            .next(),
        Some(PINNED_DAEMON_MANIFEST_SHA256)
    );
    assert_eq!(
        fs::read_to_string(bundle.join("git-daemon.sha256"))
            .expect("read daemon hash sidecar")
            .split_whitespace()
            .next(),
        Some(PINNED_DAEMON_SHA256)
    );
    assert_eq!(
        fs::read_to_string(bundle.join("git-upload-pack.sha256"))
            .expect("read daemon helper hash sidecar")
            .split_whitespace()
            .next(),
        Some(PINNED_GIT_SHA256)
    );
    let metadata = fs::symlink_metadata(&daemon).unwrap_or_else(|error| {
        panic!(
            "missing daemon parity fixture setup: fixed stock git-daemon artifact {} ({error}); no PATH fallback is permitted",
            daemon.display()
        )
    });
    assert!(
        metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
        "stock git-daemon fixture must be a regular non-symlink file: {}",
        daemon.display()
    );
    assert_eq!(
        fs::canonicalize(&daemon).expect("canonicalize stock git-daemon"),
        daemon,
        "stock git-daemon fixture must not resolve through a symlink"
    );
    let manifest_text = fs::read_to_string(&manifest).expect("read pinned daemon manifest");
    assert!(manifest_text.contains("artifact_role\tpinned_git_daemon_companion_hardened\n"));
    assert!(manifest_text.contains("upstream_git_tag\tv2.55.0\n"));
    assert!(
        manifest_text.contains("upstream_git_commit\te9019fcafe0040228b8631c30f97ae1adb61bcdc\n")
    );
    let environment_root = TempDir::new().expect("create daemon hash environment root");
    let source_tree = bundle.join("source-tree.tsv");
    let source_tree_sidecar = bundle.join("source-tree.tsv.sha256");
    for path in [&source_tree, &source_tree_sidecar] {
        let metadata = fs::symlink_metadata(path).expect("stat daemon source inventory");
        assert!(metadata.file_type().is_file() && !metadata.file_type().is_symlink());
    }
    let source_tree_expected = manifest_text
        .lines()
        .find_map(|line| line.strip_prefix("source_tree_inventory_sha256\t"))
        .expect("daemon manifest source inventory hash");
    assert_eq!(source_tree_expected.len(), 64);
    assert!(
        source_tree_expected
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "daemon source inventory hash must be lowercase hexadecimal"
    );
    assert_eq!(
        file_sha256(&source_tree, environment_root.path()),
        source_tree_expected,
        "daemon source inventory hash differs from manifest"
    );
    assert_eq!(
        fs::read_to_string(&source_tree_sidecar)
            .expect("read daemon source inventory sidecar")
            .split_whitespace()
            .next(),
        Some(source_tree_expected)
    );
    let fields = manifest_text
        .lines()
        .find(|line| line.starts_with("member\tgit-daemon\t"))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .unwrap_or_else(|| {
            panic!(
                "missing daemon parity fixture setup: {} has no member\\tgit-daemon row with a fixed SHA-256",
                manifest.display()
            )
        });
    assert!(fields.len() >= 5, "malformed git-daemon manifest row");
    let expected = fields[4];
    assert_eq!(
        expected.len(),
        64,
        "git-daemon manifest hash must be SHA-256"
    );
    assert_eq!(expected, PINNED_DAEMON_SHA256);
    assert!(
        expected.bytes().all(|byte| byte.is_ascii_hexdigit())
            && expected.bytes().all(|byte| !byte.is_ascii_uppercase()),
        "git-daemon manifest hash must be lowercase hexadecimal"
    );
    assert_eq!(
        file_sha256(&daemon, environment_root.path()),
        PINNED_DAEMON_SHA256,
        "fixed stock git-daemon artifact hash differs from manifest"
    );
    let helper_fields = manifest_text
        .lines()
        .find(|line| line.starts_with("member\tgit-upload-pack\t"))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .expect("pinned daemon manifest upload-pack member");
    assert!(
        helper_fields.len() >= 5,
        "malformed git-upload-pack manifest row"
    );
    assert_eq!(helper_fields[4], PINNED_GIT_SHA256);
    let upload_pack = bundle.join("git-upload-pack");
    let helper_metadata =
        fs::symlink_metadata(&upload_pack).expect("stat pinned daemon upload-pack helper");
    assert!(helper_metadata.file_type().is_file() && !helper_metadata.file_type().is_symlink());
    assert_eq!(
        fs::canonicalize(&upload_pack).expect("canonicalize daemon upload-pack helper"),
        upload_pack
    );
    assert_eq!(
        file_sha256(&upload_pack, environment_root.path()),
        PINNED_GIT_SHA256,
        "daemon upload-pack helper must match pinned Git"
    );
    daemon
}

fn validated_zmin_bin(cwd: &Path) -> PathBuf {
    static VALIDATED: OnceLock<(PathBuf, String)> = OnceLock::new();
    let (path, hash) = VALIDATED.get_or_init(|| {
        let configured = PathBuf::from(zmin_bin());
        assert!(configured.is_absolute(), "ZMIN_BIN must be absolute");
        let metadata = fs::symlink_metadata(&configured).expect("stat ZMIN_BIN");
        assert!(metadata.file_type().is_file() && !metadata.file_type().is_symlink());
        let canonical = fs::canonicalize(&configured).expect("canonicalize ZMIN_BIN");
        assert_eq!(canonical, configured, "ZMIN_BIN must not be a symlink");
        let mut command = generic_command(&canonical, cwd);
        command.arg("--version");
        let output = assert_success(
            run_command(command, "Zmin identity", PROCESS_TIMEOUT),
            "Zmin identity",
        );
        let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        assert!(version.starts_with("git version ") && version.contains(".zmin"));
        assert!(
            version.contains("(zmin "),
            "Zmin source identity marker missing"
        );
        let hash = file_sha256(&canonical, cwd);
        (canonical, hash)
    });
    assert_eq!(
        file_sha256(path, cwd),
        *hash,
        "Zmin binary changed after validation"
    );
    path.clone()
}

fn hermetic_git_command(cwd: &Path) -> Command {
    let mut command = Command::new(pinned_git());
    HermeticEnvironment::apply(&mut command, cwd);
    command.current_dir(cwd);
    command
}

fn run_git(cwd: &Path, args: &[&str]) -> Output {
    let mut command = hermetic_git_command(cwd);
    command.args(args);
    run_command(command, "pinned Git command", PROCESS_TIMEOUT)
}

fn run_git_input(cwd: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut command = hermetic_git_command(cwd);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_command_input(command, input, "pinned Git command", PROCESS_TIMEOUT)
}

fn assert_success(output: Output, label: &str) -> Output {
    assert!(
        output.status.success(),
        "{label} failed: {}",
        redact(&String::from_utf8_lossy(&output.stderr))
    );
    output
}

fn stock_text(cwd: &Path, args: &[&str]) -> String {
    let output = assert_success(run_git(cwd, args), "pinned Git command");
    String::from_utf8(output.stdout)
        .expect("pinned Git output UTF-8")
        .trim()
        .to_owned()
}

fn configure(cwd: &Path, key: &str, value: &str) {
    assert_success(run_git(cwd, &["config", key, value]), "configure fixture");
}

fn configure_add(cwd: &Path, key: &str, value: &str) {
    assert_success(
        run_git(cwd, &["config", "--add", key, value]),
        "append fixture configuration",
    );
}

fn unset_all(cwd: &Path, key: &str) {
    let _ = run_git(cwd, &["config", "--unset-all", key]);
}

fn prepare_remote(root: &Path, format: ObjectFormat) -> RemoteFixture {
    prepare_remote_with_backend(root, format, RefBackend::Files)
}

fn prepare_daemon_remote(root: &Path, format: ObjectFormat, backend: RefBackend) -> RemoteFixture {
    let source = prepare_remote_with_backend(root, format, backend);
    let export = root.join("daemon-export.git");
    let mut command = hermetic_git_command(root);
    command.args(["clone", "--bare", "--quiet"]);
    if backend == RefBackend::Reftable {
        command.arg("--ref-format=reftable");
    }
    command.args([
        source.repo.to_str().expect("daemon source path UTF-8"),
        export.to_str().expect("daemon export path UTF-8"),
    ]);
    assert_success(
        run_command(command, "make daemon export", PROCESS_TIMEOUT),
        "make daemon export",
    );
    RemoteFixture {
        repo: export,
        ..source
    }
}

fn prepare_remote_with_backend(
    root: &Path,
    format: ObjectFormat,
    backend: RefBackend,
) -> RemoteFixture {
    let repo = root.join("remote");
    fs::create_dir_all(&repo).expect("create fixture repository");
    let mut init = vec!["init", "--quiet", "--initial-branch=main"];
    if format == ObjectFormat::Sha256 {
        init.push("--object-format=sha256");
    }
    if backend == RefBackend::Reftable {
        init.push("--ref-format=reftable");
    }
    assert_success(run_git(&repo, &init), "init fixture repository");
    configure(&repo, "user.name", "Pack URI Oracle");
    configure(&repo, "user.email", "pack-uri-oracle@example.test");
    configure(&repo, "uploadpack.allowFilter", "true");
    fs::write(repo.join("one.txt"), b"one\n").expect("write first blob");
    fs::write(repo.join("two.txt"), b"two\n").expect("write second blob");
    assert_success(
        run_git(&repo, &["add", "one.txt", "two.txt"]),
        "stage fixture files",
    );
    assert_success(
        run_git(&repo, &["commit", "--quiet", "-m", "pack-uri"]),
        "commit fixture",
    );
    let head = stock_text(&repo, &["rev-parse", "HEAD"]);
    let tree = stock_text(&repo, &["rev-parse", "HEAD^{tree}"]);
    let one = stock_text(&repo, &["rev-parse", "HEAD:one.txt"]);
    let two = stock_text(&repo, &["rev-parse", "HEAD:two.txt"]);
    let uri_packs = vec![make_pack(&repo, &one), make_pack(&repo, &two)];
    RemoteFixture {
        repo,
        format,
        head,
        tree,
        blobs: vec![one, two],
        uri_packs,
    }
}

fn make_pack(repo: &Path, object: &str) -> PackSpec {
    let output = assert_success(
        run_git_input(
            repo,
            &["pack-objects", "--stdout"],
            format!("{object}\n").as_bytes(),
        ),
        "make URI pack",
    );
    assert!(
        output.stdout.len() <= MAX_PACK_BYTES,
        "fixture pack exceeds bound"
    );
    let pack_path = repo.join(format!("oracle-{object}.pack"));
    fs::write(&pack_path, &output.stdout).expect("write URI pack");
    let hash = stock_text(
        repo,
        &["index-pack", pack_path.to_str().expect("pack path UTF-8")],
    );
    let bytes = fs::read(&pack_path).expect("read URI pack");
    PackSpec {
        object: object.to_owned(),
        path: format!("/uri/pack-{hash}.pack"),
        hash,
        bytes,
    }
}

fn set_uri_mappings(remote: &RemoteFixture, scheme: &str, count: usize, wrong_first_hash: bool) {
    unset_all(&remote.repo, "uploadpack.blobpackfileuri");
    configure(&remote.repo, "uploadpack.allowsidebandall", "true");
    for (index, pack) in remote.uri_packs.iter().take(count).enumerate() {
        let hash = if wrong_first_hash && index == 0 {
            "0".repeat(remote.format.hex_len())
        } else {
            pack.hash.clone()
        };
        let value = format!(
            "{} {hash} {scheme}://127.0.0.1/{}",
            pack.object,
            pack.path.trim_start_matches('/')
        );
        configure_add(&remote.repo, "uploadpack.blobpackfileuri", &value);
    }
}

fn set_uri_mappings_for_server(
    remote: &RemoteFixture,
    base_url: &str,
    count: usize,
    wrong_first_hash: bool,
) {
    unset_all(&remote.repo, "uploadpack.blobpackfileuri");
    configure(&remote.repo, "uploadpack.allowsidebandall", "true");
    for (index, pack) in remote.uri_packs.iter().take(count).enumerate() {
        let hash = if wrong_first_hash && index == 0 {
            "0".repeat(remote.format.hex_len())
        } else {
            pack.hash.clone()
        };
        let value = format!("{} {hash} {base_url}{}", pack.object, pack.path);
        configure_add(&remote.repo, "uploadpack.blobpackfileuri", &value);
    }
}

fn protocol_request(
    remote: &RemoteFixture,
    uri_protocol: Option<&str>,
    sideband_all: bool,
    duplicate_uri_line: bool,
    filter: bool,
) -> Vec<u8> {
    let mut request = Vec::new();
    append_pkt(&mut request, b"command=fetch\n");
    append_pkt(&mut request, b"agent=pack-uri-oracle\n");
    append_pkt(
        &mut request,
        format!("object-format={}\n", remote.format.name()).as_bytes(),
    );
    append_delim(&mut request);
    append_pkt(&mut request, format!("want {}\n", remote.head).as_bytes());
    if filter {
        append_pkt(&mut request, b"filter blob:none\n");
    }
    if let Some(protocol) = uri_protocol {
        append_pkt(
            &mut request,
            format!("packfile-uris {protocol}\n").as_bytes(),
        );
        if duplicate_uri_line {
            append_pkt(
                &mut request,
                format!("packfile-uris {protocol}\n").as_bytes(),
            );
        }
    }
    if sideband_all {
        append_pkt(&mut request, b"sideband-all\n");
    }
    append_pkt(&mut request, b"done\n");
    append_flush(&mut request);
    request
}

fn upload_pack(
    backend: ServerBackend,
    remote: &RemoteFixture,
    args_before_command: &[&str],
    input: &[u8],
    advertise_refs: bool,
) -> Output {
    let mut command = server_command(backend, remote.repo.parent().expect("remote parent"));
    command
        .args(args_before_command)
        .args(["upload-pack", "--stateless-rpc"]);
    if advertise_refs {
        command.arg("--advertise-refs");
    }
    command.arg(&remote.repo);
    command.env("GIT_PROTOCOL", "version=2");
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_command_input(command, input, "upload-pack", PROCESS_TIMEOUT);
    assert!(
        output.stdout.len() <= MAX_BODY_BYTES,
        "upload-pack output exceeds bound"
    );
    output
}

fn server_command(backend: ServerBackend, cwd: &Path) -> Command {
    match backend {
        ServerBackend::Stock => hermetic_git_command(cwd),
        ServerBackend::Zmin => {
            let zmin = validated_zmin_bin(cwd);
            let mut command = Command::new(zmin);
            HermeticEnvironment::apply(&mut command, cwd);
            command.current_dir(cwd);
            command
        }
    }
}

fn advertisement(
    backend: ServerBackend,
    remote: &RemoteFixture,
    args_before_command: &[&str],
) -> Vec<u8> {
    assert_success(
        upload_pack(backend, remote, args_before_command, &[], true),
        "upload-pack advertisement",
    )
    .stdout
}

fn capability_lines(bytes: &[u8]) -> Vec<String> {
    parse_packets(bytes)
        .into_iter()
        .filter_map(|packet| match packet {
            Packet::Data(data) => String::from_utf8(data).ok(),
            _ => None,
        })
        .flat_map(|data| data.lines().map(str::to_owned).collect::<Vec<_>>())
        .collect()
}

fn normalize_capabilities(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            if line.starts_with("agent=") {
                "agent=<server>".to_owned()
            } else {
                line.clone()
            }
        })
        .collect()
}

fn append_pkt(out: &mut Vec<u8>, payload: &[u8]) {
    let len = payload.len() + 4;
    assert!(len <= 0xffff, "pkt-line exceeds protocol bound");
    out.extend_from_slice(format!("{len:04x}").as_bytes());
    out.extend_from_slice(payload);
}

fn append_flush(out: &mut Vec<u8>) {
    append_control(out, 0);
}

fn append_delim(out: &mut Vec<u8>) {
    append_control(out, 1);
}

fn append_control(out: &mut Vec<u8>, kind: usize) {
    assert!(kind <= 2);
    out.extend_from_slice(format!("{kind:04x}").as_bytes());
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Packet {
    Data(Vec<u8>),
    Flush,
    Delim,
    ResponseEnd,
}

fn parse_packets(bytes: &[u8]) -> Vec<Packet> {
    let mut packets = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        assert!(bytes.len() - cursor >= 4, "truncated pkt-line header");
        let header =
            std::str::from_utf8(&bytes[cursor..cursor + 4]).expect("pkt-line header UTF-8");
        let len = usize::from_str_radix(header, 16).expect("pkt-line header hex");
        cursor += 4;
        match len {
            0 => packets.push(Packet::Flush),
            1 => packets.push(Packet::Delim),
            2 => packets.push(Packet::ResponseEnd),
            3 => panic!("invalid pkt-line length 0003"),
            len => {
                assert!(
                    len >= 4 && len - 4 <= bytes.len() - cursor,
                    "truncated pkt-line body"
                );
                packets.push(Packet::Data(bytes[cursor..cursor + len - 4].to_vec()));
                cursor += len - 4;
            }
        }
    }
    packets
}

fn sideband_data(packet: &Packet) -> Option<(u8, &[u8])> {
    let Packet::Data(data) = packet else {
        return None;
    };
    data.split_first().map(|(band, payload)| (*band, payload))
}

fn response_section_names(bytes: &[u8]) -> Vec<String> {
    parse_packets(bytes)
        .iter()
        .filter_map(sideband_data)
        .filter_map(|(band, payload)| {
            (band == 1 && (payload == b"packfile-uris\n" || payload == b"packfile\n"))
                .then(|| String::from_utf8_lossy(payload).trim().to_owned())
        })
        .collect()
}

fn packet_shape(bytes: &[u8]) -> Vec<String> {
    parse_packets(bytes)
        .into_iter()
        .map(|packet| match packet {
            Packet::Flush => "flush".to_owned(),
            Packet::Delim => "delim".to_owned(),
            Packet::ResponseEnd => "response-end".to_owned(),
            Packet::Data(data) => {
                let (band, payload) = data.split_first().expect("packet data");
                if !matches!(band, 1 | 2 | 3) {
                    return "plain-data".to_owned();
                }
                assert_ne!(*band, 3, "unexpected sideband {band}");
                let kind = if payload == b"packfile-uris\n" {
                    "packfile-uris"
                } else if payload == b"packfile\n" {
                    "packfile"
                } else if payload.starts_with(b"PACK") {
                    "pack-bytes"
                } else if *band == 2 {
                    "progress"
                } else {
                    "data"
                };
                format!("band-{band}:{kind}")
            }
        })
        .filter(|shape| shape != "band-2:progress")
        .collect()
}

fn normalize_request_signature(mut signature: Vec<String>) -> Vec<String> {
    // Git may stamp a build-specific agent, but request ordering, wants, and
    // ref-prefix packets are protocol observables and must remain untouched.
    for line in &mut signature {
        if line.starts_with("agent=") {
            *line = "agent=<client>".to_owned();
        }
    }
    signature
}

fn error_category(stderr: &str) -> String {
    let stderr = redact(stderr);
    if stderr.contains("multiple packfile-uris") || stderr.contains("duplicate") {
        "duplicate-mapping".to_owned()
    } else if stderr.contains("form")
        || stderr.contains("malformed")
        || stderr.contains("pack-objects died")
    {
        "malformed-mapping".to_owned()
    } else if stderr.contains("unexpected line") {
        "unadvertised-feature".to_owned()
    } else {
        stderr
    }
}

fn uri_records(bytes: &[u8]) -> Vec<(String, String)> {
    let packets = parse_packets(bytes);
    let start = packets
        .iter()
        .position(|packet| {
            sideband_data(packet)
                .is_some_and(|(band, payload)| band == 1 && payload == b"packfile-uris\n")
        })
        .expect("packfile-uris section");
    packets[start + 1..]
        .iter()
        .take_while(|packet| !matches!(packet, Packet::Delim))
        .map(|packet| {
            let (band, payload) = sideband_data(packet).expect("URI section data packet");
            match band {
                1 => parse_uri_record(payload),
                2 => {
                    assert!(!payload.is_empty(), "empty URI progress packet");
                    assert!(
                        std::str::from_utf8(payload).is_ok(),
                        "URI progress packet is not UTF-8"
                    );
                    None
                }
                other => panic!("unexpected URI section sideband {other}"),
            }
        })
        .flatten()
        .collect()
}

fn parse_uri_record(payload: &[u8]) -> Option<(String, String)> {
    assert!(
        payload.ends_with(b"\n"),
        "URI mapping is not newline terminated"
    );
    let line_bytes = &payload[..payload.len() - 1];
    assert!(
        !line_bytes.contains(&b'\n'),
        "URI mapping contains extra newline"
    );
    let line = std::str::from_utf8(line_bytes).expect("URI mapping is UTF-8");
    let mut fields = line.split(' ');
    let hash = fields.next().expect("URI mapping hash");
    let uri = fields.next().expect("URI mapping URI");
    assert!(fields.next().is_none(), "URI mapping has extra fields");
    assert!(
        !hash.is_empty() && !uri.is_empty(),
        "URI mapping has empty field"
    );
    Some((hash.to_owned(), uri.to_owned()))
}

fn inline_pack_bytes(bytes: &[u8]) -> Vec<u8> {
    let packets = parse_packets(bytes);
    if let Some(start) = packets.iter().position(|packet| {
        sideband_data(packet).is_some_and(|(band, payload)| band == 1 && payload == b"packfile\n")
    }) {
        return packets[start + 1..]
            .iter()
            .take_while(|packet| !matches!(packet, Packet::Flush))
            .filter_map(sideband_data)
            .filter_map(|(band, payload)| (band == 1).then_some(payload))
            .flatten()
            .copied()
            .collect();
    }
    let start = packets
        .iter()
        .position(|packet| {
            sideband_data(packet)
                .is_some_and(|(band, payload)| band == 1 && payload.starts_with(b"PACK"))
        })
        .expect("inline pack data");
    packets[start..]
        .iter()
        .take_while(|packet| !matches!(packet, Packet::Flush))
        .filter_map(sideband_data)
        .filter_map(|(band, payload)| (band == 1).then_some(payload))
        .flatten()
        .copied()
        .collect()
}

fn assert_response_contract(remote: &RemoteFixture, response: &[u8], expected_uri_count: usize) {
    let packets = parse_packets(response);
    assert_eq!(
        packets
            .iter()
            .filter(|p| matches!(p, Packet::Flush))
            .count(),
        1
    );
    assert!(
        matches!(packets.last(), Some(Packet::Flush)),
        "response must end in flush"
    );
    assert_eq!(
        packets
            .iter()
            .filter(|p| matches!(p, Packet::Delim))
            .count(),
        1
    );
    assert!(!packets.iter().any(|p| matches!(p, Packet::ResponseEnd)));
    let sections = response_section_names(response);
    assert_eq!(sections, vec!["packfile-uris", "packfile"]);
    let delim = packets
        .iter()
        .position(|packet| matches!(packet, Packet::Delim))
        .expect("URI/pack delimiter");
    let uri_header = packets
        .iter()
        .position(|packet| {
            sideband_data(packet).is_some_and(|(band, p)| band == 1 && p == b"packfile-uris\n")
        })
        .expect("URI header");
    let pack_header = packets
        .iter()
        .position(|packet| {
            sideband_data(packet).is_some_and(|(band, p)| band == 1 && p == b"packfile\n")
        })
        .expect("pack header");
    assert!(
        uri_header < delim && delim < pack_header,
        "response sections are not ordered"
    );
    for packet in &packets[..uri_header] {
        let (band, payload) = sideband_data(packet).expect("pre-URI response packet");
        assert_eq!(band, 2, "unexpected pre-URI sideband {band}");
        assert!(!payload.is_empty(), "empty pre-URI progress packet");
        assert!(
            std::str::from_utf8(payload).is_ok(),
            "pre-URI progress packet is not UTF-8"
        );
    }
    assert_eq!(
        sideband_data(&packets[uri_header]).map(|(band, _)| band),
        Some(1)
    );
    for packet in &packets[uri_header + 1..delim] {
        let (band, payload) = sideband_data(packet).expect("URI section data packet");
        assert!(matches!(band, 1 | 2), "invalid URI section sideband {band}");
        if band == 2 {
            assert!(!payload.is_empty(), "empty URI progress packet");
            assert!(
                std::str::from_utf8(payload).is_ok(),
                "URI progress packet is not UTF-8"
            );
        }
    }
    assert_eq!(
        sideband_data(&packets[pack_header]).map(|(band, _)| band),
        Some(1)
    );
    for packet in &packets[delim + 1..pack_header] {
        let (band, payload) = sideband_data(packet).expect("between-section response packet");
        assert_eq!(band, 2, "unexpected between-section sideband {band}");
        assert!(!payload.is_empty(), "empty between-section progress packet");
        assert!(
            std::str::from_utf8(payload).is_ok(),
            "between-section progress packet is not UTF-8"
        );
    }
    let mut inline_band_1 = false;
    for packet in &packets[pack_header + 1..packets.len() - 1] {
        let (band, payload) = sideband_data(packet).expect("pack section data packet");
        assert!(
            matches!(band, 1 | 2),
            "invalid pack section sideband {band}"
        );
        if band == 1 && !payload.is_empty() {
            inline_band_1 = true;
        } else if band == 2 {
            assert!(!payload.is_empty(), "empty pack progress packet");
            assert!(
                std::str::from_utf8(payload).is_ok(),
                "pack progress packet is not UTF-8"
            );
        }
    }
    assert!(inline_band_1, "pack section must contain band-1 data");
    let records = uri_records(response);
    assert_eq!(
        records.len(),
        expected_uri_count,
        "URI hash count: {:?}",
        records.iter().map(|(hash, _)| hash).collect::<Vec<_>>()
    );
    for (index, (hash, uri)) in uri_records(response).into_iter().enumerate() {
        assert_eq!(hash.len(), remote.format.hex_len());
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(uri.contains("/uri/pack-"));
        assert_eq!(hash, remote.uri_packs[index].hash, "URI mapping hash");
        assert!(
            uri.ends_with(&remote.uri_packs[index].path),
            "URI mapping path"
        );
    }
    for packet in packets {
        if let Some((band, _)) = sideband_data(&packet) {
            assert!(matches!(band, 1 | 2), "invalid sideband {band}");
        }
    }
    let inline = inline_pack_bytes(response);
    assert!(inline.starts_with(b"PACK"), "inline pack must be preserved");
    assert!(inline.len() <= MAX_PACK_BYTES);
}

struct ConnectionWorker {
    cancel: Arc<AtomicBool>,
    wake_stream: TcpStream,
    handle: JoinHandle<()>,
}

struct FixtureHttpServer {
    port: u16,
    config: Arc<Mutex<HttpConfig>>,
    log: Arc<Mutex<RequestLog>>,
    stop: Arc<AtomicBool>,
    connections: Arc<Mutex<Vec<ConnectionWorker>>>,
    accept_thread: Option<JoinHandle<()>>,
}

impl FixtureHttpServer {
    fn new(config: HttpConfig) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind fixture HTTP server");
        listener
            .set_nonblocking(true)
            .expect("set fixture listener nonblocking");
        let port = listener
            .local_addr()
            .expect("fixture listener address")
            .port();
        let log = Arc::new(Mutex::new(RequestLog::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let connections = Arc::new(Mutex::new(Vec::new()));
        let ready = Arc::new((Mutex::new(false), Condvar::new()));
        let thread_ready = ready.clone();
        let thread_stop = stop.clone();
        let thread_log = log.clone();
        let thread_connections = connections.clone();
        let shared_config = Arc::new(Mutex::new(config));
        let thread_config = shared_config.clone();
        let mut accept_thread = Some(std::thread::spawn(move || {
            let (lock, signal) = &*thread_ready;
            *lock.lock().expect("fixture readiness lock") = true;
            signal.notify_all();
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let can_spawn = thread_connections
                            .lock()
                            .expect("fixture connection lock")
                            .len()
                            < MAX_CONNECTIONS;
                        if !can_spawn {
                            let _ = stream.shutdown(Shutdown::Both);
                            continue;
                        }
                        let config = thread_config.lock().expect("fixture config lock").clone();
                        let log = thread_log.clone();
                        let wake_stream = stream.try_clone().expect("clone fixture connection");
                        let cancel = Arc::new(AtomicBool::new(false));
                        let worker_cancel = cancel.clone();
                        let handle = std::thread::spawn(move || {
                            serve_connection(config, log, &mut stream, &worker_cancel)
                        });
                        thread_connections
                            .lock()
                            .expect("fixture connection lock")
                            .push(ConnectionWorker {
                                cancel,
                                wake_stream,
                                handle,
                            });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) if thread_stop.load(Ordering::Relaxed) => break,
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        }));
        let (lock, signal) = &*ready;
        let mut is_ready = lock.lock().expect("fixture readiness lock");
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        while !*is_ready {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                stop_fixture_threads(
                    &stop,
                    port,
                    &connections,
                    accept_thread.take().expect("fixture accept thread"),
                    "fixture readiness accept thread",
                );
                panic!("fixture readiness timed out");
            }
            let (next, timeout) = signal
                .wait_timeout(is_ready, left)
                .expect("wait fixture readiness");
            is_ready = next;
            if timeout.timed_out() {
                stop_fixture_threads(
                    &stop,
                    port,
                    &connections,
                    accept_thread.take().expect("fixture accept thread"),
                    "fixture readiness accept thread",
                );
                panic!("fixture readiness timed out");
            }
        }
        Self {
            port,
            config: shared_config,
            log,
            stop,
            connections,
            accept_thread,
        }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/remote.git", self.port)
    }

    fn log(&self) -> RequestLog {
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            let snapshot = self.log.lock().expect("fixture request log lock").clone();
            if !snapshot.records.is_empty() || Instant::now() >= deadline {
                return snapshot;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn set_remote(&self, remote: RemoteFixture) {
        self.config.lock().expect("fixture config lock").remote = remote;
    }
}

fn join_threads_bounded(handles: Vec<JoinHandle<()>>, label: &str) {
    let deadline = Instant::now() + SERVER_JOIN_TIMEOUT;
    while handles.iter().any(|handle| !handle.is_finished()) {
        if Instant::now() >= deadline {
            panic!("{label} did not stop before watchdog deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    for handle in handles {
        handle.join().unwrap_or_else(|_| panic!("{label} panicked"));
    }
}

fn stop_fixture_threads(
    stop: &AtomicBool,
    port: u16,
    connections: &Arc<Mutex<Vec<ConnectionWorker>>>,
    accept_thread: JoinHandle<()>,
    label: &str,
) {
    stop.store(true, Ordering::Relaxed);
    let _ = TcpStream::connect(("127.0.0.1", port));
    join_threads_bounded(vec![accept_thread], label);
    let workers = connections
        .lock()
        .expect("fixture connection lock")
        .drain(..)
        .collect::<Vec<_>>();
    let mut handles = Vec::with_capacity(workers.len());
    for worker in workers {
        worker.cancel.store(true, Ordering::Relaxed);
        let _ = worker.wake_stream.shutdown(Shutdown::Both);
        handles.push(worker.handle);
    }
    join_threads_bounded(handles, "fixture connection thread");
}

impl Drop for FixtureHttpServer {
    fn drop(&mut self) {
        if let Some(handle) = self.accept_thread.take() {
            stop_fixture_threads(
                &self.stop,
                self.port,
                &self.connections,
                handle,
                "fixture accept thread",
            );
        }
    }
}

#[derive(Clone, Debug)]
struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream, cancel: &AtomicBool) -> Option<HttpRequest> {
    let mut header = Vec::with_capacity(MAX_HEADER_BYTES);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        if header.len() == MAX_HEADER_BYTES {
            return None;
        }
        let mut byte = [0_u8; 1];
        if stream.read_exact(&mut byte).is_err() {
            return None;
        }
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let headers = String::from_utf8_lossy(&header).into_owned();
    let mut request_line = headers.lines().next()?.split_ascii_whitespace();
    let method = request_line.next()?.to_owned();
    let path = request_line.next()?.to_owned();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    if content_length > MAX_BODY_BYTES {
        return None;
    }
    let mut body = Vec::with_capacity(content_length);
    let mut buffer = [0_u8; 4096];
    while body.len() < content_length {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let read_limit = (content_length - body.len()).min(buffer.len());
        let read = stream.read(&mut buffer[..read_limit]).ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    Some(HttpRequest { method, path, body })
}

fn serve_connection(
    config: HttpConfig,
    log: Arc<Mutex<RequestLog>>,
    stream: &mut TcpStream,
    cancel: &AtomicBool,
) {
    let _ = stream.set_read_timeout(Some(WORKER_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(WORKER_IO_TIMEOUT));
    let Some(request) = read_request(stream, cancel) else {
        return;
    };
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    {
        let mut log = log.lock().expect("fixture request log lock");
        if log.records.len() >= MAX_REQUESTS {
            let _ = stream.shutdown(Shutdown::Both);
            return;
        }
        log.records.push(RequestRecord {
            method: request.method.clone(),
            path: request.path.clone(),
            body: request.body.clone(),
        });
    }
    let response = fixture_response(&config, &request);
    let _ = write_response(stream, response);
}

struct HttpResponse {
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

fn write_response(stream: &mut TcpStream, response: HttpResponse) -> std::io::Result<()> {
    assert!(
        response.body.len() <= MAX_BODY_BYTES,
        "fixture response exceeds bound"
    );
    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    );
    assert!(
        header.len() <= MAX_HEADER_BYTES,
        "fixture response header exceeds bound"
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.shutdown(Shutdown::Write)
}

fn fixture_response(config: &HttpConfig, request: &HttpRequest) -> HttpResponse {
    let path = request
        .path
        .split_once('?')
        .map_or(request.path.as_str(), |(path, _)| path);
    if request.method == "GET" && path.ends_with("/info/refs") {
        let advertisement = advertisement(config.backend, &config.remote, &[]);
        let mut body = Vec::new();
        append_pkt(&mut body, b"# service=git-upload-pack\n");
        append_flush(&mut body);
        body.extend_from_slice(&advertisement);
        return HttpResponse {
            status: "200 OK",
            content_type: "application/x-git-upload-pack-advertisement",
            body,
        };
    }
    if request.method == "POST" && path.ends_with("/git-upload-pack") {
        let output = upload_pack(config.backend, &config.remote, &[], &request.body, false);
        return HttpResponse {
            status: "200 OK",
            content_type: "application/x-git-upload-pack-result",
            body: output.stdout,
        };
    }
    if request.method == "GET" && path.starts_with("/uri/") {
        let Some(pack) = config
            .remote
            .uri_packs
            .iter()
            .find(|pack| pack.path == path)
        else {
            return HttpResponse {
                status: "404 Not Found",
                content_type: "text/plain",
                body: Vec::new(),
            };
        };
        if config.uri_get_mode == UriGetMode::NotFound {
            return HttpResponse {
                status: "404 Not Found",
                content_type: "text/plain",
                body: Vec::new(),
            };
        }
        let mut body = pack.bytes.clone();
        if config.uri_get_mode == UriGetMode::Corrupt && !body.is_empty() {
            let index = body.len() / 2;
            body[index] ^= 0x5a;
        }
        return HttpResponse {
            status: "200 OK",
            content_type: "application/x-git-packed-objects",
            body,
        };
    }
    HttpResponse {
        status: "404 Not Found",
        content_type: "text/plain",
        body: Vec::new(),
    }
}

fn clone_for_backend(
    server: &FixtureHttpServer,
    server_backend: ServerBackend,
    destination: &Path,
    backend: RefBackend,
    filter: bool,
) -> Output {
    let mut command = server_command(server_backend, destination.parent().expect("clone parent"));
    command.args([
        "-c",
        "protocol.version=2",
        "-c",
        "fetch.uriprotocols=http",
        "clone",
        "--quiet",
    ]);
    if backend == RefBackend::Reftable {
        command.arg("--ref-format=reftable");
    }
    if filter {
        command.args(["--filter=blob:none", "--no-checkout"]);
    }
    command.args([server.url(), destination.display().to_string()]);
    run_client(command, "HTTP clone")
}

#[cfg(unix)]
fn write_fake_ssh(root: &Path) -> (PathBuf, PathBuf) {
    let script = root.join("ssh");
    let log = root.join("ssh-invocations.log");
    fs::write(
        &script,
        b"#!/bin/sh\nset -eu\nprevious=\nprotocol_option=0\nremote_command=\nfor arg in \"$@\"; do\n  /usr/bin/printf '%s\\n' \"$arg\" >> \"$PACK_URI_SSH_LOG\"\n  if [ \"$previous\" = \"-o\" ] && [ \"$arg\" = \"SendEnv=GIT_PROTOCOL\" ]; then\n    protocol_option=1\n  fi\n  case \"$arg\" in\n    git-upload-pack\\ *) remote_command=\"$arg\" ;;\n  esac\n  previous=\"$arg\"\ndone\n/usr/bin/printf 'received GIT_PROTOCOL=%s\\n' \"${GIT_PROTOCOL-}\" >> \"$PACK_URI_SSH_LOG\"\n[ \"$protocol_option\" = 1 ] || exit 64\n[ \"${GIT_PROTOCOL-}\" = version=2 ] || exit 65\n[ \"$remote_command\" = \"git-upload-pack '$PACK_URI_REMOTE_PATH'\" ] || exit 66\nexec \"$PACK_URI_SERVER_BIN\" upload-pack \"$PACK_URI_REPO\" 2>>\"$PACK_URI_SSH_LOG\"\n",
    )
    .expect("write fake SSH transport");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700))
        .expect("make fake SSH transport executable");
    (script, log)
}

#[cfg(unix)]
fn clone_ssh_for_backend(
    service_backend: ServerBackend,
    client_backend: ServerBackend,
    ssh_script: &Path,
    ssh_log: &Path,
    remote: &RemoteFixture,
    destination: &Path,
    ref_backend: RefBackend,
) -> Output {
    let mut command = server_command(client_backend, destination.parent().expect("clone parent"));
    command.args([
        "-c",
        "protocol.version=2",
        "-c",
        "fetch.uriprotocols=http",
        "clone",
        "--quiet",
    ]);
    if ref_backend == RefBackend::Reftable {
        command.arg("--ref-format=reftable");
    }
    command.args([
        "ssh://pack-uri@127.0.0.1/remote",
        destination.to_str().expect("SSH clone path UTF-8"),
    ]);
    command
        .env("GIT_SSH_COMMAND", ssh_script)
        .env("GIT_SSH_VARIANT", "ssh")
        .env("PACK_URI_SSH_LOG", ssh_log)
        .env("PACK_URI_REPO", &remote.repo)
        .env(
            "PACK_URI_SERVER_BIN",
            backend_executable(
                service_backend,
                remote.repo.parent().expect("remote parent"),
            ),
        )
        .env("PACK_URI_REMOTE_PATH", "/remote");
    run_client(command, "SSH clone")
}

fn backend_executable(backend: ServerBackend, cwd: &Path) -> PathBuf {
    match backend {
        ServerBackend::Stock => pinned_git(),
        ServerBackend::Zmin => validated_zmin_bin(cwd),
    }
}

#[cfg(unix)]
fn prepare_zmin_daemon_exec_path(root: &Path, zmin: &Path) -> (PathBuf, PathBuf) {
    let exec_path = root.join("zmin-daemon-exec");
    fs::create_dir(&exec_path).expect("create Zmin daemon exec path");
    let zmin_text = zmin.to_str().expect("Zmin path UTF-8");
    let marker = root.join("zmin-daemon-upload-pack.invoked");
    let marker_text = marker.to_str().expect("Zmin daemon marker path UTF-8");
    assert!(
        !zmin_text
            .chars()
            .chain(marker_text.chars())
            .any(|character| character.is_whitespace() || character == '\''),
        "Zmin path cannot be represented by hermetic upload-pack wrapper"
    );
    let helper = exec_path.join("git-upload-pack");
    fs::write(
        &helper,
        format!("#!/bin/sh\n/usr/bin/touch {marker_text}\nexec {zmin_text} upload-pack \"$@\"\n"),
    )
    .expect("write Zmin daemon upload-pack wrapper");
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o700))
        .expect("make Zmin daemon upload-pack wrapper executable");
    (exec_path, marker)
}

#[cfg(not(unix))]
fn prepare_zmin_daemon_exec_path(_root: &Path, _zmin: &Path) -> (PathBuf, PathBuf) {
    panic!("Zmin daemon parity fixture setup requires a Unix executable wrapper")
}

fn assert_ssh_invocation(log_path: &Path) -> Vec<String> {
    let log = fs::read_to_string(log_path).expect("read fake SSH invocation log");
    let lines = log.lines().collect::<Vec<_>>();
    let args = lines
        .iter()
        .copied()
        .take_while(|line| !line.starts_with("received GIT_PROTOCOL="))
        .collect::<Vec<_>>();
    assert!(
        args.windows(2)
            .any(|pair| pair == ["-o", "SendEnv=GIT_PROTOCOL"]),
        "Git SSH path did not request SendEnv=GIT_PROTOCOL: {args:?}"
    );
    assert!(
        args.iter()
            .filter(|line| line.starts_with("git-upload-pack "))
            .count()
            == 1,
        "Git SSH path did not issue exactly one upload-pack command: {args:?}"
    );
    assert_eq!(
        args.last().copied(),
        Some("git-upload-pack '/remote'"),
        "Git SSH path requested an unexpected repository command: {args:?}"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| **line == "received GIT_PROTOCOL=version=2")
            .count(),
        1,
        "Git SSH child did not receive exactly protocol v2: {lines:?}"
    );
    args.into_iter().map(str::to_owned).collect()
}

fn assert_uri_download_sequence(
    log: &RequestLog,
    remote: &RemoteFixture,
    expected_uri_count: usize,
) -> Vec<String> {
    let requests = uri_gets(log);
    assert_eq!(
        log.records.len(),
        expected_uri_count,
        "daemon/SSH URI server saw unexpected requests: {:?}",
        log.records
            .iter()
            .map(|record| (&record.method, &record.path))
            .collect::<Vec<_>>()
    );
    assert_eq!(requests.len(), expected_uri_count);
    let paths = requests
        .iter()
        .map(|record| {
            assert_eq!(record.method, "GET");
            assert!(record.body.is_empty(), "URI GET must not carry a body");
            record.path.clone()
        })
        .collect::<Vec<_>>();
    let expected = remote
        .uri_packs
        .iter()
        .take(expected_uri_count)
        .map(|pack| pack.path.clone())
        .collect::<Vec<_>>();
    assert_eq!(paths, expected, "URI request order/path");
    paths
}

fn assert_fetch_head(repo: &Path, _remote: &RemoteFixture) {
    assert!(
        !repo.join(".git/FETCH_HEAD").exists(),
        "v2.55 clone must preserve its exact absent FETCH_HEAD state"
    );
}

struct GitDaemon {
    guard: ChildGuard,
    port: u16,
    repo_path: String,
    zmin_helper_marker: Option<PathBuf>,
}

impl GitDaemon {
    fn start(
        server_backend: ServerBackend,
        daemon_binary: &Path,
        root: &Path,
        remote: &RemoteFixture,
    ) -> Self {
        fs::write(remote.repo.join("git-daemon-export-ok"), b"")
            .expect("mark repository exportable");
        let reservation = TcpListener::bind(("127.0.0.1", 0)).expect("reserve daemon port");
        let port = reservation
            .local_addr()
            .expect("daemon reservation address")
            .port();
        drop(reservation);
        let repo_path = format!(
            "/{}",
            remote
                .repo
                .file_name()
                .expect("daemon repository name")
                .to_string_lossy()
        );
        let mut command = generic_command(daemon_binary, root);
        let (exec_path, zmin_helper_marker) = match server_backend {
            ServerBackend::Stock => (PathBuf::from(PINNED_DAEMON_BUNDLE), None),
            ServerBackend::Zmin => {
                let (path, marker) = prepare_zmin_daemon_exec_path(root, daemon_binary);
                (path, Some(marker))
            }
        };
        command.env("GIT_EXEC_PATH", &exec_path);
        let root_text = root.to_str().expect("daemon root UTF-8");
        let max_connections = "--max-connections=8";
        let listen = "--listen=127.0.0.1";
        let port_argument = format!("--port={port}");
        let base_path = format!("--base-path={root_text}");
        if server_backend == ServerBackend::Zmin {
            command.arg("daemon");
        }
        command
            .args([
                "--verbose",
                "--export-all",
                "--reuseaddr",
                max_connections,
                listen,
                &port_argument,
                &base_path,
                root_text,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut guard = ChildGuard::spawn_with_stderr_capture(&mut command, "git daemon");
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let mut ready = false;
        while Instant::now() < deadline {
            match guard.child_mut().try_wait() {
                Ok(Some(status)) => {
                    let stderr = guard.finish_stderr_capture("git daemon stderr");
                    panic!("git daemon exited before readiness: {status}; stderr={stderr}");
                }
                Ok(None) => {}
                Err(error) => panic!("git daemon readiness wait failed: {error}"),
            }
            if daemon_ready_handshake(port, &repo_path) {
                ready = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        if !ready {
            guard.terminate();
            guard.reap();
            let stderr = guard.finish_stderr_capture("git daemon stderr");
            panic!("git daemon readiness handshake timed out; stderr={stderr}");
        }
        Self {
            guard,
            port,
            repo_path,
            zmin_helper_marker,
        }
    }

    fn assert_selected_upload_pack(&self, server_backend: ServerBackend) {
        if server_backend == ServerBackend::Zmin {
            assert!(
                self.zmin_helper_marker
                    .as_ref()
                    .is_some_and(|marker| marker.is_file()),
                "Zmin daemon did not invoke its selected upload-pack helper"
            );
        }
    }

    fn url(&self) -> String {
        format!("git://127.0.0.1:{}{}", self.port, self.repo_path)
    }
}

fn daemon_ready_handshake(port: u16, repo_path: &str) -> bool {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(50)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    let payload = format!("git-upload-pack {repo_path}\0host=127.0.0.1\0");
    let mut request = Vec::new();
    append_pkt(&mut request, payload.as_bytes());
    if stream.write_all(&request).is_err() {
        return false;
    }
    let mut header = [0_u8; 4];
    if stream.read_exact(&mut header).is_err() {
        return false;
    }
    &header != b"ERR "
}

fn daemon_upload_pack_response(
    daemon: &GitDaemon,
    remote: &RemoteFixture,
    uri_protocol: &str,
) -> Vec<u8> {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], daemon.port));
    let mut stream = TcpStream::connect_timeout(&address, REQUEST_TIMEOUT)
        .expect("connect daemon protocol probe");
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .expect("set daemon probe read timeout");
    stream
        .set_write_timeout(Some(REQUEST_TIMEOUT))
        .expect("set daemon probe write timeout");
    let handshake = format!(
        "git-upload-pack {}\0host=127.0.0.1\0\0version=2\0",
        daemon.repo_path
    );
    let mut input = Vec::new();
    append_pkt(&mut input, handshake.as_bytes());
    stream
        .write_all(&input)
        .expect("write daemon protocol probe");
    let mut response = read_pkt_section(&mut stream);
    response.extend_from_slice(&protocol_request(
        remote,
        Some(uri_protocol),
        true,
        false,
        false,
    ));
    let request = response.split_off(first_flush_offset(&response));
    stream
        .write_all(&request)
        .expect("write daemon protocol request");
    let trailing = read_pkt_section(&mut stream);
    assert!(
        response.len() + trailing.len() <= MAX_BODY_BYTES,
        "daemon response exceeds bound"
    );
    response.extend_from_slice(&trailing);
    response
}

fn read_pkt_section(stream: &mut TcpStream) -> Vec<u8> {
    let mut section = Vec::new();
    loop {
        let mut header = [0_u8; 4];
        stream
            .read_exact(&mut header)
            .expect("read daemon packet header");
        let length = usize::from_str_radix(
            std::str::from_utf8(&header).expect("daemon packet header UTF-8"),
            16,
        )
        .expect("daemon packet length");
        section.extend_from_slice(&header);
        if length == 0 {
            assert!(
                section.len() <= MAX_BODY_BYTES,
                "daemon section exceeds bound"
            );
            return section;
        }
        if length == 3 {
            panic!("invalid daemon pkt-line length 0003");
        }
        if length == 1 || length == 2 {
            continue;
        }
        let body_length = length - 4;
        assert!(
            section.len() + body_length <= MAX_BODY_BYTES,
            "daemon section exceeds bound"
        );
        let mut body = vec![0_u8; body_length];
        stream
            .read_exact(&mut body)
            .expect("read daemon packet body");
        section.extend_from_slice(&body);
    }
}

fn first_flush_offset(bytes: &[u8]) -> usize {
    let mut cursor = 0;
    while cursor < bytes.len() {
        assert!(
            bytes.len() - cursor >= 4,
            "truncated daemon response packet"
        );
        let header = std::str::from_utf8(&bytes[cursor..cursor + 4])
            .expect("daemon response packet header UTF-8");
        let length = usize::from_str_radix(header, 16).expect("daemon response packet length");
        cursor += 4;
        if length == 0 {
            return cursor;
        }
        assert!(length >= 4 && length - 4 <= bytes.len() - cursor);
        cursor += length - 4;
    }
    panic!("daemon response omitted capability flush")
}

fn before_first_flush(bytes: &[u8]) -> &[u8] {
    &bytes[..first_flush_offset(bytes) - 4]
}

fn after_first_flush(bytes: &[u8]) -> &[u8] {
    &bytes[first_flush_offset(bytes)..]
}

fn clone_git_daemon_for_backend(
    client_backend: ServerBackend,
    daemon: &GitDaemon,
    destination: &Path,
    ref_backend: RefBackend,
) -> Output {
    let mut command = server_command(client_backend, destination.parent().expect("clone parent"));
    command.args([
        "-c",
        "protocol.version=2",
        "-c",
        "fetch.uriprotocols=http",
        "clone",
        "--quiet",
    ]);
    if ref_backend == RefBackend::Reftable {
        command.arg("--ref-format=reftable");
    }
    command.args([
        &daemon.url(),
        destination.to_str().expect("daemon clone path UTF-8"),
    ]);
    run_client(command, "git:// clone")
}

fn assert_inline_clone_layout(repo: &Path, remote: &RemoteFixture, backend: RefBackend) {
    let packs = pack_objects(repo, remote.format);
    assert_eq!(packs.len(), 1, "fallback must preserve one inline pack");
    assert_eq!(
        packs.values().next().expect("inline pack"),
        &graph_objects(remote),
        "fallback inline object set"
    );
    match backend {
        RefBackend::Files => assert!(repo.join(".git/refs").is_dir()),
        RefBackend::Reftable => assert!(repo.join(".git/reftable/tables.list").is_file()),
    }
}

fn run_client(command: Command, label: &str) -> Output {
    run_command(command, label, CLIENT_TIMEOUT)
}

fn uri_gets(log: &RequestLog) -> Vec<&RequestRecord> {
    log.records
        .iter()
        .filter(|record| record.method == "GET" && record.path.starts_with("/uri/"))
        .collect()
}

fn post_requests(log: &RequestLog) -> Vec<&RequestRecord> {
    log.records
        .iter()
        .filter(|record| record.method == "POST" && record.path.ends_with("/git-upload-pack"))
        .collect()
}

fn request_signature(body: &[u8]) -> Vec<String> {
    let packets = parse_packets(body);
    assert!(
        matches!(packets.last(), Some(Packet::Flush)),
        "request must end in flush"
    );
    assert_eq!(
        packets
            .iter()
            .filter(|p| matches!(p, Packet::Flush))
            .count(),
        1
    );
    assert_eq!(
        packets
            .iter()
            .filter(|p| matches!(p, Packet::Delim))
            .count(),
        1
    );
    let mut signature = Vec::new();
    for packet in packets {
        match packet {
            Packet::Data(data) => {
                let text = String::from_utf8(data).expect("request data UTF-8");
                assert!(!text.is_empty(), "empty request data packet");
                for line in text.split_terminator('\n') {
                    assert!(!line.is_empty(), "request contains an empty line");
                    signature.push(line.to_owned());
                }
            }
            Packet::Delim => signature.push("<delim>".to_owned()),
            Packet::Flush => signature.push("<flush>".to_owned()),
            Packet::ResponseEnd => panic!("request response-end packet"),
        }
    }
    signature
}

fn assert_object(repo: &Path, object: &str, present: bool) {
    let mut command = hermetic_git_command(repo);
    command
        .args(["cat-file", "-e", object])
        .env("GIT_NO_LAZY_FETCH", "1");
    let output = run_command(command, "check fixture object", PROCESS_TIMEOUT);
    assert_eq!(
        output.status.success(),
        present,
        "object presence mismatch for {object}"
    );
}

fn pack_objects(repo: &Path, format: ObjectFormat) -> BTreeMap<String, BTreeSet<String>> {
    let mut result = BTreeMap::new();
    let directory = repo.join(".git/objects/pack");
    for entry in fs::read_dir(&directory).expect("read clone pack directory") {
        let path = entry.expect("read pack entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("idx") {
            continue;
        }
        let output = assert_success(
            run_git(
                repo,
                &[
                    "verify-pack",
                    &format!("--object-format={}", format.name()),
                    "-v",
                    path.to_str().expect("idx path UTF-8"),
                ],
            ),
            "verify clone pack",
        );
        let objects = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let object = fields.next()?;
                let kind = fields.next()?;
                (object.len() == format.hex_len() && matches!(kind, "commit" | "tree" | "blob"))
                    .then(|| object.to_owned())
            })
            .collect::<BTreeSet<_>>();
        result.insert(
            path.file_stem()
                .expect("idx stem")
                .to_string_lossy()
                .into_owned(),
            objects,
        );
    }
    result
}

fn assert_clone_layout(repo: &Path, remote: &RemoteFixture, backend: RefBackend) {
    let packs = pack_objects(repo, remote.format);
    assert_eq!(
        packs.len(),
        remote.uri_packs.len() + 1,
        "inline and URI packs must remain distinct"
    );
    for pack in &remote.uri_packs {
        let objects = packs
            .get(&format!("pack-{}", pack.hash))
            .expect("URI pack missing");
        assert_eq!(objects, &BTreeSet::from([pack.object.clone()]));
    }
    let uri_stems = remote
        .uri_packs
        .iter()
        .map(|pack| format!("pack-{}", pack.hash))
        .collect::<BTreeSet<_>>();
    let inline = packs
        .iter()
        .filter(|(name, _)| !uri_stems.contains(*name))
        .flat_map(|(_, objects)| objects.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    assert!(inline.contains(&remote.head) && inline.contains(&remote.tree));
    assert!(
        remote.blobs.iter().all(|blob| !inline.contains(blob)),
        "excluded blobs leaked into inline pack"
    );
    let expected = BTreeSet::from_iter(
        std::iter::once(remote.head.clone())
            .chain(std::iter::once(remote.tree.clone()))
            .chain(remote.blobs.iter().cloned()),
    );
    let observed: BTreeSet<String> = packs
        .values()
        .flat_map(|objects| objects.iter().cloned())
        .collect();
    assert_eq!(
        observed, expected,
        "clone contains an unexpected object set"
    );
    match backend {
        RefBackend::Files => assert!(repo.join(".git/refs").is_dir()),
        RefBackend::Reftable => assert!(repo.join(".git/reftable/tables.list").is_file()),
    }
}

fn assert_filtered_layout(repo: &Path, remote: &RemoteFixture, uri_count: usize) {
    let packs = pack_objects(repo, remote.format);
    assert_eq!(packs.len(), uri_count + 1, "filtered clone pack count");
    let expected = BTreeSet::from_iter(
        std::iter::once(remote.head.clone())
            .chain(std::iter::once(remote.tree.clone()))
            .chain(remote.blobs.iter().cloned()),
    );
    let observed: BTreeSet<String> = packs
        .values()
        .flat_map(|objects| objects.iter().cloned())
        .collect();
    assert!(
        observed.is_subset(&expected),
        "filtered clone contains unexpected objects"
    );
    assert!(remote.blobs.iter().all(|blob| !observed.contains(blob)));
    assert!(observed.contains(&remote.head) && observed.contains(&remote.tree));
}

fn inline_object_set(remote: &RemoteFixture, bytes: &[u8]) -> BTreeSet<String> {
    let root = TempDir::new().expect("inline pack root");
    let pack = root.path().join("inline.pack");
    fs::write(&pack, bytes).expect("write inline pack");
    let index = assert_success(
        run_git(
            root.path(),
            &[
                "index-pack",
                &format!("--object-format={}", remote.format.name()),
                pack.to_str().expect("inline pack path UTF-8"),
            ],
        ),
        "index inline pack",
    );
    let hash = String::from_utf8_lossy(&index.stdout).trim().to_owned();
    let idx = root.path().join(format!("inline.pack-{}.idx", hash));
    let idx = if idx.is_file() {
        idx
    } else {
        pack.with_extension("idx")
    };
    let output = assert_success(
        run_git(
            root.path(),
            &[
                "verify-pack",
                &format!("--object-format={}", remote.format.name()),
                "-v",
                idx.to_str().expect("inline index path UTF-8"),
            ],
        ),
        "verify inline pack",
    );
    parse_pack_object_lines(&output.stdout, remote.format)
}

fn parse_pack_object_lines(bytes: &[u8], format: ObjectFormat) -> BTreeSet<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let object = fields.next()?;
            let kind = fields.next()?;
            (object.len() == format.hex_len() && matches!(kind, "commit" | "tree" | "blob"))
                .then(|| object.to_owned())
        })
        .collect()
}

fn redact(input: &str) -> String {
    let mut output = input.to_owned();
    for secret in [
        "origin-user",
        "origin-pass",
        "fixture-secret",
        "top-secret",
        "Bearer",
    ] {
        output = output.replace(secret, REDACTED);
    }
    let mut redacted = String::with_capacity(output.len());
    let mut cursor = 0;
    while cursor < output.len() {
        let remainder = &output[cursor..];
        let Some(relative) = ["http://", "https://"]
            .iter()
            .filter_map(|prefix| remainder.find(prefix))
            .min()
        else {
            redacted.push_str(remainder);
            break;
        };
        let start = cursor + relative;
        redacted.push_str(&output[cursor..start]);
        let end = output[start..]
            .find(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '"' | '\'' | ')' | ']' | '}'))
            .map_or(output.len(), |offset| start + offset);
        redacted.push_str(REDACTED);
        cursor = end;
    }
    redacted
}

fn all_server_backends() -> Vec<ServerBackend> {
    match std::env::var("PACK_URI_SERVER_BACKEND").as_deref() {
        Ok("stock") => {
            eprintln!(
                "FOCUSED-ONLY transport run: PACK_URI_SERVER_BACKEND=\"stock\"; no parity claim"
            );
            vec![ServerBackend::Stock]
        }
        Ok("zmin") => {
            eprintln!(
                "FOCUSED-ONLY transport run: PACK_URI_SERVER_BACKEND=\"zmin\"; no parity claim"
            );
            vec![ServerBackend::Zmin]
        }
        Ok(value) => panic!("unsupported PACK_URI_SERVER_BACKEND={value:?}"),
        Err(_) => vec![ServerBackend::Stock, ServerBackend::Zmin],
    }
}

fn differential_server_backends() -> Vec<ServerBackend> {
    if std::env::var_os("PACK_URI_SERVER_BACKEND").is_some() {
        return all_server_backends();
    }
    vec![ServerBackend::Stock, ServerBackend::Zmin]
}

fn graph_objects(remote: &RemoteFixture) -> BTreeSet<String> {
    BTreeSet::from_iter(
        std::iter::once(remote.head.clone())
            .chain(std::iter::once(remote.tree.clone()))
            .chain(remote.blobs.iter().cloned()),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResponseObservable {
    sections: Vec<String>,
    records: Vec<(String, String)>,
    inline_objects: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DirectObservable {
    sections: Vec<String>,
    packet_shape: Vec<String>,
    inline_objects: BTreeSet<String>,
}

fn direct_observable(remote: &RemoteFixture, response: &[u8]) -> DirectObservable {
    DirectObservable {
        sections: response_section_names(response),
        packet_shape: packet_shape(response)
            .into_iter()
            .filter(|shape| shape != "band-1:data")
            .collect(),
        inline_objects: inline_object_set(remote, &inline_pack_bytes(response)),
    }
}

#[test]
fn direct_capabilities_follow_config_and_sideband_gate_for_stock_and_zmin() {
    let mut baseline = None;
    for backend in all_server_backends() {
        let root = TempDir::new().expect("capability root");
        let remote =
            prepare_remote_with_backend(root.path(), ObjectFormat::Sha256, RefBackend::Reftable);
        unset_all(&remote.repo, "uploadpack.blobpackfileuri");
        let plain = capability_lines(&advertisement(backend, &remote, &[]));
        assert!(!plain.iter().any(|line| line.starts_with("packfile-uris")));
        assert!(!plain.iter().any(|line| line.starts_with("sideband-all")));

        let mut empty_capabilities = Vec::new();
        for args in [
            vec!["-c", "uploadpack.blobpackfileuri="],
            vec!["-c", "uploadpack.blobpackfileuri"],
        ] {
            let empty = capability_lines(&advertisement(backend, &remote, &args));
            assert!(!empty.iter().any(|line| line.starts_with("packfile-uris")));
            empty_capabilities.push(empty);
        }
        let implicit = capability_lines(&advertisement(
            backend,
            &remote,
            &["-c", "uploadpack.blobpackfileuri=anything"],
        ));
        assert!(implicit.iter().any(|line| line.contains("packfile-uris")));
        let sideband = capability_lines(&advertisement(
            backend,
            &remote,
            &["-c", "uploadpack.allowsidebandall=true"],
        ));
        assert!(sideband.iter().any(|line| line.contains("sideband-all")));

        set_uri_mappings(&remote, "http", 2, false);
        let configured = capability_lines(&advertisement(backend, &remote, &[]));
        assert!(configured.iter().any(|line| line.contains("packfile-uris")));
        assert!(configured.iter().any(|line| line.contains("sideband-all")));
        let observable = vec![
            normalize_capabilities(&plain),
            normalize_capabilities(&empty_capabilities.concat()),
            normalize_capabilities(&implicit),
            normalize_capabilities(&sideband),
            normalize_capabilities(&configured),
        ];
        if let Some(stock) = &baseline {
            assert_eq!(stock, &observable, "stock/Zmin capability set diverged");
        } else {
            baseline = Some(observable);
        }
    }
}

#[test]
fn direct_packfile_uri_response_has_ordered_sections_for_hash_and_ref_backends() {
    for format in [ObjectFormat::Sha1, ObjectFormat::Sha256] {
        for ref_backend in [RefBackend::Files, RefBackend::Reftable] {
            let root = TempDir::new().expect("direct response root");
            let remote = prepare_remote_with_backend(root.path(), format, ref_backend);
            set_uri_mappings(&remote, "http", 2, false);
            let mut baseline = None;
            for backend in all_server_backends() {
                let response = assert_success(
                    upload_pack(
                        backend,
                        &remote,
                        &[],
                        &protocol_request(&remote, Some("http"), true, false, false),
                        false,
                    ),
                    "direct URI response",
                );
                assert_response_contract(&remote, &response.stdout, remote.uri_packs.len());
                let records = uri_records(&response.stdout);
                assert_eq!(records[0].0, remote.uri_packs[0].hash);
                assert_eq!(records[1].0, remote.uri_packs[1].hash);
                let inline = inline_pack_bytes(&response.stdout);
                assert_eq!(
                    inline_object_set(&remote, &inline),
                    BTreeSet::from([remote.head.clone(), remote.tree.clone()])
                );
                let observable = ResponseObservable {
                    sections: response_section_names(&response.stdout),
                    records,
                    inline_objects: inline_object_set(&remote, &inline),
                };
                if let Some(stock) = &baseline {
                    assert_eq!(
                        stock, &observable,
                        "stock/Zmin response observable diverged"
                    );
                } else {
                    baseline = Some(observable);
                }
            }
        }
    }
}

#[test]
fn direct_uri_fallbacks_and_request_errors_match_v255_for_stock_and_zmin() {
    let mut mismatch_baseline = None;
    let mut without_uri_baseline = None;
    let mut no_sideband_baseline = None;
    let mut sideband_error_baseline = None;
    let mut unadvertised_baseline = None;
    let mut duplicate_baseline = None;
    for backend in all_server_backends() {
        let root = TempDir::new().expect("fallback root");
        let remote = prepare_remote(root.path(), ObjectFormat::Sha1);
        set_uri_mappings(&remote, "http", 1, false);

        let mismatch = assert_success(
            upload_pack(
                backend,
                &remote,
                &[],
                &protocol_request(&remote, Some("https"), true, false, false),
                false,
            ),
            "mismatched protocol fallback",
        );
        assert!(!response_section_names(&mismatch.stdout).contains(&"packfile-uris".to_owned()));
        assert_eq!(
            inline_object_set(&remote, &inline_pack_bytes(&mismatch.stdout)),
            graph_objects(&remote)
        );
        let mismatch_observable = direct_observable(&remote, &mismatch.stdout);
        if let Some(stock) = &mismatch_baseline {
            assert_eq!(stock, &mismatch_observable, "mismatch fallback diverged");
        } else {
            mismatch_baseline = Some(mismatch_observable);
        }

        let without_uri = assert_success(
            upload_pack(
                backend,
                &remote,
                &[],
                &protocol_request(&remote, None, true, false, false),
                false,
            ),
            "request without packfile-uris fallback",
        );
        assert!(!response_section_names(&without_uri.stdout).contains(&"packfile-uris".to_owned()));
        assert_eq!(
            inline_object_set(&remote, &inline_pack_bytes(&without_uri.stdout)),
            graph_objects(&remote)
        );
        let without_uri_observable = direct_observable(&remote, &without_uri.stdout);
        if let Some(stock) = &without_uri_baseline {
            assert_eq!(
                stock, &without_uri_observable,
                "missing URI request diverged"
            );
        } else {
            without_uri_baseline = Some(without_uri_observable);
        }

        configure(&remote.repo, "uploadpack.allowsidebandall", "false");
        let no_sideband = assert_success(
            upload_pack(
                backend,
                &remote,
                &[],
                &protocol_request(&remote, Some("http"), false, false, false),
                false,
            ),
            "sideband-all absent fallback",
        );
        assert!(!response_section_names(&no_sideband.stdout).contains(&"packfile-uris".to_owned()));
        assert_eq!(
            inline_object_set(&remote, &inline_pack_bytes(&no_sideband.stdout)),
            graph_objects(&remote)
        );
        let no_sideband_observable = direct_observable(&remote, &no_sideband.stdout);
        if let Some(stock) = &no_sideband_baseline {
            assert_eq!(stock, &no_sideband_observable, "sideband fallback diverged");
        } else {
            no_sideband_baseline = Some(no_sideband_observable);
        }
        let unadvertised_sideband = upload_pack(
            backend,
            &remote,
            &[],
            &protocol_request(&remote, Some("http"), true, false, false),
            false,
        );
        assert!(!unadvertised_sideband.status.success());
        if backend == ServerBackend::Stock {
            assert!(
                redact(&String::from_utf8_lossy(&unadvertised_sideband.stderr))
                    .contains("unexpected line")
            );
        }
        let sideband_error = (
            unadvertised_sideband.status.success(),
            error_category(&String::from_utf8_lossy(&unadvertised_sideband.stderr)),
        );
        if let Some(stock) = &sideband_error_baseline {
            assert_eq!(stock, &sideband_error, "sideband error diverged");
        } else {
            sideband_error_baseline = Some(sideband_error);
        }
        configure(&remote.repo, "uploadpack.allowsidebandall", "true");

        unset_all(&remote.repo, "uploadpack.blobpackfileuri");
        let unadvertised = upload_pack(
            backend,
            &remote,
            &[],
            &protocol_request(&remote, Some("http"), true, false, false),
            false,
        );
        assert!(!unadvertised.status.success());
        if backend == ServerBackend::Stock {
            assert!(
                redact(&String::from_utf8_lossy(&unadvertised.stderr)).contains("unexpected line")
            );
        }
        let unadvertised_observable = (
            unadvertised.status.success(),
            error_category(&String::from_utf8_lossy(&unadvertised.stderr)),
        );
        if let Some(stock) = &unadvertised_baseline {
            assert_eq!(
                stock, &unadvertised_observable,
                "unadvertised URI error diverged"
            );
        } else {
            unadvertised_baseline = Some(unadvertised_observable);
        }

        set_uri_mappings(&remote, "http", 1, false);
        let duplicate = upload_pack(
            backend,
            &remote,
            &[],
            &protocol_request(&remote, Some("http"), true, true, false),
            false,
        );
        assert!(!duplicate.status.success());
        if backend == ServerBackend::Stock {
            assert!(
                redact(&String::from_utf8_lossy(&duplicate.stderr))
                    .contains("multiple packfile-uris lines forbidden")
            );
        }
        let duplicate_observable = (
            duplicate.status.success(),
            error_category(&String::from_utf8_lossy(&duplicate.stderr)),
        );
        if let Some(stock) = &duplicate_baseline {
            assert_eq!(stock, &duplicate_observable, "duplicate URI error diverged");
        } else {
            duplicate_baseline = Some(duplicate_observable);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HttpObservable {
    post_count: usize,
    uri_paths: Vec<String>,
    request_signatures: Vec<Vec<String>>,
    packs: BTreeMap<String, BTreeSet<String>>,
}

fn assert_http_sequence(
    log: &RequestLog,
    remote: &RemoteFixture,
    expected_uri_count: usize,
    filtered: bool,
) -> HttpObservable {
    assert!(!log.records.is_empty(), "HTTP server saw no requests");
    assert_eq!(
        log.records[0].method, "GET",
        "advertisement must be the first HTTP request"
    );
    assert_eq!(
        log.records[0].path,
        "/remote.git/info/refs?service=git-upload-pack"
    );
    let posts = post_requests(log);
    assert_eq!(
        posts.len(),
        2,
        "expected ls-refs and fetch POSTs: {:?}",
        log.records
            .iter()
            .map(|record| (&record.method, &record.path))
            .collect::<Vec<_>>()
    );
    assert_eq!(log.records[1].method, "POST");
    assert_eq!(log.records[1].path, "/remote.git/git-upload-pack");
    assert_eq!(log.records[2].method, "POST");
    assert_eq!(log.records[2].path, "/remote.git/git-upload-pack");
    let ls_refs = request_signature(&posts[0].body);
    let fetch = request_signature(&posts[1].body);
    assert_eq!(ls_refs.first().map(String::as_str), Some("command=ls-refs"));
    assert_eq!(fetch.first().map(String::as_str), Some("command=fetch"));
    assert!(
        fetch
            .iter()
            .any(|line| line == &format!("object-format={}", remote.format.name()))
    );
    assert!(
        fetch
            .iter()
            .any(|line| line == &format!("want {}", remote.head))
    );
    assert!(fetch.iter().any(|line| line == "packfile-uris http"));
    assert!(fetch.iter().any(|line| line == "sideband-all"));
    if filtered {
        assert!(fetch.iter().any(|line| line == "filter blob:none"));
    }
    let uri_indices = log
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.path.starts_with("/uri/"))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if filtered {
        assert!(uri_indices.len() <= expected_uri_count);
    } else {
        assert_eq!(uri_indices.len(), expected_uri_count);
    }
    let last_post = log
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.method == "POST")
        .map(|(index, _)| index)
        .max()
        .expect("fetch POST");
    assert!(
        uri_indices.iter().all(|index| *index > last_post),
        "URI GET must follow origin POST"
    );
    let observed_uri_paths = uri_gets(log)
        .into_iter()
        .map(|record| record.path.clone())
        .collect::<Vec<_>>();
    let expected_uri_paths = remote
        .uri_packs
        .iter()
        .take(observed_uri_paths.len())
        .map(|pack| pack.path.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        observed_uri_paths, expected_uri_paths,
        "URI request order/path"
    );
    assert_eq!(
        log.records.len(),
        3 + observed_uri_paths.len(),
        "unexpected HTTP request after URI downloads"
    );
    HttpObservable {
        post_count: posts.len(),
        uri_paths: observed_uri_paths,
        request_signatures: vec![
            normalize_request_signature(ls_refs),
            normalize_request_signature(fetch),
        ],
        packs: BTreeMap::new(),
    }
}

#[test]
fn http_packfile_uri_preserves_exclusion_and_inline_pack_for_all_hash_ref_backends() {
    for format in [ObjectFormat::Sha1, ObjectFormat::Sha256] {
        for ref_backend in [RefBackend::Files, RefBackend::Reftable] {
            let mut baseline = None;
            for service_backend in differential_server_backends() {
                let root = TempDir::new().expect("HTTP matrix root");
                let remote = prepare_remote_with_backend(root.path(), format, ref_backend);
                let server = FixtureHttpServer::new(HttpConfig {
                    remote: remote.clone(),
                    uri_get_mode: UriGetMode::Serve,
                    backend: service_backend,
                });
                set_uri_mappings_for_server(
                    &remote,
                    &format!("http://127.0.0.1:{}", server.port),
                    2,
                    false,
                );
                server.set_remote(remote.clone());
                let destination =
                    root.path()
                        .join(format!("clone-{}-{}", format.name(), ref_backend.name()));
                let output =
                    clone_for_backend(&server, service_backend, &destination, ref_backend, false);
                assert!(
                    output.status.success(),
                    "HTTP matrix clone failed: {}",
                    redact(&String::from_utf8_lossy(&output.stderr))
                );
                let log = server.log();
                let mut observable = assert_http_sequence(&log, &remote, 2, false);
                assert_clone_layout(&destination, &remote, ref_backend);
                assert_object(&destination, &remote.head, true);
                assert_object(&destination, &remote.tree, true);
                for blob in &remote.blobs {
                    assert_object(&destination, blob, true);
                }
                observable.packs = pack_objects(&destination, remote.format);
                if let Some(stock) = &baseline {
                    assert_eq!(stock, &observable, "stock/Zmin HTTP observable diverged");
                } else {
                    baseline = Some(observable);
                }
            }
        }
    }
}

#[test]
fn http_filtered_fetch_omits_filtered_blobs_for_all_hash_ref_backends() {
    for format in [ObjectFormat::Sha1, ObjectFormat::Sha256] {
        for ref_backend in [RefBackend::Files, RefBackend::Reftable] {
            let mut baseline = None;
            for service_backend in differential_server_backends() {
                let root = TempDir::new().expect("filtered HTTP root");
                let remote = prepare_remote_with_backend(root.path(), format, ref_backend);
                let server = FixtureHttpServer::new(HttpConfig {
                    remote: remote.clone(),
                    uri_get_mode: UriGetMode::Serve,
                    backend: service_backend,
                });
                set_uri_mappings_for_server(
                    &remote,
                    &format!("http://127.0.0.1:{}", server.port),
                    1,
                    false,
                );
                server.set_remote(remote.clone());
                let destination = root.path().join("filtered");
                let output =
                    clone_for_backend(&server, service_backend, &destination, ref_backend, true);
                assert!(
                    output.status.success(),
                    "filtered clone failed: {}",
                    redact(&String::from_utf8_lossy(&output.stderr))
                );
                let log = server.log();
                let observable = assert_http_sequence(&log, &remote, 1, true);
                assert_filtered_layout(&destination, &remote, observable.uri_paths.len());
                assert_object(&destination, &remote.head, true);
                assert_object(&destination, &remote.tree, true);
                for blob in &remote.blobs {
                    assert_object(&destination, blob, false);
                }
                if let Some(stock) = &baseline {
                    assert_eq!(
                        stock, &observable,
                        "stock/Zmin filtered observable diverged"
                    );
                } else {
                    baseline = Some(observable);
                }
            }
        }
    }
}

#[test]
fn http_wrong_hash_not_found_and_corrupt_uri_fail_for_all_hash_ref_backends() {
    for format in [ObjectFormat::Sha1, ObjectFormat::Sha256] {
        for ref_backend in [RefBackend::Files, RefBackend::Reftable] {
            for mode in [UriGetMode::Serve, UriGetMode::NotFound, UriGetMode::Corrupt] {
                let mut baseline = None;
                for service_backend in all_server_backends() {
                    let root = TempDir::new().expect("failure HTTP root");
                    let remote = prepare_remote_with_backend(root.path(), format, ref_backend);
                    let server = FixtureHttpServer::new(HttpConfig {
                        remote: remote.clone(),
                        uri_get_mode: mode,
                        backend: service_backend,
                    });
                    set_uri_mappings_for_server(
                        &remote,
                        &format!("http://127.0.0.1:{}", server.port),
                        1,
                        mode == UriGetMode::Serve,
                    );
                    server.set_remote(remote.clone());
                    let destination = root.path().join("failure");
                    let output = clone_for_backend(
                        &server,
                        service_backend,
                        &destination,
                        ref_backend,
                        false,
                    );
                    assert!(
                        !output.status.success(),
                        "failure mode unexpectedly succeeded: {mode:?}"
                    );
                    let log = server.log();
                    let expected_gets = if mode == UriGetMode::Serve { 1 } else { 1 };
                    let observable = assert_http_sequence(&log, &remote, expected_gets, false);
                    if service_backend == ServerBackend::Stock && mode == UriGetMode::Serve {
                        assert!(
                            redact(&String::from_utf8_lossy(&output.stderr))
                                .contains("does not match expected hash")
                        );
                    }
                    if let Some(stock) = &baseline {
                        assert_eq!(stock, &observable, "stock/Zmin failure observable diverged");
                    } else {
                        baseline = Some(observable);
                    }
                }
            }
        }
    }
}

#[test]
fn direct_malformed_and_duplicate_mapping_failures_are_fatal_for_stock_and_zmin() {
    let mut malformed_baseline = None;
    let mut duplicate_baseline = None;
    for backend in all_server_backends() {
        let root = TempDir::new().expect("mapping failure root");
        let remote =
            prepare_remote_with_backend(root.path(), ObjectFormat::Sha256, RefBackend::Reftable);
        configure(&remote.repo, "uploadpack.allowsidebandall", "true");
        unset_all(&remote.repo, "uploadpack.blobpackfileuri");
        configure(
            &remote.repo,
            "uploadpack.blobpackfileuri",
            "malformed mapping",
        );
        let malformed = upload_pack(
            backend,
            &remote,
            &[],
            &protocol_request(&remote, Some("http"), true, false, false),
            false,
        );
        assert!(!malformed.status.success());
        if backend == ServerBackend::Stock {
            assert!(
                redact(&String::from_utf8_lossy(&malformed.stderr))
                    .contains("git-pack-objects died")
            );
        }
        let malformed_observable = (
            malformed.status.success(),
            error_category(&String::from_utf8_lossy(&malformed.stderr)),
        );
        if let Some(stock) = &malformed_baseline {
            assert_eq!(stock, &malformed_observable, "malformed mapping diverged");
        } else {
            malformed_baseline = Some(malformed_observable);
        }

        set_uri_mappings(&remote, "http", 1, false);
        let pack = &remote.uri_packs[0];
        let duplicate = format!(
            "{} {} http://fixture.invalid/duplicate.pack",
            pack.object, pack.hash
        );
        configure_add(&remote.repo, "uploadpack.blobpackfileuri", &duplicate);
        let duplicate_output = upload_pack(
            backend,
            &remote,
            &[],
            &protocol_request(&remote, Some("http"), true, false, false),
            false,
        );
        assert!(!duplicate_output.status.success());
        if backend == ServerBackend::Stock {
            assert!(
                redact(&String::from_utf8_lossy(&duplicate_output.stderr))
                    .contains("git-pack-objects died")
            );
        }
        let duplicate_observable = (
            duplicate_output.status.success(),
            error_category(&String::from_utf8_lossy(&duplicate_output.stderr)),
        );
        if let Some(stock) = &duplicate_baseline {
            assert_eq!(stock, &duplicate_observable, "duplicate mapping diverged");
        } else {
            duplicate_baseline = Some(duplicate_observable);
        }
    }
}

#[cfg(unix)]
#[test]
fn ssh_packfile_uri_round_trip_uses_real_git_ssh_path_for_hash_and_ref_backends() {
    for format in [ObjectFormat::Sha1, ObjectFormat::Sha256] {
        for ref_backend in [RefBackend::Files, RefBackend::Reftable] {
            let mut baseline = None;
            for service_backend in differential_server_backends() {
                let root = TempDir::new().expect("SSH matrix root");
                let remote = prepare_remote_with_backend(root.path(), format, ref_backend);
                let server = FixtureHttpServer::new(HttpConfig {
                    remote: remote.clone(),
                    uri_get_mode: UriGetMode::Serve,
                    backend: service_backend,
                });
                set_uri_mappings_for_server(
                    &remote,
                    &format!("http://127.0.0.1:{}", server.port),
                    2,
                    false,
                );
                server.set_remote(remote.clone());
                let (ssh_script, ssh_log) = write_fake_ssh(root.path());
                let destination = root.path().join("ssh-clone");
                let output = clone_ssh_for_backend(
                    service_backend,
                    service_backend,
                    &ssh_script,
                    &ssh_log,
                    &remote,
                    &destination,
                    ref_backend,
                );
                assert!(
                    output.status.success(),
                    "SSH clone failed: {}; invocation={}",
                    redact(&String::from_utf8_lossy(&output.stderr)),
                    redact(&fs::read_to_string(&ssh_log).unwrap_or_else(|error| error.to_string())),
                );
                let ssh_invocation = assert_ssh_invocation(&ssh_log);
                let log = server.log();
                let uri_paths = assert_uri_download_sequence(&log, &remote, 2);
                assert_clone_layout(&destination, &remote, ref_backend);
                assert_fetch_head(&destination, &remote);
                let observable = (
                    ssh_invocation,
                    uri_paths,
                    pack_objects(&destination, format),
                );
                if let Some(stock) = &baseline {
                    assert_eq!(stock, &observable, "stock/Zmin SSH observable diverged");
                } else {
                    baseline = Some(observable);
                }
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn ssh_packfile_uri_protocol_mismatch_falls_back_to_exact_inline_pack() {
    for format in [ObjectFormat::Sha1, ObjectFormat::Sha256] {
        for ref_backend in [RefBackend::Files, RefBackend::Reftable] {
            let mut baseline = None;
            for service_backend in differential_server_backends() {
                let root = TempDir::new().expect("SSH fallback root");
                let remote = prepare_remote_with_backend(root.path(), format, ref_backend);
                set_uri_mappings(&remote, "https", 1, false);
                let (ssh_script, ssh_log) = write_fake_ssh(root.path());
                let destination = root.path().join("ssh-fallback");
                let output = clone_ssh_for_backend(
                    service_backend,
                    service_backend,
                    &ssh_script,
                    &ssh_log,
                    &remote,
                    &destination,
                    ref_backend,
                );
                assert!(
                    output.status.success(),
                    "SSH fallback failed: {}",
                    redact(&String::from_utf8_lossy(&output.stderr))
                );
                let ssh_invocation = assert_ssh_invocation(&ssh_log);
                assert_inline_clone_layout(&destination, &remote, ref_backend);
                assert_fetch_head(&destination, &remote);
                let observable = (ssh_invocation, pack_objects(&destination, format));
                if let Some(stock) = &baseline {
                    assert_eq!(stock, &observable, "stock/Zmin SSH fallback diverged");
                } else {
                    baseline = Some(observable);
                }
            }
        }
    }
}

#[test]
fn git_daemon_packfile_uri_round_trip_uses_bounded_real_daemon_for_matrix() {
    for format in [ObjectFormat::Sha1, ObjectFormat::Sha256] {
        for ref_backend in [RefBackend::Files, RefBackend::Reftable] {
            let mut baseline = None;
            for daemon_backend in differential_server_backends() {
                for client_backend in differential_server_backends() {
                    let root = TempDir::new().expect("daemon matrix root");
                    let remote = prepare_daemon_remote(root.path(), format, ref_backend);
                    let server = FixtureHttpServer::new(HttpConfig {
                        remote: remote.clone(),
                        uri_get_mode: UriGetMode::Serve,
                        backend: daemon_backend,
                    });
                    set_uri_mappings_for_server(
                        &remote,
                        &format!("http://127.0.0.1:{}", server.port),
                        2,
                        false,
                    );
                    server.set_remote(remote.clone());
                    let daemon_binary = match daemon_backend {
                        ServerBackend::Stock => require_pinned_stock_daemon(),
                        ServerBackend::Zmin => validated_zmin_bin(root.path()),
                    };
                    let daemon =
                        GitDaemon::start(daemon_backend, &daemon_binary, root.path(), &remote);
                    let daemon_response = daemon_upload_pack_response(&daemon, &remote, "http");
                    daemon.assert_selected_upload_pack(daemon_backend);
                    let daemon_capabilities =
                        capability_lines(before_first_flush(&daemon_response));
                    assert!(
                        daemon_capabilities
                            .iter()
                            .any(|line| line.contains("packfile-uris")),
                        "daemon export did not advertise configured packfile-uris for {daemon_backend:?}: {daemon_capabilities:?}"
                    );
                    let daemon_response_body = after_first_flush(&daemon_response);
                    assert_response_contract(&remote, daemon_response_body, 2);
                    let daemon_observable = (
                        normalize_capabilities(&daemon_capabilities),
                        direct_observable(&remote, daemon_response_body),
                    );
                    let destination = root.path().join("daemon-clone");
                    let output = clone_git_daemon_for_backend(
                        client_backend,
                        &daemon,
                        &destination,
                        ref_backend,
                    );
                    assert!(
                        output.status.success(),
                        "git:// clone failed for {format:?}/{ref_backend:?}/{daemon_backend:?}/{client_backend:?}: {}",
                        redact(&String::from_utf8_lossy(&output.stderr))
                    );
                    let log = server.log();
                    let uri_paths = assert_uri_download_sequence(&log, &remote, 2);
                    assert_clone_layout(&destination, &remote, ref_backend);
                    assert_fetch_head(&destination, &remote);
                    let observable = (
                        daemon_observable,
                        uri_paths,
                        pack_objects(&destination, format),
                    );
                    if let Some(stock) = &baseline {
                        assert_eq!(stock, &observable, "stock/Zmin daemon observable diverged");
                    } else {
                        baseline = Some(observable);
                    }
                }
            }
        }
    }
}

#[test]
fn git_daemon_packfile_uri_protocol_mismatch_falls_back_to_exact_inline_pack() {
    for format in [ObjectFormat::Sha1, ObjectFormat::Sha256] {
        for ref_backend in [RefBackend::Files, RefBackend::Reftable] {
            let mut baseline = None;
            for daemon_backend in differential_server_backends() {
                for client_backend in differential_server_backends() {
                    let root = TempDir::new().expect("daemon fallback root");
                    let remote = prepare_daemon_remote(root.path(), format, ref_backend);
                    set_uri_mappings(&remote, "https", 1, false);
                    let daemon_binary = match daemon_backend {
                        ServerBackend::Stock => require_pinned_stock_daemon(),
                        ServerBackend::Zmin => validated_zmin_bin(root.path()),
                    };
                    let daemon =
                        GitDaemon::start(daemon_backend, &daemon_binary, root.path(), &remote);
                    let daemon_response = daemon_upload_pack_response(&daemon, &remote, "http");
                    daemon.assert_selected_upload_pack(daemon_backend);
                    let daemon_response_body = after_first_flush(&daemon_response);
                    assert!(
                        !response_section_names(daemon_response_body)
                            .contains(&"packfile-uris".to_owned())
                    );
                    assert_eq!(
                        inline_object_set(&remote, &inline_pack_bytes(daemon_response_body)),
                        graph_objects(&remote),
                        "daemon mismatch fallback inline object set"
                    );
                    let daemon_observable = direct_observable(&remote, daemon_response_body);
                    let destination = root.path().join("daemon-fallback");
                    let output = clone_git_daemon_for_backend(
                        client_backend,
                        &daemon,
                        &destination,
                        ref_backend,
                    );
                    assert!(
                        output.status.success(),
                        "git:// fallback failed for {format:?}/{ref_backend:?}/{daemon_backend:?}/{client_backend:?}: {}",
                        redact(&String::from_utf8_lossy(&output.stderr))
                    );
                    assert_inline_clone_layout(&destination, &remote, ref_backend);
                    assert_fetch_head(&destination, &remote);
                    let observable = (daemon_observable, pack_objects(&destination, format));
                    if let Some(stock) = &baseline {
                        assert_eq!(stock, &observable, "stock/Zmin daemon fallback diverged");
                    } else {
                        baseline = Some(observable);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod harness_regressions {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn streaming_capture_enforces_exact_limit_without_overshoot() {
        let exact = vec![0x5a; 17];
        assert_eq!(
            capture_stream(Cursor::new(exact.clone()), exact.len()).unwrap(),
            exact
        );
        let oversized = vec![0x5a; 18];
        assert!(matches!(
            capture_stream(Cursor::new(oversized), 17),
            Err(CaptureError::Limit(17))
        ));
    }

    #[test]
    fn watchdog_handles_a_non_reader_before_stdin_write_can_complete() {
        let root = TempDir::new().expect("watchdog regression root");
        let mut command = Command::new("/bin/sh");
        HermeticEnvironment::apply(&mut command, root.path());
        command.current_dir(root.path()).args(["-c", "sleep 2"]);
        let input = vec![0_u8; MAX_BODY_BYTES];
        let result = catch_unwind(AssertUnwindSafe(|| {
            run_command_input(
                command,
                &input,
                "non-reader watchdog regression",
                Duration::from_millis(100),
            )
        }));
        assert!(result.is_err(), "non-reader must hit the watchdog");
    }

    #[test]
    fn malformed_packet_and_request_sequences_are_rejected_structurally() {
        for bytes in [format!("{:04x}", 3).into_bytes(), b"000".to_vec()] {
            assert!(catch_unwind(AssertUnwindSafe(|| parse_packets(&bytes))).is_err());
        }
        let malformed_request = format!("{:04x}command=fetch", 17).into_bytes();
        assert!(catch_unwind(AssertUnwindSafe(|| request_signature(&malformed_request))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| parse_uri_record(b"hash uri extra\n"))).is_err());
    }
}
