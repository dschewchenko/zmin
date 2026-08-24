//! Strict, independent Git v2.55 partial-clone/filter oracle.
//!
//! The oracle deliberately owns its fixture, transports, process watchdog, and
//! packet parser.  It does not import another transport test: the partial-clone
//! contract is too easy to accidentally weaken by sharing a permissive helper.
//! Every cell runs the pinned stock Git and the current Zmin binary and checks
//! the observable repository state plus the protocol request that produced it.

#![allow(clippy::too_many_lines)]

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(unix)]
use libc::{self, SIGKILL};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::process::CommandExt;

use tempfile::{NamedTempFile, TempDir};

const GIT_BUNDLE: &str = "/Users/dschewchenko/.cache/zmin/git-upstream/http-bundle-v2.55.0-e9019fcafe0040228b8631c30f97ae1adb61bcdc-Darwin-arm64-with-http-fetch-pinned";
const STOCK_GIT: &str = "/Users/dschewchenko/.cache/zmin/git-upstream/http-bundle-v2.55.0-e9019fcafe0040228b8631c30f97ae1adb61bcdc-Darwin-arm64-with-http-fetch-pinned/git";
const DAEMON_BUNDLE: &str = "/Users/dschewchenko/.cache/zmin/git-upstream/git-daemon-v2.55.0-e9019fcafe0040228b8631c30f97ae1adb61bcdc-Darwin-arm64-pinned";
const STOCK_DAEMON: &str = "/Users/dschewchenko/.cache/zmin/git-upstream/git-daemon-v2.55.0-e9019fcafe0040228b8631c30f97ae1adb61bcdc-Darwin-arm64-pinned/git-daemon";
const STOCK_SUBMODULE_BUNDLE: &str = "/Users/dschewchenko/.cache/zmin/git-upstream/git-submodule-v2.55.0-e9019fcafe0040228b8631c30f97ae1adb61bcdc-Darwin-arm64-pinned";
const STOCK_SUBMODULE: &str = "/Users/dschewchenko/.cache/zmin/git-upstream/git-submodule-v2.55.0-e9019fcafe0040228b8631c30f97ae1adb61bcdc-Darwin-arm64-pinned/git-submodule";
const STOCK_SUBMODULE_SHA256: &str =
    "55a1a450b48fb98cc8c3f5745c411e3391ac7659f46eb4b7eef9053aa3614353";
const SUBMODULE_HELPER_TOTAL_BYTES: u64 = 12_632_333;
const SUBMODULE_HELPER_EXECUTABLES: &[(&str, BundlePermissionExpectation)] = &[
    ("basename", BundlePermissionExpectation::EXECUTABLE),
    ("expr", BundlePermissionExpectation::EXECUTABLE),
    ("git", BundlePermissionExpectation::EXECUTABLE),
    ("git-sh-i18n", BundlePermissionExpectation::WRITABLE_DATA),
    (
        "git-sh-i18n--envsubst",
        BundlePermissionExpectation::EXECUTABLE,
    ),
    ("git-sh-setup", BundlePermissionExpectation::WRITABLE_DATA),
    ("git-submodule", BundlePermissionExpectation::EXECUTABLE),
    ("git-upload-pack", BundlePermissionExpectation::EXECUTABLE),
    ("sed", BundlePermissionExpectation::EXECUTABLE),
    ("uname", BundlePermissionExpectation::EXECUTABLE),
    ("wc", BundlePermissionExpectation::EXECUTABLE),
];
const SUBMODULE_HELPER_METADATA: &[(&str, BundlePermissionExpectation)] = &[
    ("manifest.tsv", BundlePermissionExpectation::READ_ONLY_DATA),
    (
        "manifest.tsv.sha256",
        BundlePermissionExpectation::READ_ONLY_DATA,
    ),
    (
        "source-manifest.tsv",
        BundlePermissionExpectation::READ_ONLY_DATA,
    ),
    (
        "source-manifest.tsv.sha256",
        BundlePermissionExpectation::READ_ONLY_DATA,
    ),
    (
        "source-tree.tsv",
        BundlePermissionExpectation::READ_ONLY_DATA,
    ),
    (
        "source-tree.tsv.sha256",
        BundlePermissionExpectation::READ_ONLY_DATA,
    ),
];
const STOCK_GIT_SHA256: &str = "ca63eda87df1aaffa2b80710c4a9de6212eba6c84e8dfb3011a2498b36e841cb";
const STOCK_HTTP_FETCH_SHA256: &str =
    "fc2e8b9e47cafb39140ca90f56fbc6cb09912c6d715feb020157733868f08b0e";
const STOCK_HTTP_BACKEND_SHA256: &str =
    "558316c4ea88b9e50327def4dc234c7404e11790ab77da1c2a6bf52879bd1b43";
const STOCK_REMOTE_HTTP_SHA256: &str =
    "6ba041e1c71c11eb3d5f66579ea29734135446a4575ec3ecf0574437b795b640";
const STOCK_MANIFEST_SHA256: &str =
    "e70ca5308dbddac10ac951a0a16a31645f9a7f4338be026e4257d961b3eb29ad";
const STOCK_BUNDLE_TABLE_SHA256: &str =
    "cc295dc42051d204e2505acfab55d43767260fdd0cb016c6e5d3f507cf74bfdb";
const STOCK_MANIFEST_SIDECAR_SHA256: &str =
    "55c0258449052cd26f3868e464b96d9fded1e479c6f10c5286bf700bf2a4f0fd";
const STOCK_BUNDLE_SIDECAR_SHA256: &str =
    "a65872f1994b02a855776fed8554710a104c2869073976b021c031e93a2c945b";
const STOCK_DAEMON_SHA256: &str =
    "476cb91fe4b8f362da2136d4ecd5129c68c86973594350ced5a89d9a20c5c28b";
const STOCK_DAEMON_MANIFEST_SHA256: &str =
    "f9a00fcc8c39b3772c753b6b68bf029af50b156b27ee0efa3051aaf406a3505f";
const STOCK_DAEMON_MANIFEST_SIDECAR_SHA256: &str =
    "e92c1aba3f894e4f957c8570102c2ff63fc94615445720ff3817dafe5be89cac";
const STOCK_DAEMON_HASH_SIDECAR_SHA256: &str =
    "7adedf3bfaafebf6be5e69c09152669fd2bb5e9a41b5ffc75379a075b45231da";
const STOCK_DAEMON_UPLOAD_PACK_SIDECAR_SHA256: &str =
    "de42a0de7e3175452d1926fb8fd0cc2bb01bca61fdf787e17b44776a1c593eef";
const MAX_OUTPUT: usize = 4 * 1024 * 1024;
const MAX_HTTP_HEADER: usize = 128 * 1024;
const MAX_HTTP_BODY: usize = 8 * 1024 * 1024;
const MAX_HTTP_REQUESTS: usize = 96;
const MAX_HTTP_LOG_BYTES: usize = 16 * 1024 * 1024;
const PROCESS_TIMEOUT: Duration = Duration::from_secs(20);
const THREAD_TIMEOUT: Duration = Duration::from_secs(5);
const ZMIN_BIN_SHA256_ENV: &str = "ZMIN_BIN_SHA256";
const STOCK_SUBMODULE_ENV: &str = "ZMIN_STOCK_GIT_SUBMODULE";
const STOCK_SUBMODULE_SHA256_ENV: &str = "ZMIN_STOCK_GIT_SUBMODULE_SHA256";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OracleMode {
    Differential,
    StockOnly,
    ZminOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BundlePermissionExpectation {
    #[cfg(unix)]
    unix_mode: u32,
    #[cfg(windows)]
    windows_readonly: bool,
}

impl BundlePermissionExpectation {
    const EXECUTABLE: Self = Self::new(0o555, true);
    const WRITABLE_DATA: Self = Self::new(0o644, false);
    const READ_ONLY_DATA: Self = Self::new(0o444, true);

    const fn new(unix_mode: u32, windows_readonly: bool) -> Self {
        #[cfg(not(unix))]
        let _ = unix_mode;
        #[cfg(not(windows))]
        let _ = windows_readonly;
        Self {
            #[cfg(unix)]
            unix_mode,
            #[cfg(windows)]
            windows_readonly,
        }
    }
}

fn oracle_mode() -> OracleMode {
    match std::env::var("ZMIN_ORACLE_MODE").as_deref() {
        Ok("differential") | Err(_) => OracleMode::Differential,
        Ok("stock") => OracleMode::StockOnly,
        Ok("zmin") => OracleMode::ZminOnly,
        Ok(value) => panic!("ZMIN_ORACLE_MODE must be differential, stock, or zmin, got {value}"),
    }
}

static VALIDATED_ZMIN: OnceLock<(PathBuf, String)> = OnceLock::new();
static VALIDATED_SUBMODULE: OnceLock<PathBuf> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HashFormat {
    Sha1,
    Sha256,
}

impl HashFormat {
    fn init_arg(self) -> Option<&'static str> {
        match self {
            Self::Sha1 => None,
            Self::Sha256 => Some("--object-format=sha256"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RefFormat {
    Files,
    Reftable,
}

impl RefFormat {
    fn init_arg(self) -> Option<&'static str> {
        match self {
            Self::Files => None,
            Self::Reftable => Some("--ref-format=reftable"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Reftable => "reftable",
        }
    }
}

#[derive(Clone, Debug)]
struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    head: String,
    tree: String,
    previous_tree: String,
    previous_dir: String,
    previous_commit: String,
    blobs: Vec<(String, String)>,
    expected_objects: Option<BTreeSet<String>>,
    source_expected_objects: Option<BTreeSet<String>>,
    blob_objects: BTreeSet<String>,
    metadata_objects: BTreeSet<String>,
    submodule_head: Option<String>,
    submodule_tree: Option<String>,
    submodule_blob: Option<String>,
    submodule_expected_objects: Option<BTreeSet<String>>,
    submodule_gitmodules_blob: Option<String>,
    refs: RefFormat,
}

#[derive(Clone)]
struct RequestRecord {
    method: String,
    path: String,
    body: Vec<u8>,
    response: Vec<u8>,
}

#[derive(Clone)]
struct RequestLog {
    records: Arc<Mutex<Vec<RequestRecord>>>,
    accepted: Arc<AtomicUsize>,
    completed: Arc<AtomicUsize>,
}

impl Default for RequestLog {
    fn default() -> Self {
        Self {
            records: Arc::new(Mutex::new(Vec::new())),
            accepted: Arc::new(AtomicUsize::new(0)),
            completed: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl RequestLog {
    fn accepted(&self) {
        self.accepted.fetch_add(1, Ordering::AcqRel);
    }

    fn completed(&self) {
        self.completed.fetch_add(1, Ordering::AcqRel);
    }

    fn push(&self, record: RequestRecord) {
        let mut records = self.records.lock().expect("request log lock");
        assert!(
            records.len() < MAX_HTTP_REQUESTS,
            "HTTP request bound exceeded"
        );
        let logged_bytes = records
            .iter()
            .map(|record| record.body.len().saturating_add(record.response.len()))
            .sum::<usize>();
        assert!(
            logged_bytes
                .saturating_add(record.body.len())
                .saturating_add(record.response.len())
                <= MAX_HTTP_LOG_BYTES,
            "HTTP request log bound exceeded"
        );
        records.push(record);
    }

    fn snapshot(&self) -> Vec<RequestRecord> {
        self.records.lock().expect("request log lock").clone()
    }

    fn wait_quiescent(&self, minimum_records: usize) -> Vec<RequestRecord> {
        let deadline = Instant::now() + THREAD_TIMEOUT;
        loop {
            let records = self.snapshot();
            if self.completed.load(Ordering::Acquire) == self.accepted.load(Ordering::Acquire)
                && records.len() >= minimum_records
            {
                return records;
            }
            assert!(Instant::now() < deadline, "HTTP request barrier timed out");
            thread::sleep(Duration::from_millis(2));
        }
    }
}

struct Hermetic {
    _root: TempDir,
    home: PathBuf,
    xdg: PathBuf,
}

impl Hermetic {
    fn new(parent: &Path) -> Self {
        let root = tempfile::Builder::new()
            .prefix("partial-filter-env-")
            .tempdir_in(parent)
            .expect("create hermetic environment");
        let home = root.path().join("home");
        let xdg = root.path().join("xdg");
        fs::create_dir_all(&home).expect("create hermetic HOME");
        fs::create_dir_all(xdg.join("config")).expect("create hermetic XDG config");
        fs::create_dir_all(xdg.join("cache")).expect("create hermetic XDG cache");
        fs::create_dir_all(xdg.join("data")).expect("create hermetic XDG data");
        Self {
            _root: root,
            home,
            xdg,
        }
    }

    fn apply(&self, command: &mut Command) {
        let helper_dir = VALIDATED_SUBMODULE
            .get()
            .and_then(|path| path.parent())
            .map(|path| path.display().to_string());
        let path = helper_dir.map_or_else(
            || GIT_BUNDLE.to_owned(),
            |helper_dir| format!("{helper_dir}:{GIT_BUNDLE}"),
        );
        command
            .env_clear()
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.xdg.join("config"))
            .env("XDG_CACHE_HOME", self.xdg.join("cache"))
            .env("XDG_DATA_HOME", self.xdg.join("data"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", null_device())
            .env("GIT_CONFIG_SYSTEM", null_device())
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_TEMPLATE_DIR", Path::new(GIT_BUNDLE).join("templates"))
            .env("GIT_EXEC_PATH", GIT_BUNDLE)
            .env("PATH", path)
            .env("GIT_AUTHOR_NAME", "Partial Filter Oracle")
            .env("GIT_AUTHOR_EMAIL", "partial-filter-oracle@example.invalid")
            .env("GIT_AUTHOR_DATE", "1700000000 +0000")
            .env("GIT_COMMITTER_NAME", "Partial Filter Oracle")
            .env(
                "GIT_COMMITTER_EMAIL",
                "partial-filter-oracle@example.invalid",
            )
            .env("GIT_COMMITTER_DATE", "1700000000 +0000")
            .env("LC_ALL", "C")
            .env("LANG", "C");
    }
}

#[rustfmt::skip]
fn null_device() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    fn spawn(command: &mut Command, label: &str) -> Self {
        #[cfg(unix)]
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command
            .spawn()
            .unwrap_or_else(|error| panic!("{label} spawn failed: {error}"));
        Self { child: Some(child) }
    }

    fn child(&mut self) -> &mut Child {
        self.child.as_mut().expect("child guard present")
    }

    fn kill_group(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        #[cfg(unix)]
        unsafe {
            let _ = libc::kill(-(child.id() as libc::pid_t), SIGKILL);
        }
        let _ = child.kill();
    }

    fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.as_mut().expect("child guard present").wait()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.kill_group();
        let _ = self.wait();
    }
}

fn bounded_read<R: Read>(mut reader: R) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len() + count > MAX_OUTPUT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "output bound exceeded",
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

fn run_bounded(mut command: Command, label: &str) -> Output {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut guard = ChildGuard::spawn(&mut command, label);
    let stdout = guard.child().stdout.take().expect("stdout pipe");
    let stderr = guard.child().stderr.take().expect("stderr pipe");
    let stdout_thread = thread::spawn(|| bounded_read(stdout));
    let stderr_thread = thread::spawn(|| bounded_read(stderr));
    let deadline = Instant::now() + PROCESS_TIMEOUT;
    let status = loop {
        match guard.child().try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                guard.kill_group();
                break guard
                    .wait()
                    .unwrap_or_else(|error| panic!("{label} timeout wait: {error}"));
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => panic!("{label} wait failed: {error}"),
        }
    };
    guard.kill_group();
    let stdout = join_reader(stdout_thread, "stdout", &mut guard);
    let stderr = join_reader(stderr_thread, "stderr", &mut guard);
    Output {
        status,
        stdout,
        stderr,
    }
}

fn run_bounded_input(mut command: Command, input: &[u8], label: &str) -> Output {
    assert!(input.len() <= MAX_HTTP_BODY, "{label} input bound exceeded");
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut guard = ChildGuard::spawn(&mut command, label);
    let stdin = guard.child().stdin.take().expect("stdin pipe");
    let stdout = guard.child().stdout.take().expect("stdout pipe");
    let stderr = guard.child().stderr.take().expect("stderr pipe");
    let input = input.to_vec();
    let stdin_thread = thread::spawn(move || {
        let mut stdin = stdin;
        let result = stdin.write_all(&input);
        drop(stdin);
        result
    });
    let stdout_thread = thread::spawn(|| bounded_read(stdout));
    let stderr_thread = thread::spawn(|| bounded_read(stderr));
    let deadline = Instant::now() + PROCESS_TIMEOUT;
    let status = loop {
        match guard.child().try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                guard.kill_group();
                break guard
                    .wait()
                    .unwrap_or_else(|error| panic!("{label} timeout wait: {error}"));
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => panic!("{label} wait failed: {error}"),
        }
    };
    guard.kill_group();
    join_writer(stdin_thread, &mut guard, label);
    let stdout = join_reader(stdout_thread, "stdout", &mut guard);
    let stderr = join_reader(stderr_thread, "stderr", &mut guard);
    Output {
        status,
        stdout,
        stderr,
    }
}

fn join_writer(handle: JoinHandle<io::Result<()>>, guard: &mut ChildGuard, label: &str) {
    let deadline = Instant::now() + THREAD_TIMEOUT;
    while !handle.is_finished() {
        if Instant::now() >= deadline {
            guard.kill_group();
            let _ = guard.wait();
            let joined = handle.join();
            panic!(
                "{label} stdin writer did not stop; cancellation reaped={}",
                joined.is_ok()
            );
        }
        thread::sleep(Duration::from_millis(5));
    }
    handle
        .join()
        .unwrap_or_else(|_| panic!("{label} stdin writer panicked"))
        .unwrap_or_else(|error| panic!("{label} stdin writer failed: {error}"));
}

fn join_reader(
    handle: JoinHandle<io::Result<Vec<u8>>>,
    name: &str,
    guard: &mut ChildGuard,
) -> Vec<u8> {
    let deadline = Instant::now() + THREAD_TIMEOUT;
    while !handle.is_finished() {
        if Instant::now() >= deadline {
            guard.kill_group();
            let _ = guard.wait();
            let joined = handle.join();
            panic!(
                "{name} reader did not stop; cancellation reaped={}",
                joined.is_ok()
            );
        }
        thread::sleep(Duration::from_millis(5));
    }
    handle
        .join()
        .unwrap_or_else(|_| panic!("{name} reader panicked"))
        .unwrap_or_else(|error| panic!("{name} reader failed: {error}"))
}

fn sha256(path: &Path, env: &Hermetic) -> String {
    digest_file(path, env, "256")
}

fn digest_file(path: &Path, env: &Hermetic, algorithm: &str) -> String {
    let mut command = Command::new("/usr/bin/shasum");
    env.apply(&mut command);
    command.args(["-a", algorithm, path.to_str().expect("hash path UTF-8")]);
    let output = run_bounded(command, "pinned digest");
    assert!(
        output.status.success(),
        "pinned digest failed; stderr {}",
        redacted_stderr(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .expect("pinned digest output")
        .to_owned()
}

fn assert_regular_hash(path: &Path, expected: &str, env: &Hermetic) {
    let metadata = fs::symlink_metadata(path).unwrap_or_else(|_| panic!("pinned artifact missing"));
    assert!(
        metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
        "pinned artifact is not a regular file"
    );
    assert_eq!(
        fs::canonicalize(path).expect("canonicalize artifact"),
        path,
        "pinned artifact must not be symlinked"
    );
    assert_eq!(sha256(path, env), expected, "pinned artifact hash changed");
}

fn validate_submodule_helper(parent: &Path, env: &Hermetic) -> PathBuf {
    let configured = PathBuf::from(STOCK_SUBMODULE);
    if let Some(value) = std::env::var_os(STOCK_SUBMODULE_ENV) {
        assert_eq!(
            PathBuf::from(value),
            configured,
            "{STOCK_SUBMODULE_ENV} does not match the fixed pinned helper"
        );
    }
    assert!(
        configured.is_absolute(),
        "pinned submodule helper must be absolute"
    );
    assert!(
        configured.file_name().and_then(|name| name.to_str()) == Some("git-submodule"),
        "pinned submodule helper has an unexpected name"
    );
    assert_eq!(
        configured.parent().expect("pinned helper parent"),
        Path::new(STOCK_SUBMODULE_BUNDLE),
        "pinned submodule helper bundle changed"
    );
    if let Ok(value) = std::env::var(STOCK_SUBMODULE_SHA256_ENV) {
        assert_eq!(
            value, STOCK_SUBMODULE_SHA256,
            "{STOCK_SUBMODULE_SHA256_ENV} does not match the fixed helper hash"
        );
    }
    assert_regular_hash(&configured, STOCK_SUBMODULE_SHA256, env);
    let helper = fs::canonicalize(&configured).expect("canonicalize pinned submodule helper");
    assert_eq!(
        helper, configured,
        "pinned submodule helper must not be symlinked"
    );
    let _ = parent;
    helper
}

fn assert_bundle_permissions(
    path: &Path,
    metadata: &fs::Metadata,
    expected: BundlePermissionExpectation,
) {
    #[cfg(unix)]
    assert_eq!(
        metadata.permissions().mode() & 0o777,
        expected.unix_mode,
        "pinned helper bundle mode changed for {}",
        path.display()
    );
    #[cfg(windows)]
    assert_eq!(
        metadata.permissions().readonly(),
        expected.windows_readonly,
        "pinned helper bundle readonly state changed for {}",
        path.display()
    );
    fs::File::open(path).unwrap_or_else(|error| {
        panic!(
            "pinned helper bundle entry is not readable at {}: {error}",
            path.display()
        )
    });
}

fn validate_submodule_bundle(helper: &Path, env: &Hermetic) {
    let bundle = helper.parent().expect("pinned helper bundle");
    let entries = fs::read_dir(bundle)
        .expect("read pinned helper bundle")
        .collect::<Result<Vec<_>, _>>()
        .expect("enumerate pinned helper bundle");
    assert!(
        entries.len() <= 64,
        "pinned helper bundle file bound exceeded"
    );
    let mut expected_names = SUBMODULE_HELPER_EXECUTABLES
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    expected_names.extend(
        SUBMODULE_HELPER_EXECUTABLES
            .iter()
            .map(|(name, _)| format!("{name}.sha256")),
    );
    expected_names.extend(
        SUBMODULE_HELPER_METADATA
            .iter()
            .map(|(name, _)| (*name).to_owned()),
    );
    let actual_names = entries
        .iter()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual_names, expected_names,
        "pinned helper file set changed"
    );
    let total_bytes = entries
        .iter()
        .map(|entry| {
            fs::symlink_metadata(entry.path())
                .expect("stat pinned helper bundle entry")
                .len()
        })
        .sum::<u64>();
    assert_eq!(
        total_bytes, SUBMODULE_HELPER_TOTAL_BYTES,
        "pinned helper bundle size changed"
    );
    for (name, permissions) in SUBMODULE_HELPER_EXECUTABLES
        .iter()
        .chain(SUBMODULE_HELPER_METADATA.iter())
    {
        let path = bundle.join(name);
        let metadata = fs::symlink_metadata(&path).expect("stat pinned helper bundle file");
        assert!(
            metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
            "pinned helper bundle entry is not a regular file"
        );
        assert_bundle_permissions(&path, &metadata, *permissions);
        assert_eq!(
            fs::canonicalize(&path).expect("canonicalize helper bundle file"),
            path,
            "pinned helper bundle file must not be symlinked"
        );
    }
    for (name, _) in SUBMODULE_HELPER_EXECUTABLES {
        let sidecar = bundle.join(format!("{name}.sha256"));
        let metadata = fs::symlink_metadata(&sidecar).expect("stat helper sidecar");
        assert!(
            metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
            "helper sidecar is not a regular file"
        );
        assert_bundle_permissions(
            &sidecar,
            &metadata,
            BundlePermissionExpectation::READ_ONLY_DATA,
        );
        assert_eq!(
            fs::canonicalize(&sidecar).expect("canonicalize helper sidecar"),
            sidecar
        );
    }
    for (name, expected) in [
        ("git", STOCK_GIT_SHA256),
        ("git-upload-pack", STOCK_GIT_SHA256),
        (
            "git-sh-i18n",
            "5fe5d73ff4ed9f878349c0abee7abb1fde00993ee550d56dd58e13cd23e7ef90",
        ),
        (
            "git-sh-setup",
            "e24dfe8128b884658789019797299c2d815b17153aaa5f1c0151d93f0a9a30a5",
        ),
        (
            "git-sh-i18n--envsubst",
            "87a615429af6cdc4e444cdfe9fb54a6cf7f361252c01b16b6a371c68778a2f7d",
        ),
        (
            "basename",
            "2ead20f3a006d422b6c74da27d06cab4c1e2036f3df9c47139c394e0b9a15b2c",
        ),
        (
            "expr",
            "2b12e5dabea6c7e1e203302dceb54d7f0874e18b030c13e12a60b30168c87567",
        ),
        (
            "sed",
            "0cc19a5ef118f38f0e7806d0f1450d6588b008b3afcc83994a0eafbba8f0cd09",
        ),
        (
            "uname",
            "92e8e756b997dc539a3407c67d2525dd18be974200fa49789c61d506b22bc4f1",
        ),
        (
            "wc",
            "042cb28d986929e7b32e863501379eb7f0e44ea31acdec0289e8d3d6a7d2b0f0",
        ),
        (
            "basename.sha256",
            "4383096992747d572ef7acd6eb6d584d9ad2f67b6c84a8f7f813559cfafc2b53",
        ),
        (
            "expr.sha256",
            "081ea0e25ba2fb2bdd75e0bf26eb2ea8bc3b421f4ed9500bb14cd054dec6ba16",
        ),
        (
            "sed.sha256",
            "98e3d185cad5694b61d166a96be2c17ddc6eb7b080c18a285636ad6f071af033",
        ),
        (
            "uname.sha256",
            "360c70fc458e9463d1e66b401b80379d17b0f1d9559f8b8320e759d55237b05c",
        ),
        (
            "wc.sha256",
            "b4f36f1656ff2fe3111fe86601fa362b80463db50dcae3a61691181f8280c3e0",
        ),
        (
            "manifest.tsv",
            "4a11b8aa18f7b7c9c897ededda112f51ad7e1e29aee6a512578ad6b36eb9b2d1",
        ),
        (
            "manifest.tsv.sha256",
            "980a98dcdf840ae320e529eb3017f495eacf4a62b7efdf4a4a236165c844d7aa",
        ),
        (
            "source-manifest.tsv",
            "0e377e4906b6cf19556042540c4749304217a86fddbb5ebe5d73ebfbb603f7c8",
        ),
        (
            "source-manifest.tsv.sha256",
            "d14e7e611726e3aae832111de9a755e63699ae3eafa50c9a248adbde95e555bf",
        ),
        (
            "source-tree.tsv",
            "341b0de000913805d1aed9d11afea57e98f2e25397b1b8579900d6817b1b9708",
        ),
        (
            "source-tree.tsv.sha256",
            "78fe0459dee08e2dea4b888f92df3bc72766d4d6bb2a2a4b626b1c8fc30e574f",
        ),
        (
            "git.sha256",
            "45a4c1e51fd4d37b0c2d418eac818d28fe00327e6537a4424fc167cce9c07d48",
        ),
        (
            "git-upload-pack.sha256",
            STOCK_DAEMON_UPLOAD_PACK_SIDECAR_SHA256,
        ),
        (
            "git-submodule.sha256",
            "1647c343edc4301ce69a2ddf4343c7334e2fd132014a149038803d6203e8e048",
        ),
        (
            "git-sh-i18n.sha256",
            "d1a12ec6e7c35c968eacfe1210d2afc6b620cf7b7702b1e29e00f51e11691a03",
        ),
        (
            "git-sh-setup.sha256",
            "695f9660063386c0fea8483319cfcb7d9c9be2c58e1f4a2bd97c74ded3ec7c44",
        ),
        (
            "git-sh-i18n--envsubst.sha256",
            "99df0bd876664444dc5bf3f87768709350077554ddbcc9cc3b63d7d5825adf45",
        ),
    ] {
        assert_regular_hash(&bundle.join(name), expected, env);
    }
    for (name, expected) in [
        (
            "basename",
            "2ead20f3a006d422b6c74da27d06cab4c1e2036f3df9c47139c394e0b9a15b2c",
        ),
        (
            "expr",
            "2b12e5dabea6c7e1e203302dceb54d7f0874e18b030c13e12a60b30168c87567",
        ),
        (
            "sed",
            "0cc19a5ef118f38f0e7806d0f1450d6588b008b3afcc83994a0eafbba8f0cd09",
        ),
        (
            "uname",
            "92e8e756b997dc539a3407c67d2525dd18be974200fa49789c61d506b22bc4f1",
        ),
        (
            "wc",
            "042cb28d986929e7b32e863501379eb7f0e44ea31acdec0289e8d3d6a7d2b0f0",
        ),
        ("git-submodule", STOCK_SUBMODULE_SHA256),
        ("git", STOCK_GIT_SHA256),
        ("git-upload-pack", STOCK_GIT_SHA256),
        (
            "git-sh-i18n",
            "5fe5d73ff4ed9f878349c0abee7abb1fde00993ee550d56dd58e13cd23e7ef90",
        ),
        (
            "git-sh-setup",
            "e24dfe8128b884658789019797299c2d815b17153aaa5f1c0151d93f0a9a30a5",
        ),
        (
            "git-sh-i18n--envsubst",
            "87a615429af6cdc4e444cdfe9fb54a6cf7f361252c01b16b6a371c68778a2f7d",
        ),
    ] {
        let contents = fs::read_to_string(bundle.join(format!("{name}.sha256")))
            .expect("read helper executable sidecar");
        assert_eq!(
            contents,
            format!("{expected}  {name}\n"),
            "helper sidecar mapping"
        );
    }
    let manifest = fs::read_to_string(bundle.join("manifest.tsv")).expect("read helper manifest");
    assert!(manifest.contains("upstream_git_tag\tv2.55.0\n"));
    assert!(manifest.contains("upstream_git_commit\te9019fcafe0040228b8631c30f97ae1adb61bcdc\n"));
    assert_eq!(
        fs::read_to_string(bundle.join("manifest.tsv.sha256"))
            .expect("read helper manifest sidecar"),
        "4a11b8aa18f7b7c9c897ededda112f51ad7e1e29aee6a512578ad6b36eb9b2d1  manifest.tsv\n"
    );
    assert_eq!(
        fs::read_to_string(bundle.join("source-manifest.tsv.sha256"))
            .expect("read source manifest sidecar"),
        "0e377e4906b6cf19556042540c4749304217a86fddbb5ebe5d73ebfbb603f7c8  source-manifest.tsv\n"
    );
    assert_eq!(
        fs::read_to_string(bundle.join("source-tree.tsv.sha256"))
            .expect("read source tree sidecar"),
        "341b0de000913805d1aed9d11afea57e98f2e25397b1b8579900d6817b1b9708  source-tree.tsv\n"
    );
}

fn zmin_path() -> PathBuf {
    PathBuf::from(std::env::var_os("ZMIN_BIN").expect("ZMIN_BIN is mandatory for parity mode"))
}

fn validate_zmin(env: &Hermetic) -> (PathBuf, String) {
    let configured = zmin_path();
    assert!(configured.is_absolute(), "ZMIN_BIN must be absolute");
    let metadata = fs::symlink_metadata(&configured).expect("stat ZMIN_BIN");
    assert!(metadata.file_type().is_file() && !metadata.file_type().is_symlink());
    let canonical = fs::canonicalize(&configured).expect("canonicalize ZMIN_BIN");
    assert_eq!(canonical, configured, "ZMIN_BIN must not be symlinked");
    let hash = sha256(&canonical, env);
    let expected = std::env::var(ZMIN_BIN_SHA256_ENV)
        .unwrap_or_else(|_| panic!("{ZMIN_BIN_SHA256_ENV} is mandatory for Zmin parity probes"));
    assert!(expected.len() == 64 && expected.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(hash, expected, "ZMIN_BIN SHA-256 provenance mismatch");
    let mut command = Command::new(&canonical);
    env.apply(&mut command);
    command.arg("--version");
    let output = run_bounded(command, "Zmin identity");
    assert!(
        output.status.success(),
        "Zmin identity failed; stderr {}",
        redacted_stderr(&output.stderr)
    );
    let version = text(&output);
    assert!(
        version.starts_with("git version ") && version.contains("(zmin "),
        "Zmin source marker missing"
    );
    (canonical, hash)
}

fn validate_artifacts(parent: &Path) -> PathBuf {
    let env = Hermetic::new(parent);
    let helper = validate_submodule_helper(parent, &env);
    let _ = VALIDATED_SUBMODULE.set(helper);
    validate_submodule_bundle(
        VALIDATED_SUBMODULE
            .get()
            .expect("validated submodule helper"),
        &env,
    );
    let git = PathBuf::from(STOCK_GIT);
    assert_regular_hash(&git, STOCK_GIT_SHA256, &env);
    for (name, expected) in [
        ("git-http-fetch", STOCK_HTTP_FETCH_SHA256),
        ("git-http-backend", STOCK_HTTP_BACKEND_SHA256),
        ("git-remote-http", STOCK_REMOTE_HTTP_SHA256),
        ("manifest.tsv", STOCK_MANIFEST_SHA256),
        ("bundle.tsv", STOCK_BUNDLE_TABLE_SHA256),
        ("manifest.tsv.sha256", STOCK_MANIFEST_SIDECAR_SHA256),
        ("bundle.tsv.sha256", STOCK_BUNDLE_SIDECAR_SHA256),
    ] {
        assert_regular_hash(&Path::new(GIT_BUNDLE).join(name), expected, &env);
    }
    let version = run_git(&env, parent, &["--version"]);
    assert_eq!(text(&version), "git version 2.55.0");
    let git_manifest =
        fs::read_to_string(Path::new(GIT_BUNDLE).join("manifest.tsv")).expect("read Git manifest");
    assert!(git_manifest.contains("upstream_git_tag\tv2.55.0\n"));
    assert!(
        git_manifest.contains("upstream_git_commit\te9019fcafe0040228b8631c30f97ae1adb61bcdc\n")
    );
    let daemon = PathBuf::from(STOCK_DAEMON);
    assert_regular_hash(&daemon, STOCK_DAEMON_SHA256, &env);
    assert_regular_hash(
        &Path::new(DAEMON_BUNDLE).join("git-upload-pack"),
        STOCK_GIT_SHA256,
        &env,
    );
    for (name, expected) in [
        ("manifest.tsv", STOCK_DAEMON_MANIFEST_SHA256),
        ("manifest.tsv.sha256", STOCK_DAEMON_MANIFEST_SIDECAR_SHA256),
        ("git-daemon.sha256", STOCK_DAEMON_HASH_SIDECAR_SHA256),
        (
            "git-upload-pack.sha256",
            STOCK_DAEMON_UPLOAD_PACK_SIDECAR_SHA256,
        ),
    ] {
        assert_regular_hash(&Path::new(DAEMON_BUNDLE).join(name), expected, &env);
    }
    let manifest = fs::read_to_string(Path::new(DAEMON_BUNDLE).join("manifest.tsv"))
        .expect("read daemon manifest");
    assert!(manifest.contains("upstream_git_tag\tv2.55.0\n"));
    assert!(manifest.contains("upstream_git_commit\te9019fcafe0040228b8631c30f97ae1adb61bcdc\n"));
    assert_eq!(
        fs::read_to_string(Path::new(GIT_BUNDLE).join("manifest.tsv.sha256"))
            .expect("read Git manifest sidecar")
            .split_whitespace()
            .next(),
        Some(STOCK_MANIFEST_SHA256)
    );
    assert_eq!(
        fs::read_to_string(Path::new(GIT_BUNDLE).join("bundle.tsv.sha256"))
            .expect("read Git bundle sidecar")
            .split_whitespace()
            .next(),
        Some(STOCK_BUNDLE_TABLE_SHA256)
    );
    if oracle_mode() != OracleMode::StockOnly {
        let zmin = validate_zmin(&env);
        let _ = VALIDATED_ZMIN.set(zmin);
    }
    git
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn run_git(env: &Hermetic, cwd: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(STOCK_GIT);
    env.apply(&mut command);
    command.current_dir(cwd).args(args);
    run_bounded(command, "pinned Git")
}

fn run_zmin(env: &Hermetic, cwd: &Path, args: &[&str], extra: &[(&str, &Path)]) -> Output {
    let (canonical, expected_hash) = VALIDATED_ZMIN
        .get()
        .unwrap_or_else(|| panic!("Zmin binary must be validated before execution"));
    let current_hash = sha256(canonical, env);
    assert_eq!(
        current_hash, *expected_hash,
        "Zmin binary changed after validation"
    );
    let mut command = Command::new(canonical);
    env.apply(&mut command);
    command.current_dir(cwd).args(args);
    for (key, value) in extra {
        command.env(key, value);
    }
    run_bounded(command, "Zmin")
}

fn require_success(output: Output, label: &str) {
    assert!(
        output.status.success(),
        "{label} failed with status {:?}; stderr {}",
        output.status.code(),
        redacted_stderr(&output.stderr)
    );
}

fn redacted_stderr(stderr: &[u8]) -> String {
    format!("<redacted stderr: {} bytes>", stderr.len())
}

fn git_ok(env: &Hermetic, cwd: &Path, args: &[&str]) {
    require_success(run_git(env, cwd, args), "fixture Git");
}

fn init_repo(env: &Hermetic, cwd: &Path, format: HashFormat, refs: RefFormat) {
    let mut args = vec!["init", "--quiet", "--initial-branch=main"];
    if let Some(arg) = format.init_arg() {
        args.push(arg);
    }
    if let Some(arg) = refs.init_arg() {
        args.push(arg);
    }
    args.push(cwd.to_str().expect("repo path UTF-8"));
    require_success(
        run_git(env, cwd.parent().expect("repo parent"), &args),
        "fixture init",
    );
    git_ok(env, cwd, &["config", "user.name", "Partial Filter Oracle"]);
    git_ok(
        env,
        cwd,
        &[
            "config",
            "user.email",
            "partial-filter-oracle@example.invalid",
        ],
    );
    git_ok(env, cwd, &["config", "commit.gpgsign", "false"]);
    git_ok(env, cwd, &["config", "uploadpack.allowFilter", "true"]);
    git_ok(
        env,
        cwd,
        &["config", "uploadpack.allowAnySHA1InWant", "true"],
    );
}

fn digest_bytes(env: &Hermetic, parent: &Path, algorithm: &str, bytes: &[u8]) -> String {
    let mut input = NamedTempFile::new_in(parent).expect("independent hash input");
    input
        .write_all(bytes)
        .expect("write independent hash input");
    digest_file(input.path(), env, algorithm)
}

fn independent_blob_id(env: &Hermetic, parent: &Path, format: HashFormat, bytes: &[u8]) -> String {
    let mut input = format!("blob {}\0", bytes.len()).into_bytes();
    input.extend_from_slice(bytes);
    digest_bytes(
        env,
        parent,
        match format {
            HashFormat::Sha1 => "1",
            HashFormat::Sha256 => "256",
        },
        &input,
    )
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push_str(&format!("{byte:02x}"));
    }
    result
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    assert!(hex.len() % 2 == 0 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).expect("hex object id"))
        .collect()
}

fn independent_object_id(
    env: &Hermetic,
    parent: &Path,
    format: HashFormat,
    kind: &str,
    bytes: &[u8],
) -> String {
    let mut input = format!("{kind} {}\0", bytes.len()).into_bytes();
    input.extend_from_slice(bytes);
    digest_bytes(
        env,
        parent,
        match format {
            HashFormat::Sha1 => "1",
            HashFormat::Sha256 => "256",
        },
        &input,
    )
}

fn independent_base_ids(
    env: &Hermetic,
    parent: &Path,
    format: HashFormat,
    blobs: &[String],
) -> (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
) {
    let tree = |one: &str| {
        let nested = {
            let mut bytes = b"100644 two.txt\0".to_vec();
            bytes.extend(hex_bytes(&blobs[2]));
            independent_object_id(env, parent, format, "tree", &bytes)
        };
        let mut bytes = b"40000 nested\0".to_vec();
        bytes.extend(hex_bytes(&nested));
        bytes.extend(b"100644 one.txt\0");
        bytes.extend(hex_bytes(one));
        let dir = independent_object_id(env, parent, format, "tree", &bytes);
        let mut bytes = b"40000 dir\0".to_vec();
        bytes.extend(hex_bytes(&dir));
        bytes.extend(b"100644 patterns\0");
        bytes.extend(hex_bytes(&blobs[3]));
        bytes.extend(b"100644 root.txt\0");
        bytes.extend(hex_bytes(&blobs[0]));
        let root = independent_object_id(env, parent, format, "tree", &bytes);
        (root, dir, nested)
    };
    let initial_blob = independent_blob_id(env, parent, format, b"one-blob\n");
    let (tree1, dir1, nested) = tree(&initial_blob);
    let (tree2, dir2, _) = tree(&blobs[1]);
    let commit = |tree: &str, parent_commit: Option<&str>, message: &str| {
        let bytes = format!(
            "tree {tree}\n{}author Partial Filter Oracle <partial-filter-oracle@example.invalid> 1700000000 +0000\ncommitter Partial Filter Oracle <partial-filter-oracle@example.invalid> 1700000000 +0000\n\n{message}\n",
            parent_commit.map_or_else(String::new, |parent| format!("parent {parent}\n")),
        )
        .into_bytes();
        independent_object_id(env, parent, format, "commit", &bytes)
    };
    let commit1 = commit(&tree1, None, "partial-filter-one");
    let commit2 = commit(&tree2, Some(&commit1), "partial-filter-two");
    (
        commit1,
        commit2,
        tree1,
        tree2,
        nested,
        dir1,
        dir2,
        initial_blob,
    )
}

fn independent_submodule_objects(
    env: &Hermetic,
    parent: &Path,
    format: HashFormat,
    blobs: &[String],
    sub_url: &str,
) -> (BTreeSet<String>, BTreeSet<String>, String, String, String) {
    let (commit1, commit2, tree1, tree2, base_nested, base_dir1, base_dir2, initial_blob) =
        independent_base_ids(env, parent, format, blobs);
    let sub_blob = independent_blob_id(env, parent, HashFormat::Sha1, b"submodule-blob\n");
    let mut sub_tree_bytes = b"100644 sub.txt\0".to_vec();
    sub_tree_bytes.extend(hex_bytes(&sub_blob));
    let sub_tree = independent_object_id(env, parent, HashFormat::Sha1, "tree", &sub_tree_bytes);
    let sub_commit_bytes = format!(
        "tree {sub_tree}\nauthor Partial Filter Oracle <partial-filter-oracle@example.invalid> 1700000000 +0000\ncommitter Partial Filter Oracle <partial-filter-oracle@example.invalid> 1700000000 +0000\n\nsubmodule\n"
    )
    .into_bytes();
    let sub_commit =
        independent_object_id(env, parent, HashFormat::Sha1, "commit", &sub_commit_bytes);
    let gitmodules =
        format!("[submodule \"modules/sub\"]\n\tpath = modules/sub\n\turl = {sub_url}\n");
    let gitmodules_blob = independent_blob_id(env, parent, format, gitmodules.as_bytes());
    let mut nested_bytes = b"100644 two.txt\0".to_vec();
    nested_bytes.extend(hex_bytes(&blobs[2]));
    let nested = independent_object_id(env, parent, format, "tree", &nested_bytes);
    let mut dir_bytes = b"40000 nested\0".to_vec();
    dir_bytes.extend(hex_bytes(&nested));
    dir_bytes.extend(b"100644 one.txt\0");
    dir_bytes.extend(hex_bytes(&blobs[1]));
    let dir = independent_object_id(env, parent, format, "tree", &dir_bytes);
    let mut modules_tree_bytes = b"160000 sub\0".to_vec();
    modules_tree_bytes.extend(hex_bytes(&sub_commit));
    let modules_tree = independent_object_id(env, parent, format, "tree", &modules_tree_bytes);
    let mut final_tree_bytes = b"100644 .gitmodules\0".to_vec();
    final_tree_bytes.extend(hex_bytes(&gitmodules_blob));
    final_tree_bytes.extend(b"40000 dir\0");
    final_tree_bytes.extend(hex_bytes(&dir));
    final_tree_bytes.extend(b"40000 modules\0");
    final_tree_bytes.extend(hex_bytes(&modules_tree));
    final_tree_bytes.extend(b"100644 patterns\0");
    final_tree_bytes.extend(hex_bytes(&blobs[3]));
    final_tree_bytes.extend(b"100644 root.txt\0");
    final_tree_bytes.extend(hex_bytes(&blobs[0]));
    let final_tree = independent_object_id(env, parent, format, "tree", &final_tree_bytes);
    let final_commit_bytes = format!(
        "tree {final_tree}\nparent {commit2}\nauthor Partial Filter Oracle <partial-filter-oracle@example.invalid> 1700000000 +0000\ncommitter Partial Filter Oracle <partial-filter-oracle@example.invalid> 1700000000 +0000\n\nadd-submodule\n"
    )
    .into_bytes();
    let final_commit = independent_object_id(env, parent, format, "commit", &final_commit_bytes);
    let all: BTreeSet<String> = [
        commit1,
        commit2,
        tree1,
        tree2,
        final_tree,
        final_commit,
        sub_blob.clone(),
        sub_tree.clone(),
        sub_commit.clone(),
        modules_tree,
        gitmodules_blob,
    ]
    .into_iter()
    .chain([base_nested, base_dir1, base_dir2, initial_blob])
    .chain(blobs.iter().cloned())
    .collect();
    let mut source = all.clone();
    source.remove(&sub_blob);
    source.remove(&sub_tree);
    source.remove(&sub_commit);
    (all, source, sub_blob, sub_tree, sub_commit)
}

fn make_fixture(
    parent: &Path,
    format: HashFormat,
    refs: RefFormat,
    with_submodule: bool,
) -> Fixture {
    let env = Hermetic::new(parent);
    let root = parent.join(format!(
        "fixture-{}-{}",
        if format == HashFormat::Sha1 {
            "sha1"
        } else {
            "sha256"
        },
        refs.label()
    ));
    let repo = root.join("remote.git");
    fs::create_dir_all(&root).expect("fixture root");
    init_repo(&env, &repo, format, refs);
    fs::create_dir_all(repo.join("dir/nested")).expect("fixture nested tree");
    fs::write(repo.join("root.txt"), b"root-blob\n").expect("fixture root blob");
    fs::write(repo.join("dir/one.txt"), b"one-blob\n").expect("fixture first blob");
    fs::write(
        repo.join("dir/nested/two.txt"),
        b"two-blob-with-enough-bytes\n",
    )
    .expect("fixture second blob");
    fs::write(repo.join("patterns"), b"/*\n!/*/\n/dir/\n").expect("fixture sparse patterns");
    git_ok(&env, &repo, &["add", "-A"]);
    git_ok(
        &env,
        &repo,
        &["commit", "--quiet", "-m", "partial-filter-one"],
    );
    fs::write(repo.join("dir/one.txt"), b"one-blob-second-commit\n").expect("fixture changed blob");
    git_ok(
        &env,
        &repo,
        &["commit", "--quiet", "-am", "partial-filter-two"],
    );
    let submodule_info = if with_submodule {
        let sub = root.join("submodule.git");
        init_repo(&env, &sub, HashFormat::Sha1, RefFormat::Files);
        fs::write(sub.join("sub.txt"), b"submodule-blob\n").expect("submodule blob");
        git_ok(&env, &sub, &["add", "sub.txt"]);
        git_ok(&env, &sub, &["commit", "--quiet", "-m", "submodule"]);
        let sub_head = text(&run_git(&env, &sub, &["rev-parse", "HEAD"]));
        let sub_url = format!("file://{}", sub.display());
        fs::write(
            repo.join(".gitmodules"),
            format!("[submodule \"modules/sub\"]\n\tpath = modules/sub\n\turl = {sub_url}\n"),
        )
        .expect("write fixture .gitmodules");
        git_ok(&env, &repo, &["add", ".gitmodules"]);
        git_ok(
            &env,
            &repo,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{sub_head},modules/sub"),
            ],
        );
        git_ok(&env, &repo, &["commit", "--quiet", "-m", "add-submodule"]);
        Some((sub, sub_url))
    } else {
        None
    };
    let head = text(&run_git(&env, &repo, &["rev-parse", "HEAD"]));
    let tree = text(&run_git(&env, &repo, &["rev-parse", "HEAD^{tree}"]));
    let previous_blob_ids = [
        independent_blob_id(&env, &root, format, b"root-blob\n"),
        independent_blob_id(&env, &root, format, b"one-blob-second-commit\n"),
        independent_blob_id(&env, &root, format, b"two-blob-with-enough-bytes\n"),
        independent_blob_id(&env, &root, format, b"/*\n!/*/\n/dir/\n"),
    ];
    let (previous_commit, _, previous_tree, _, _, previous_dir, _, _) =
        independent_base_ids(&env, &root, format, &previous_blob_ids);
    let mut blobs = Vec::new();
    for path in ["root.txt", "dir/one.txt", "dir/nested/two.txt", "patterns"] {
        blobs.push((
            path.to_owned(),
            text(&run_git(
                &env,
                &repo,
                &["rev-parse", &format!("HEAD:{path}")],
            )),
        ));
    }
    for ((path, oid), bytes) in blobs.iter().zip([
        b"root-blob\n".as_slice(),
        b"one-blob-second-commit\n".as_slice(),
        b"two-blob-with-enough-bytes\n".as_slice(),
        b"/*\n!/*/\n/dir/\n".as_slice(),
    ]) {
        assert_eq!(
            oid,
            &independent_blob_id(&env, &root, format, bytes),
            "fixture blob id differs from independent content hash"
        );
        assert!(
            path == "root.txt"
                || path == "dir/one.txt"
                || path == "dir/nested/two.txt"
                || path == "patterns",
            "unexpected fixture blob path"
        );
    }
    let mut blob_objects = blobs
        .iter()
        .map(|(_, oid)| oid.clone())
        .collect::<BTreeSet<_>>();
    blob_objects.insert(independent_blob_id(&env, &root, format, b"one-blob\n"));
    let submodule_gitmodules_blob = submodule_info.as_ref().map(|(_, sub_url)| {
        let gitmodules =
            format!("[submodule \"modules/sub\"]\n\tpath = modules/sub\n\turl = {sub_url}\n");
        independent_blob_id(&env, &root, format, gitmodules.as_bytes())
    });
    let (expected_objects, source_expected_objects, submodule_head, submodule_tree, submodule_blob) =
        if !with_submodule {
            let blob_ids = blobs.iter().map(|(_, oid)| oid.clone()).collect::<Vec<_>>();
            let (commit1, commit2, tree1, tree2, nested, dir1, dir2, initial_blob) =
                independent_base_ids(&env, &root, format, &blob_ids);
            assert_eq!(
                text(&run_git(&env, &repo, &["rev-parse", "HEAD~1"])),
                commit1,
                "independent first commit id"
            );
            assert_eq!(head, commit2, "independent HEAD id");
            assert_eq!(
                text(&run_git(&env, &repo, &["rev-parse", "HEAD~1^{tree}"])),
                tree1,
                "independent first tree id"
            );
            assert_eq!(tree, tree2, "independent HEAD tree id");
            let objects = [
                commit1,
                commit2,
                tree1,
                tree2,
                nested,
                dir1,
                dir2,
                initial_blob,
            ]
            .into_iter()
            .chain(blob_ids)
            .collect::<BTreeSet<_>>();
            (objects.clone(), objects, None, None, None)
        } else {
            let (_, sub_url) = submodule_info.as_ref().expect("submodule fixture metadata");
            let blob_ids = blobs.iter().map(|(_, oid)| oid.clone()).collect::<Vec<_>>();
            let (expected, source, sub_blob, sub_tree, sub_commit) =
                independent_submodule_objects(&env, &root, format, &blob_ids, sub_url);
            let gitmodules =
                format!("[submodule \"modules/sub\"]\n\tpath = modules/sub\n\turl = {sub_url}\n");
            blob_objects.insert(independent_blob_id(
                &env,
                &root,
                format,
                gitmodules.as_bytes(),
            ));
            assert!(
                expected.contains(&head),
                "independent submodule HEAD id differs"
            );
            assert!(
                expected.contains(&tree),
                "independent submodule tree id differs"
            );
            (
                expected,
                source,
                Some(sub_commit.clone()),
                Some(sub_tree.clone()),
                Some(sub_blob.clone()),
            )
        };
    let metadata_objects = source_expected_objects
        .difference(&blob_objects)
        .cloned()
        .collect();
    let submodule_expected_objects = submodule_head
        .as_ref()
        .zip(submodule_tree.as_ref())
        .zip(submodule_blob.as_ref())
        .map(|((head, tree), blob)| {
            [head.clone(), tree.clone(), blob.clone()]
                .into_iter()
                .collect::<BTreeSet<_>>()
        });
    Fixture {
        root,
        repo,
        head,
        tree,
        previous_tree,
        previous_dir,
        previous_commit,
        blobs,
        expected_objects: Some(expected_objects),
        source_expected_objects: Some(source_expected_objects),
        blob_objects,
        metadata_objects,
        submodule_head,
        submodule_tree,
        submodule_blob,
        submodule_expected_objects,
        submodule_gitmodules_blob,
        refs,
    }
}

fn configure_remote(env: &Hermetic, repo: &Path, key: &str, value: &str) {
    git_ok(env, repo, &["config", key, value]);
}

fn clone_args<'a>(
    url: &'a str,
    destination: &'a Path,
    format: RefFormat,
    filters: &[&'a str],
    no_filter_last: bool,
) -> Vec<String> {
    let mut args = vec![
        "clone".to_owned(),
        "--quiet".to_owned(),
        "--no-checkout".to_owned(),
    ];
    if let Some(arg) = format.init_arg() {
        args.push(arg.to_owned());
    }
    for filter in filters {
        args.push(format!("--filter={filter}"));
    }
    if no_filter_last {
        args.push("--no-filter".to_owned());
    }
    args.push(url.to_owned());
    args.push(destination.display().to_string());
    args
}

fn run_clone(
    env: &Hermetic,
    kind: Runner,
    cwd: &Path,
    args: &[String],
    ssh: Option<&Path>,
) -> Output {
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    match kind {
        Runner::Stock => run_git(env, cwd, &refs),
        Runner::Zmin => {
            let mut extras = Vec::new();
            if let Some(ssh) = ssh {
                extras.push(("GIT_SSH_COMMAND", ssh));
            }
            run_zmin(env, cwd, &refs, &extras)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Runner {
    Stock,
    Zmin,
}

fn active_runners<'a>(stock: &'a Path, zmin: &'a Path) -> Vec<(Runner, &'a Path)> {
    match oracle_mode() {
        OracleMode::Differential => vec![(Runner::Stock, stock), (Runner::Zmin, zmin)],
        OracleMode::StockOnly => vec![(Runner::Stock, stock)],
        OracleMode::ZminOnly => vec![(Runner::Zmin, zmin)],
    }
}

fn repo_git_runner(env: &Hermetic, runner: Runner, repo: &Path, args: &[&str]) -> String {
    let output = match runner {
        Runner::Stock => run_git(env, repo, args),
        Runner::Zmin => run_zmin(env, repo, args, &[]),
    };
    assert!(
        output.status.success(),
        "repository inspection {:?} failed with status {:?}; stderr {}",
        runner,
        output.status.code(),
        redacted_stderr(&output.stderr)
    );
    text(&output)
}

fn config_runner(env: &Hermetic, runner: Runner, repo: &Path, key: &str) -> Output {
    match runner {
        Runner::Stock => run_git(env, repo, &["config", "--get", key]),
        Runner::Zmin => run_zmin(env, repo, &["config", "--get", key], &[]),
    }
}

fn has_object_runner(env: &Hermetic, runner: Runner, repo: &Path, oid: &str) -> bool {
    let mut command = match runner {
        Runner::Stock => Command::new(STOCK_GIT),
        Runner::Zmin => {
            let (path, _) = VALIDATED_ZMIN
                .get()
                .expect("Zmin must be validated before object inspection");
            Command::new(path)
        }
    };
    env.apply(&mut command);
    command
        .current_dir(repo)
        .env("GIT_NO_LAZY_FETCH", "1")
        .args(["cat-file", "-e", oid]);
    run_bounded(command, "object presence").status.success()
}

fn promisor_files(repo: &Path) -> Vec<String> {
    let git_dir = if repo.join(".git").is_dir() {
        repo.join(".git")
    } else {
        repo.to_owned()
    };
    let pack_dir = git_dir.join("objects/pack");
    let mut names = fs::read_dir(pack_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|ext| ext.to_str()) == Some("promisor"))
                .then(|| entry.file_name().to_string_lossy().to_string())
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn object_files(repo: &Path) -> BTreeSet<String> {
    let git_dir = if repo.join(".git").is_dir() {
        repo.join(".git")
    } else {
        repo.to_owned()
    };
    let objects = git_dir.join("objects");
    let mut result = BTreeSet::new();
    let mut stack = vec![objects.clone()];
    while let Some(path) = stack.pop() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let child = entry.path();
                if child.is_dir() {
                    stack.push(child);
                } else if child.is_file() {
                    let relative = child
                        .strip_prefix(&objects)
                        .expect("object prefix")
                        .to_string_lossy()
                        .replace('\\', "/");
                    if !relative.starts_with("pack/") && relative != "info" {
                        result.insert(relative);
                    }
                }
            }
        }
    }
    result
}

fn all_object_ids_runner(env: &Hermetic, runner: Runner, repo: &Path) -> BTreeSet<String> {
    let output = match runner {
        Runner::Stock => run_git(
            env,
            repo,
            &[
                "cat-file",
                "--batch-all-objects",
                "--batch-check=%(objectname)",
            ],
        ),
        Runner::Zmin => run_zmin(
            env,
            repo,
            &[
                "cat-file",
                "--batch-all-objects",
                "--batch-check=%(objectname)",
            ],
            &[],
        ),
    };
    assert!(
        output.status.success(),
        "object enumeration failed with status {:?}; stderr {}",
        output.status.code(),
        redacted_stderr(&output.stderr)
    );
    text(&output)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn exact_expected_clone_objects(
    fixture: &Fixture,
    expected_filter: Option<&str>,
    repo: &Path,
) -> Option<Vec<BTreeSet<String>>> {
    let source = fixture.source_expected_objects.as_ref()?.clone();
    if repo
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("local-"))
    {
        return Some(vec![source.clone()]);
    }
    let http_filtered = repo
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with("http-")
                || name.starts_with("family-")
                || name.starts_with("combine-")
                || name.starts_with("uri-")
                || name.starts_with("stress-")
        });
    let blob_excluding = || {
        let mut expected = source
            .difference(&fixture.blob_objects)
            .cloned()
            .collect::<BTreeSet<_>>();
        if let Some(gitmodules_blob) = &fixture.submodule_gitmodules_blob {
            expected.insert(gitmodules_blob.clone());
            // The pinned recursive file transport retains the complete
            // superproject blob set while propagating the filter to the submodule.
            expected.extend([
                fixture.blobs[0].1.clone(),
                fixture.blobs[1].1.clone(),
                fixture.blobs[2].1.clone(),
                fixture.blobs[3].1.clone(),
            ]);
        }
        if http_filtered {
            // The pinned HTTP upload-pack has two exact tree-pack forms: a
            // reused pack retains the previous root/directory trees, while a
            // freshly filtered pack omits those independently known IDs.
            let mut alternate = expected.clone();
            alternate.remove(&fixture.previous_tree);
            alternate.remove(&fixture.previous_dir);
            vec![expected, alternate]
        } else {
            vec![expected]
        }
    };
    match expected_filter {
        None => Some(vec![source.clone()]),
        Some("blob:none") | Some("blob:limit=1") => Some(blob_excluding()),
        Some("tree:0") => {
            let mut expected = source
                .difference(&fixture.blob_objects)
                .cloned()
                .collect::<BTreeSet<_>>();
            expected.remove(&fixture.previous_tree);
            expected.remove(&fixture.previous_dir);
            Some(vec![expected])
        }
        Some("object:type=blob") => {
            let mut expected = source;
            expected.remove(&fixture.previous_commit);
            expected.remove(&fixture.previous_tree);
            expected.remove(&fixture.previous_dir);
            Some(vec![expected])
        }
        Some(filter) if filter.starts_with("sparse:oid=") => Some(vec![source.clone()]),
        Some("auto") => Some(vec![source.clone()]),
        Some(filter) if filter.starts_with("combine:blob:none+") => Some(blob_excluding()),
        Some(_) => None,
    }
}

fn assert_clone_object_contract(
    env: &Hermetic,
    fixture: &Fixture,
    runner: Runner,
    repo: &Path,
    expected_filter: Option<&str>,
) {
    assert!(fixture.metadata_objects.contains(&fixture.head));
    assert!(fixture.metadata_objects.contains(&fixture.tree));
    assert!(has_object_runner(env, runner, repo, &fixture.head));
    assert!(has_object_runner(env, runner, repo, &fixture.tree));
    if let Some(expected_sets) = exact_expected_clone_objects(fixture, expected_filter, repo) {
        let local_objects = all_object_ids_runner(env, runner, repo);
        assert!(
            expected_sets
                .iter()
                .any(|expected| expected == &local_objects),
            "complete clone object set differs from independent fixture contract"
        );
        assert_eq!(
            all_object_ids_runner(env, runner, repo),
            local_objects,
            "clone object enumeration changed during contract inspection"
        );
    }
}

fn assert_submodule_content_contract(
    env: &Hermetic,
    fixture: &Fixture,
    runner: Runner,
    repo: &Path,
) {
    let subrepo = repo.join("modules/sub");
    let sub_head = fixture.submodule_head.as_ref().expect("submodule head");
    let sub_tree = fixture.submodule_tree.as_ref().expect("submodule tree");
    let sub_blob = fixture.submodule_blob.as_ref().expect("submodule blob");
    let expected_submodule_objects = fixture
        .submodule_expected_objects
        .as_ref()
        .expect("submodule expected object set");
    assert_eq!(
        repo_git_runner(env, runner, &subrepo, &["rev-parse", "HEAD"]),
        *sub_head,
        "submodule HEAD differs from independent gitlink"
    );
    assert_eq!(
        repo_git_runner(env, runner, &subrepo, &["rev-parse", "HEAD^{tree}"]),
        *sub_tree,
        "submodule tree differs from independent gitlink"
    );
    assert!(has_object_runner(env, runner, &subrepo, sub_blob));
    for object_id in expected_submodule_objects {
        assert!(
            has_object_runner(env, runner, &subrepo, object_id),
            "independent submodule object is missing"
        );
    }
    let actual_submodule_objects = all_object_ids_runner(env, runner, &subrepo);
    assert_eq!(
        actual_submodule_objects, *expected_submodule_objects,
        "complete submodule object set differs from independent fixture contract"
    );
    assert_eq!(
        all_object_ids_runner(env, runner, &subrepo),
        actual_submodule_objects,
        "submodule object enumeration changed during contract inspection"
    );
    let gitlink = repo_git_runner(env, runner, repo, &["ls-tree", "HEAD", "modules/sub"]);
    assert!(
        gitlink.contains(sub_head),
        "superproject gitlink does not contain independent submodule commit"
    );
    let gitmodules = repo_git_runner(env, runner, repo, &["cat-file", "blob", "HEAD:.gitmodules"]);
    assert!(
        gitmodules.contains("path = modules/sub") && gitmodules.contains("url = file://"),
        "superproject .gitmodules content changed"
    );
}

fn assert_single_repo_contract(
    env: &Hermetic,
    fixture: &Fixture,
    runner: Runner,
    repo: &Path,
    expected_filter: Option<&str>,
) {
    assert_eq!(
        repo_git_runner(env, runner, repo, &["rev-parse", "HEAD"]),
        fixture.head,
        "single-runner HEAD"
    );
    assert_eq!(
        repo_git_runner(env, runner, repo, &["rev-parse", "HEAD^{tree}"]),
        fixture.tree,
        "single-runner tree"
    );
    assert!(
        !repo_git_runner(env, runner, repo, &["show-ref"]).is_empty(),
        "single-runner refs"
    );
    assert_clone_object_contract(env, fixture, runner, repo, expected_filter);
    if let Some(expected_objects) = &fixture.expected_objects {
        assert!(expected_objects.contains(&fixture.head));
        assert!(expected_objects.contains(&fixture.tree));
    }
    if let Some(expected_objects) = &fixture.source_expected_objects {
        for oid in expected_objects {
            assert!(
                has_object_runner(env, runner, &fixture.repo, oid),
                "independent fixture object missing from source"
            );
        }
    }
    let configured = check_config_runner(env, runner, repo, "remote.origin.partialclonefilter");
    match expected_filter {
        Some(filter) => {
            assert_eq!(configured.as_deref(), Some(filter), "persisted filter");
            assert_eq!(
                check_config_runner(env, runner, repo, "remote.origin.promisor").as_deref(),
                Some("true"),
                "promisor config"
            );
            assert!(
                promisor_files(repo).len() <= 8,
                "filtered clone emitted too many .promisor pack markers"
            );
        }
        None => {
            assert!(configured.is_none(), "--no-filter persisted a filter");
            assert!(
                check_config_runner(env, runner, repo, "remote.origin.promisor").is_none(),
                "--no-filter persisted promisor config"
            );
            assert!(
                promisor_files(repo).is_empty(),
                "--no-filter marked a pack promisor"
            );
        }
    }
}

fn assert_mode_contract(
    env: &Hermetic,
    fixture: &Fixture,
    stock: &Path,
    zmin: &Path,
    expected_filter: Option<&str>,
) {
    match oracle_mode() {
        OracleMode::Differential => {
            assert_repo_contract(env, fixture, stock, zmin, expected_filter)
        }
        OracleMode::StockOnly => {
            assert_single_repo_contract(env, fixture, Runner::Stock, stock, expected_filter)
        }
        OracleMode::ZminOnly => {
            assert_single_repo_contract(env, fixture, Runner::Zmin, zmin, expected_filter)
        }
    }
}

fn assert_repo_contract(
    env: &Hermetic,
    fixture: &Fixture,
    stock: &Path,
    zmin: &Path,
    expected_filter: Option<&str>,
) {
    for (runner, repo) in [(Runner::Stock, stock), (Runner::Zmin, zmin)] {
        assert_eq!(
            repo_git_runner(env, runner, repo, &["rev-parse", "HEAD"]),
            fixture.head,
            "runner HEAD"
        );
        assert_eq!(
            repo_git_runner(env, runner, repo, &["rev-parse", "HEAD^{tree}"]),
            fixture.tree,
            "runner tree"
        );
        assert!(!repo_git_runner(env, runner, repo, &["show-ref"]).is_empty());
        assert_clone_object_contract(env, fixture, runner, repo, expected_filter);
        if let Some(expected_objects) = &fixture.expected_objects {
            assert!(expected_objects.contains(&fixture.head));
            assert!(expected_objects.contains(&fixture.tree));
        }
        if let Some(expected_objects) = &fixture.source_expected_objects {
            for oid in expected_objects {
                assert!(
                    has_object_runner(env, runner, &fixture.repo, oid),
                    "source object missing under runner"
                );
            }
        }
    }
    assert_eq!(
        repo_git_runner(env, Runner::Stock, stock, &["rev-parse", "HEAD"]),
        fixture.head,
        "stock HEAD"
    );
    assert_eq!(
        repo_git_runner(env, Runner::Zmin, zmin, &["rev-parse", "HEAD"]),
        fixture.head,
        "Zmin HEAD"
    );
    assert_eq!(
        repo_git_runner(env, Runner::Stock, stock, &["rev-parse", "HEAD^{tree}"]),
        fixture.tree,
        "stock tree"
    );
    assert_eq!(
        repo_git_runner(env, Runner::Zmin, zmin, &["rev-parse", "HEAD^{tree}"]),
        fixture.tree,
        "Zmin tree"
    );
    assert_eq!(
        repo_git_runner(env, Runner::Stock, stock, &["show-ref"]),
        repo_git_runner(env, Runner::Zmin, zmin, &["show-ref"]),
        "refs"
    );
    if exact_expected_clone_objects(fixture, expected_filter, stock).is_some() {
        assert_eq!(
            all_object_ids_runner(env, Runner::Stock, stock),
            all_object_ids_runner(env, Runner::Zmin, zmin),
            "independent complete object sets differ"
        );
    }
    let stock_filter = run_git(
        env,
        stock,
        &["config", "--get", "remote.origin.partialclonefilter"],
    );
    let zmin_filter = config_runner(env, Runner::Zmin, zmin, "remote.origin.partialclonefilter");
    match expected_filter {
        Some(filter) => {
            assert_eq!(text(&stock_filter), filter, "stock persisted filter");
            assert_eq!(text(&zmin_filter), filter, "Zmin persisted filter");
            assert_eq!(
                text(&run_git(
                    env,
                    stock,
                    &["config", "--get", "remote.origin.promisor"]
                )),
                "true",
                "stock promisor config"
            );
            assert_eq!(
                text(&config_runner(
                    env,
                    Runner::Zmin,
                    zmin,
                    "remote.origin.promisor",
                )),
                "true",
                "Zmin promisor config"
            );
            let stock_promisors = promisor_files(stock);
            let zmin_promisors = promisor_files(zmin);
            assert_eq!(
                stock_promisors.len(),
                zmin_promisors.len(),
                "promisor marker count"
            );
            if stock_promisors.is_empty() {
                assert!(
                    zmin_promisors.is_empty(),
                    "local ignored filter became a Zmin promisor pack"
                );
            } else {
                assert!(
                    !zmin_promisors.is_empty(),
                    "filtered Zmin clone has no .promisor pack"
                );
            }
        }
        None => {
            assert!(
                !stock_filter.status.success(),
                "stock --no-filter persisted a filter"
            );
            assert!(
                !zmin_filter.status.success(),
                "Zmin --no-filter persisted a filter"
            );
            assert!(
                promisor_files(stock).is_empty(),
                "stock --no-filter marked a pack promisor"
            );
            assert!(
                promisor_files(zmin).is_empty(),
                "Zmin --no-filter marked a pack promisor"
            );
        }
    }
    assert_eq!(
        object_files(stock),
        object_files(zmin),
        "object storage shape"
    );
    for (_path, oid) in &fixture.blobs {
        assert_eq!(
            has_object_runner(env, Runner::Stock, stock, oid),
            has_object_runner(env, Runner::Zmin, zmin, oid),
            "blob presence {oid}"
        );
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RawPacket {
    Data(Vec<u8>),
    Flush,
    Delim,
    ResponseEnd,
}

fn find_packet_payloads(bytes: &[u8]) -> Vec<RawPacket> {
    parse_packet_payloads(bytes).expect("valid pkt-line request")
}

fn parse_packet_payloads(bytes: &[u8]) -> Result<Vec<RawPacket>, &'static str> {
    let mut offset = 0;
    let mut packets = Vec::new();
    while offset + 4 <= bytes.len() {
        let header = &bytes[offset..offset + 4];
        offset += 4;
        if header == b"0000" {
            packets.push(RawPacket::Flush);
        } else if header == b"0001" {
            packets.push(RawPacket::Delim);
        } else if header == b"0002" {
            packets.push(RawPacket::ResponseEnd);
        } else {
            let header_text = std::str::from_utf8(header).map_err(|_| "pkt header is not UTF-8")?;
            let length =
                usize::from_str_radix(header_text, 16).map_err(|_| "pkt length is invalid")?;
            if !(4..=MAX_HTTP_BODY).contains(&length) {
                return Err("pkt length is out of bounds");
            }
            if offset + length - 4 > bytes.len() {
                return Err("pkt payload is truncated");
            }
            packets.push(RawPacket::Data(bytes[offset..offset + length - 4].to_vec()));
            offset += length - 4;
        }
    }
    if offset != bytes.len() {
        return Err("trailing unparsed pkt bytes");
    }
    Ok(packets)
}

fn fetch_packets(records: &[RequestRecord]) -> Vec<Vec<RawPacket>> {
    records
        .iter()
        .filter(|record| {
            record.method == "POST"
                && (record.path.contains("git-upload-pack") || record.path == "ssh")
        })
        .flat_map(|record| {
            let packets = find_packet_payloads(&record.body);
            if record.path == "ssh" {
                let mut sections = Vec::new();
                let mut section = Vec::new();
                for packet in packets {
                    let flush = packet == RawPacket::Flush;
                    section.push(packet);
                    if flush {
                        sections.push(std::mem::take(&mut section));
                    }
                }
                if !section.is_empty() {
                    sections.push(section);
                }
                sections
            } else {
                vec![packets]
            }
        })
        .collect()
}

fn filter_fetch_packets(records: &[RequestRecord]) -> Vec<Vec<RawPacket>> {
    fetch_packets(records)
        .into_iter()
        .filter(|packets| packet_starts_with(packets, b"filter "))
        .collect()
}

fn packet_has_exact_filter(packets: &[RawPacket], filter: &str) -> bool {
    let expected = format!("filter {filter}").into_bytes();
    packet_count(packets, &expected) == 1
}

fn packet_data(packet: &RawPacket) -> Option<&[u8]> {
    match packet {
        RawPacket::Data(bytes) => Some(bytes),
        RawPacket::Flush | RawPacket::Delim | RawPacket::ResponseEnd => None,
    }
}

fn packet_count(packets: &[RawPacket], payload: &[u8]) -> usize {
    packets
        .iter()
        .filter(|packet| packet_data(packet) == Some(payload))
        .count()
}

fn packet_position(packets: &[RawPacket], payload: &[u8]) -> Option<usize> {
    packets
        .iter()
        .position(|packet| packet_data(packet) == Some(payload))
}

fn packet_position_prefix(packets: &[RawPacket], prefix: &[u8]) -> Option<usize> {
    packets
        .iter()
        .position(|packet| packet_data(packet).is_some_and(|payload| payload.starts_with(prefix)))
}

fn packet_starts_with(packets: &[RawPacket], prefix: &[u8]) -> bool {
    packets
        .iter()
        .any(|packet| packet_data(packet).is_some_and(|payload| payload.starts_with(prefix)))
}

fn semantic_pkt_line(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.is_empty() || payload.contains(&b'\r') {
        return None;
    }
    if payload.ends_with(b"\n") {
        if payload[..payload.len() - 1].contains(&b'\n') {
            return None;
        }
        Some(payload.to_vec())
    } else if !payload.contains(&b'\n') {
        let mut line = payload.to_vec();
        line.push(b'\n');
        Some(line)
    } else {
        None
    }
}

fn packet_line_count(packets: &[RawPacket], expected_line: &[u8]) -> usize {
    assert!(expected_line.ends_with(b"\n"));
    packets
        .iter()
        .filter(|packet| {
            packet_data(packet).and_then(semantic_pkt_line).as_deref() == Some(expected_line)
        })
        .count()
}

fn packet_line_position(packets: &[RawPacket], expected_line: &[u8]) -> Option<usize> {
    assert!(expected_line.ends_with(b"\n"));
    packets.iter().position(|packet| {
        packet_data(packet).and_then(semantic_pkt_line).as_deref() == Some(expected_line)
    })
}

fn assert_fetch_filter_packets(
    packets: &[RawPacket],
    filter: &str,
    head: &str,
    no_haves: bool,
    expected_v2_wants: usize,
) {
    let command_line = b"command=fetch\n";
    let v2 = packet_line_count(packets, command_line) != 0;
    let filter_line = format!("filter {filter}").into_bytes();
    assert_eq!(
        packet_count(packets, &filter_line),
        1,
        "filter packet count"
    );
    let filter_position = packet_position(packets, &filter_line).expect("exact filter packet");
    let want_line = format!("want {head}\n").into_bytes();
    let want_position = packet_position(packets, &want_line).expect("want packet");
    if v2 {
        assert_eq!(
            packet_count(packets, b"version 2\n"),
            0,
            "v2 version packet count"
        );
        assert_eq!(
            packet_line_count(packets, command_line),
            1,
            "v2 fetch command count"
        );
        let want_count = packet_count(packets, &want_line);
        assert_eq!(want_count, expected_v2_wants, "pinned Git v2 want count");
        match expected_v2_wants {
            1 => {
                assert_eq!(packets.len(), 11, "v2 single-want fetch packet count");
                assert_eq!(packets[8], RawPacket::Data(want_line.clone()));
                assert_eq!(packets[9], RawPacket::Data(b"done\n".to_vec()));
                assert_eq!(packets[10], RawPacket::Flush, "v2 request flush");
            }
            2 => {
                assert_eq!(packets.len(), 12, "v2 duplicate-want fetch packet count");
                assert_eq!(packets[8], RawPacket::Data(want_line.clone()));
                assert_eq!(packets[9], RawPacket::Data(want_line.clone()));
                assert_eq!(packets[10], RawPacket::Data(b"done\n".to_vec()));
                assert_eq!(packets[11], RawPacket::Flush, "v2 request flush");
            }
            _ => panic!("unsupported pinned Git v2 want contract"),
        }
        assert_eq!(
            packet_line_position(packets, command_line),
            Some(0),
            "v2 command semantic line position"
        );
        assert!(matches!(&packets[1], RawPacket::Data(payload) if payload.starts_with(b"agent=")));
        assert!(
            matches!(&packets[2], RawPacket::Data(payload) if payload.starts_with(b"object-format="))
        );
        assert_eq!(
            packets[3],
            RawPacket::Delim,
            "v2 capability section delimiter"
        );
        assert_eq!(packets[4], RawPacket::Data(b"thin-pack".to_vec()));
        assert_eq!(packets[5], RawPacket::Data(b"no-progress".to_vec()));
        assert_eq!(packets[6], RawPacket::Data(b"ofs-delta".to_vec()));
        assert_eq!(packets[7], RawPacket::Data(filter_line.clone()));
        for capability in [b"thin-pack".as_slice(), b"ofs-delta", b"no-progress"] {
            assert!(
                packet_count(packets, capability) != 0,
                "missing v2 fetch capability"
            );
            assert_eq!(
                packet_count(packets, capability),
                1,
                "duplicate v2 fetch capability"
            );
        }
        let command_position = packet_line_position(packets, command_line).expect("command packet");
        let delimiter_position = packets
            .iter()
            .position(|packet| *packet == RawPacket::Delim)
            .expect("v2 delimiter");
        let agent_position = packet_position_prefix(packets, b"agent=").expect("agent capability");
        let object_format_position =
            packet_position_prefix(packets, b"object-format=").expect("object format capability");
        let thin_position = packet_position(packets, b"thin-pack").expect("thin-pack capability");
        let no_progress_position =
            packet_position(packets, b"no-progress").expect("no-progress capability");
        let ofs_delta_position =
            packet_position(packets, b"ofs-delta").expect("ofs-delta capability");
        let done_position = packet_position(packets, b"done\n").expect("done packet");
        assert_eq!(command_position, 0);
        assert_eq!(agent_position, 1);
        assert_eq!(object_format_position, 2);
        assert_eq!(delimiter_position, 3);
        assert_eq!(thin_position, 4);
        assert_eq!(no_progress_position, 5);
        assert_eq!(ofs_delta_position, 6);
        assert_eq!(filter_position, 7);
        assert_eq!(want_position, 8);
        assert_eq!(done_position, 8 + expected_v2_wants);
    } else {
        assert_eq!(packets.len(), 4, "v0 fetch packet count");
        let first_want = format!(
            "want {head} multi_ack_detailed side-band-64k thin-pack no-progress ofs-delta deepen-since deepen-not agent=git/2.55.0-Darwin filter\n"
        )
        .into_bytes();
        assert_eq!(packets[0], RawPacket::Data(first_want.clone()));
        assert_eq!(packets[1], RawPacket::Data(want_line));
        assert_eq!(packets[2], RawPacket::Data(filter_line));
        assert_eq!(packets[3], RawPacket::Flush, "v0 request flush");
        assert_eq!(filter_position, 2);
        assert_eq!(want_position, 1);
        for capability in [
            b"multi_ack_detailed".as_slice(),
            b"side-band-64k",
            b"thin-pack",
            b"no-progress",
            b"ofs-delta",
            b"deepen-since",
            b"deepen-not",
            b"agent=git/2.55.0-Darwin",
            b"filter",
        ] {
            assert!(
                first_want
                    .windows(capability.len())
                    .any(|window| window == capability),
                "missing v0 capability"
            );
        }
    }
    if no_haves {
        assert!(
            !packet_starts_with(packets, b"have "),
            "--refetch sent a have packet"
        );
    }
}

fn assert_fetch_filter(
    records: &[RequestRecord],
    filter: &str,
    head: &str,
    no_haves: bool,
    expected_v2_wants: usize,
) {
    let packets = filter_fetch_packets(records)
        .into_iter()
        .filter(|packets| packet_has_exact_filter(packets, filter))
        .collect::<Vec<_>>();
    assert!(!packets.is_empty(), "no upload-pack request captured");
    assert_fetch_filter_packets(
        packets.last().expect("last fetch request"),
        filter,
        head,
        no_haves,
        expected_v2_wants,
    );
}

fn assert_first_fetch_filter(records: &[RequestRecord], filter: &str, head: &str) {
    let packets = filter_fetch_packets(records)
        .into_iter()
        .filter(|packets| packet_has_exact_filter(packets, filter))
        .collect::<Vec<_>>();
    assert!(!packets.is_empty(), "no upload-pack request captured");
    assert_fetch_filter_packets(&packets[0], filter, head, false, 2);
}

fn assert_first_combined_filter(records: &[RequestRecord], expected: &str, head: &str) {
    let packets = filter_fetch_packets(records)
        .into_iter()
        .filter(|packets| packet_has_exact_filter(packets, expected))
        .collect::<Vec<_>>();
    assert!(!packets.is_empty(), "no upload-pack request captured");
    let packets = &packets[0];
    assert_fetch_filter_packets(packets, expected, head, false, 2);
    let v2 = packet_line_count(packets, b"command=fetch\n") != 0;
    let filter_line = format!("filter {expected}").into_bytes();
    assert_eq!(
        packet_count(packets, &filter_line),
        1,
        "combined filter packet count"
    );
    let want_line = format!("want {head}\n").into_bytes();
    assert_eq!(
        packet_count(packets, &want_line),
        if v2 { 2 } else { 1 },
        "combined want packet count"
    );
}

fn assert_no_fetch_haves(records: &[RequestRecord]) {
    let packets = filter_fetch_packets(records);
    assert!(!packets.is_empty(), "no fetch packets captured");
    for request in packets {
        assert!(
            !packet_starts_with(&request, b"have "),
            "fetch request carried a have packet"
        );
    }
}

fn assert_sideband_response(records: &[RequestRecord]) {
    let responses = records
        .iter()
        .filter(|record| {
            record.method == "POST"
                && (record.path.contains("git-upload-pack") || record.path == "ssh")
                && !record.response.is_empty()
        })
        .map(|record| {
            let packets = parse_packet_payloads(&record.response).expect("valid sideband response");
            if record.path == "ssh" {
                let mut sections = Vec::new();
                let mut section = Vec::new();
                for packet in packets {
                    let flush = packet == RawPacket::Flush;
                    section.push(packet);
                    if flush {
                        sections.push(std::mem::take(&mut section));
                    }
                }
                sections
                    .into_iter()
                    .rev()
                    .find(|section| section.iter().any(|packet| packet_data(packet).is_some()))
                    .expect("SSH sideband response section")
            } else {
                packets
            }
        })
        .collect::<Vec<_>>();
    assert!(!responses.is_empty(), "no upload-pack response captured");
    let response = responses.last().expect("last upload-pack response");
    assert!(matches!(
        response.first(),
        Some(RawPacket::Data(payload)) if payload == b"NAK\n" || payload == b"packfile\n"
    ));
    assert_eq!(response.last(), Some(&RawPacket::Flush));
    assert!(
        response[1..response.len() - 1].iter().all(
            |packet| matches!(packet, RawPacket::Data(payload) if payload.first() == Some(&1))
        ),
        "upload-pack response contains a non-sideband or non-pack packet"
    );
    assert!(
        response[1..response.len() - 1]
            .iter()
            .any(|packet| matches!(packet, RawPacket::Data(payload) if payload.len() > 1)),
        "upload-pack response has no sideband pack data"
    );
}

fn pkt_line(payload: &str) -> Vec<u8> {
    let length = 4 + payload.len();
    let mut packet = format!("{length:04x}").into_bytes();
    packet.extend_from_slice(payload.as_bytes());
    packet
}

fn upload_pack_probe(env: &Hermetic, runner: Runner, repo: &Path, input: &[u8]) -> Output {
    let mut command = match runner {
        Runner::Stock => Command::new(STOCK_GIT),
        Runner::Zmin => {
            let (path, _) = VALIDATED_ZMIN
                .get()
                .expect("Zmin must be validated before protocol probes");
            Command::new(path)
        }
    };
    env.apply(&mut command);
    command
        .current_dir(repo.parent().expect("upload-pack parent"))
        .env("GIT_PROTOCOL", "version=2")
        .args([
            "upload-pack",
            repo.to_str().expect("upload-pack path UTF-8"),
        ]);
    run_bounded_input(command, input, "upload-pack protocol probe")
}

struct HttpServer {
    port: u16,
    log: RequestLog,
    stop: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    uri_root: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UriMode {
    Serve,
    NotFound,
    Corrupt,
}

impl HttpServer {
    fn new(root: &Path, uri_root: &Path) -> Self {
        Self::with_uri_mode(root, uri_root, UriMode::Serve)
    }

    fn with_uri_mode(root: &Path, uri_root: &Path, uri_mode: UriMode) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind HTTP server");
        listener
            .set_nonblocking(true)
            .expect("nonblocking HTTP listener");
        let port = listener.local_addr().expect("HTTP address").port();
        let log = RequestLog::default();
        let stop = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_ready = Arc::clone(&ready);
        let thread_log = log.clone();
        let server_root = root.to_owned();
        let server_uri = uri_root.to_owned();
        let server_uri_mode = uri_mode;
        let thread = thread::spawn(move || {
            thread_ready.store(true, Ordering::Release);
            let mut served = 0_usize;
            while !thread_stop.load(Ordering::Acquire) && served < MAX_HTTP_REQUESTS {
                match listener.accept() {
                    Ok((stream, _)) => {
                        served += 1;
                        thread_log.accepted();
                        handle_http(
                            stream,
                            &server_root,
                            &server_uri,
                            server_uri_mode,
                            &thread_log,
                        );
                        thread_log.completed();
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => break,
                }
            }
        });
        let server = Self {
            port,
            log,
            stop,
            ready,
            thread: Some(thread),
            uri_root: uri_root.to_owned(),
        };
        let deadline = Instant::now() + THREAD_TIMEOUT;
        while !server.ready.load(Ordering::Acquire) {
            assert!(
                Instant::now() < deadline,
                "HTTP listener readiness timed out"
            );
            thread::sleep(Duration::from_millis(2));
        }
        server
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/remote.git", self.port)
    }

    fn wait_for_requests(&self, minimum_records: usize) -> Vec<RequestRecord> {
        self.log.wait_quiescent(minimum_records)
    }

    fn wait_for_connections(&self, minimum_connections: usize) -> Vec<RequestRecord> {
        let deadline = Instant::now() + THREAD_TIMEOUT;
        loop {
            if self.log.accepted.load(Ordering::Acquire) >= minimum_connections
                && self.log.completed.load(Ordering::Acquire)
                    == self.log.accepted.load(Ordering::Acquire)
            {
                return self.log.snapshot();
            }
            assert!(
                Instant::now() < deadline,
                "HTTP connection barrier timed out"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            thread.join().expect("HTTP server thread");
        }
    }
}

fn read_http_chunk(
    stream: &mut TcpStream,
    chunk: &mut [u8],
    deadline: Instant,
) -> io::Result<usize> {
    loop {
        match stream.read(chunk) {
            Ok(count) => return Ok(count),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error),
        }
    }
}

fn read_http_request(
    stream: &mut TcpStream,
) -> io::Result<(String, String, Vec<u8>, Option<String>)> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = read_http_chunk(stream, &mut chunk, deadline)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "empty HTTP request",
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > MAX_HTTP_HEADER {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP header bound exceeded",
            ));
        }
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let header_bytes = &bytes[..header_end - 4];
    let header = std::str::from_utf8(header_bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "HTTP headers are not UTF-8"))?;
    let mut lines = header.split("\r\n");
    let request_line = lines
        .next()
        .filter(|line| !line.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing HTTP request line"))?;
    let request_line = std::str::from_utf8(request_line.as_bytes()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "HTTP request line is not UTF-8")
    })?;
    let request_fields = request_line.split(' ').collect::<Vec<_>>();
    if request_fields.len() != 3
        || request_fields.iter().any(|field| field.is_empty())
        || request_fields[2] != "HTTP/1.1"
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid HTTP request-line grammar",
        ));
    }
    let method = request_fields[0].to_owned();
    let path = request_fields[1].to_owned();
    if !matches!(method.as_str(), "GET" | "POST")
        || !path.starts_with('/')
        || path.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported HTTP request target",
        ));
    }
    let mut content_length = 0_usize;
    let mut saw_content_length = false;
    let mut git_protocol = None;
    let mut saw_git_protocol = false;
    let mut seen_headers = BTreeSet::new();
    for line in lines {
        let line = line.as_bytes();
        if line.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "malformed HTTP header terminator",
            ));
        }
        let colon = line.iter().position(|byte| *byte == b':').ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "HTTP header has no colon")
        })?;
        let name = &line[..colon];
        if name.is_empty()
            || name.iter().any(|byte| {
                *byte <= 0x20
                    || *byte >= 0x7f
                    || matches!(
                        *byte,
                        b'(' | b')'
                            | b'<'
                            | b'>'
                            | b'@'
                            | b','
                            | b';'
                            | b'\\'
                            | b'"'
                            | b'['
                            | b']'
                            | b'?'
                            | b'='
                            | b'{'
                            | b'}'
                    )
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid HTTP header name",
            ));
        }
        let normalized_name = name
            .iter()
            .map(|byte| byte.to_ascii_lowercase())
            .collect::<Vec<_>>();
        if !seen_headers.insert(normalized_name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "duplicate HTTP header",
            ));
        }
        let value = &line[colon + 1..];
        if value
            .iter()
            .any(|byte| (*byte < 0x20 && *byte != b'\t') || *byte == 0x7f)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid HTTP header value",
            ));
        }
        let value = value
            .iter()
            .copied()
            .skip_while(u8::is_ascii_whitespace)
            .collect::<Vec<_>>();
        let value = value
            .iter()
            .copied()
            .rev()
            .skip_while(u8::is_ascii_whitespace)
            .collect::<Vec<_>>();
        let value = value.into_iter().rev().collect::<Vec<_>>();
        if name.eq_ignore_ascii_case(b"content-length") {
            if saw_content_length {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate Content-Length",
                ));
            }
            if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid Content-Length",
                ));
            }
            content_length = std::str::from_utf8(&value)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length"))?
                .parse::<usize>()
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length")
                })?;
            saw_content_length = true;
        } else if name.eq_ignore_ascii_case(b"git-protocol") {
            if saw_git_protocol {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate Git-Protocol",
                ));
            }
            git_protocol = Some(
                std::str::from_utf8(&value)
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "Git-Protocol is not UTF-8")
                    })?
                    .to_owned(),
            );
            saw_git_protocol = true;
        } else if name.eq_ignore_ascii_case(b"transfer-encoding") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported Transfer-Encoding",
            ));
        }
    }
    if method == "POST" && !saw_content_length {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "POST request has no Content-Length",
        ));
    }
    if content_length > MAX_HTTP_BODY {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP body bound exceeded",
        ));
    }
    if method == "GET" && content_length != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "GET request has a body",
        ));
    }
    while bytes.len() < header_end + content_length {
        let count = read_http_chunk(stream, &mut chunk, deadline)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated HTTP body",
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    if bytes.len() != header_end + content_length {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP request has trailing bytes",
        ));
    }
    Ok((
        method,
        path,
        bytes[header_end..header_end + content_length].to_vec(),
        git_protocol,
    ))
}

fn handle_http(
    mut stream: TcpStream,
    root: &Path,
    uri_root: &Path,
    uri_mode: UriMode,
    log: &RequestLog,
) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let Ok((method, path, body, git_protocol)) = read_http_request(&mut stream) else {
        return;
    };
    if method == "GET" && path.starts_with("/uri/") {
        let mut bytes = safe_uri_file(uri_root, &path)
            .and_then(read_bounded_file)
            .unwrap_or_default();
        if uri_mode == UriMode::NotFound {
            bytes.clear();
        } else if uri_mode == UriMode::Corrupt && !bytes.is_empty() {
            bytes[0] ^= 0xff;
        }
        let response = format!(
            "HTTP/1.1 {}\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
            if uri_mode == UriMode::NotFound || bytes.is_empty() {
                "404 Not Found"
            } else {
                "200 OK"
            },
            bytes.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(&bytes);
        let _ = stream.shutdown(Shutdown::Both);
        log.push(RequestRecord {
            method,
            path,
            body,
            response: bytes,
        });
        return;
    }
    let query = path.split_once('?').map_or("", |(_, query)| query);
    let path_info = path.split_once('?').map_or(path.as_str(), |(path, _)| path);
    let mut command = Command::new(Path::new(GIT_BUNDLE).join("git-http-backend"));
    let env = Hermetic::new(root);
    env.apply(&mut command);
    command
        .current_dir(root)
        .env("GIT_PROJECT_ROOT", root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_INFO", path_info)
        .env("QUERY_STRING", query)
        .env("REQUEST_METHOD", &method)
        .env("CONTENT_LENGTH", body.len().to_string())
        .env("CONTENT_TYPE", "application/x-git-upload-pack-request")
        .env("REMOTE_USER", "oracle")
        .stdin(Stdio::piped());
    if let Some(git_protocol) = git_protocol {
        command.env("HTTP_GIT_PROTOCOL", git_protocol);
    }
    let output = run_bounded_input(command, &body, "git-http-backend");
    assert!(
        output.status.success(),
        "git-http-backend failed; stderr {}",
        redacted_stderr(&output.stderr)
    );
    let response_body = backend_body(&output.stdout).to_vec();
    write_backend_http_response(&mut stream, &output.stdout);
    let _ = stream.shutdown(Shutdown::Both);
    log.push(RequestRecord {
        method,
        path,
        body,
        response: response_body,
    });
}

fn read_bounded_file(path: PathBuf) -> Option<Vec<u8>> {
    let length = fs::metadata(&path).ok()?.len();
    if length > MAX_HTTP_BODY as u64 {
        return None;
    }
    let mut file = fs::File::open(path).ok()?;
    let mut bytes = Vec::with_capacity(length as usize);
    file.read_to_end(&mut bytes).ok()?;
    (bytes.len() <= MAX_HTTP_BODY).then_some(bytes)
}

fn safe_uri_file(uri_root: &Path, request_path: &str) -> Option<PathBuf> {
    let relative = request_path.strip_prefix("/uri/")?;
    if relative.is_empty()
        || relative.contains('\\')
        || relative.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
    {
        return None;
    }
    let root = fs::canonicalize(uri_root).ok()?;
    let candidate = root.join(relative);
    let canonical = fs::canonicalize(candidate).ok()?;
    canonical.starts_with(&root).then_some(canonical)
}

fn direct_http_get(server: &HttpServer, path: &str) -> Vec<u8> {
    let mut stream =
        TcpStream::connect(("127.0.0.1", server.port)).expect("connect HTTP negative fixture");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("HTTP negative read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .expect("HTTP negative write timeout");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .expect("write HTTP negative request");
    bounded_read(stream).expect("read HTTP negative response")
}

fn direct_http_raw(server: &HttpServer, request: &[u8]) -> Vec<u8> {
    let mut stream =
        TcpStream::connect(("127.0.0.1", server.port)).expect("connect raw HTTP negative fixture");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("raw HTTP negative read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .expect("raw HTTP negative write timeout");
    stream
        .write_all(request)
        .expect("write raw HTTP negative request");
    bounded_read(stream).expect("read raw HTTP negative response")
}

fn assert_uri_pack_bytes(server: &HttpServer, uri_path: &str, pack_path: &Path) {
    let response = direct_http_get(server, uri_path);
    assert!(
        response.starts_with(b"HTTP/1.1 200 OK\r\n"),
        "packfile-URI response status contract"
    );
    let separator = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("packfile-URI response headers");
    let body = &response[separator + 4..];
    let expected = fs::read(pack_path).expect("packfile-URI fixture pack");
    assert_eq!(body, expected, "packfile-URI bytes changed");
}

fn backend_body(backend: &[u8]) -> &[u8] {
    backend
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| &backend[index + 4..])
        .or_else(|| {
            backend
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| &backend[index + 2..])
        })
        .expect("CGI response header terminator")
}

fn write_backend_http_response(stream: &mut TcpStream, backend: &[u8]) {
    let (header_end, separator_len) = backend
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4))
        .or_else(|| {
            backend
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| (index, 2))
        })
        .expect("CGI response header terminator");
    let headers = &backend[..header_end];
    let body = &backend[header_end + separator_len..];
    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n");
    let mut saw_length = false;
    let mut saw_connection = false;
    for raw_line in headers.split(|byte| *byte == b'\n') {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if !line.is_empty() && !line.starts_with(b"Status:") {
            if let Some(colon) = line.iter().position(|byte| *byte == b':') {
                let name = &line[..colon];
                let value = line[colon + 1..]
                    .iter()
                    .copied()
                    .skip_while(u8::is_ascii_whitespace)
                    .collect::<Vec<_>>();
                if name.eq_ignore_ascii_case(b"content-length") {
                    saw_length = true;
                }
                if name.eq_ignore_ascii_case(b"connection") {
                    saw_connection = true;
                }
                let _ = stream.write_all(name);
                let _ = stream.write_all(b": ");
                let _ = stream.write_all(&value);
                let _ = stream.write_all(b"\r\n");
            }
        }
    }
    if !saw_length {
        let _ = write!(stream, "Content-Length: {}\r\n", body.len());
    }
    if !saw_connection {
        let _ = stream.write_all(b"Connection: close\r\n");
    }
    let _ = stream.write_all(b"\r\n");
    let _ = stream.write_all(body);
}

struct Daemon {
    port: u16,
    _guard: ChildGuard,
}

impl Daemon {
    fn spawn(env: &Hermetic, root: &Path) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("reserve daemon port");
        let port = listener.local_addr().expect("daemon port").port();
        drop(listener);
        let mut command = Command::new(STOCK_DAEMON);
        env.apply(&mut command);
        let base_path = format!("--base-path={}", root.display());
        let port_arg = format!("--port={port}");
        command.args([
            "--reuseaddr",
            "--verbose",
            "--export-all",
            "--listen=127.0.0.1",
            &base_path,
            &port_arg,
        ]);
        command.stdout(Stdio::null()).stderr(Stdio::null());
        let mut guard = ChildGuard::spawn(&mut command, "git-daemon");
        let deadline = Instant::now() + PROCESS_TIMEOUT;
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "git-daemon did not listen");
            thread::sleep(Duration::from_millis(10));
        }
        guard.child().stdin.take();
        Self {
            port,
            _guard: guard,
        }
    }

    fn url(&self) -> String {
        format!("git://127.0.0.1:{}/remote.git", self.port)
    }
}

fn write_ssh_wrapper(root: &Path, stock_git: &Path, log: &Path) -> PathBuf {
    let path = root.join("oracle-ssh");
    let script = format!(
        "#!/bin/sh\nset -eu\nlast=\"\"\nfor arg in \"$@\"; do last=\"$arg\"; done\nprintf '%s\\n' \"$@\" > \"{}-args\"\nprintf '%s\\n' \"$last\" > \"{}-command\"\nrepo=\"$last\"\ncase \"$repo\" in git-upload-pack\\ *) repo=\"${{repo#git-upload-pack }}\" ;; esac\nrepo=\"${{repo#\\\'}}\"\nrepo=\"${{repo%\\\'}}\"\nexec /usr/bin/tee \"{}\" | \"{}\" upload-pack \"$repo\" | /usr/bin/tee \"{}-response\"\n",
        log.display(),
        log.display(),
        log.display(),
        stock_git.display(),
        log.display()
    );
    fs::write(&path, script).expect("write SSH wrapper");
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&path)
            .expect("SSH wrapper metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("chmod SSH wrapper");
    }
    path
}

fn setup_http_uri(env: &Hermetic, fixture: &Fixture, server: &HttpServer) -> (PathBuf, String) {
    let uri_root = server.uri_root.clone();
    fs::create_dir_all(&uri_root).expect("URI directory");
    let blob = &fixture.blobs[0].1;
    let mut pack_command = Command::new(STOCK_GIT);
    env.apply(&mut pack_command);
    pack_command
        .current_dir(&fixture.repo)
        .args(["pack-objects", "--stdout"]);
    pack_command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_bounded_input(pack_command, format!("{blob}\n").as_bytes(), "URI pack");
    assert!(
        output.status.success(),
        "URI pack failed; stderr {}",
        redacted_stderr(&output.stderr)
    );
    let temporary_pack = uri_root.join("pack-input.pack");
    fs::write(&temporary_pack, &output.stdout).expect("write URI pack input");
    let mut index = Command::new(STOCK_GIT);
    env.apply(&mut index);
    index.current_dir(&fixture.repo).args([
        "index-pack",
        temporary_pack.to_str().expect("URI pack path"),
    ]);
    let pack_hash = text(&run_bounded(index, "URI index-pack"));
    assert_eq!(
        pack_hash.len(),
        fixture.head.len(),
        "URI pack checksum width"
    );
    assert!(pack_hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let pack_path = uri_root.join(format!("pack-{pack_hash}.pack"));
    fs::rename(&temporary_pack, &pack_path).expect("name URI pack by checksum");
    let pack = fs::read(&pack_path).expect("read URI pack checksum");
    let trailer_width = fixture.head.len() / 2;
    assert!(pack.len() > trailer_width, "URI pack is truncated");
    let algorithm = if trailer_width == 20 { "1" } else { "256" };
    let independent_hash = digest_bytes(
        env,
        &uri_root,
        algorithm,
        &pack[..pack.len() - trailer_width],
    );
    assert_eq!(
        independent_hash,
        hex_lower(&pack[pack.len() - trailer_width..]),
        "URI pack trailer checksum"
    );
    assert_eq!(independent_hash, pack_hash, "URI pack path checksum");
    let uri = format!("http://127.0.0.1:{}/uri/pack-{pack_hash}.pack", server.port);
    assert_uri_pack_bytes(server, &format!("/uri/pack-{pack_hash}.pack"), &pack_path);
    configure_remote(env, &fixture.repo, "uploadpack.allowsidebandall", "true");
    configure_remote(
        env,
        &fixture.repo,
        "uploadpack.blobpackfileuri",
        &format!("{blob} {pack_hash} {uri}"),
    );
    (pack_path, uri)
}

fn assert_uri_get_records(records: &[RequestRecord], pack_path: &Path) -> usize {
    let expected_path = format!(
        "/uri/{}",
        pack_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("URI pack filename")
    );
    let expected_bytes = fs::read(pack_path).expect("URI pack bytes");
    let gets = records
        .iter()
        .filter(|record| record.method == "GET" && record.path.starts_with("/uri/"))
        .collect::<Vec<_>>();
    assert!(!gets.is_empty(), "packfile-URI GET request missing");
    for record in &gets {
        assert_eq!(record.path, expected_path, "packfile-URI path changed");
        assert!(record.body.is_empty(), "packfile-URI GET carried a body");
        assert_eq!(
            record.response, expected_bytes,
            "packfile-URI bytes changed"
        );
    }
    gets.len()
}

fn check_config_runner(env: &Hermetic, runner: Runner, repo: &Path, key: &str) -> Option<String> {
    let output = config_runner(env, runner, repo, key);
    output.status.success().then(|| text(&output))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Transport {
    Local,
    File,
    Ssh,
    Daemon,
    Http,
}

fn run_matrix_cell(parent: &Path, format: HashFormat, refs: RefFormat, transport: Transport) {
    let fixture = make_fixture(parent, format, refs, false);
    let env = Hermetic::new(parent);
    let source_url = format!("file://{}", fixture.repo.display());
    if transport == Transport::Local {
        let local_stock = fixture.root.join("local-stock");
        let local_zmin = fixture.root.join("local-zmin");
        for (kind, destination) in active_runners(&local_stock, &local_zmin) {
            let args = clone_args(
                fixture.repo.to_str().expect("local path UTF-8"),
                destination,
                refs,
                &["blob:none"],
                false,
            );
            let output = run_clone(&env, kind, &fixture.root, &args, None);
            require_success(output, "local clone");
            if kind == Runner::Stock {
                assert_single_repo_contract(
                    &env,
                    &fixture,
                    Runner::Stock,
                    &local_stock,
                    Some("blob:none"),
                );
            }
        }
        assert_mode_contract(&env, &fixture, &local_stock, &local_zmin, Some("blob:none"));
    }

    if transport == Transport::File {
        let file_stock = fixture.root.join("file-stock");
        let file_zmin = fixture.root.join("file-zmin");
        for (kind, destination) in active_runners(&file_stock, &file_zmin) {
            let args = clone_args(&source_url, destination, refs, &["blob:none"], false);
            let output = run_clone(&env, kind, &fixture.root, &args, None);
            require_success(output, "file clone");
            if kind == Runner::Stock {
                assert_single_repo_contract(
                    &env,
                    &fixture,
                    Runner::Stock,
                    &file_stock,
                    Some("blob:none"),
                );
            }
        }
        assert_mode_contract(&env, &fixture, &file_stock, &file_zmin, Some("blob:none"));
    }

    if transport == Transport::Ssh {
        let ssh_url = format!("ssh://oracle@127.0.0.1{}", fixture.repo.display());
        let ssh_stock = fixture.root.join("ssh-stock");
        let ssh_zmin = fixture.root.join("ssh-zmin");
        for (kind, destination) in active_runners(&ssh_stock, &ssh_zmin) {
            let runner_name = match kind {
                Runner::Stock => "stock",
                Runner::Zmin => "zmin",
            };
            let ssh_log = fixture.root.join(format!("ssh-wire-{runner_name}"));
            let ssh = write_ssh_wrapper(&fixture.root, Path::new(STOCK_GIT), &ssh_log);
            let args = clone_args(&ssh_url, destination, refs, &["blob:none"], false);
            let command_args = args;
            let output = if kind == Runner::Stock {
                let mut command = Command::new(STOCK_GIT);
                env.apply(&mut command);
                command
                    .current_dir(&fixture.root)
                    .env("GIT_SSH_COMMAND", &ssh)
                    .args(&command_args);
                run_bounded(command, "stock SSH clone")
            } else {
                run_clone(&env, kind, &fixture.root, &command_args, Some(&ssh))
            };
            require_success(output, "SSH clone");
            assert_single_repo_contract(&env, &fixture, kind, destination, Some("blob:none"));
            let ssh_wire = fs::read(&ssh_log).expect("SSH wire log");
            let ssh_response =
                fs::read(format!("{}-response", ssh_log.display())).expect("SSH response wire log");
            assert!(
                !ssh_wire.is_empty(),
                "SSH wrapper captured no protocol bytes"
            );
            assert_fetch_filter(
                &[RequestRecord {
                    method: "POST".into(),
                    path: "ssh".into(),
                    body: ssh_wire,
                    response: Vec::new(),
                }],
                "blob:none",
                &fixture.head,
                false,
                2,
            );
            assert_sideband_response(&[RequestRecord {
                method: "POST".into(),
                path: "ssh".into(),
                body: Vec::new(),
                response: ssh_response,
            }]);
        }
        assert_mode_contract(&env, &fixture, &ssh_stock, &ssh_zmin, Some("blob:none"));
    }

    if transport == Transport::Daemon {
        let daemon_stock = fixture.root.join("daemon-stock");
        let daemon_zmin = fixture.root.join("daemon-zmin");
        for (kind, destination) in active_runners(&daemon_stock, &daemon_zmin) {
            let daemon = Daemon::spawn(&env, &fixture.root);
            let daemon_url = daemon.url();
            let args = clone_args(&daemon_url, destination, refs, &["blob:none"], false);
            let output = run_clone(&env, kind, &fixture.root, &args, None);
            require_success(output, "git:// clone");
            assert_single_repo_contract(&env, &fixture, kind, destination, Some("blob:none"));
            drop(daemon);
        }
        assert_mode_contract(
            &env,
            &fixture,
            &daemon_stock,
            &daemon_zmin,
            Some("blob:none"),
        );
    }

    if transport == Transport::Http {
        let http_stock = fixture.root.join("http-stock");
        let http_zmin = fixture.root.join("http-zmin");
        let stock_server = (oracle_mode() != OracleMode::ZminOnly)
            .then(|| HttpServer::new(&fixture.root, &fixture.root.join("uri-stock")));
        let zmin_server = (oracle_mode() != OracleMode::StockOnly)
            .then(|| HttpServer::new(&fixture.root, &fixture.root.join("uri-zmin")));
        let mut clone_record_counts = [0_usize; 2];
        for (kind, destination) in active_runners(&http_stock, &http_zmin) {
            let server = match kind {
                Runner::Stock => stock_server.as_ref().expect("stock HTTP server"),
                Runner::Zmin => zmin_server.as_ref().expect("Zmin HTTP server"),
            };
            let args = clone_args(&server.url(), destination, refs, &["blob:none"], false);
            let output = if kind == Runner::Stock {
                let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
                let mut command = Command::new(STOCK_GIT);
                env.apply(&mut command);
                command.current_dir(&fixture.root).args(refs);
                run_bounded(command, "stock HTTP clone")
            } else {
                run_clone(&env, kind, &fixture.root, &args, None)
            };
            require_success(output, "HTTP clone");
            let records = server.wait_for_requests(1);
            assert_first_fetch_filter(&records, "blob:none", &fixture.head);
            assert_sideband_response(&records);
            assert_single_repo_contract(&env, &fixture, kind, destination, Some("blob:none"));
            clone_record_counts[kind as usize] = records.len();
        }
        assert_mode_contract(&env, &fixture, &http_stock, &http_zmin, Some("blob:none"));
        let stock_before_refetch = promisor_files(&http_stock);
        let zmin_before_refetch = promisor_files(&http_zmin);
        let refetch_args = [
            "fetch",
            "--quiet",
            "--refetch",
            "--filter=blob:none",
            "origin",
            "main",
        ];
        for (kind, repo) in active_runners(&http_stock, &http_zmin) {
            let server = match kind {
                Runner::Stock => stock_server.as_ref().expect("stock HTTP server"),
                Runner::Zmin => zmin_server.as_ref().expect("Zmin HTTP server"),
            };
            let output = match kind {
                Runner::Stock => run_git(&env, repo, &refetch_args),
                Runner::Zmin => run_zmin(&env, repo, &refetch_args, &[]),
            };
            require_success(output, "HTTP refetch");
            let records = server.wait_for_requests(clone_record_counts[kind as usize] + 1);
            let expected_refetch_wants = match kind {
                Runner::Stock => 1,
                Runner::Zmin => 2,
            };
            assert_fetch_filter(
                &records,
                "blob:none",
                &fixture.head,
                true,
                expected_refetch_wants,
            );
            assert_no_fetch_haves(&records);
            assert_sideband_response(&records);
        }
        if oracle_mode() == OracleMode::Differential {
            assert_eq!(
                promisor_files(&http_zmin) != zmin_before_refetch,
                promisor_files(&http_stock) != stock_before_refetch,
                "--refetch promisor-pack replacement mode"
            );
        }

        let combine_http_stock = fixture.root.join("combine-http-stock");
        let combine_http_zmin = fixture.root.join("combine-http-zmin");
        let combine_stock_server = (oracle_mode() != OracleMode::ZminOnly)
            .then(|| HttpServer::new(&fixture.root, &fixture.root.join("combine-uri-stock")));
        let combine_zmin_server = (oracle_mode() != OracleMode::StockOnly)
            .then(|| HttpServer::new(&fixture.root, &fixture.root.join("combine-uri-zmin")));
        for (kind, destination) in active_runners(&combine_http_stock, &combine_http_zmin) {
            let server = match kind {
                Runner::Stock => combine_stock_server
                    .as_ref()
                    .expect("stock combined HTTP server"),
                Runner::Zmin => combine_zmin_server
                    .as_ref()
                    .expect("Zmin combined HTTP server"),
            };
            let args = clone_args(
                &server.url(),
                destination,
                refs,
                &["blob:none", "tree:0"],
                false,
            );
            let output = if kind == Runner::Stock {
                let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
                let mut command = Command::new(STOCK_GIT);
                env.apply(&mut command);
                command.current_dir(&fixture.root).args(refs);
                run_bounded(command, "stock combined HTTP clone")
            } else {
                run_clone(&env, kind, &fixture.root, &args, None)
            };
            require_success(output, "combined HTTP clone");
            let records = server.wait_for_requests(1);
            assert_first_combined_filter(&records, "combine:blob:none+tree:0", &fixture.head);
            assert_sideband_response(&records);
            assert_single_repo_contract(
                &env,
                &fixture,
                kind,
                destination,
                Some("combine:blob:none+tree:0"),
            );
        }
        assert_mode_contract(
            &env,
            &fixture,
            &combine_http_stock,
            &combine_http_zmin,
            Some("combine:blob:none+tree:0"),
        );
    }
}

macro_rules! matrix_test {
    ($name:ident, $format:expr, $refs:expr, $transport:expr) => {
        #[test]
        fn $name() {
            let root = TempDir::new().expect("oracle root");
            let _ = validate_artifacts(root.path());
            run_matrix_cell(root.path(), $format, $refs, $transport);
        }
    };
}

matrix_test!(
    partial_clone_sha1_files_local,
    HashFormat::Sha1,
    RefFormat::Files,
    Transport::Local
);
matrix_test!(
    partial_clone_sha1_files_file,
    HashFormat::Sha1,
    RefFormat::Files,
    Transport::File
);
matrix_test!(
    partial_clone_sha1_files_ssh,
    HashFormat::Sha1,
    RefFormat::Files,
    Transport::Ssh
);
matrix_test!(
    partial_clone_sha1_files_daemon,
    HashFormat::Sha1,
    RefFormat::Files,
    Transport::Daemon
);
matrix_test!(
    partial_clone_sha1_files_http,
    HashFormat::Sha1,
    RefFormat::Files,
    Transport::Http
);
matrix_test!(
    partial_clone_sha1_reftable_local,
    HashFormat::Sha1,
    RefFormat::Reftable,
    Transport::Local
);
matrix_test!(
    partial_clone_sha1_reftable_file,
    HashFormat::Sha1,
    RefFormat::Reftable,
    Transport::File
);
matrix_test!(
    partial_clone_sha1_reftable_ssh,
    HashFormat::Sha1,
    RefFormat::Reftable,
    Transport::Ssh
);
matrix_test!(
    partial_clone_sha1_reftable_daemon,
    HashFormat::Sha1,
    RefFormat::Reftable,
    Transport::Daemon
);
matrix_test!(
    partial_clone_sha1_reftable_http,
    HashFormat::Sha1,
    RefFormat::Reftable,
    Transport::Http
);
matrix_test!(
    partial_clone_sha256_files_local,
    HashFormat::Sha256,
    RefFormat::Files,
    Transport::Local
);
matrix_test!(
    partial_clone_sha256_files_file,
    HashFormat::Sha256,
    RefFormat::Files,
    Transport::File
);
matrix_test!(
    partial_clone_sha256_files_ssh,
    HashFormat::Sha256,
    RefFormat::Files,
    Transport::Ssh
);
matrix_test!(
    partial_clone_sha256_files_daemon,
    HashFormat::Sha256,
    RefFormat::Files,
    Transport::Daemon
);
matrix_test!(
    partial_clone_sha256_files_http,
    HashFormat::Sha256,
    RefFormat::Files,
    Transport::Http
);
matrix_test!(
    partial_clone_sha256_reftable_local,
    HashFormat::Sha256,
    RefFormat::Reftable,
    Transport::Local
);
matrix_test!(
    partial_clone_sha256_reftable_file,
    HashFormat::Sha256,
    RefFormat::Reftable,
    Transport::File
);
matrix_test!(
    partial_clone_sha256_reftable_ssh,
    HashFormat::Sha256,
    RefFormat::Reftable,
    Transport::Ssh
);
matrix_test!(
    partial_clone_sha256_reftable_daemon,
    HashFormat::Sha256,
    RefFormat::Reftable,
    Transport::Daemon
);
matrix_test!(
    partial_clone_sha256_reftable_http,
    HashFormat::Sha256,
    RefFormat::Reftable,
    Transport::Http
);

fn validated_fixture(parent: &Path, with_submodule: bool) -> (Fixture, Hermetic) {
    let _ = validate_artifacts(parent);
    let fixture = make_fixture(parent, HashFormat::Sha1, RefFormat::Files, with_submodule);
    let env = Hermetic::new(parent);
    (fixture, env)
}

#[test]
fn partial_clone_promisor_lazy_fetch_and_refetch_are_strict() {
    let root = TempDir::new().expect("lazy/refetch root");
    let (fixture, env) = validated_fixture(root.path(), false);
    let url = format!("file://{}", fixture.repo.display());
    let stock = fixture.root.join("lazy-stock");
    let zmin = fixture.root.join("lazy-zmin");
    for (kind, destination) in active_runners(&stock, &zmin) {
        let args = clone_args(&url, destination, fixture.refs, &["blob:none"], false);
        require_success(
            run_clone(&env, kind, &fixture.root, &args, None),
            "lazy partial clone",
        );
        if kind == Runner::Stock {
            assert_single_repo_contract(&env, &fixture, Runner::Stock, &stock, Some("blob:none"));
        }
    }
    assert_mode_contract(&env, &fixture, &stock, &zmin, Some("blob:none"));
    let missing_blob = &fixture.blobs[1].1;
    for (runner, repo) in active_runners(&stock, &zmin) {
        assert!(!has_object_runner(&env, runner, repo, missing_blob));
        let output = match runner {
            Runner::Stock => run_git(&env, repo, &["cat-file", "-p", missing_blob]),
            Runner::Zmin => run_zmin(&env, repo, &["cat-file", "-p", missing_blob], &[]),
        };
        require_success(output, "lazy object fetch");
        assert!(has_object_runner(&env, runner, repo, missing_blob));
    }

    let refetch_stock = fixture.root.join("refetch-stock");
    let refetch_zmin = fixture.root.join("refetch-zmin");
    for (kind, destination) in active_runners(&refetch_stock, &refetch_zmin) {
        let args = clone_args(&url, destination, fixture.refs, &["blob:none"], false);
        require_success(
            run_clone(&env, kind, &fixture.root, &args, None),
            "refetch setup",
        );
        if kind == Runner::Stock {
            assert_single_repo_contract(
                &env,
                &fixture,
                Runner::Stock,
                &refetch_stock,
                Some("blob:none"),
            );
        }
    }
    let refetch_args = [
        "fetch",
        "--quiet",
        "--refetch",
        "--filter=blob:none",
        "origin",
        "main",
    ];
    for (runner, repo) in active_runners(&refetch_stock, &refetch_zmin) {
        let output = match runner {
            Runner::Stock => run_git(&env, repo, &refetch_args),
            Runner::Zmin => run_zmin(&env, repo, &refetch_args, &[]),
        };
        require_success(output, "refetch");
        assert!(!promisor_files(repo).is_empty());
    }
    if oracle_mode() == OracleMode::Differential {
        assert_eq!(
            promisor_files(&refetch_stock).len(),
            promisor_files(&refetch_zmin).len(),
            "refetch marker count"
        );
    }
}

#[test]
fn partial_clone_also_filter_submodules_propagates_strictly() {
    let root = TempDir::new().expect("submodule root");
    let (fixture, env) = validated_fixture(root.path(), true);
    let url = format!("file://{}", fixture.repo.display());
    let stock = fixture.root.join("submodules-stock");
    let zmin = fixture.root.join("submodules-zmin");
    for (kind, destination) in active_runners(&stock, &zmin) {
        let mut args = clone_args(&url, destination, fixture.refs, &["blob:none"], false);
        args.retain(|arg| arg != "--no-checkout");
        args.insert(2, "--recurse-submodules".to_owned());
        args.insert(3, "--also-filter-submodules".to_owned());
        let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        let output = if kind == Runner::Stock {
            let mut command = Command::new(STOCK_GIT);
            env.apply(&mut command);
            command
                .current_dir(&fixture.root)
                .env("GIT_ALLOW_PROTOCOL", "file")
                .env(
                    "GIT_EXEC_PATH",
                    VALIDATED_SUBMODULE
                        .get()
                        .and_then(|path| path.parent())
                        .expect("validated helper bundle"),
                )
                .args(refs);
            run_bounded(command, "stock recursive filtered clone")
        } else {
            run_zmin(
                &env,
                &fixture.root,
                &refs,
                &[
                    ("GIT_ALLOW_PROTOCOL", Path::new("file")),
                    (
                        "GIT_EXEC_PATH",
                        VALIDATED_SUBMODULE
                            .get()
                            .and_then(|path| path.parent())
                            .expect("validated helper bundle"),
                    ),
                ],
            )
        };
        require_success(output, "recursive filtered clone");
        assert_single_repo_contract(&env, &fixture, kind, destination, Some("blob:none"));
        assert_submodule_content_contract(&env, &fixture, kind, destination);
    }
    for (runner, repo) in active_runners(&stock, &zmin) {
        assert_eq!(
            check_config_runner(
                &env,
                runner,
                &repo.join("modules/sub"),
                "remote.origin.partialclonefilter"
            ),
            Some("blob:none".to_owned())
        );
    }
}

#[test]
fn partial_clone_promisor_auto_is_separate_from_blob_none() {
    let root = TempDir::new().expect("auto root");
    let (fixture, env) = validated_fixture(root.path(), false);
    let url = format!("file://{}", fixture.repo.display());
    configure_remote(&env, &fixture.repo, "promisor.advertise", "true");
    configure_remote(
        &env,
        &fixture.repo,
        "promisor.sendFields",
        "partialCloneFilter",
    );
    configure_remote(
        &env,
        &fixture.repo,
        "remote.blobstore.url",
        "https://blobstore.example.invalid/remote.git",
    );
    configure_remote(&env, &fixture.repo, "remote.blobstore.promisor", "true");
    configure_remote(
        &env,
        &fixture.repo,
        "remote.blobstore.partialCloneFilter",
        "blob:limit=1",
    );
    let stock = fixture.root.join("auto-stock");
    let zmin = fixture.root.join("auto-zmin");
    for (kind, destination) in active_runners(&stock, &zmin) {
        let mut args = clone_args(&url, destination, fixture.refs, &["auto"], false);
        args.insert(1, "--config=protocol.version=2".to_owned());
        args.insert(2, "--config=promisor.acceptFromServer=all".to_owned());
        require_success(
            run_clone(&env, kind, &fixture.root, &args, None),
            "auto-filter clone",
        );
        if kind == Runner::Stock {
            assert_single_repo_contract(&env, &fixture, Runner::Stock, &stock, Some("auto"));
        }
    }
    assert_mode_contract(&env, &fixture, &stock, &zmin, Some("auto"));
    for (runner, repo) in active_runners(&stock, &zmin) {
        assert_ne!(
            check_config_runner(&env, runner, repo, "remote.origin.partialclonefilter"),
            Some("blob:none".to_owned())
        );
    }
}

#[test]
fn partial_clone_filter_packfile_uri_interaction_is_strict() {
    let root = TempDir::new().expect("URI parity root");
    let (fixture, env) = validated_fixture(root.path(), false);
    let stock = fixture.root.join("uri-stock");
    let zmin = fixture.root.join("uri-zmin");
    let mut stock_gets = None;
    if oracle_mode() != OracleMode::ZminOnly {
        let stock_server = HttpServer::new(&fixture.root, &fixture.root.join("uri"));
        let (stock_pack_path, _) = setup_http_uri(&env, &fixture, &stock_server);
        let mut stock_args = clone_args(
            &stock_server.url(),
            &stock,
            fixture.refs,
            &["blob:none"],
            false,
        );
        stock_args.insert(1, "--config=protocol.version=2".to_owned());
        stock_args.insert(2, "--config=fetch.uriprotocols=http".to_owned());
        let stock_refs = stock_args.iter().map(String::as_str).collect::<Vec<_>>();
        let mut command = Command::new(STOCK_GIT);
        env.apply(&mut command);
        command.current_dir(&fixture.root).args(stock_refs);
        require_success(run_bounded(command, "stock URI clone"), "stock URI clone");
        let records = stock_server.wait_for_requests(1);
        stock_gets = Some(assert_uri_get_records(&records, &stock_pack_path));
        assert_single_repo_contract(&env, &fixture, Runner::Stock, &stock, Some("blob:none"));
    }
    let mut zmin_gets = None;
    if oracle_mode() != OracleMode::StockOnly {
        let zmin_server = HttpServer::new(&fixture.root, &fixture.root.join("uri"));
        let (zmin_pack_path, _) = setup_http_uri(&env, &fixture, &zmin_server);
        let mut zmin_args = clone_args(
            &zmin_server.url(),
            &zmin,
            fixture.refs,
            &["blob:none"],
            false,
        );
        zmin_args.insert(1, "--config=protocol.version=2".to_owned());
        zmin_args.insert(2, "--config=fetch.uriprotocols=http".to_owned());
        let zmin_refs = zmin_args.iter().map(String::as_str).collect::<Vec<_>>();
        require_success(
            run_zmin(&env, &fixture.root, &zmin_refs, &[]),
            "Zmin URI clone",
        );
        let records = zmin_server.wait_for_requests(1);
        zmin_gets = Some(assert_uri_get_records(&records, &zmin_pack_path));
    }
    match oracle_mode() {
        OracleMode::Differential => assert_eq!(zmin_gets, stock_gets, "packfile-URI GET count"),
        OracleMode::StockOnly => assert!(stock_gets.is_some_and(|count| count > 0)),
        OracleMode::ZminOnly => assert!(zmin_gets.is_some_and(|count| count > 0)),
    }
    assert_mode_contract(&env, &fixture, &stock, &zmin, Some("blob:none"));
}

fn run_filter_family_contract(filter: String) {
    let root = TempDir::new().expect("filter family root");
    let (fixture, env) = validated_fixture(root.path(), false);
    let stock = fixture.root.join("family-stock");
    let zmin = fixture.root.join("family-zmin");
    for (kind, destination) in active_runners(&stock, &zmin) {
        let runner_name = match kind {
            Runner::Stock => "stock",
            Runner::Zmin => "zmin",
        };
        let server = HttpServer::new(
            &fixture.root,
            &fixture.root.join(format!("family-uri-{runner_name}")),
        );
        let args = clone_args(
            &server.url(),
            destination,
            fixture.refs,
            &[filter.as_str()],
            false,
        );
        let output = run_clone(&env, kind, &fixture.root, &args, None);
        require_success(output, "filter family clone");
        assert_single_repo_contract(&env, &fixture, kind, destination, Some(filter.as_str()));
        let records = server.wait_for_requests(1);
        assert_first_fetch_filter(&records, &filter, &fixture.head);
        assert_sideband_response(&records);
    }
    assert_mode_contract(&env, &fixture, &stock, &zmin, Some(filter.as_str()));
}

#[test]
fn partial_clone_filter_blob_none_wire_contract() {
    run_filter_family_contract("blob:none".to_owned());
}

#[test]
fn partial_clone_filter_blob_limit_wire_contract() {
    run_filter_family_contract("blob:limit=1".to_owned());
}

#[test]
fn partial_clone_filter_tree_zero_wire_contract() {
    run_filter_family_contract("tree:0".to_owned());
}

#[test]
fn partial_clone_filter_object_type_wire_contract() {
    run_filter_family_contract("object:type=blob".to_owned());
}

#[test]
fn partial_clone_filter_sparse_oid_wire_contract() {
    let root = TempDir::new().expect("sparse filter root");
    let (fixture, _env) = validated_fixture(root.path(), false);
    let filter = format!("sparse:oid={}", fixture.blobs[3].1);
    drop(fixture);
    drop(root);
    run_filter_family_contract(filter);
}

#[test]
fn partial_clone_filter_combine_wire_order_contract() {
    let root = TempDir::new().expect("combined filter root");
    let (fixture, env) = validated_fixture(root.path(), false);
    let stock = fixture.root.join("combine-stock");
    let zmin = fixture.root.join("combine-zmin");
    for (kind, destination) in active_runners(&stock, &zmin) {
        let runner_name = match kind {
            Runner::Stock => "stock",
            Runner::Zmin => "zmin",
        };
        let server = HttpServer::new(
            &fixture.root,
            &fixture.root.join(format!("combine-uri-{runner_name}")),
        );
        let args = clone_args(
            &server.url(),
            destination,
            fixture.refs,
            &["blob:none", "tree:0"],
            false,
        );
        require_success(
            run_clone(&env, kind, &fixture.root, &args, None),
            "combined filter clone",
        );
        assert_single_repo_contract(
            &env,
            &fixture,
            kind,
            destination,
            Some("combine:blob:none+tree:0"),
        );
        let records = server.wait_for_requests(1);
        assert_first_combined_filter(&records, "combine:blob:none+tree:0", &fixture.head);
        assert_sideband_response(&records);
    }
    assert_mode_contract(
        &env,
        &fixture,
        &stock,
        &zmin,
        Some("combine:blob:none+tree:0"),
    );
}

#[test]
fn partial_clone_filter_no_filter_last_option_contract() {
    let root = TempDir::new().expect("no-filter root");
    let (fixture, env) = validated_fixture(root.path(), false);
    let url = format!("file://{}", fixture.repo.display());
    let no_filter_stock = fixture.root.join("no-filter-stock");
    let no_filter_zmin = fixture.root.join("no-filter-zmin");
    for (kind, destination) in active_runners(&no_filter_stock, &no_filter_zmin) {
        let args = clone_args(&url, destination, fixture.refs, &["blob:none"], true);
        require_success(
            run_clone(&env, kind, &fixture.root, &args, None),
            "no-filter clone",
        );
        if kind == Runner::Stock {
            assert_single_repo_contract(&env, &fixture, Runner::Stock, &no_filter_stock, None);
        }
    }
    assert_mode_contract(&env, &fixture, &no_filter_stock, &no_filter_zmin, None);

    let filter_stock = fixture.root.join("filter-after-no-filter-stock");
    let filter_zmin = fixture.root.join("filter-after-no-filter-zmin");
    for (kind, destination) in active_runners(&filter_stock, &filter_zmin) {
        let mut args = clone_args(&url, destination, fixture.refs, &[], false);
        args.insert(2, "--no-filter".to_owned());
        args.insert(3, "--filter=blob:none".to_owned());
        require_success(
            run_clone(&env, kind, &fixture.root, &args, None),
            "filter after no-filter clone",
        );
        if kind == Runner::Stock {
            assert_single_repo_contract(
                &env,
                &fixture,
                Runner::Stock,
                &filter_stock,
                Some("blob:none"),
            );
        }
    }
    assert_mode_contract(
        &env,
        &fixture,
        &filter_stock,
        &filter_zmin,
        Some("blob:none"),
    );
}

#[test]
fn focused_sha256_reftable_http_is_stable_for_twenty_runs() {
    let root = TempDir::new().expect("HTTP stress root");
    let _ = validate_artifacts(root.path());
    for run in 0..20 {
        let iteration = root.path().join(format!("stress-{run}"));
        fs::create_dir_all(&iteration).expect("HTTP stress iteration");
        let fixture = make_fixture(&iteration, HashFormat::Sha256, RefFormat::Reftable, false);
        let env = Hermetic::new(&iteration);
        let stock = fixture.root.join("stress-stock");
        let zmin = fixture.root.join("stress-zmin");
        for (runner, destination) in active_runners(&stock, &zmin) {
            let runner_name = match runner {
                Runner::Stock => "stock",
                Runner::Zmin => "zmin",
            };
            let server = HttpServer::new(
                &fixture.root,
                &fixture.root.join(format!("stress-uri-{runner_name}")),
            );
            let args = clone_args(
                &server.url(),
                destination,
                RefFormat::Reftable,
                &["blob:none"],
                false,
            );
            require_success(
                run_clone(&env, runner, &fixture.root, &args, None),
                "HTTP stress clone",
            );
            let records = server.wait_for_requests(1);
            assert_first_fetch_filter(&records, "blob:none", &fixture.head);
            assert_sideband_response(&records);
            assert_single_repo_contract(&env, &fixture, runner, destination, Some("blob:none"));
        }
        assert_mode_contract(&env, &fixture, &stock, &zmin, Some("blob:none"));
    }
}

#[test]
fn focused_http_uri_negative_contracts_are_bounded_and_non_traversable() {
    let root = TempDir::new().expect("URI negative root");
    let uri_root = root.path().join("uri");
    fs::create_dir_all(&uri_root).expect("URI negative directory");
    fs::write(uri_root.join("pack-deadbeef.pack"), b"known-pack-bytes").expect("URI negative pack");
    assert!(safe_uri_file(&uri_root, "/uri/pack-deadbeef.pack").is_some());
    for path in [
        "/uri/../secret",
        "/uri/%2e%2e/secret",
        "/uri/..\\secret",
        "/uri/",
    ] {
        assert!(
            safe_uri_file(&uri_root, path).is_none(),
            "URI traversal accepted"
        );
    }
    let not_found = HttpServer::with_uri_mode(root.path(), &uri_root, UriMode::NotFound);
    let response = direct_http_get(&not_found, "/uri/pack-deadbeef.pack");
    assert!(
        response.starts_with(b"HTTP/1.1 404 Not Found\r\n"),
        "URI 404 status contract"
    );
    drop(not_found);
    let corrupt = HttpServer::with_uri_mode(root.path(), &uri_root, UriMode::Corrupt);
    let response = direct_http_get(&corrupt, "/uri/pack-deadbeef.pack");
    assert!(
        response.starts_with(b"HTTP/1.1 200 OK\r\n"),
        "URI corrupt response status contract"
    );
    assert!(
        !response.ends_with(b"known-pack-bytes"),
        "corrupt URI served original bytes"
    );
    let grammar = HttpServer::new(root.path(), &uri_root);
    let duplicate_headers = direct_http_raw(
        &grammar,
        b"POST /remote.git/git-upload-pack HTTP/1.1\r\nHost: localhost\r\ncOnTeNt-LeNgTh: 0\r\nCONTENT-LENGTH: 0\r\n\r\n",
    );
    assert!(
        duplicate_headers.is_empty(),
        "duplicate header was accepted"
    );
    assert!(grammar.wait_for_connections(1).is_empty());
    let malformed_request_line = direct_http_raw(
        &grammar,
        b"GET /remote.git/info/refs HTTP/1.1 extra\r\nHost: localhost\r\n\r\n",
    );
    assert!(
        malformed_request_line.is_empty(),
        "extra request-line field was accepted"
    );
    assert!(grammar.wait_for_connections(2).is_empty());
    let unsupported_framing = direct_http_raw(
        &grammar,
        b"POST /remote.git/git-upload-pack HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n",
    );
    assert!(
        unsupported_framing.is_empty(),
        "chunked framing was accepted"
    );
    assert!(grammar.wait_for_connections(3).is_empty());
}

#[test]
fn focused_pkt_line_parser_preserves_invalid_contract_fixtures() {
    assert!(parse_packet_payloads(b"0003").is_err());
    assert_eq!(
        semantic_pkt_line(b"command=fetch"),
        Some(b"command=fetch\n".to_vec())
    );
    assert_eq!(
        semantic_pkt_line(b"command=fetch\n"),
        Some(b"command=fetch\n".to_vec())
    );
    assert!(semantic_pkt_line(b"command=fetch\nextra").is_none());
    assert!(semantic_pkt_line(b"command=fetch\n\n").is_none());

    let mut duplicate = Vec::new();
    duplicate.extend(pkt_line("version 2\n"));
    duplicate.extend(pkt_line("command=fetch\n"));
    duplicate.extend(pkt_line("thin-pack\n"));
    duplicate.extend(pkt_line("ofs-delta\n"));
    duplicate.extend(pkt_line("no-progress\n"));
    duplicate.extend_from_slice(b"0001");
    duplicate.extend(pkt_line("want 0123456789012345678901234567890123456789\n"));
    duplicate.extend(pkt_line("filter blob:none\n"));
    duplicate.extend(pkt_line("filter blob:none\n"));
    duplicate.extend_from_slice(b"0000");
    let duplicate_packets = parse_packet_payloads(&duplicate).expect("duplicate packet fixture");
    assert_eq!(packet_count(&duplicate_packets, b"filter blob:none\n"), 2);

    let mut missing_capability = Vec::new();
    missing_capability.extend(pkt_line("version 2\n"));
    missing_capability.extend(pkt_line("command=fetch\n"));
    missing_capability.extend_from_slice(b"0001");
    missing_capability.extend(pkt_line("want 0123456789012345678901234567890123456789\n"));
    missing_capability.extend(pkt_line("filter blob:none\n"));
    missing_capability.extend_from_slice(b"0000");
    let missing_packets =
        parse_packet_payloads(&missing_capability).expect("missing capability packet fixture");
    assert_eq!(packet_count(&missing_packets, b"command=fetch\n"), 1);
    assert_eq!(packet_count(&missing_packets, b"thin-pack\n"), 0);
    assert_eq!(packet_count(&missing_packets, b"ofs-delta\n"), 0);
    assert_eq!(packet_count(&missing_packets, b"no-progress\n"), 0);
}

#[test]
fn focused_stock_and_zmin_reject_invalid_upload_pack_contracts() {
    let root = TempDir::new().expect("invalid protocol root");
    let _ = validate_artifacts(root.path());
    let fixture = make_fixture(root.path(), HashFormat::Sha1, RefFormat::Files, false);
    let env = Hermetic::new(root.path());
    let mut duplicate_command = Vec::new();
    duplicate_command.extend(pkt_line("version 2\n"));
    duplicate_command.extend(pkt_line("command=fetch\n"));
    duplicate_command.extend(pkt_line("command=fetch\n"));
    duplicate_command.extend_from_slice(b"0001");
    duplicate_command.extend_from_slice(b"0000");
    let mut missing_required_command = Vec::new();
    missing_required_command.extend(pkt_line("version 2\n"));
    missing_required_command.extend_from_slice(b"0001");
    missing_required_command.extend_from_slice(b"0000");
    let mut duplicate_filter = Vec::new();
    duplicate_filter.extend(pkt_line("command=fetch"));
    duplicate_filter.extend(pkt_line("agent=git/2.55.0-Darwin"));
    duplicate_filter.extend(pkt_line("object-format=sha1"));
    duplicate_filter.extend_from_slice(b"0001");
    duplicate_filter.extend(pkt_line("thin-pack"));
    duplicate_filter.extend(pkt_line("no-progress"));
    duplicate_filter.extend(pkt_line("ofs-delta"));
    duplicate_filter.extend(pkt_line("filter blob:none"));
    duplicate_filter.extend(pkt_line("filter blob:none"));
    duplicate_filter.extend(pkt_line(&format!("want {}\n", fixture.head)));
    duplicate_filter.extend(pkt_line("done\n"));
    duplicate_filter.extend_from_slice(b"0000");
    let mut missing_delimiter = Vec::new();
    missing_delimiter.extend(pkt_line("command=fetch"));
    missing_delimiter.extend(pkt_line("agent=git/2.55.0-Darwin"));
    missing_delimiter.extend(pkt_line("object-format=sha1"));
    missing_delimiter.extend(pkt_line("thin-pack"));
    missing_delimiter.extend(pkt_line("no-progress"));
    missing_delimiter.extend(pkt_line("ofs-delta"));
    missing_delimiter.extend(pkt_line("filter blob:none"));
    missing_delimiter.extend(pkt_line(&format!("want {}\n", fixture.head)));
    missing_delimiter.extend(pkt_line("done\n"));
    missing_delimiter.extend_from_slice(b"0000");
    let zmin_probe = fixture.root.join("invalid-zmin-placeholder");
    for (runner, _) in active_runners(&fixture.repo, &zmin_probe) {
        let duplicate = upload_pack_probe(&env, runner, &fixture.repo, &duplicate_command);
        assert!(
            !duplicate.status.success() && !duplicate.stderr.is_empty(),
            "duplicate command was not rejected with a diagnostic"
        );
        let missing = upload_pack_probe(&env, runner, &fixture.repo, &missing_required_command);
        assert!(
            !missing.status.success() && !missing.stderr.is_empty(),
            "missing required fetch command was not rejected with a diagnostic"
        );
        let duplicate = upload_pack_probe(&env, runner, &fixture.repo, &duplicate_filter);
        assert!(
            !duplicate.status.success() && !duplicate.stderr.is_empty(),
            "duplicate filter was not rejected with a diagnostic"
        );
        let missing = upload_pack_probe(&env, runner, &fixture.repo, &missing_delimiter);
        assert!(
            !missing.status.success() && !missing.stderr.is_empty(),
            "missing section delimiter was not rejected with a diagnostic"
        );
    }
}
