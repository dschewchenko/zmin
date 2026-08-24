mod common;

use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::process::{Command, Stdio};

use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use tempfile::TempDir;
use zmin_git_core::{GitHashAlgorithm, GitObjectHash, GitObjectKind, ObjectId, hash_object};

use common::{
    command_any_output, command_any_output_with_stdin, command_failure_output,
    command_failure_output_with_env, command_output, command_output_with_env, configure_identity,
    ensure_remote_http_helper, git, git_args, git_init, git_status_args, git_with_env,
    git_with_stdin_args, git_with_stdin_bytes, required_pinned_stock_git, run_zmin, run_zmin_args,
    run_zmin_failure_output, run_zmin_with_env, run_zmin_with_stdin_args, stock_git_bin, zmin_bin,
};

fn unused_local_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("bind unused local port")
        .local_addr()
        .expect("local addr")
        .port()
}

fn wait_for_tcp_port(port: u16) {
    let addr = ("127.0.0.1", port);
    for _ in 0..100 {
        if std::net::TcpStream::connect(addr).is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("tcp port {port} did not open");
}

fn pinned_http_helper_for_evidence() -> &'static std::path::Path {
    static HELPER: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    HELPER
        .get_or_init(|| {
            let helper_name = if cfg!(windows) {
                "zmin-git-remote-http.exe"
            } else {
                "zmin-git-remote-http"
            };
            let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(std::path::Path::parent)
                .expect("workspace root");
            let helper = workspace_root
                .join("target/test-remote-http-helper/debug")
                .join(helper_name);
            assert!(
                helper.is_file(),
                "pinned evidence HTTP helper is missing: {}",
                helper.display()
            );
            helper
        })
        .as_path()
}

fn run_hermetic_zmin_http_fetch(
    cwd: &std::path::Path,
    commit: &str,
    url: &str,
    helper: &std::path::Path,
    http_version: Option<&str>,
) -> std::process::Output {
    let mut command = hermetic_zmin_command(cwd, helper, http_version, false);
    command.args(["http-fetch", "-a", "-w", "refs/heads/main", commit, url]);
    command.output().expect("run hermetic zmin http-fetch")
}

fn hermetic_zmin_command(
    cwd: &std::path::Path,
    helper: &std::path::Path,
    http_version: Option<&str>,
    tls_no_verify: bool,
) -> Command {
    let home = cwd.join("hermetic-home");
    fs::create_dir_all(home.join("config")).expect("create hermetic config directory");
    let null_config = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let path = if cfg!(windows) {
        r"C:\Windows\System32"
    } else {
        "/usr/bin:/bin"
    };

    let mut command = Command::new(zmin_bin());
    command
        .env_clear()
        .current_dir(cwd)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null_config)
        .env("GIT_CONFIG_SYSTEM", null_config)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("PATH", path)
        .env("GIT_SSL_NO_VERIFY", if tls_no_verify { "1" } else { "0" });
    for key in [
        "ZMIN_GIT_REMOTE_HTTP",
        "ZMIN_GIT_HTTP_BUNDLE",
        "ZMIN_STOCK_GIT",
    ] {
        command.env_remove(key);
    }
    command.env("ZMIN_GIT_REMOTE_HTTP", helper);
    if let Some(http_version) = http_version {
        command.env("ZMIN_GIT_HTTP_VERSION", http_version);
    }
    command
}

#[cfg(unix)]
fn capture_git_daemon_request(
    command: &std::path::Path,
    virtual_host: &OsString,
) -> (std::process::Output, Vec<u8>) {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("bind daemon request capture listener");
    let port = listener
        .local_addr()
        .expect("daemon request capture address")
        .port();
    let request = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept daemon request capture");
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("set daemon request capture timeout");
        let mut header = [0_u8; 4];
        stream
            .read_exact(&mut header)
            .expect("read daemon request header");
        let packet_len = usize::from_str_radix(
            std::str::from_utf8(&header).expect("daemon request header hex"),
            16,
        )
        .expect("parse daemon request length");
        assert!(packet_len >= 4, "daemon request packet must include header");
        let mut payload = vec![0_u8; packet_len - 4];
        stream
            .read_exact(&mut payload)
            .expect("read daemon request payload");
        stream
            .write_all(b"0000")
            .expect("write daemon request flush");
        payload
    });
    let url = format!("git://127.0.0.1:{port}/repo.git");
    let output = Command::new(command)
        .env("GIT_OVERRIDE_VIRTUAL_HOST", virtual_host)
        .args(["-c", "protocol.version=0", "ls-remote", url.as_str()])
        .output()
        .expect("run daemon request capture client");
    let request = request.join().expect("join daemon request capture");
    (output, request)
}

fn run_newline_clone_probe(
    command: &str,
    cwd: &std::path::Path,
    destination: &std::path::Path,
) -> ((i32, String, String), bool) {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind probe listener");
    let port = listener
        .local_addr()
        .expect("probe listener address")
        .port();
    listener
        .set_nonblocking(true)
        .expect("set probe listener nonblocking");
    let connection = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            match listener.accept() {
                Ok(_) => return true,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        return false;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(_) => return false,
            }
        }
    });
    let url = format!("git://127.0.0.1:{port}/repo\n.git");
    let args = [
        "clone",
        url.as_str(),
        destination.to_str().expect("probe destination path"),
    ];
    let output = command_any_output(command, cwd, &args, "newline git daemon clone");
    let connected = connection.join().expect("join probe listener");
    (output, connected)
}

fn wait_for_ref(repo: &std::path::Path, suffix: &str) -> String {
    let mut last_error = String::new();
    for _ in 0..200 {
        let output = Command::new(stock_git_bin())
            .current_dir(repo)
            .arg("show-ref")
            .output()
            .expect("run git show-ref");
        if output.status.success() {
            let stdout = String::from_utf8(output.stdout).expect("show-ref stdout utf8");
            if stdout.lines().any(|line| line.ends_with(suffix)) {
                return stdout;
            }
            last_error = stdout;
        } else {
            last_error = String::from_utf8_lossy(&output.stderr).into_owned();
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("ref ending with {suffix} did not appear:\n{last_error}");
}

fn assert_background_fetch_hydrated(repo: &std::path::Path) {
    assert_eq!(
        run_zmin(
            repo,
            ["config", "--get", "zmin.worktreeFirstBackgroundFetch"]
        ),
        "true"
    );
    assert_eq!(
        run_zmin(
            repo,
            ["config", "--get", "zmin.worktreeFirstBackgroundFetchRemote"]
        ),
        "origin"
    );
    let hydrated_refs = wait_for_ref(repo, " refs/remotes/origin/feature");
    let hydrated_refs = if hydrated_refs
        .lines()
        .any(|line| line.ends_with(" refs/tags/v1"))
    {
        hydrated_refs
    } else {
        wait_for_ref(repo, " refs/tags/v1")
    };
    assert!(
        hydrated_refs
            .lines()
            .any(|line| line.ends_with(" refs/tags/v1")),
        "background fetch should hydrate followed tag refs:\n{hydrated_refs}"
    );
    git(repo, ["fsck", "--strict"]);
}

fn remove_all_pack_files(repo: &std::path::Path) {
    let pack_dir = repo.join(".git/objects/pack");
    for entry in fs::read_dir(&pack_dir).expect("read pack dir") {
        let path = entry.expect("pack dir entry").path();
        if matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("pack" | "idx" | "rev")
        ) {
            fs::remove_file(&path)
                .unwrap_or_else(|error| panic!("remove pack artifact {}: {error}", path.display()));
        }
    }
}

fn assert_demand_hydrate_config(repo: &std::path::Path) {
    assert_eq!(
        run_zmin(repo, ["config", "--get", "remote.origin.promisor"]),
        "true"
    );
    assert_eq!(
        run_zmin(repo, ["config", "--get", "zmin.worktreeFirstDemandHydrate"]),
        "true"
    );
    assert_eq!(
        run_zmin(
            repo,
            ["config", "--get", "zmin.worktreeFirstDemandHydrateRemote"]
        ),
        "origin"
    );
}

fn command_any_output_with_env(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
    label: &str,
) -> (i32, String, String) {
    let program = if command == "git" {
        stock_git_bin().as_os_str().to_owned()
    } else {
        OsString::from(command)
    };
    let mut process = Command::new(program);
    process.args(args).current_dir(cwd);
    for (key, value) in envs {
        process.env(key, value);
    }
    let output = process
        .output()
        .unwrap_or_else(|err| panic!("run {label}: {err}"));
    (
        output.status.code().expect("process exit code"),
        String::from_utf8(output.stdout)
            .expect("stdout utf8")
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .expect("stderr utf8")
            .trim_end_matches('\n')
            .to_owned(),
    )
}

fn pinned_command_any_output(
    cwd: &std::path::Path,
    args: &[&str],
    label: &str,
) -> (i32, String, String) {
    pinned_command_any_output_with_env(cwd, args, &[], label)
}

fn pinned_command_any_output_with_env(
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
    label: &str,
) -> (i32, String, String) {
    let mut command = pinned_git_command(cwd, args, envs);
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("run {label}: {error}"));
    (
        output.status.code().expect("pinned Git exit code"),
        String::from_utf8(output.stdout)
            .expect("pinned Git stdout UTF-8")
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .expect("pinned Git stderr UTF-8")
            .trim_end_matches('\n')
            .to_owned(),
    )
}

fn pinned_git_args<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> String {
    let output = pinned_git_command(cwd, &args, &[])
        .output()
        .expect("run pinned Git");
    assert!(
        output.status.success(),
        "pinned Git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("pinned Git stdout UTF-8")
        .trim_end_matches('\n')
        .to_owned()
}

fn pinned_git_with_env<const N: usize>(
    cwd: &std::path::Path,
    args: [&str; N],
    env: &[(&str, &str)],
) -> String {
    let output = pinned_git_command(cwd, &args, env)
        .output()
        .expect("run pinned Git");
    assert!(
        output.status.success(),
        "pinned Git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("pinned Git stdout UTF-8")
        .trim_end_matches('\n')
        .to_owned()
}

fn pinned_git_command(cwd: &std::path::Path, args: &[&str], envs: &[(&str, &str)]) -> Command {
    let bundle = pinned_http_bundle_root();
    let mut command = Command::new(required_pinned_stock_git());
    command
        .env("PATH", pinned_git_helper_path(&bundle))
        .env("GIT_EXEC_PATH", pinned_stock_git_exec_path())
        .env("GIT_TEMPLATE_DIR", bundle.join("templates"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .envs(envs.iter().copied())
        .args(args)
        .current_dir(cwd);
    command
}

fn pinned_git_helper_path(bundle: &std::path::Path) -> OsString {
    let mut paths = vec![bundle.to_path_buf()];
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(paths).expect("construct pinned Git helper PATH")
}

fn pinned_http_v2_exec_path(root: &std::path::Path) -> std::path::PathBuf {
    let pinned_exec_path = pinned_stock_git_exec_path();
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let remote_http = pinned_exec_path.join(format!("git-remote-http{suffix}"));
    assert!(
        remote_http.is_file(),
        "pinned Git helper is missing: {}",
        remote_http.display()
    );
    let exec_path = root.join("pinned-http-v2-exec");
    fs::create_dir_all(&exec_path).expect("create pinned HTTP helper directory");
    for helper in [
        format!("git-remote-http{suffix}"),
        format!("git-remote-https{suffix}"),
    ] {
        let path = exec_path.join(helper);
        let remote_http = remote_http.display().to_string().replace('\'', "'\\''");
        let script = format!(
            "#!/bin/sh\nexport GIT_PROTOCOL=version=2\nexport GIT_CONFIG_COUNT=1\nexport GIT_CONFIG_KEY_0=protocol.version\nexport GIT_CONFIG_VALUE_0=2\nexec '{remote_http}' \"$@\"\n"
        );
        fs::write(&path, script).expect("write pinned HTTP helper wrapper");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&path)
                .expect("pinned HTTP helper wrapper metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions).expect("make pinned HTTP helper executable");
        }
    }
    exec_path
}

fn pinned_stock_git_exec_path() -> std::path::PathBuf {
    pinned_http_bundle_root()
}

fn pinned_http_bundle_root() -> std::path::PathBuf {
    static BUNDLE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    BUNDLE
        .get_or_init(|| {
            let bundle = std::path::PathBuf::from(
                std::env::var_os("ZMIN_GIT_HTTP_BUNDLE")
                    .expect("ZMIN_GIT_HTTP_BUNDLE must select the validated Git HTTP bundle"),
            );
            assert!(
                bundle.is_absolute(),
                "HTTP bundle path must be absolute: {bundle:?}"
            );
            let stock = required_pinned_stock_git();
            let cache = bundle
                .parent()
                .expect("HTTP bundle cache parent")
                .to_path_buf();
            let validator = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tools/git-upstream-http-provenance.sh");
            let output = Command::new(&validator)
                .arg("validate")
                .env("ZMIN_UPSTREAM_GIT_CACHE", &cache)
                .env("ZMIN_GIT_HTTP_BUNDLE", &bundle)
                .env("ZMIN_STOCK_GIT", &stock)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .expect("run pinned Git HTTP provenance validator");
            assert!(
                output.status.success(),
                "pinned Git HTTP provenance validation failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let validated = String::from_utf8(output.stdout)
                .expect("pinned Git HTTP validator output")
                .trim()
                .to_owned();
            let canonical = fs::canonicalize(&bundle).expect("canonical HTTP bundle");
            assert_eq!(validated, canonical.display().to_string());
            canonical
        })
        .clone()
}

fn assert_any_ls_remote_output_matches_stock_git(
    cwd: &std::path::Path,
    args: &[&str],
    label: &str,
) {
    assert_eq!(
        command_any_output(zmin_bin(), cwd, args, label),
        command_any_output("git", cwd, args, label),
        "{label}: {args:?}"
    );
}

fn assert_any_ls_remote_output_matches_stock_git_with_env(
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
    label: &str,
) {
    assert_eq!(
        command_any_output_with_env(zmin_bin(), cwd, args, envs, label),
        command_any_output_with_env("git", cwd, args, envs, label),
        "{label}: {args:?}"
    );
}

fn read_http_request_headers(stream: &mut std::net::TcpStream) -> io::Result<Vec<u8>> {
    let mut request = Vec::new();
    let mut buf = [0_u8; 1024];
    loop {
        let read = stream.read(&mut buf)?;
        if read == 0 {
            return Ok(request);
        }
        request.extend_from_slice(&buf[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(request);
        }
    }
}

#[cfg(unix)]
const H2_TLS_SERVER_SCRIPT: &str = r#"
import os
import socket
import ssl
import sys

port = int(sys.argv[1])
cert = sys.argv[2]
key = sys.argv[3]
pack = sys.argv[4]
ready = sys.argv[5]
marker = sys.argv[6]

context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(cert, key)
context.set_alpn_protocols(["h2"])

listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(("127.0.0.1", port))
listener.listen(1)
with open(ready, "w", encoding="ascii") as stream:
    stream.write("ready\n")

connection, _ = listener.accept()
with context.wrap_socket(connection, server_side=True) as stream:
    stream.settimeout(10)
    with open(marker, "w", encoding="ascii") as output:
        output.write((stream.selected_alpn_protocol() or "none") + "\n")
        output.flush()

    def read_exact(length):
        result = bytearray()
        while len(result) < length:
            chunk = stream.recv(length - len(result))
            if not chunk:
                raise RuntimeError("unexpected EOF in HTTP/2 frame")
            result.extend(chunk)
        return bytes(result)

    preface = read_exact(24)
    if preface != b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n":
        raise RuntimeError("missing HTTP/2 connection preface")

    def read_frame():
        header = read_exact(9)
        length = int.from_bytes(header[:3], "big")
        frame_type = header[3]
        flags = header[4]
        stream_id = int.from_bytes(header[5:9], "big") & 0x7fffffff
        return frame_type, flags, stream_id, read_exact(length)

    def frame(frame_type, flags, stream_id, payload):
        return (
            len(payload).to_bytes(3, "big")
            + bytes((frame_type, flags))
            + (stream_id & 0x7fffffff).to_bytes(4, "big")
            + payload
        )

    stream.sendall(frame(4, 0, 0, b""))
    request_stream = None
    while request_stream is None:
        frame_type, flags, stream_id, _payload = read_frame()
        if frame_type == 1 and stream_id != 0:
            request_stream = stream_id
            while not (flags & 4):
                continuation_type, continuation_flags, continuation_stream, _payload = read_frame()
                if continuation_type != 9 or continuation_stream != request_stream:
                    raise RuntimeError("invalid HTTP/2 continuation sequence")
                flags = continuation_flags
    with open(marker, "a", encoding="ascii") as output:
        output.write("request\n")
    body = open(pack, "rb").read()
    stream.sendall(frame(1, 4, request_stream, b"\x88"))
    offset = 0
    while offset < len(body):
        chunk = body[offset:offset + 16384]
        offset += len(chunk)
        stream.sendall(frame(0, 1 if offset == len(body) else 0, request_stream, chunk))
"#;

#[cfg(unix)]
struct H2TlsServer {
    port: u16,
    ready: std::path::PathBuf,
    marker: std::path::PathBuf,
    stderr: std::path::PathBuf,
    child: Option<std::process::Child>,
}

#[cfg(unix)]
impl H2TlsServer {
    fn new(root: &std::path::Path, pack: &std::path::Path) -> Self {
        let cert = root.join("h2-server-cert.pem");
        let key = root.join("h2-server-key.pem");
        let openssl = Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                key.to_str().expect("H2 key path"),
                "-out",
                cert.to_str().expect("H2 cert path"),
                "-days",
                "1",
                "-subj",
                "/CN=localhost",
                "-addext",
                "subjectAltName=IP:127.0.0.1",
            ])
            .output()
            .expect("generate H2 TLS certificate");
        assert!(
            openssl.status.success(),
            "H2 TLS certificate generation failed: {}",
            String::from_utf8_lossy(&openssl.stderr)
        );

        let port = unused_local_port();
        let ready = root.join("h2-server-ready");
        let marker = root.join("h2-server-marker");
        let stderr = root.join("h2-server-stderr");
        let child = Command::new("python3")
            .args([
                "-c",
                H2_TLS_SERVER_SCRIPT,
                &port.to_string(),
                cert.to_str().expect("H2 cert path"),
                key.to_str().expect("H2 key path"),
                pack.to_str().expect("H2 pack path"),
                ready.to_str().expect("H2 ready path"),
                marker.to_str().expect("H2 marker path"),
            ])
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(
                fs::File::create(&stderr).expect("create H2 server stderr log"),
            ))
            .spawn()
            .expect("spawn H2 TLS server");
        let mut server = Self {
            port,
            ready,
            marker,
            stderr,
            child: Some(child),
        };
        server.wait_ready();
        server
    }

    fn wait_ready(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if self.ready.is_file() {
                return;
            }
            if let Some(child) = self.child.as_mut() {
                if let Ok(Some(status)) = child.try_wait() {
                    panic!("H2 TLS server exited before readiness: {status}");
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "H2 TLS server did not become ready"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn wait_for_h2_request(&self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if fs::read_to_string(&self.marker).is_ok_and(|marker| marker == "h2\nrequest\n") {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "H2 TLS server did not observe an ALPN h2 request"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn stderr(&self) -> String {
        fs::read_to_string(&self.stderr).unwrap_or_default()
    }
}

struct StaticHttpServer {
    port: u16,
    request_headers: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl StaticHttpServer {
    fn new(root: std::path::PathBuf) -> Self {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind static server");
        let port = listener.local_addr().expect("local addr").port();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let request_headers = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let thread_stop = stop.clone();
        let thread_request_headers = request_headers.clone();
        let handle = std::thread::spawn(move || {
            loop {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let root = root.clone();
                let request_headers = thread_request_headers.clone();
                std::thread::spawn(move || {
                    serve_static_http_connection_with_recording(
                        &root,
                        &mut stream,
                        &request_headers,
                    )
                });
            }
        });
        Self {
            port,
            request_headers,
            stop,
            handle: Some(handle),
        }
    }

    fn request_headers_text(&self) -> Vec<String> {
        self.request_headers
            .lock()
            .expect("static HTTP request headers lock")
            .iter()
            .map(|headers| String::from_utf8_lossy(headers).into_owned())
            .collect()
    }
}

struct WritableHttpServer {
    root: TempDir,
    port: u16,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct SmartHttpServer {
    port: u16,
    upload_pack_requests: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    git_protocol_requests: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    upload_pack_bodies: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    request_headers: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct BackendHttpServer {
    port: u16,
    request_headers: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct TruncatedHttpServer {
    port: u16,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct ConflictingLengthHttpServer {
    port: u16,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct ChunkedHttpServer {
    port: u16,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct NonChunkedTransferEncodingHttpServer {
    port: u16,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct AuthorizationCaptureHttpServer {
    port: u16,
    request: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct OneShotRedirectHttpServer {
    port: u16,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct StockGitDaemon {
    child: std::process::Child,
}

struct ZminGitDaemon {
    child: std::process::Child,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SmartHttpResponseMutation {
    None,
    CorruptCommitPack(GitHashAlgorithm),
}

#[derive(Clone, Copy, Debug)]
enum FsckPromisorConfiguration {
    Ordinary,
    RemotePromisor,
    PartialCloneRemote,
    UnrelatedPromisor,
}

impl FsckPromisorConfiguration {
    fn label(self) -> &'static str {
        match self {
            Self::Ordinary => "ordinary",
            Self::RemotePromisor => "remote-promisor",
            Self::PartialCloneRemote => "partialclone-remote",
            Self::UnrelatedPromisor => "unrelated-promisor",
        }
    }
}

#[test]
fn zmin_daemon_emits_ready_to_rumble_before_accepting_connections() {
    let dir = TempDir::new().expect("daemon temp dir");
    let port = unused_local_port();
    let port_arg = format!("--port={port}");
    let base_path = format!("--base-path={}", dir.path().display());
    let mut child = Command::new(zmin_bin())
        .args([
            "daemon",
            "--verbose",
            "--listen=127.0.0.1",
            port_arg.as_str(),
            base_path.as_str(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zmin daemon");
    let stderr = child.stderr.take().expect("zmin daemon stderr");
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let result = std::io::BufReader::new(stderr).read_line(&mut line);
        let _ = ready_tx.send((result, line));
    });

    let ready = ready_rx.recv_timeout(std::time::Duration::from_secs(2));
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();

    let (read, line) = ready.expect("zmin daemon readiness line");
    read.expect("read zmin daemon readiness line");
    let line = line.trim_end_matches(['\r', '\n']);
    let pid = line
        .strip_prefix('[')
        .and_then(|line| line.strip_suffix("] Ready to rumble"));
    assert!(
        pid.is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit())),
        "unexpected daemon readiness line: {line:?}"
    );
}

#[test]
fn zmin_daemon_validates_numeric_options_like_stock_git() {
    let dir = TempDir::new().expect("daemon option temp dir");
    let invalid = [
        (
            "--init-timeout=3a",
            "fatal: invalid init-timeout '3a', expecting a non-negative integer",
        ),
        (
            "--init-timeout=-3",
            "fatal: invalid init-timeout '-3', expecting a non-negative integer",
        ),
        (
            "--timeout=3a",
            "fatal: invalid timeout '3a', expecting a non-negative integer",
        ),
        (
            "--timeout=-3",
            "fatal: invalid timeout '-3', expecting a non-negative integer",
        ),
        (
            "--max-connections=3a",
            "fatal: invalid max-connections '3a', expecting an integer",
        ),
    ];
    for (index, (option, expected_stderr)) in invalid.into_iter().enumerate() {
        let port = unused_local_port();
        let port_arg = format!("--port={port}");
        let pid_file = dir.path().join(format!("daemon-{index}.pid"));
        let pid_file_arg = format!("--pid-file={}", pid_file.display());
        let args = [
            "daemon",
            option,
            "--listen=127.0.0.1",
            port_arg.as_str(),
            pid_file_arg.as_str(),
        ];
        let (code, stdout, stderr) =
            command_any_output(zmin_bin(), dir.path(), &args, "zmin daemon invalid option");
        assert_eq!(code, 128, "{option}");
        assert!(stdout.is_empty(), "{option}: unexpected stdout: {stdout:?}");
        assert_eq!(stderr, expected_stderr, "{option}");
        assert!(!pid_file.exists(), "{option}: pid file was created");
        std::net::TcpListener::bind(("127.0.0.1", port))
            .unwrap_or_else(|error| panic!("{option}: port remained bound: {error}"));
    }

    for option in [
        "--timeout=0",
        "--timeout=4294967295",
        "--init-timeout=0",
        "--init-timeout=4294967295",
        "--max-connections=-1",
        "--max-connections=0",
        "--max-connections=2147483647",
    ] {
        let args = ["daemon", "--inetd", option];
        let (code, stdout, stderr) = command_any_output_with_stdin(
            zmin_bin(),
            dir.path(),
            &args,
            "",
            "zmin daemon valid boundary",
        );
        assert_eq!(code, 0, "{option}");
        assert!(stdout.is_empty(), "{option}: unexpected stdout: {stdout:?}");
        assert!(stderr.is_empty(), "{option}: unexpected stderr: {stderr:?}");
    }
}

fn write_fake_ssh(root: &std::path::Path) -> std::path::PathBuf {
    let script = root.join("fake-ssh.sh");
    fs::write(
        &script,
        r#"#!/bin/sh
set -eu
while [ "$#" -gt 0 ]; do
  case "$1" in
    -p|-l|-o|-F|-i|-J)
      shift 2
      ;;
    --)
      shift
      break
      ;;
    -*)
      shift
      ;;
    *)
      break
      ;;
  esac
done
if [ "$#" -lt 2 ]; then
  echo "fake ssh missing remote command" >&2
  exit 1
fi
shift
cmd="$*"
cmd="$(printf '%s\n' "$cmd" | sed -E "s#'/(.):#'\1:#g; s#\"/(.):#\"\1:#g; s# /(.:)# \1#g")"
tee -a '{}' | /bin/sh -c "$cmd"
"#,
    )
    .expect("write fake ssh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = fs::metadata(&script)
            .expect("fake ssh metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("chmod fake ssh");
    }
    script
}

fn write_logging_fake_ssh(root: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let script = root.join("fake-ssh-logging.sh");
    let log = root.join("fake-ssh-requests.log");
    fs::write(
        &script,
        format!(
            r#"#!/bin/sh
set -eu
while [ "$#" -gt 0 ]; do
  case "$1" in
    -p|-l|-o|-F|-i|-J)
      shift 2
      ;;
    --)
      shift
      break
      ;;
    -*)
      shift
      ;;
    *)
      break
      ;;
  esac
done
if [ "$#" -lt 2 ]; then
  echo "fake ssh missing remote command" >&2
  exit 1
fi
shift
cmd="$*"
cmd="$(printf '%s\n' "$cmd" | sed -E "s#'/(.):#'\1:#g; s#\"/(.):#\"\1:#g; s# /(.:)# \1#g")"
printf 'GIT_PROTOCOL=%s\n' "${{GIT_PROTOCOL-}}" >> '{}'
printf 'REMOTE_COMMAND=%s\n' "$cmd" >> '{}'
printf '%s\n' '--- request ---' >> '{}'
tee -a '{}' | /bin/sh -c "$cmd"
"#,
            log.display(),
            log.display(),
            log.display(),
            log.display(),
        ),
    )
    .expect("write logging fake ssh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = fs::metadata(&script)
            .expect("logging fake ssh metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("chmod logging fake ssh");
    }
    (script, log)
}

fn write_pinned_logging_fake_ssh(
    root: &std::path::Path,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let (script, log) = write_logging_fake_ssh(root);
    let pinned_git = required_pinned_stock_git()
        .display()
        .to_string()
        .replace('\'', "'\\''");
    let mut contents = fs::read_to_string(&script).expect("read logging fake ssh");
    let marker = "printf 'GIT_PROTOCOL=%s\\n'";
    let replacement = format!(
        "cmd=\"$(printf '%s\\n' \"$cmd\" | sed -E \"s#^git-upload-pack #'{pinned_git}' upload-pack #\")\"\n{marker}"
    );
    assert!(contents.contains(marker), "logging fake ssh marker missing");
    contents = contents.replacen(marker, &replacement, 1);
    fs::write(&script, contents).expect("pin logging fake ssh");
    (script, log)
}

fn write_fail_after_first_fake_ssh(
    root: &std::path::Path,
) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let script = root.join("fake-ssh-fail-hydration.sh");
    let log = root.join("fake-ssh-fail-hydration.log");
    let counter = root.join("fake-ssh-fail-hydration.count");
    fs::write(
        &script,
        format!(
            r#"#!/bin/sh
set -eu
while [ "$#" -gt 0 ]; do
  case "$1" in
    -p|-l|-o|-F|-i|-J)
      shift 2
      ;;
    --)
      shift
      break
      ;;
    -*)
      shift
      ;;
    *)
      break
      ;;
  esac
done
if [ "$#" -lt 2 ]; then
  echo "fake ssh missing remote command" >&2
  exit 1
fi
shift
cmd="$*"
cmd="$(printf '%s\n' "$cmd" | sed -E "s#'/(.):#'\1:#g; s#\"/(.):#\"\1:#g; s# /(.:)# \1#g")"
count=$(cat '{counter}' 2>/dev/null || printf '0')
count=$((count + 1))
printf '%s\n' "$count" > '{counter}'
printf 'GIT_PROTOCOL=%s\n' "${{GIT_PROTOCOL-}}" >> '{log}'
printf 'REMOTE_COMMAND=%s\n' "$cmd" >> '{log}'
printf '%s\n' '--- request ---' >> '{log}'
if [ "$count" -eq 2 ]; then
  sleep 0.1
  printf '0000'
  exit 0
fi
tee -a '{log}' | /bin/sh -c "$cmd"
"#,
            counter = counter.display(),
            log = log.display(),
        ),
    )
    .expect("write failing fake ssh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = fs::metadata(&script)
            .expect("failing fake ssh metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("chmod failing fake ssh");
    }
    (script, log, counter)
}

fn write_pinned_fail_after_first_fake_ssh(
    root: &std::path::Path,
) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let (script, log, counter) = write_fail_after_first_fake_ssh(root);
    let pinned_git = required_pinned_stock_git()
        .display()
        .to_string()
        .replace('\'', "'\\''");
    let mut contents = fs::read_to_string(&script).expect("read failing fake ssh");
    let marker = "count=$(cat ";
    let replacement = format!(
        "cmd=\"$(printf '%s\\n' \"$cmd\" | sed -E \"s#^git-upload-pack #'{pinned_git}' upload-pack #\")\"\n{marker}"
    );
    assert!(contents.contains(marker), "failing fake ssh marker missing");
    contents = contents.replacen(marker, &replacement, 1);
    fs::write(&script, contents).expect("pin failing fake ssh");
    (script, log, counter)
}

fn fake_ssh_command_arg(script: &std::path::Path) -> String {
    let path = script.display().to_string();
    #[cfg(windows)]
    {
        path.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        path
    }
}

fn run_logged_ssh_filtered_clone(
    root: &std::path::Path,
    remote: &std::path::Path,
    protocol_v2: bool,
    label: &str,
    expected_remote_commands: usize,
    expected_filter_specs: usize,
) -> (std::path::PathBuf, String) {
    let clone = root.join(label);
    let (fake_ssh, request_log) = write_pinned_logging_fake_ssh(root);
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let helper_path = pinned_git_helper_path(&pinned_http_bundle_root());
    let helper_path = helper_path
        .to_str()
        .expect("pinned Git helper path is UTF-8");
    let url = ssh_url_for_remote(remote);
    let mut args = vec!["clone".to_owned(), "-q".to_owned()];
    if protocol_v2 {
        args.push("--config=protocol.version=2".to_owned());
    }
    args.push("--filter=blob:none".to_owned());
    args.push(url);
    args.push(clone.display().to_string());
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = command_any_output_with_env(
        zmin_bin(),
        root,
        &args,
        &[
            ("GIT_SSH_COMMAND", fake_ssh_arg.as_str()),
            ("PATH", helper_path),
        ],
        label,
    );
    let log = fs::read_to_string(&request_log).expect("read SSH request log");
    assert_eq!(output.0, 0, "{label}: {output:?}; SSH log: {log}");
    assert_eq!(
        log.matches("REMOTE_COMMAND=").count(),
        expected_remote_commands,
        "{label}: remote command count"
    );
    assert_eq!(
        log.matches("filter blob:none").count(),
        expected_filter_specs,
        "{label}: filter request count"
    );
    if expected_remote_commands > 1 {
        assert!(
            log.matches("want ").count() > expected_remote_commands,
            "{label}: checkout hydration was not a multi-object batch"
        );
    }
    let roles = http_pack_role_snapshot(&clone);
    assert!(
        !roles.iter().any(|name| name.ends_with(".keep")),
        "{label}: owned keep survived successful checkout"
    );
    if expected_remote_commands > 1 {
        assert!(
            roles.iter().any(|name| name.ends_with(".promisor")),
            "{label}: no promisor marker"
        );
        let marker_contents = roles
            .iter()
            .filter(|name| name.ends_with(".promisor"))
            .map(|name| fs::read(clone.join(".git/objects/pack").join(name)).expect("read marker"))
            .collect::<Vec<_>>();
        assert!(
            marker_contents.iter().any(Vec::is_empty),
            "{label}: hydration promisor marker is missing"
        );
    }
    (clone, log)
}

fn write_upload_pack_wrapper(
    root: &std::path::Path,
    label: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let script = root.join(format!("upload-pack-{label}.sh"));
    let log = script.with_extension("sh.log");
    fs::write(
        &script,
        b"#!/bin/sh\nprintf 'invoked %s\\n' \"$*\" >> \"$0.log\"\nexec git-upload-pack \"$@\"\n",
    )
    .expect("write upload-pack wrapper");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = fs::metadata(&script)
            .expect("upload-pack wrapper metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("chmod upload-pack wrapper");
    }
    (script, log)
}

fn set_bare_head_to_main(remote: &std::path::Path) {
    git(remote, ["symbolic-ref", "HEAD", "refs/heads/main"]);
}

fn set_bare_head_to_main_pinned(remote: &std::path::Path) {
    pinned_git_args(remote, ["symbolic-ref", "HEAD", "refs/heads/main"]);
}

fn ssh_url_for_remote(remote: &std::path::Path) -> String {
    let path = remote.display().to_string();
    #[cfg(windows)]
    {
        format!("ssh://example.test/{}", path.replace('\\', "/"))
    }
    #[cfg(not(windows))]
    {
        format!("ssh://example.test{path}")
    }
}

fn scp_url_for_remote(remote: &std::path::Path) -> String {
    let path = remote.display().to_string();
    #[cfg(windows)]
    {
        format!("example.test:{}", path.replace('\\', "/"))
    }
    #[cfg(not(windows))]
    {
        format!("example.test:{path}")
    }
}

fn git_object_exists(repo: &std::path::Path, object: &str) -> bool {
    Command::new(stock_git_bin())
        .args(["cat-file", "-e", object])
        .current_dir(repo)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn assert_matching_shallow_state(
    zmin_repo: &std::path::Path,
    git_repo: &std::path::Path,
    missing_object: &str,
) {
    assert_eq!(
        git(zmin_repo, ["rev-parse", "--is-shallow-repository"]),
        git(git_repo, ["rev-parse", "--is-shallow-repository"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.join(".git/shallow")).expect("read zmin shallow"),
        fs::read_to_string(git_repo.join(".git/shallow")).expect("read git shallow")
    );
    assert_eq!(
        git_object_exists(zmin_repo, missing_object),
        git_object_exists(git_repo, missing_object)
    );
}

fn assert_matching_shallow_state_for_missing_objects(
    zmin_repo: &std::path::Path,
    git_repo: &std::path::Path,
    missing_objects: &[String],
) {
    assert_eq!(
        git(zmin_repo, ["rev-parse", "--is-shallow-repository"]),
        git(git_repo, ["rev-parse", "--is-shallow-repository"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.join(".git/shallow")).expect("read zmin shallow"),
        fs::read_to_string(git_repo.join(".git/shallow")).expect("read git shallow")
    );
    for missing_object in missing_objects {
        assert_eq!(
            git_object_exists(zmin_repo, missing_object),
            git_object_exists(git_repo, missing_object),
            "object presence differs for {missing_object}"
        );
    }
}

fn prepare_two_branch_shallow_remote(
    root: &std::path::Path,
) -> (std::path::PathBuf, String, String) {
    let remote = root.join("remote.git");
    let work = root.join("work");
    git(root, ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(root, ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("main.txt"), b"main base\n").expect("write main base");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "main base"]);
    let main_parent = git(&work, ["rev-parse", "HEAD"]);
    fs::write(work.join("main.txt"), b"main tip\n").expect("write main tip");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "main tip"]);
    git(&work, ["switch", "-c", "feature", &main_parent]);
    fs::write(work.join("feature.txt"), b"feature base\n").expect("write feature base");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature base"]);
    let feature_parent = git(&work, ["rev-parse", "HEAD"]);
    fs::write(work.join("feature.txt"), b"feature tip\n").expect("write feature tip");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature tip"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature"]);
    set_bare_head_to_main(&remote);
    (remote, main_parent, feature_parent)
}

fn prepare_shallow_since_remote(root: &std::path::Path) -> std::path::PathBuf {
    let remote = root.join("remote.git");
    let work = root.join("work");
    git(root, ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(root, ["init", "-b", "main", "work"]);
    pinned_git_args(&work, ["config", "user.name", "Bench"]);
    pinned_git_args(&work, ["config", "user.email", "bench@example.test"]);
    pinned_git_args(&work, ["config", "commit.gpgsign", "false"]);
    for idx in 1..=4 {
        fs::write(work.join("file.txt"), format!("commit {idx}\n")).expect("write source file");
        git(&work, ["add", "-A"]);
        let date = format!("2020-01-0{idx}T00:00:00 +0000");
        command_output_with_env(
            "git",
            &work,
            &["commit", "-m", &format!("commit {idx}")],
            &[
                ("GIT_AUTHOR_DATE", date.as_str()),
                ("GIT_COMMITTER_DATE", date.as_str()),
            ],
            "git commit",
        );
    }
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);
    remote
}

fn prepare_two_branch_shallow_since_remote(root: &std::path::Path) -> std::path::PathBuf {
    let remote = root.join("remote.git");
    let work = root.join("work");
    git(root, ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(root, ["init", "-b", "main", "work"]);
    configure_identity(&work);

    fs::write(work.join("main.txt"), b"base\n").expect("write base");
    git(&work, ["add", "-A"]);
    command_output_with_env(
        "git",
        &work,
        &["commit", "-m", "base"],
        &[
            ("GIT_AUTHOR_DATE", "2020-01-01T00:00:00 +0000"),
            ("GIT_COMMITTER_DATE", "2020-01-01T00:00:00 +0000"),
        ],
        "git commit base",
    );
    let base = git(&work, ["rev-parse", "HEAD"]);

    fs::write(work.join("main.txt"), b"main tip\n").expect("write main tip");
    git(&work, ["add", "-A"]);
    command_output_with_env(
        "git",
        &work,
        &["commit", "-m", "main tip"],
        &[
            ("GIT_AUTHOR_DATE", "2020-01-04T00:00:00 +0000"),
            ("GIT_COMMITTER_DATE", "2020-01-04T00:00:00 +0000"),
        ],
        "git commit main tip",
    );

    git(&work, ["switch", "-c", "feature", &base]);
    fs::write(work.join("feature.txt"), b"feature tip\n").expect("write feature tip");
    git(&work, ["add", "-A"]);
    command_output_with_env(
        "git",
        &work,
        &["commit", "-m", "feature tip"],
        &[
            ("GIT_AUTHOR_DATE", "2020-01-04T00:00:00 +0000"),
            ("GIT_COMMITTER_DATE", "2020-01-04T00:00:00 +0000"),
        ],
        "git commit feature tip",
    );

    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature"]);
    set_bare_head_to_main(&remote);
    remote
}

fn prepare_shallow_exclude_remote(root: &std::path::Path) -> std::path::PathBuf {
    let remote = root.join("remote.git");
    let work = root.join("work");
    git(root, ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(root, ["init", "-b", "main", "work"]);
    configure_identity(&work);
    for name in ["base 1", "base 2"] {
        fs::write(work.join("file.txt"), format!("{name}\n")).expect("write source file");
        git(&work, ["add", "-A"]);
        git_with_env(&work, ["commit", "-m", name]);
    }
    git(&work, ["branch", "base"]);
    for name in ["main 1", "main 2"] {
        fs::write(work.join("file.txt"), format!("{name}\n")).expect("write source file");
        git(&work, ["add", "-A"]);
        git_with_env(&work, ["commit", "-m", name]);
    }
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "base"]);
    set_bare_head_to_main(&remote);
    remote
}

fn prepare_repeated_shallow_exclude_remote(
    root: &std::path::Path,
) -> (std::path::PathBuf, String, String) {
    let remote = root.join("remote.git");
    let work = root.join("work");
    git(root, ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(root, ["init", "-b", "main", "work"]);
    configure_identity(&work);

    fs::write(work.join("root.txt"), "root\n").expect("write root");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "root"]);
    git(&work, ["checkout", "-b", "left"]);
    fs::write(work.join("left.txt"), "left\n").expect("write left");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "left"]);
    let left_tip = git(&work, ["rev-parse", "left"]);
    git(&work, ["checkout", "main"]);
    git(&work, ["checkout", "-b", "right"]);
    fs::write(work.join("right.txt"), "right\n").expect("write right");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "right"]);
    let right_tip = git(&work, ["rev-parse", "right"]);
    git(&work, ["checkout", "main"]);
    git(&work, ["merge", "--no-ff", "left", "-m", "merge left"]);
    git(&work, ["merge", "--no-ff", "right", "-m", "merge right"]);

    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "left", "right"]);
    set_bare_head_to_main(&remote);
    (remote, left_tip, right_tip)
}

fn prepare_two_branch_shallow_exclude_remote(root: &std::path::Path) -> std::path::PathBuf {
    let remote = root.join("remote.git");
    let work = root.join("work");
    git(root, ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(root, ["init", "-b", "main", "work"]);
    configure_identity(&work);

    for name in ["base 1", "base 2"] {
        fs::write(work.join("base.txt"), format!("{name}\n")).expect("write base file");
        git(&work, ["add", "-A"]);
        git_with_env(&work, ["commit", "-m", name]);
    }
    git(&work, ["branch", "base"]);
    let base = git(&work, ["rev-parse", "HEAD"]);

    fs::write(work.join("main.txt"), b"main tip\n").expect("write main tip");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "main tip"]);

    git(&work, ["switch", "-c", "feature", &base]);
    fs::write(work.join("feature.txt"), b"feature tip\n").expect("write feature tip");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature tip"]);

    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "base"]);
    set_bare_head_to_main(&remote);
    remote
}

fn prepare_update_shallow_remote(root: &std::path::Path) -> std::path::PathBuf {
    let source = root.join("source");
    let remote = root.join("shallow.git");
    git(root, ["init", "-b", "main", "source"]);
    configure_identity(&source);
    for idx in 1..=4 {
        fs::write(source.join("file.txt"), format!("commit {idx}\n")).expect("write source file");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", &format!("commit {idx}")]);
    }
    let source_url = format!("file://{}", source.display());
    git(
        root,
        [
            "clone",
            "--bare",
            "--depth=2",
            &source_url,
            remote.to_str().expect("remote path"),
        ],
    );
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    set_bare_head_to_main(&remote);
    remote
}

fn prepare_two_branch_update_shallow_remote(root: &std::path::Path) -> std::path::PathBuf {
    let source = root.join("source-two-branch");
    let remote = root.join("shallow-two-branch.git");
    git(root, ["init", "-b", "main", "source-two-branch"]);
    configure_identity(&source);
    for idx in 1..=4 {
        fs::write(source.join("main.txt"), format!("main {idx}\n")).expect("write main");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", &format!("main {idx}")]);
    }
    git(&source, ["switch", "-c", "feature", "HEAD~2"]);
    for idx in 1..=3 {
        fs::write(source.join("feature.txt"), format!("feature {idx}\n")).expect("write feature");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", &format!("feature {idx}")]);
    }
    git(&source, ["switch", "main"]);
    let source_url = format!("file://{}", source.display());
    git(
        root,
        [
            "clone",
            "--bare",
            "--depth=2",
            "--no-single-branch",
            &source_url,
            remote.to_str().expect("remote path"),
        ],
    );
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    set_bare_head_to_main(&remote);
    remote
}

fn init_network_fetch_clients(
    root: &std::path::Path,
    label: &str,
    url: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let git_client = root.join(format!("git-client-{label}"));
    let zmin_client = root.join(format!("zmin-client-{label}"));
    for client in [&git_client, &zmin_client] {
        git(root, ["init", client.to_str().expect("client path")]);
        git(client, ["remote", "add", "origin", url]);
    }
    (git_client, zmin_client)
}

fn init_pinned_network_fetch_clients(
    root: &std::path::Path,
    label: &str,
    url: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let git_client = root.join(format!("git-client-{label}"));
    let zmin_client = root.join(format!("zmin-client-{label}"));
    for client in [&git_client, &zmin_client] {
        pinned_git_args(root, ["init", client.to_str().expect("client path")]);
        pinned_git_args(client, ["remote", "add", "origin", url]);
    }
    (git_client, zmin_client)
}

fn assert_network_branch_shallow_fetch_matches_stock_git(
    label: &str,
    git_client: &std::path::Path,
    zmin_client: &std::path::Path,
) {
    assert_eq!(
        git(zmin_client, ["show-ref"]),
        git(git_client, ["show-ref"]),
        "{label}"
    );
    assert_eq!(
        git(zmin_client, ["rev-parse", "--is-shallow-repository"]),
        git(git_client, ["rev-parse", "--is-shallow-repository"]),
        "{label}"
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/shallow")).expect("zmin shallow"),
        fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow"),
        "{label}"
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
        "{label}"
    );
    assert_eq!(
        git(zmin_client, ["rev-list", "--count", "origin/main"]),
        git(git_client, ["rev-list", "--count", "origin/main"]),
        "{label}"
    );
}

fn assert_network_branch_unshallow_fetch_matches_stock_git(
    label: &str,
    git_client: &std::path::Path,
    zmin_client: &std::path::Path,
) {
    assert_eq!(
        git(zmin_client, ["show-ref"]),
        git(git_client, ["show-ref"]),
        "{label}"
    );
    assert_eq!(
        git(zmin_client, ["rev-parse", "--is-shallow-repository"]),
        git(git_client, ["rev-parse", "--is-shallow-repository"]),
        "{label}"
    );
    assert_eq!(
        zmin_client.join(".git/shallow").exists(),
        git_client.join(".git/shallow").exists(),
        "{label}"
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
        "{label}"
    );
    assert_eq!(
        git(zmin_client, ["rev-list", "--count", "origin/main"]),
        git(git_client, ["rev-list", "--count", "origin/main"]),
        "{label}"
    );
}

fn assert_no_alternates(repo: &std::path::Path) {
    assert!(
        !repo.join(".git/objects/info/alternates").exists(),
        "unexpected alternates file in {}",
        repo.display()
    );
}

fn canonical_alternates(path: &std::path::Path) -> Vec<std::path::PathBuf> {
    fs::read_to_string(path)
        .expect("read alternates")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(std::path::PathBuf::from)
        .map(|path| fs::canonicalize(&path).unwrap_or(path))
        .collect()
}

impl WritableHttpServer {
    fn new() -> Self {
        let root = TempDir::new().expect("writable http root");
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind writable http");
        let port = listener.local_addr().expect("local addr").port();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread_root = root.path().to_path_buf();
        let handle = std::thread::spawn(move || {
            loop {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let root = thread_root.clone();
                std::thread::spawn(move || serve_writable_http_connection(&root, &mut stream));
            }
        });
        Self {
            root,
            port,
            stop,
            handle: Some(handle),
        }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/remote.git", self.port)
    }

    fn remote_git_dir(&self) -> std::path::PathBuf {
        self.root.path().join("remote.git")
    }
}

impl SmartHttpServer {
    fn new(project_root: std::path::PathBuf) -> Self {
        Self::with_response_mutation(project_root, true, SmartHttpResponseMutation::None)
    }

    fn bitbucket_style(project_root: std::path::PathBuf) -> Self {
        Self::with_response_mutation(project_root, false, SmartHttpResponseMutation::None)
    }

    fn corrupt_commit_pack(project_root: std::path::PathBuf, algorithm: GitHashAlgorithm) -> Self {
        Self::with_response_mutation(
            project_root,
            true,
            SmartHttpResponseMutation::CorruptCommitPack(algorithm),
        )
    }

    fn with_response_mutation(
        project_root: std::path::PathBuf,
        service_newline: bool,
        response_mutation: SmartHttpResponseMutation,
    ) -> Self {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind smart http");
        let port = listener.local_addr().expect("local addr").port();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let upload_pack_requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let git_protocol_requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let upload_pack_bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let request_headers = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let thread_stop = stop.clone();
        let thread_upload_pack_requests = upload_pack_requests.clone();
        let thread_git_protocol_requests = git_protocol_requests.clone();
        let thread_upload_pack_bodies = upload_pack_bodies.clone();
        let thread_request_headers = request_headers.clone();
        let thread_root = project_root;
        let handle = std::thread::spawn(move || {
            loop {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let root = thread_root.clone();
                let upload_pack_requests = thread_upload_pack_requests.clone();
                let git_protocol_requests = thread_git_protocol_requests.clone();
                let upload_pack_bodies = thread_upload_pack_bodies.clone();
                let request_headers = thread_request_headers.clone();
                std::thread::spawn(move || {
                    serve_smart_http_connection(
                        &root,
                        service_newline,
                        &upload_pack_requests,
                        &git_protocol_requests,
                        &upload_pack_bodies,
                        &request_headers,
                        response_mutation,
                        &mut stream,
                    )
                });
            }
        });
        Self {
            port,
            upload_pack_requests,
            git_protocol_requests,
            upload_pack_bodies,
            request_headers,
            stop,
            handle: Some(handle),
        }
    }

    fn upload_pack_requests(&self) -> usize {
        self.upload_pack_requests
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn git_protocol_requests(&self) -> usize {
        self.git_protocol_requests
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn upload_pack_bodies_text(&self) -> Vec<String> {
        self.upload_pack_bodies
            .lock()
            .expect("upload-pack bodies lock")
            .iter()
            .map(|body| String::from_utf8_lossy(body).into_owned())
            .collect()
    }

    fn request_headers_text(&self) -> Vec<String> {
        self.request_headers
            .lock()
            .expect("request headers lock")
            .iter()
            .map(|headers| String::from_utf8_lossy(headers).into_owned())
            .collect()
    }
}

impl BackendHttpServer {
    fn new(command: String, project_root: std::path::PathBuf) -> Self {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind backend http");
        let port = listener.local_addr().expect("local addr").port();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let request_headers = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let thread_stop = stop.clone();
        let thread_request_headers = request_headers.clone();
        let handle = std::thread::spawn(move || {
            loop {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let root = project_root.clone();
                let command = command.clone();
                let request_headers = thread_request_headers.clone();
                std::thread::spawn(move || {
                    serve_backend_http_connection(&command, &root, &request_headers, &mut stream)
                });
            }
        });
        Self {
            port,
            request_headers,
            stop,
            handle: Some(handle),
        }
    }

    fn request_headers_text(&self) -> Vec<String> {
        self.request_headers
            .lock()
            .expect("backend request headers lock")
            .iter()
            .map(|headers| String::from_utf8_lossy(headers).into_owned())
            .collect()
    }
}

impl TruncatedHttpServer {
    fn new() -> Self {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind truncated http");
        let port = listener.local_addr().expect("local addr").port();
        let handle = std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = read_http_request_headers(&mut stream);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nshort",
            );
        });
        Self {
            port,
            handle: Some(handle),
        }
    }
}

impl ConflictingLengthHttpServer {
    fn new() -> Self {
        let listener =
            std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind conflicting length http");
        let port = listener.local_addr().expect("local addr").port();
        let handle = std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = read_http_request_headers(&mut stream);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Length: 6\r\nConnection: close\r\n\r\nshort",
            );
        });
        Self {
            port,
            handle: Some(handle),
        }
    }
}

impl ChunkedHttpServer {
    fn new() -> Self {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind chunked http");
        let port = listener.local_addr().expect("local addr").port();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_stop = stop.clone();
        let handle = std::thread::spawn(move || {
            while !thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                if thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                let _ = read_http_request_headers(&mut stream);
                let body = b"1111111111111111111111111111111111111111\trefs/heads/main\n";
                let header =
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(header);
                let _ = write!(stream, "{:x}\r\n", body.len());
                let _ = stream.write_all(body);
                let _ = stream.write_all(b"\r\n0\r\n\r\n");
                let _ = stream.flush();
                let _ = stream.shutdown(std::net::Shutdown::Write);
            }
        });
        Self {
            port,
            stop,
            handle: Some(handle),
        }
    }
}

impl NonChunkedTransferEncodingHttpServer {
    fn new() -> Self {
        let listener =
            std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind transfer-encoding http");
        let port = listener.local_addr().expect("local addr").port();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_stop = stop.clone();
        let handle = std::thread::spawn(move || {
            while !thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                if thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                let _ = read_http_request_headers(&mut stream);
                let body = b"1111111111111111111111111111111111111111\trefs/heads/main\n";
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\nConnection: close\r\n\r\n",
                );
                let _ = stream.write_all(body);
                let _ = stream.flush();
                let _ = stream.shutdown(std::net::Shutdown::Write);
            }
        });
        Self {
            port,
            stop,
            handle: Some(handle),
        }
    }
}

impl AuthorizationCaptureHttpServer {
    fn new() -> Self {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind auth http");
        let port = listener.local_addr().expect("local addr").port();
        let request = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let thread_request = request.clone();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                if let Ok(headers) = read_http_request_headers(&mut stream) {
                    *thread_request.lock().expect("request lock") = headers;
                }
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            }
        });
        Self {
            port,
            request,
            handle: Some(handle),
        }
    }

    fn request_text(&self) -> String {
        String::from_utf8_lossy(&self.request.lock().expect("request lock")).into_owned()
    }
}

impl OneShotRedirectHttpServer {
    fn new(target_base: String) -> Self {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind redirect http");
        let port = listener.local_addr().expect("local addr").port();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let request = read_http_request_headers(&mut stream).unwrap_or_default();
                let request = String::from_utf8_lossy(&request);
                let raw_path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_ascii_whitespace().nth(1))
                    .unwrap_or("/");
                let location = format!("{target_base}{raw_path}");
                let response = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            port,
            handle: Some(handle),
        }
    }
}

impl Drop for StaticHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(unix)]
impl Drop for H2TlsServer {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for WritableHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SmartHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for BackendHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for TruncatedHttpServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ConflictingLengthHttpServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ChunkedHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for NonChunkedTransferEncodingHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AuthorizationCaptureHttpServer {
    fn drop(&mut self) {
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for OneShotRedirectHttpServer {
    fn drop(&mut self) {
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl StockGitDaemon {
    fn spawn(root: &std::path::Path, port: u16) -> Self {
        Self::spawn_with_args(root, port, &[])
    }

    fn spawn_pinned(root: &std::path::Path, port: u16) -> Self {
        Self::spawn_pinned_with_args_and_env(root, port, &[], &[])
    }

    fn spawn_with_args(root: &std::path::Path, port: u16, extra_args: &[&str]) -> Self {
        Self::spawn_with_args_and_env(root, port, extra_args, &[])
    }

    fn spawn_with_args_and_env(
        root: &std::path::Path,
        port: u16,
        extra_args: &[&str],
        envs: &[(&str, &str)],
    ) -> Self {
        Self::spawn_with_binary(root, port, extra_args, envs, stock_git_bin())
    }

    fn spawn_pinned_with_args_and_env(
        root: &std::path::Path,
        port: u16,
        extra_args: &[&str],
        envs: &[(&str, &str)],
    ) -> Self {
        let pinned_git = required_pinned_stock_git();
        let helper_path = pinned_git_helper_path(&pinned_http_bundle_root());
        let helper_path = helper_path
            .to_str()
            .expect("pinned Git helper path is UTF-8");
        let exec_path = pinned_stock_git_exec_path();
        let exec_path = exec_path.to_str().expect("pinned Git exec path is UTF-8");
        let mut pinned_envs = vec![
            ("PATH", helper_path),
            ("GIT_EXEC_PATH", exec_path),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_SYSTEM", "/dev/null"),
        ];
        pinned_envs.extend_from_slice(envs);
        Self::spawn_with_binary(root, port, extra_args, &pinned_envs, &pinned_git)
    }

    fn spawn_with_binary(
        root: &std::path::Path,
        port: u16,
        extra_args: &[&str],
        envs: &[(&str, &str)],
        git_binary: &std::path::Path,
    ) -> Self {
        let port_arg = format!("--port={port}");
        let base_path = format!("--base-path={}", root.display());
        let mut command = Command::new(git_binary);
        command.args([
            "daemon",
            "--export-all",
            "--listen=127.0.0.1",
            port_arg.as_str(),
            base_path.as_str(),
        ]);
        command.args(extra_args);
        for (key, value) in envs {
            command.env(key, value);
        }
        let child = command
            .arg(root.to_str().expect("root path"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn git daemon");
        wait_for_tcp_port(port);
        Self { child }
    }
}

impl Drop for StockGitDaemon {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &self.child.id().to_string(), "/T", "/F"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl ZminGitDaemon {
    fn spawn(root: &std::path::Path, port: u16) -> Self {
        let port_arg = format!("--port={port}");
        let base_path = format!("--base-path={}", root.display());
        let child = Command::new(zmin_bin())
            .args([
                "daemon",
                "--export-all",
                "--listen=127.0.0.1",
                port_arg.as_str(),
                base_path.as_str(),
            ])
            .arg(root.to_str().expect("root path"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn zmin git daemon");
        wait_for_tcp_port(port);
        Self { child }
    }
}

impl Drop for ZminGitDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn serve_static_http_connection_with_recording(
    root: &std::path::Path,
    stream: &mut std::net::TcpStream,
    request_headers: &std::sync::Mutex<Vec<Vec<u8>>>,
) {
    let mut request = Vec::new();
    let mut buf = [0_u8; 1024];
    loop {
        let Ok(read) = stream.read(&mut buf) else {
            return;
        };
        if read == 0 {
            write_static_http_response(stream, "400 Bad Request", &[]);
            return;
        }
        request.extend_from_slice(&buf[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    request_headers
        .lock()
        .expect("static HTTP request headers lock")
        .push(request.clone());
    let line = String::from_utf8_lossy(&request)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    if method != "GET" {
        write_static_http_response(stream, "405 Method Not Allowed", &[]);
        return;
    }
    let path = path
        .split_once('?')
        .map(|(path, _)| path)
        .unwrap_or(path)
        .trim_start_matches('/');
    if path.split('/').any(|component| component == "..") {
        write_static_http_response(stream, "400 Bad Request", &[]);
        return;
    }
    match fs::read(root.join(path)) {
        Ok(body) => {
            write_static_http_response(stream, "200 OK", &body);
        }
        Err(_) => {
            write_static_http_response(stream, "404 Not Found", &[]);
        }
    }
}

fn serve_smart_http_connection(
    project_root: &std::path::Path,
    service_newline: bool,
    upload_pack_requests: &std::sync::atomic::AtomicUsize,
    git_protocol_requests: &std::sync::atomic::AtomicUsize,
    upload_pack_bodies: &std::sync::Mutex<Vec<Vec<u8>>>,
    request_headers: &std::sync::Mutex<Vec<Vec<u8>>>,
    response_mutation: SmartHttpResponseMutation,
    stream: &mut std::net::TcpStream,
) {
    let mut request = Vec::new();
    let mut buf = [0_u8; 1024];
    let header_end = loop {
        let Ok(read) = stream.read(&mut buf) else {
            return;
        };
        if read == 0 {
            write_static_http_response(stream, "400 Bad Request", &[]);
            return;
        }
        request.extend_from_slice(&buf[..read]);
        if let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break header_end;
        }
    };
    request_headers
        .lock()
        .expect("request headers lock")
        .push(request[..header_end].to_vec());
    let headers = String::from_utf8_lossy(&request[..header_end]).to_string();
    let mut lines = headers.lines();
    let line = lines.next().unwrap_or_default().to_owned();
    let mut content_len = 0_usize;
    let mut git_protocol = None::<String>;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_len = value.trim().parse::<usize>().unwrap_or(0);
        } else if name.eq_ignore_ascii_case("git-protocol") {
            git_protocol = Some(value.trim().to_owned());
        }
    }
    let mut body = request[header_end + 4..].to_vec();
    while body.len() < content_len {
        let read = stream.read(&mut buf).expect("read smart body");
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buf[..read]);
    }
    body.truncate(content_len);
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let raw_path = parts.next().unwrap_or_default();
    let (path, query) = raw_path.split_once('?').unwrap_or((raw_path, ""));
    if git_protocol.is_some() {
        git_protocol_requests.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    if method == "POST" && path.ends_with("/git-upload-pack") {
        upload_pack_requests.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        upload_pack_bodies
            .lock()
            .expect("upload-pack bodies lock")
            .push(body.clone());
    }
    let mut backend = http_backend_response_with_body_and_protocol(
        "git",
        project_root,
        path,
        query,
        method,
        &body,
        git_protocol.as_deref(),
    );
    if method == "POST"
        && path.ends_with("/git-upload-pack")
        && response_mutation != SmartHttpResponseMutation::None
    {
        backend = mutate_smart_http_response(backend, response_mutation);
    }
    if !service_newline
        && method == "GET"
        && path.ends_with("/info/refs")
        && let Some(position) = backend
            .windows(b"001e# service=git-upload-pack\n".len())
            .position(|window| window == b"001e# service=git-upload-pack\n")
    {
        backend.splice(
            position..position + b"001e# service=git-upload-pack\n".len(),
            b"001d# service=git-upload-pack".iter().copied(),
        );
    }
    write_backend_http_response(stream, &backend);
}

fn mutate_smart_http_response(response: Vec<u8>, mutation: SmartHttpResponseMutation) -> Vec<u8> {
    let SmartHttpResponseMutation::CorruptCommitPack(algorithm) = mutation else {
        return response;
    };
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("semantic corruption response headers");
    let body = &response[header_end + 4..];
    let mut cursor = 0_usize;
    let mut control_end = None;
    let mut pack = Vec::new();
    while cursor < body.len() {
        let frame_start = cursor;
        let Some(line) = read_pkt_line(body, &mut cursor) else {
            continue;
        };
        if line.first() == Some(&1) {
            control_end.get_or_insert(frame_start);
            pack.extend_from_slice(&line[1..]);
        }
    }
    let Some(control_end) = control_end else {
        return response;
    };
    let pack = corrupt_undeltified_commit_pack(&pack, algorithm);
    let mut new_body = body[..control_end].to_vec();
    const SIDEBAND_CHUNK_SIZE: usize = 65_530;
    for chunk in pack.chunks(SIDEBAND_CHUNK_SIZE) {
        let mut payload = Vec::with_capacity(chunk.len() + 1);
        payload.push(1);
        payload.extend_from_slice(chunk);
        new_body.extend_from_slice(&pkt_line_bytes(&payload));
    }
    new_body.extend_from_slice(b"0000");
    replace_http_response_body(&response, &new_body)
}

fn replace_http_response_body(response: &[u8], body: &[u8]) -> Vec<u8> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("response headers");
    let mut result = Vec::with_capacity(header_end + 4 + body.len());
    for raw_line in response[..header_end].split(|byte| *byte == b'\n') {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        let Some(colon) = line.iter().position(|byte| *byte == b':') else {
            result.extend_from_slice(line);
            result.extend_from_slice(b"\r\n");
            continue;
        };
        let name = &line[..colon];
        if name.eq_ignore_ascii_case(b"Content-Length") {
            result.extend_from_slice(b"Content-Length: ");
            result.extend_from_slice(body.len().to_string().as_bytes());
        } else {
            result.extend_from_slice(line);
        }
        result.extend_from_slice(b"\r\n");
    }
    result.extend_from_slice(b"\r\n");
    result.extend_from_slice(body);
    result
}

fn corrupt_undeltified_commit_pack(pack: &[u8], algorithm: GitHashAlgorithm) -> Vec<u8> {
    let trailer_len = algorithm.digest_len();
    assert!(
        pack.len() > 12 + trailer_len,
        "semantic corruption pack too small"
    );
    assert_eq!(&pack[..4], b"PACK", "semantic corruption pack signature");
    let object_count = u32::from_be_bytes(pack[8..12].try_into().expect("pack count"));
    let body_end = pack.len() - trailer_len;
    let mut cursor = 12_usize;
    let mut rewritten = pack[..12].to_vec();
    let mut changed = false;
    for _ in 0..object_count {
        let entry_start = cursor;
        let first = *pack.get(cursor).expect("pack object header");
        cursor += 1;
        let object_type = (first >> 4) & 0x07;
        while pack.get(cursor - 1).is_some_and(|byte| byte & 0x80 != 0) {
            cursor += 1;
            assert!(cursor < body_end, "pack object header overflow");
        }
        let compressed_start = cursor;
        let mut decoder = ZlibDecoder::new(&pack[compressed_start..body_end]);
        let mut content = Vec::new();
        decoder
            .read_to_end(&mut content)
            .expect("decode semantic corruption pack object");
        let compressed_len = decoder.total_in() as usize;
        assert!(
            compressed_len > 0,
            "semantic corruption object has no zlib data"
        );
        cursor = compressed_start + compressed_len;
        if object_type != 1 || changed {
            rewritten.extend_from_slice(&pack[entry_start..cursor]);
            continue;
        }
        let Some(tree_header) = content
            .windows(b"tree ".len())
            .position(|window| window == b"tree ")
        else {
            rewritten.extend_from_slice(&pack[entry_start..cursor]);
            continue;
        };
        let mut corrupted = content;
        *corrupted.get_mut(tree_header).expect("commit tree header") = b'x';
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&corrupted)
            .expect("encode semantic corruption commit");
        let compressed = encoder.finish().expect("finish semantic corruption commit");
        rewritten.extend_from_slice(&pack[entry_start..compressed_start]);
        rewritten.extend_from_slice(&compressed);
        changed = true;
    }
    assert_eq!(cursor, body_end, "semantic corruption pack parse boundary");
    assert!(
        changed,
        "semantic corruption fixture did not find a commit object"
    );
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update(&rewritten);
    rewritten.extend_from_slice(hasher.finalize().as_bytes());
    rewritten
}

fn serve_backend_http_connection(
    command: &str,
    project_root: &std::path::Path,
    request_headers: &std::sync::Mutex<Vec<Vec<u8>>>,
    stream: &mut std::net::TcpStream,
) {
    let mut request = Vec::new();
    let mut buf = [0_u8; 1024];
    let header_end = loop {
        let Ok(read) = stream.read(&mut buf) else {
            return;
        };
        if read == 0 {
            write_static_http_response(stream, "400 Bad Request", &[]);
            return;
        }
        request.extend_from_slice(&buf[..read]);
        if let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break header_end;
        }
    };
    request_headers
        .lock()
        .expect("backend request headers lock")
        .push(request[..header_end].to_vec());
    let headers = String::from_utf8_lossy(&request[..header_end]).to_string();
    let mut lines = headers.lines();
    let line = lines.next().unwrap_or_default().to_owned();
    let mut content_len = 0_usize;
    let mut git_protocol = None::<String>;
    for header in lines {
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_len = value.trim().parse::<usize>().unwrap_or(0);
        } else if name.eq_ignore_ascii_case("git-protocol") {
            git_protocol = Some(value.trim().to_owned());
        }
    }
    let mut body = request[header_end + 4..].to_vec();
    while body.len() < content_len {
        let read = stream.read(&mut buf).expect("read backend body");
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buf[..read]);
    }
    body.truncate(content_len);
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let raw_path = parts.next().unwrap_or_default();
    let (path, query) = raw_path.split_once('?').unwrap_or((raw_path, ""));
    let backend = http_backend_response_with_body_and_protocol(
        command,
        project_root,
        path,
        query,
        method,
        &body,
        git_protocol.as_deref(),
    );
    write_backend_http_response(stream, &backend);
}

fn serve_writable_http_connection(root: &std::path::Path, stream: &mut std::net::TcpStream) {
    let mut request = Vec::new();
    let mut buf = [0_u8; 1024];
    let header_end = loop {
        let Ok(read) = stream.read(&mut buf) else {
            return;
        };
        if read == 0 {
            write_static_http_response(stream, "400 Bad Request", &[]);
            return;
        }
        request.extend_from_slice(&buf[..read]);
        if let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break header_end;
        }
    };
    let headers = String::from_utf8_lossy(&request[..header_end]).to_string();
    let mut lines = headers.lines();
    let request_line = lines.next().unwrap_or_default().to_owned();
    let content_len = lines
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let mut body = request[header_end + 4..].to_vec();
    while body.len() < content_len {
        let read = stream.read(&mut buf).expect("read writable body");
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buf[..read]);
    }
    body.truncate(content_len);
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let path = path
        .split_once('?')
        .map(|(path, _)| path)
        .unwrap_or(path)
        .trim_start_matches('/');
    if path.split('/').any(|component| component == "..") {
        write_static_http_response(stream, "400 Bad Request", &[]);
        return;
    }
    let path = root.join(path);
    match method {
        "GET" => match fs::read(path) {
            Ok(body) => write_static_http_response(stream, "200 OK", &body),
            Err(_) => write_static_http_response(stream, "404 Not Found", &[]),
        },
        "PUT" => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create writable http parent");
            }
            fs::write(path, body).expect("write writable http body");
            write_static_http_response(stream, "201 Created", &[]);
        }
        "DELETE" => match fs::remove_file(path) {
            Ok(()) => write_static_http_response(stream, "204 No Content", &[]),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                write_static_http_response(stream, "404 Not Found", &[])
            }
            Err(_) => write_static_http_response(stream, "500 Internal Server Error", &[]),
        },
        _ => write_static_http_response(stream, "405 Method Not Allowed", &[]),
    }
}

fn write_static_http_response(stream: &mut std::net::TcpStream, status: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

fn static_smart_http_v2_capabilities(lines: &[&[u8]]) -> Vec<u8> {
    let mut body = Vec::new();
    let service = b"# service=git-upload-pack\n";
    body.extend_from_slice(format!("{:04x}", service.len() + 4).as_bytes());
    body.extend_from_slice(service);
    body.extend_from_slice(b"0000");
    for line in lines {
        let length = line.len() + 4;
        body.extend_from_slice(format!("{length:04x}").as_bytes());
        body.extend_from_slice(line);
    }
    body.extend_from_slice(b"0000");
    body
}

fn http_backend_response(command: &str, project_root: &std::path::Path) -> Vec<u8> {
    http_backend_response_with_body(
        command,
        project_root,
        "/remote.git/info/refs",
        "service=git-upload-pack",
        "GET",
        &[],
    )
}

fn http_backend_response_with_translated_path(
    command: &str,
    project_root: &std::path::Path,
) -> Vec<u8> {
    http_backend_response_with_translated_path_at(command, project_root, "/remote.git/info/refs")
}

fn http_backend_response_with_translated_path_at(
    command: &str,
    project_root: &std::path::Path,
    path_info: &str,
) -> Vec<u8> {
    let path_translated = project_root.join(path_info.trim_start_matches('/'));
    let output = backend_command(command)
        .arg("http-backend")
        .env_remove("GIT_PROJECT_ROOT")
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_TRANSLATED", path_translated)
        .env("PATH_INFO", path_info)
        .env("QUERY_STRING", "service=git-upload-pack")
        .env("REQUEST_METHOD", "GET")
        .env("CONTENT_LENGTH", "0")
        .env("CONTENT_TYPE", "application/x-git-upload-pack-request")
        .stdout(Stdio::piped())
        .output()
        .unwrap_or_else(|err| panic!("run {command} http-backend: {err}"));
    assert!(
        output.status.success(),
        "{command} http-backend failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn http_backend_response_with_body(
    command: &str,
    project_root: &std::path::Path,
    path_info: &str,
    query_string: &str,
    method: &str,
    body: &[u8],
) -> Vec<u8> {
    http_backend_response_with_body_and_protocol(
        command,
        project_root,
        path_info,
        query_string,
        method,
        body,
        None,
    )
}

fn http_backend_response_with_body_and_protocol(
    command: &str,
    project_root: &std::path::Path,
    path_info: &str,
    query_string: &str,
    method: &str,
    body: &[u8],
    git_protocol: Option<&str>,
) -> Vec<u8> {
    let content_type = if path_info.ends_with("/git-receive-pack") {
        "application/x-git-receive-pack-request"
    } else {
        "application/x-git-upload-pack-request"
    };
    let mut child = backend_command(command);
    child
        .arg("http-backend")
        .env("GIT_PROJECT_ROOT", project_root)
        .env("PATH_INFO", path_info)
        .env("QUERY_STRING", query_string)
        .env("REQUEST_METHOD", method)
        .env("CONTENT_LENGTH", body.len().to_string())
        .env("CONTENT_TYPE", content_type);
    if let Some(git_protocol) = git_protocol {
        child.env("HTTP_GIT_PROTOCOL", git_protocol);
    }
    let output = child
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if !body.is_empty() {
                child.stdin.as_mut().expect("stdin pipe").write_all(body)?;
            }
            child.wait_with_output()
        })
        .unwrap_or_else(|err| panic!("run {command} http-backend: {err}"));
    assert!(
        output.status.success(),
        "{command} http-backend failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn write_backend_http_response(stream: &mut std::net::TcpStream, backend: &[u8]) {
    let (headers, body) = if let Some(idx) = backend.windows(4).position(|w| w == b"\r\n\r\n") {
        (&backend[..idx], &backend[idx + 4..])
    } else if let Some(idx) = backend.windows(2).position(|w| w == b"\n\n") {
        (&backend[..idx], &backend[idx + 2..])
    } else {
        panic!("backend response missing header terminator");
    };
    let headers = String::from_utf8_lossy(headers);
    let mut saw_length = false;
    let mut saw_connection = false;
    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n");
    for line in headers.lines() {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').expect("backend header");
        if name.eq_ignore_ascii_case("content-length") {
            saw_length = true;
        } else if name.eq_ignore_ascii_case("connection") {
            saw_connection = true;
        }
        let _ = stream.write_all(name.as_bytes());
        let _ = stream.write_all(b": ");
        let _ = stream.write_all(value.trim().as_bytes());
        let _ = stream.write_all(b"\r\n");
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

fn http_backend_failure_with_body(
    command: &str,
    project_root: &std::path::Path,
    path_info: &str,
    query_string: &str,
    method: &str,
    body: &[u8],
) -> (i32, String, String) {
    let content_type = if path_info.ends_with("/git-receive-pack") {
        "application/x-git-receive-pack-request"
    } else {
        "application/x-git-upload-pack-request"
    };
    let output = backend_command(command)
        .arg("http-backend")
        .env("GIT_PROJECT_ROOT", project_root)
        .env("PATH_INFO", path_info)
        .env("QUERY_STRING", query_string)
        .env("REQUEST_METHOD", method)
        .env("CONTENT_LENGTH", body.len().to_string())
        .env("CONTENT_TYPE", content_type)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if !body.is_empty() {
                child.stdin.as_mut().expect("stdin pipe").write_all(body)?;
            }
            child.wait_with_output()
        })
        .unwrap_or_else(|err| panic!("run {command} http-backend: {err}"));
    assert!(
        !output.status.success(),
        "{command} http-backend unexpectedly succeeded"
    );
    (
        output.status.code().expect("process exit code"),
        String::from_utf8(output.stdout)
            .expect("stdout utf8")
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .expect("stderr utf8")
            .trim_end_matches('\n')
            .to_owned(),
    )
}

fn backend_command(command: &str) -> Command {
    let pinned = required_pinned_stock_git();
    if command == "git" || command == pinned.to_string_lossy() {
        let mut command = Command::new(pinned);
        let bundle = pinned_http_bundle_root();
        command
            .env("PATH", pinned_git_helper_path(&bundle))
            .env("GIT_EXEC_PATH", pinned_stock_git_exec_path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null");
        return command;
    }
    Command::new(command)
}

fn pkt_line_bytes(payload: &[u8]) -> Vec<u8> {
    let len = payload.len() + 4;
    let mut out = format!("{len:04x}").into_bytes();
    out.extend_from_slice(payload);
    out
}

fn sideband_pack_from_http_response(response: &[u8]) -> Vec<u8> {
    let body = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|idx| &response[idx + 4..])
        .expect("http headers terminator");
    let mut cursor = 0_usize;
    while cursor < body.len() {
        let Some(line) = read_pkt_line(body, &mut cursor) else {
            continue;
        };
        if line.starts_with(b"shallow ") {
            continue;
        }
        assert!(
            line == b"NAK\n" || line.starts_with(b"ACK "),
            "unexpected upload-pack ACK/NAK line: {}",
            String::from_utf8_lossy(line)
        );
        break;
    }
    let mut pack = Vec::new();
    while cursor < body.len() {
        let Some(line) = read_pkt_line(body, &mut cursor) else {
            break;
        };
        assert_eq!(line.first(), Some(&1));
        pack.extend_from_slice(&line[1..]);
    }
    pack
}

fn upload_pack_control_lines(response: &[u8]) -> Vec<String> {
    let body = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|idx| &response[idx + 4..])
        .expect("http headers terminator");
    let mut cursor = 0_usize;
    let mut lines = Vec::new();
    while cursor < body.len() {
        let Some(line) = read_pkt_line(body, &mut cursor) else {
            continue;
        };
        if line.first() == Some(&1) {
            break;
        }
        let line = String::from_utf8(line.to_vec()).expect("control line utf8");
        lines.push(line.trim_end_matches('\n').to_owned());
        if line == "NAK\n" || line.starts_with("ACK ") {
            break;
        }
    }
    lines
}

fn smart_http_ref_lines(response: &[u8]) -> Vec<String> {
    let body = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|idx| &response[idx + 4..])
        .expect("http headers terminator");
    let mut cursor = 0_usize;
    let service = read_pkt_line(body, &mut cursor).expect("service pkt");
    assert_eq!(service, b"# service=git-upload-pack\n");
    assert!(read_pkt_line(body, &mut cursor).is_none());
    let mut lines = Vec::new();
    while cursor < body.len() {
        let Some(line) = read_pkt_line(body, &mut cursor) else {
            break;
        };
        let line = line
            .split(|byte| *byte == 0)
            .next()
            .expect("line before capabilities");
        lines.push(
            String::from_utf8(line.to_vec())
                .expect("pkt utf8")
                .trim_end_matches('\n')
                .to_owned(),
        );
    }
    lines
}

fn read_pkt_line<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    let header = bytes.get(*cursor..*cursor + 4)?;
    *cursor += 4;
    let len = std::str::from_utf8(header)
        .expect("pkt header utf8")
        .chars()
        .try_fold(0_usize, |acc, ch| {
            ch.to_digit(16).map(|value| acc * 16 + value as usize)
        })
        .expect("pkt header hex");
    if len == 0 {
        return None;
    }
    let payload_len = len.checked_sub(4).expect("pkt length includes header");
    let payload = bytes
        .get(*cursor..*cursor + payload_len)
        .expect("pkt payload");
    *cursor += payload_len;
    Some(payload)
}

#[test]
fn daemon_serves_stock_git_clone_protocol_v1() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let clone = dir.path().join("clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    fs::create_dir_all(work.join("dir")).expect("create dir");
    fs::write(work.join("dir/b.txt"), b"world\n").expect("write b");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let output = Command::new(stock_git_bin())
        .args([
            "-c",
            "protocol.version=0",
            "clone",
            url.as_str(),
            clone.to_str().expect("clone path"),
        ])
        .output()
        .expect("git clone via zmin daemon");
    assert!(
        output.status.success(),
        "git clone failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(clone.join("a.txt")).expect("read a"),
        "hello\n"
    );
    assert_eq!(
        fs::read_to_string(clone.join("dir/b.txt")).expect("read b"),
        "world\n"
    );
    assert_eq!(
        git(&clone, ["rev-parse", "HEAD"]),
        git(&work, ["rev-parse", "HEAD"])
    );
}

#[test]
fn git_daemon_protocol_v1_and_v2_match_stock_negotiation() {
    let dir = TempDir::new().expect("protocol daemon temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("protocol.txt"), b"protocol\n").expect("write protocol file");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "protocol"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);
    fs::write(remote.join("git-daemon-export-ok"), b"").expect("export marker");
    let empty_template = dir.path().join("empty-template");
    fs::create_dir(&empty_template).expect("empty template");

    for version in ["1", "2"] {
        let stock_port = unused_local_port();
        let stock_daemon = StockGitDaemon::spawn(dir.path(), stock_port);
        let stock_url = format!("git://127.0.0.1:{stock_port}/remote.git");
        let stock = Command::new(stock_git_bin())
            .env("GIT_TRACE_PACKET", "1")
            .args([
                "-c",
                &format!("protocol.version={version}"),
                "ls-remote",
                stock_url.as_str(),
            ])
            .output()
            .expect("stock protocol probe");
        assert!(
            stock.status.success(),
            "stock protocol probe failed: {stock:?}"
        );
        assert!(
            String::from_utf8_lossy(&stock.stderr).contains(&format!("version {version}")),
            "stock response did not negotiate protocol {version}: {}",
            String::from_utf8_lossy(&stock.stderr)
        );
        let expected_stdout = stock.stdout;
        drop(stock_daemon);

        let zmin_port = unused_local_port();
        let zmin_daemon = ZminGitDaemon::spawn(dir.path(), zmin_port);
        let zmin_url = format!("git://127.0.0.1:{zmin_port}/remote.git");
        let zmin_stock_client = Command::new(stock_git_bin())
            .env("GIT_TRACE_PACKET", "1")
            .args([
                "-c",
                &format!("protocol.version={version}"),
                "ls-remote",
                zmin_url.as_str(),
            ])
            .output()
            .expect("stock client against zmin daemon");
        assert!(
            zmin_stock_client.status.success(),
            "stock client against zmin daemon failed: {zmin_stock_client:?}"
        );
        assert_eq!(zmin_stock_client.stdout, expected_stdout);
        let zmin_trace = String::from_utf8_lossy(&zmin_stock_client.stderr);
        assert!(
            zmin_trace.contains(&format!("version {version}")),
            "zmin daemon did not negotiate protocol {version}: {zmin_trace}"
        );
        assert!(
            zmin_trace.contains(&format!("version={version}")),
            "client request did not carry protocol {version}: {zmin_trace}"
        );
        if version == "2" {
            assert!(
                zmin_trace.contains("ls-refs"),
                "v2 ls-refs exchange missing: {zmin_trace}"
            );
        }

        let zmin_client = Command::new(zmin_bin())
            .args([
                "-c",
                &format!("protocol.version={version}"),
                "ls-remote",
                zmin_url.as_str(),
            ])
            .output()
            .expect("zmin client against zmin daemon");
        assert!(
            zmin_client.status.success(),
            "zmin protocol probe failed: {zmin_client:?}"
        );
        assert_eq!(zmin_client.stdout, expected_stdout);
        assert!(zmin_client.stderr.is_empty());
        drop(zmin_daemon);

        let stock_clone = dir.path().join(format!("stock-clone-v{version}"));
        let stock_clone_port = unused_local_port();
        let stock_clone_daemon = StockGitDaemon::spawn(dir.path(), stock_clone_port);
        let stock_clone_url = format!("git://127.0.0.1:{stock_clone_port}/remote.git");
        let stock_clone_output = Command::new(stock_git_bin())
            .env("GIT_TEMPLATE_DIR", &empty_template)
            .args([
                "-c",
                &format!("protocol.version={version}"),
                "clone",
                "-q",
                stock_clone_url.as_str(),
                stock_clone.to_str().expect("stock clone path"),
            ])
            .output()
            .expect("stock protocol clone");
        drop(stock_clone_daemon);

        let zmin_clone = dir.path().join(format!("zmin-clone-v{version}"));
        let zmin_clone_port = unused_local_port();
        let zmin_clone_daemon = ZminGitDaemon::spawn(dir.path(), zmin_clone_port);
        let zmin_clone_url = format!("git://127.0.0.1:{zmin_clone_port}/remote.git");
        let zmin_clone_output = Command::new(stock_git_bin())
            .env("GIT_TEMPLATE_DIR", &empty_template)
            .args([
                "-c",
                &format!("protocol.version={version}"),
                "clone",
                "-q",
                zmin_clone_url.as_str(),
                zmin_clone.to_str().expect("zmin clone path"),
            ])
            .output()
            .expect("stock client against zmin protocol daemon clone");
        drop(zmin_clone_daemon);
        assert_eq!(
            zmin_clone_output.status.code(),
            stock_clone_output.status.code(),
            "protocol v{version} clone exit differs"
        );
        assert_eq!(zmin_clone_output.stdout, stock_clone_output.stdout);
        assert_eq!(zmin_clone_output.stderr, stock_clone_output.stderr);
        assert!(stock_clone_output.status.success());
        assert_eq!(
            git(&zmin_clone, ["rev-parse", "HEAD"]),
            git(&stock_clone, ["rev-parse", "HEAD"])
        );
        assert_eq!(
            git(&zmin_clone, ["status", "--porcelain=v1"]),
            git(&stock_clone, ["status", "--porcelain=v1"])
        );
        assert_eq!(
            fs::read(zmin_clone.join("protocol.txt")).expect("read zmin clone"),
            fs::read(stock_clone.join("protocol.txt")).expect("read stock clone")
        );

        let zmin_client_clone = dir.path().join(format!("zmin-client-clone-v{version}"));
        let zmin_client_clone_port = unused_local_port();
        let zmin_client_clone_daemon = StockGitDaemon::spawn(dir.path(), zmin_client_clone_port);
        let zmin_client_clone_url = format!("git://127.0.0.1:{zmin_client_clone_port}/remote.git");
        let zmin_client_clone_output = Command::new(zmin_bin())
            .env("GIT_TEMPLATE_DIR", &empty_template)
            .args([
                "-c",
                &format!("protocol.version={version}"),
                "clone",
                "-q",
                zmin_client_clone_url.as_str(),
                zmin_client_clone.to_str().expect("zmin client clone path"),
            ])
            .output()
            .expect("zmin client against stock protocol daemon clone");
        drop(zmin_client_clone_daemon);
        assert_eq!(
            zmin_client_clone_output.status.code(),
            stock_clone_output.status.code(),
            "zmin client protocol v{version} clone exit differs"
        );
        assert_eq!(zmin_client_clone_output.stdout, stock_clone_output.stdout);
        assert_eq!(zmin_client_clone_output.stderr, stock_clone_output.stderr);
        assert!(zmin_client_clone_output.status.success());
        assert_eq!(
            git(&zmin_client_clone, ["rev-parse", "HEAD"]),
            git(&stock_clone, ["rev-parse", "HEAD"])
        );
        assert_eq!(
            fs::read(zmin_client_clone.join("protocol.txt")).expect("read zmin client clone"),
            fs::read(stock_clone.join("protocol.txt")).expect("read stock clone")
        );

        if version == "2" {
            let traced_clone = dir.path().join("traced-v2-clone");
            let traced_port = unused_local_port();
            let traced_daemon = ZminGitDaemon::spawn(dir.path(), traced_port);
            let traced_url = format!("git://127.0.0.1:{traced_port}/remote.git");
            let traced_output = Command::new(stock_git_bin())
                .env("GIT_TEMPLATE_DIR", &empty_template)
                .env("GIT_TRACE_PACKET", "1")
                .args([
                    "-c",
                    "protocol.version=2",
                    "clone",
                    "-q",
                    traced_url.as_str(),
                    traced_clone.to_str().expect("traced clone path"),
                ])
                .output()
                .expect("traced v2 clone");
            drop(traced_daemon);
            assert!(
                traced_output.status.success(),
                "traced v2 clone failed: {traced_output:?}"
            );
            let traced_packets = String::from_utf8_lossy(&traced_output.stderr);
            assert!(traced_packets.contains("ls-refs"), "missing v2 ls-refs");
            assert!(traced_packets.contains("fetch"), "missing v2 fetch");
            assert!(traced_packets.contains("packfile"), "missing v2 packfile");
            assert!(
                !traced_packets.contains("acknowledgments") && !traced_packets.contains("NAK"),
                "done fetch emitted negotiation response: {traced_packets}"
            );

            let stock_fetch_repo = git_init();
            let zmin_fetch_repo = git_init();
            git(
                stock_fetch_repo.path(),
                ["remote", "add", "origin", "git://127.0.0.1:1/unused"],
            );
            git(
                zmin_fetch_repo.path(),
                ["remote", "add", "origin", "git://127.0.0.1:1/unused"],
            );
            let fetch_port = unused_local_port();
            let fetch_daemon = StockGitDaemon::spawn(dir.path(), fetch_port);
            let fetch_url = format!("git://127.0.0.1:{fetch_port}/remote.git");
            git(
                stock_fetch_repo.path(),
                ["remote", "set-url", "origin", fetch_url.as_str()],
            );
            git(
                zmin_fetch_repo.path(),
                ["remote", "set-url", "origin", fetch_url.as_str()],
            );
            let stock_fetch_output = Command::new(stock_git_bin())
                .args([
                    "-c",
                    "protocol.version=2",
                    "fetch",
                    "-q",
                    "--server-option=protocol-test",
                    "origin",
                    "main",
                ])
                .current_dir(stock_fetch_repo.path())
                .output()
                .expect("stock protocol fetch");
            let zmin_fetch_output = Command::new(zmin_bin())
                .args([
                    "-c",
                    "protocol.version=2",
                    "fetch",
                    "-q",
                    "--server-option=protocol-test",
                    "origin",
                    "main",
                ])
                .current_dir(zmin_fetch_repo.path())
                .output()
                .expect("zmin protocol fetch against stock daemon");
            drop(fetch_daemon);
            assert_eq!(
                zmin_fetch_output.status.code(),
                stock_fetch_output.status.code(),
                "protocol v2 fetch exit differs"
            );
            assert_eq!(zmin_fetch_output.stdout, stock_fetch_output.stdout);
            assert_eq!(zmin_fetch_output.stderr, stock_fetch_output.stderr);
            assert!(stock_fetch_output.status.success());
            assert_eq!(
                git(
                    zmin_fetch_repo.path(),
                    ["rev-parse", "refs/remotes/origin/main"]
                ),
                git(
                    stock_fetch_repo.path(),
                    ["rev-parse", "refs/remotes/origin/main"]
                )
            );
        }
    }
}

#[test]
fn git_daemon_clone_rejects_newline_url_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let stock_destination = dir.path().join("stock-destination");
    let zmin_destination = dir.path().join("zmin-destination");
    let (stock, stock_connected) = run_newline_clone_probe("git", dir.path(), &stock_destination);
    let (zmin, zmin_connected) = run_newline_clone_probe(zmin_bin(), dir.path(), &zmin_destination);

    assert_eq!(stock.0, 128);
    assert_eq!(zmin.0, stock.0);
    assert!(stock.1.is_empty());
    assert_eq!(zmin.1, stock.1);
    assert_eq!(
        stock.2.lines().last(),
        Some("fatal: newline is forbidden in git:// hosts and repo paths")
    );
    assert_eq!(zmin.2.lines().last(), stock.2.lines().last());
    assert!(!stock_destination.exists());
    assert!(!zmin_destination.exists());
    assert!(!stock_connected);
    assert!(!zmin_connected);
}

#[test]
fn daemon_clone_removes_new_destination_after_repository_error() {
    let dir = TempDir::new().expect("temp dir");
    let port = unused_local_port();
    let port_arg = format!("--port={port}");
    let base_path = format!("--base-path={}", dir.path().display());
    let mut daemon = Command::new(zmin_bin())
        .args([
            "daemon",
            "--listen=127.0.0.1",
            port_arg.as_str(),
            base_path.as_str(),
        ])
        .arg(dir.path().to_str().expect("daemon root"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn zmin daemon");
    wait_for_tcp_port(port);

    let destination = dir.path().join("nowhere");
    let url = format!("git://127.0.0.1:{port}/nowhere.git");
    let output = Command::new(zmin_bin())
        .args([
            "clone",
            url.as_str(),
            destination.to_str().expect("clone destination"),
        ])
        .current_dir(dir.path())
        .output()
        .expect("clone missing daemon repository");

    assert_eq!(output.status.code(), Some(128));
    assert!(!destination.exists(), "failed clone left destination state");
    assert!(daemon.try_wait().expect("daemon status").is_none());
    daemon.kill().expect("stop zmin daemon");
    let _ = daemon.wait().expect("wait zmin daemon");
}

#[test]
fn daemon_clone_filter_rejects_before_network_or_destination() {
    let dir = TempDir::new().expect("temp dir");
    let destination = dir.path().join("daemon-filter-destination");
    let listener =
        std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind daemon preflight listener");
    listener
        .set_nonblocking(true)
        .expect("set daemon preflight listener nonblocking");
    let port = listener
        .local_addr()
        .expect("daemon preflight listener address")
        .port();
    let url = format!("git://127.0.0.1:{port}/repo.git");
    let output = command_any_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--filter=blob:none",
            url.as_str(),
            destination.to_str().expect("destination path"),
        ],
        "daemon clone filter preflight",
    );

    assert_eq!(output.0, 129);
    assert!(output.1.is_empty());
    assert_eq!(
        output.2,
        "fatal: clone --filter over SSH and git daemon requires --no-checkout or --bare"
    );
    assert!(!destination.exists());
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "daemon was contacted before preflight rejection"
    );
}

#[test]
fn daemon_clone_preserves_preexisting_destination_after_advertisement_eof() {
    let dir = TempDir::new().expect("temp dir");
    let port = unused_local_port();
    let port_arg = format!("--port={port}");
    let base_path = format!("--base-path={}", dir.path().display());
    let mut daemon = Command::new(zmin_bin())
        .args([
            "daemon",
            "--listen=127.0.0.1",
            port_arg.as_str(),
            base_path.as_str(),
        ])
        .arg(dir.path().to_str().expect("daemon root"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn zmin daemon");
    wait_for_tcp_port(port);

    let destination = dir.path().join("user-destination");
    fs::create_dir(&destination).expect("create user destination");
    fs::write(destination.join("user.txt"), b"keep\n").expect("write user file");
    let url = format!("git://127.0.0.1:{port}/nowhere.git");
    let output = Command::new(zmin_bin())
        .args([
            "clone",
            url.as_str(),
            destination.to_str().expect("clone destination"),
        ])
        .current_dir(dir.path())
        .output()
        .expect("clone into user destination");

    assert_eq!(output.status.code(), Some(128));
    assert_eq!(
        fs::read(destination.join("user.txt")).expect("read user file"),
        b"keep\n"
    );
    assert!(daemon.try_wait().expect("daemon status").is_none());
    daemon.kill().expect("stop zmin daemon");
    let _ = daemon.wait().expect("wait zmin daemon");
}

#[test]
fn daemon_clone_preserves_preexisting_empty_bare_destination_before_init() {
    let dir = TempDir::new().expect("temp dir");
    let destination = dir.path().join("bare-destination");
    fs::create_dir(&destination).expect("create bare destination");
    let url = format!("git://127.0.0.1:{}/missing.git", unused_local_port());
    let output = Command::new(zmin_bin())
        .args([
            "clone",
            "--bare",
            url.as_str(),
            destination.to_str().expect("clone destination"),
        ])
        .current_dir(dir.path())
        .output()
        .expect("clone into preexisting bare destination");

    assert!(!output.status.success());
    assert!(destination.is_dir());
    assert!(
        fs::read_dir(&destination)
            .expect("destination entries")
            .next()
            .is_none()
    );
}

#[test]
fn daemon_empty_clone_keeps_destination_after_flush_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("empty.git");
    git(dir.path(), ["init", "--bare", "empty.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/empty.git");
    let stock_destination = dir.path().join("stock-empty");
    let zmin_destination = dir.path().join("zmin-empty");
    let stock = Command::new(stock_git_bin())
        .args([
            "clone",
            url.as_str(),
            stock_destination.to_str().expect("stock destination"),
        ])
        .output()
        .expect("stock empty clone");
    let zmin = Command::new(zmin_bin())
        .args([
            "clone",
            url.as_str(),
            zmin_destination.to_str().expect("zmin destination"),
        ])
        .output()
        .expect("zmin empty clone");

    assert!(stock.status.success());
    assert!(zmin.status.success());
    assert!(stock_destination.join(".git").is_dir());
    assert!(zmin_destination.join(".git").is_dir());
}

#[test]
fn ls_remote_reads_git_daemon_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "tag"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    for args in [
        vec!["ls-remote", url.as_str()],
        vec!["ls-remote", "--heads", url.as_str()],
        vec!["ls-remote", "--tags", url.as_str()],
        vec!["ls-remote", "--refs", url.as_str()],
        vec!["ls-remote", url.as_str(), "v*"],
    ] {
        assert_eq!(
            run_zmin_args(dir.path(), &args),
            git_args(dir.path(), &args),
            "args: {args:?}"
        );
    }
}

#[test]
fn ls_remote_option_family_matches_stock_git_for_git_daemon_remote() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "tag"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    for (label, args) in [
        (
            "ls-remote --branches daemon",
            vec!["ls-remote", "--branches", url.as_str()],
        ),
        ("ls-remote -b daemon", vec!["ls-remote", "-b", url.as_str()]),
        (
            "ls-remote --quiet daemon",
            vec!["ls-remote", "--quiet", url.as_str()],
        ),
        ("ls-remote -q daemon", vec!["ls-remote", "-q", url.as_str()]),
        (
            "ls-remote --get-url daemon",
            vec!["ls-remote", "--get-url", url.as_str()],
        ),
        (
            "ls-remote --symref daemon",
            vec!["ls-remote", "--symref", url.as_str()],
        ),
        (
            "ls-remote --exit-code match daemon",
            vec!["ls-remote", "--exit-code", url.as_str(), "main"],
        ),
        (
            "ls-remote --exit-code miss daemon",
            vec!["ls-remote", "--exit-code", url.as_str(), "no-such*"],
        ),
        (
            "ls-remote --server-option=foo daemon",
            vec!["ls-remote", "--server-option=foo", url.as_str()],
        ),
        (
            "ls-remote -o foo daemon",
            vec!["ls-remote", "-o", "foo", url.as_str()],
        ),
        (
            "ls-remote --sort=refname daemon",
            vec!["ls-remote", "--sort=refname", url.as_str()],
        ),
        (
            "ls-remote --sort=-refname daemon",
            vec!["ls-remote", "--sort=-refname", url.as_str()],
        ),
        ("ls-remote -t daemon", vec!["ls-remote", "-t", url.as_str()]),
    ] {
        assert_any_ls_remote_output_matches_stock_git(dir.path(), &args, label);
    }
}

#[test]
fn fetch_reads_git_daemon_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"one\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "tag"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    for client in [&git_client, &zmin_client] {
        git(dir.path(), ["init", client.to_str().expect("client path")]);
        git(client, ["remote", "add", "origin", url.as_str()]);
    }

    git(&git_client, ["fetch", "origin"]);
    run_zmin(&zmin_client, ["fetch", "origin"]);
    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "origin/main:a.txt"]),
        git(&git_client, ["cat-file", "-p", "origin/main:a.txt"])
    );
}

#[test]
fn clone_reads_git_daemon_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::create_dir_all(work.join("dir")).expect("create dir");
    fs::write(work.join("dir/a.txt"), b"hello\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature"]);
    set_bare_head_to_main(&remote);

    let git_port = unused_local_port();
    let zmin_port = unused_local_port();
    let _git_daemon = StockGitDaemon::spawn(dir.path(), git_port);
    let git_url = format!("git://127.0.0.1:{git_port}/remote.git");
    git(
        dir.path(),
        [
            "clone",
            git_url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    let _zmin_daemon = StockGitDaemon::spawn(dir.path(), zmin_port);
    let zmin_url = format!("git://127.0.0.1:{zmin_port}/remote.git");
    run_zmin(
        dir.path(),
        [
            "clone",
            zmin_url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );
    assert_eq!(
        fs::read_to_string(zmin_clone.join("dir/a.txt")).expect("read zmin a"),
        fs::read_to_string(git_clone.join("dir/a.txt")).expect("read git a")
    );
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&git_clone, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&zmin_clone, ["show-ref"]),
        git(&git_clone, ["show-ref"])
    );
}

#[test]
fn remote_set_head_auto_git_daemon_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("file"), "one\n").expect("write file");
    git(&work, ["add", "file"]);
    git_with_env(&work, ["commit", "-m", "one"]);
    git(
        &work,
        [
            "remote",
            "add",
            "public",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "public", "main:main"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    git(
        dir.path(),
        [
            "clone",
            url.as_str(),
            git_clone.to_str().expect("stock clone path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    git(&work, ["push", "-q", "public", "main:other"]);
    let stock_delete = command_any_output(
        "git",
        &git_clone,
        &["remote", "set-head", "-d", "origin"],
        "stock remote set-head delete",
    );
    let zmin_delete = command_any_output(
        zmin_bin(),
        &zmin_clone,
        &["remote", "set-head", "-d", "origin"],
        "zmin remote set-head delete",
    );
    assert_eq!(zmin_delete, stock_delete);
    assert_eq!(stock_delete, (0, String::new(), String::new()));

    let stock_auto = command_any_output(
        "git",
        &git_clone,
        &["remote", "set-head", "-a", "origin"],
        "stock remote set-head auto",
    );
    let zmin_auto = command_any_output(
        zmin_bin(),
        &zmin_clone,
        &["remote", "set-head", "-a", "origin"],
        "zmin remote set-head auto",
    );
    assert_eq!(zmin_auto, stock_auto);
    assert_eq!(
        stock_auto,
        (
            0,
            "'origin/HEAD' is now created and points to 'main'".to_owned(),
            String::new(),
        )
    );
    assert_eq!(
        git(&zmin_clone, ["symbolic-ref", "refs/remotes/origin/HEAD"]),
        "refs/remotes/origin/main"
    );
    assert_eq!(
        git(&zmin_clone, ["show-ref"]),
        git(&git_clone, ["show-ref"])
    );
}

#[test]
fn git_daemon_verbose_clone_and_fetch_report_connection_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("file"), "one\n").expect("write initial file");
    git(&work, ["add", "file"]);
    git_with_env(&work, ["commit", "-m", "one"]);
    git(
        &work,
        [
            "remote",
            "add",
            "public",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "public", "main:main"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let stock_clone_output = command_any_output(
        "git",
        dir.path(),
        &[
            "clone",
            "-v",
            url.as_str(),
            git_clone.to_str().expect("stock clone path"),
        ],
        "stock verbose git daemon clone",
    );
    let zmin_clone_output = command_any_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "-v",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
        "zmin verbose git daemon clone",
    );
    assert_eq!(stock_clone_output.0, 0);
    assert_eq!(zmin_clone_output.0, 0);
    for stderr in [&stock_clone_output.2, &zmin_clone_output.2] {
        assert_eq!(stderr.matches("Looking up ").count(), 1);
        assert_eq!(stderr.matches("Connecting to ").count(), 1);
        assert!(
            stderr.contains("Looking up 127.0.0.1 ... done."),
            "missing lookup diagnostic: {stderr}"
        );
        assert!(
            stderr.contains(&format!(
                "Connecting to 127.0.0.1 (port {port}) ... 127.0.0.1 done."
            )),
            "missing connect diagnostic: {stderr}"
        );
    }
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&git_clone, ["rev-parse", "HEAD"])
    );

    fs::write(work.join("file"), "one\ntwo\n").expect("write second file");
    git(&work, ["commit", "-am", "two"]);
    git(&work, ["push", "-q", "public", "main:main"]);

    for (label, command, client) in [
        ("stock", "git", git_clone.as_path()),
        ("zmin", zmin_bin(), zmin_clone.as_path()),
    ] {
        let output = command_any_output(
            command,
            client,
            &["pull", "-v"],
            &format!("{label} verbose git daemon pull"),
        );
        assert_eq!(output.0, 0, "{label} pull stderr: {}", output.2);
        assert_eq!(output.2.matches("Looking up ").count(), 1);
        assert_eq!(output.2.matches("Connecting to ").count(), 1);
        assert!(
            output.2.contains("Looking up 127.0.0.1 ... done."),
            "{label} pull missing lookup diagnostic: {}",
            output.2
        );
        assert!(
            output.2.contains(&format!(
                "Connecting to 127.0.0.1 (port {port}) ... 127.0.0.1 done."
            )),
            "{label} pull missing connect diagnostic: {}",
            output.2
        );
        assert_eq!(
            fs::read_to_string(client.join("file")).expect("read client file"),
            "one\ntwo\n"
        );
    }

    for (label, command, client) in [
        ("stock", "git", git_clone.as_path()),
        ("zmin", zmin_bin(), zmin_clone.as_path()),
    ] {
        let output = command_any_output(
            command,
            client,
            &["fetch", "-v"],
            &format!("{label} verbose git daemon no-op fetch"),
        );
        assert_eq!(output.0, 0, "{label} fetch stderr: {}", output.2);
        assert_eq!(output.2.matches("Looking up ").count(), 1);
        assert_eq!(output.2.matches("Connecting to ").count(), 1);
        assert!(
            output.2.contains("Looking up 127.0.0.1 ... done."),
            "{label} fetch missing lookup diagnostic: {}",
            output.2
        );
        assert!(
            output.2.contains(&format!(
                "Connecting to 127.0.0.1 (port {port}) ... 127.0.0.1 done."
            )),
            "{label} fetch missing connect diagnostic: {}",
            output.2
        );
    }

    git(&work, ["push", "-q", "public", "main:feature"]);
    let multi_refspec_outputs = [
        (
            "stock",
            command_any_output(
                "git",
                &git_clone,
                &[
                    "fetch",
                    "-v",
                    "origin",
                    "refs/heads/main:refs/remotes/origin/main",
                    "refs/heads/feature:refs/remotes/origin/feature",
                ],
                "stock verbose git daemon multi-refspec fetch",
            ),
        ),
        (
            "zmin",
            command_any_output(
                zmin_bin(),
                &zmin_clone,
                &[
                    "fetch",
                    "-v",
                    "origin",
                    "refs/heads/main:refs/remotes/origin/main",
                    "refs/heads/feature:refs/remotes/origin/feature",
                ],
                "zmin verbose git daemon multi-refspec fetch",
            ),
        ),
    ];
    assert_eq!(multi_refspec_outputs[0].1.0, 0);
    assert_eq!(multi_refspec_outputs[1].1.0, 0);
    for (label, output) in multi_refspec_outputs {
        assert!(
            output.2.matches("Looking up ").count() >= 1,
            "{label} multi-refspec fetch missing lookup diagnostic: {}",
            output.2
        );
        assert!(
            output.2.matches("Connecting to ").count() >= 1,
            "{label} multi-refspec fetch missing connect diagnostic: {}",
            output.2
        );
    }

    let pull_all_outputs = [
        (
            "stock",
            command_any_output(
                "git",
                &git_clone,
                &["pull", "--all", "-v"],
                "stock verbose git daemon pull all",
            ),
        ),
        (
            "zmin",
            command_any_output(
                zmin_bin(),
                &zmin_clone,
                &["pull", "--all", "-v"],
                "zmin verbose git daemon pull all",
            ),
        ),
    ];
    for (label, output) in pull_all_outputs {
        assert_eq!(output.0, 0, "{label} pull --all stderr: {}", output.2);
        assert!(output.2.matches("Looking up ").count() >= 1);
        assert!(output.2.matches("Connecting to ").count() >= 1);
        assert!(output.2.contains("Looking up 127.0.0.1 ... done."));
        assert!(output.2.contains(&format!(
            "Connecting to 127.0.0.1 (port {port}) ... 127.0.0.1 done."
        )));
    }

    let stock_symref = command_any_output(
        "git",
        dir.path(),
        &["ls-remote", "--symref", url.as_str()],
        "stock git daemon symref advertisement",
    );
    let zmin_symref = command_any_output(
        zmin_bin(),
        dir.path(),
        &["ls-remote", "--symref", url.as_str()],
        "zmin git daemon symref advertisement",
    );
    assert_eq!(zmin_symref, stock_symref);
    assert!(zmin_symref.1.contains("ref: refs/heads/main\tHEAD"));

    let localhost_url = format!("git://localhost:{port}/remote.git");
    let stock_localhost = command_any_output(
        "git",
        dir.path(),
        &[
            "clone",
            "-v",
            localhost_url.as_str(),
            dir.path()
                .join("git-localhost-clone")
                .to_str()
                .expect("stock localhost clone path"),
        ],
        "stock localhost verbose git daemon clone",
    );
    let zmin_localhost = command_any_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "-v",
            localhost_url.as_str(),
            dir.path()
                .join("zmin-localhost-clone")
                .to_str()
                .expect("zmin localhost clone path"),
        ],
        "zmin localhost verbose git daemon clone",
    );
    assert_eq!(stock_localhost.0, 0);
    assert_eq!(zmin_localhost.0, 0);
    for stderr in [&stock_localhost.2, &zmin_localhost.2] {
        assert_eq!(stderr.matches("Looking up ").count(), 1);
        assert_eq!(stderr.matches("Connecting to ").count(), 1);
        assert!(stderr.contains("Looking up localhost ... done."));
        assert!(stderr.contains(&format!(
            "Connecting to localhost (port {port}) ... 127.0.0.1 done."
        )));
    }
}

#[test]
fn clone_instant_git_daemon_materializes_head_then_fetch_hydrates_refs() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_clone = dir.path().join("git-daemon-clone");
    let zmin_clone = dir.path().join("zmin-daemon-instant");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join(".gitattributes"), b"crlf.txt -text\n").expect("write attributes");
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    fs::write(work.join("crlf.txt"), b"line one\r\nline two\r\n").expect("write crlf");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["switch", "-c", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    git(&work, ["switch", "main"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "release"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    git(
        dir.path(),
        [
            "clone",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--instant",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&git_clone, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD^{tree}"]),
        git(&git_clone, ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        fs::read(zmin_clone.join("crlf.txt")).expect("zmin crlf"),
        fs::read(git_clone.join("crlf.txt")).expect("git crlf")
    );
    let initial_refs = git(&zmin_clone, ["show-ref"]);
    assert!(
        initial_refs
            .lines()
            .any(|line| line.ends_with(" refs/remotes/origin/main")),
        "instant clone should write the fetched HEAD branch ref:\n{initial_refs}"
    );
    assert!(
        !initial_refs
            .lines()
            .any(|line| line.ends_with(" refs/remotes/origin/feature")),
        "instant clone should not write refs for objects it did not request:\n{initial_refs}"
    );
    assert!(
        !initial_refs
            .lines()
            .any(|line| line.ends_with(" refs/tags/v1")),
        "instant clone should leave non-target tags for later fetch:\n{initial_refs}"
    );

    run_zmin(&zmin_clone, ["fetch", "origin"]);
    let hydrated_refs = git(&zmin_clone, ["show-ref"]);
    assert!(
        hydrated_refs
            .lines()
            .any(|line| line.ends_with(" refs/remotes/origin/feature")),
        "fetch should hydrate additional remote branch refs:\n{hydrated_refs}"
    );
    assert!(
        hydrated_refs
            .lines()
            .any(|line| line.ends_with(" refs/tags/v1")),
        "fetch should hydrate followed tag refs:\n{hydrated_refs}"
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
}

#[test]
fn clone_instant_git_daemon_demand_hydrate_recovers_missing_head_objects() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let zmin_clone = dir.path().join("zmin-daemon-instant-demand");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    run_zmin(
        dir.path(),
        [
            "clone",
            "--instant",
            "--demand-hydrate",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_demand_hydrate_config(&zmin_clone);
    let head = git(&zmin_clone, ["rev-parse", "HEAD"]);
    remove_all_pack_files(&zmin_clone);

    assert_eq!(run_zmin(&zmin_clone, ["cat-file", "-t", "HEAD"]), "commit");
    assert_eq!(run_zmin(&zmin_clone, ["cat-file", "-t", &head]), "commit");
    git(&zmin_clone, ["fsck", "--strict"]);
}

#[test]
fn clone_worktree_first_git_daemon_demand_hydrate_recovers_missing_head_objects() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let zmin_clone = dir.path().join("zmin-daemon-worktree-first-demand");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    run_zmin(
        dir.path(),
        [
            "clone",
            "--worktree-first",
            "--demand-hydrate",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_demand_hydrate_config(&zmin_clone);
    let head = git(&zmin_clone, ["rev-parse", "HEAD"]);
    remove_all_pack_files(&zmin_clone);

    assert_eq!(run_zmin(&zmin_clone, ["cat-file", "-t", "HEAD"]), "commit");
    assert_eq!(run_zmin(&zmin_clone, ["cat-file", "-t", &head]), "commit");
    git(&zmin_clone, ["fsck", "--strict"]);
}

#[test]
fn clone_instant_git_daemon_background_fetch_hydrates_refs() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let zmin_clone = dir.path().join("zmin-daemon-instant-background");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["switch", "-c", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    git(&work, ["switch", "main"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "release"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    run_zmin(
        dir.path(),
        [
            "clone",
            "--instant",
            "--background-fetch",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_background_fetch_hydrated(&zmin_clone);
}

#[test]
fn clone_worktree_first_git_daemon_background_fetch_hydrates_refs() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let zmin_clone = dir.path().join("zmin-daemon-worktree-first-background");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["switch", "-c", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    git(&work, ["switch", "main"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "release"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    run_zmin(
        dir.path(),
        [
            "clone",
            "--worktree-first",
            "--background-fetch",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_background_fetch_hydrated(&zmin_clone);
}

#[test]
fn ls_remote_reads_ssh_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "tag"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let ssh_url = ssh_url_for_remote(&remote);
    let scp_url = scp_url_for_remote(&remote);
    for args in [
        vec!["ls-remote", ssh_url.as_str()],
        vec!["ls-remote", "--heads", ssh_url.as_str()],
        vec!["ls-remote", "--tags", ssh_url.as_str()],
        vec!["ls-remote", "--refs", ssh_url.as_str()],
        vec!["ls-remote", ssh_url.as_str(), "v*"],
        vec!["ls-remote", scp_url.as_str()],
    ] {
        assert_eq!(
            command_output_with_env(
                "git",
                dir.path(),
                &args,
                &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
                "git",
            )
            .1,
            command_output_with_env(
                zmin_bin(),
                dir.path(),
                &args,
                &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
                "zmin",
            )
            .1,
            "args: {args:?}"
        );
    }
}

#[test]
fn ls_remote_option_family_matches_stock_git_for_ssh_remote() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "tag"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let ssh_url = ssh_url_for_remote(&remote);
    let envs = [("GIT_SSH_COMMAND", fake_ssh_arg.as_str())];
    for (label, args) in [
        (
            "ls-remote --branches ssh",
            vec!["ls-remote", "--branches", ssh_url.as_str()],
        ),
        (
            "ls-remote -b ssh",
            vec!["ls-remote", "-b", ssh_url.as_str()],
        ),
        (
            "ls-remote --quiet ssh",
            vec!["ls-remote", "--quiet", ssh_url.as_str()],
        ),
        (
            "ls-remote -q ssh",
            vec!["ls-remote", "-q", ssh_url.as_str()],
        ),
        (
            "ls-remote --get-url ssh",
            vec!["ls-remote", "--get-url", ssh_url.as_str()],
        ),
        (
            "ls-remote --symref ssh",
            vec!["ls-remote", "--symref", ssh_url.as_str()],
        ),
        (
            "ls-remote --exit-code match ssh",
            vec!["ls-remote", "--exit-code", ssh_url.as_str(), "main"],
        ),
        (
            "ls-remote --exit-code miss ssh",
            vec!["ls-remote", "--exit-code", ssh_url.as_str(), "no-such*"],
        ),
        (
            "ls-remote --server-option=foo ssh",
            vec!["ls-remote", "--server-option=foo", ssh_url.as_str()],
        ),
        (
            "ls-remote -o foo ssh",
            vec!["ls-remote", "-o", "foo", ssh_url.as_str()],
        ),
        (
            "ls-remote --sort=refname ssh",
            vec!["ls-remote", "--sort=refname", ssh_url.as_str()],
        ),
        (
            "ls-remote --sort=-refname ssh",
            vec!["ls-remote", "--sort=-refname", ssh_url.as_str()],
        ),
        (
            "ls-remote -t ssh",
            vec!["ls-remote", "-t", ssh_url.as_str()],
        ),
    ] {
        assert_any_ls_remote_output_matches_stock_git_with_env(dir.path(), &args, &envs, label);
    }
}

#[test]
fn fetch_reads_ssh_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"one\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "tag"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    for client in [&git_client, &zmin_client] {
        git(dir.path(), ["init", client.to_str().expect("client path")]);
        git(client, ["remote", "add", "origin", url.as_str()]);
    }

    command_output_with_env(
        "git",
        &git_client,
        &["fetch", "origin"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git fetch",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["fetch", "origin"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin fetch",
    );
    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "origin/main:a.txt"]),
        git(&git_client, ["cat-file", "-p", "origin/main:a.txt"])
    );
}

#[test]
fn fetch_ssh_wildcard_refspec_prune_no_tags_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("main.txt"), b"main\n").expect("write main");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "main"]);
    git(&work, ["checkout", "-b", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    git(&work, ["tag", "v1"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    for client in [&git_client, &zmin_client] {
        git(dir.path(), ["init", client.to_str().expect("client path")]);
        git(client, ["remote", "add", "origin", url.as_str()]);
    }

    let refspec = "+refs/heads/*:refs/remotes/origin/*";
    command_output_with_env(
        "git",
        &git_client,
        &["fetch", "origin", refspec, "--prune", "--no-tags"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git fetch",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["fetch", "origin", refspec, "--prune", "--no-tags"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin fetch",
    );

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        )
    );
}

#[test]
fn fetch_shallow_since_network_branch_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_shallow_since_remote(dir.path());
    let cutoff = "2020-01-03T00:00:00 +0000";

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "since-http", url.as_str());
    command_output(
        "git",
        &git_client,
        &[
            "fetch",
            "--quiet",
            "--shallow-since",
            cutoff,
            "origin",
            "main",
        ],
        "git shallow-since http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &[
            "fetch",
            "--quiet",
            "--shallow-since",
            cutoff,
            "origin",
            "main",
        ],
        "zmin shallow-since http",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "smart-http shallow-since",
        &git_client,
        &zmin_client,
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "since-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &[
            "fetch",
            "--quiet",
            "--shallow-since",
            cutoff,
            "origin",
            "main",
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git shallow-since ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &[
            "fetch",
            "--quiet",
            "--shallow-since",
            cutoff,
            "origin",
            "main",
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin shallow-since ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "ssh shallow-since",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "since-daemon", url.as_str());
    command_output(
        "git",
        &git_client,
        &[
            "fetch",
            "--quiet",
            "--shallow-since",
            cutoff,
            "origin",
            "main",
        ],
        "git shallow-since daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &[
            "fetch",
            "--quiet",
            "--shallow-since",
            cutoff,
            "origin",
            "main",
        ],
        "zmin shallow-since daemon",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "git-daemon shallow-since",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_shallow_since_network_multiple_refspecs_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_two_branch_shallow_since_remote(dir.path());
    let cutoff = "2020-01-03T00:00:00 +0000";
    let args = [
        "fetch",
        "--quiet",
        "--shallow-since",
        cutoff,
        "origin",
        "refs/heads/main:refs/remotes/origin/main",
        "refs/heads/feature:refs/remotes/origin/feature",
    ];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "since-multi-http", url.as_str());
    command_output("git", &git_client, &args, "git shallow-since multi http");
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin shallow-since multi http",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "smart-http shallow-since multi",
        &git_client,
        &zmin_client,
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        )
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "since-multi-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git shallow-since multi ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin shallow-since multi ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "ssh shallow-since multi",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "since-multi-daemon", url.as_str());
    command_output("git", &git_client, &args, "git shallow-since multi daemon");
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin shallow-since multi daemon",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "git-daemon shallow-since multi",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_shallow_since_network_branchless_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_shallow_since_remote(dir.path());
    let cutoff = "2020-01-03T00:00:00 +0000";
    let args = ["fetch", "--quiet", "--shallow-since", cutoff, "origin"];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "since-branchless-http", url.as_str());
    command_output(
        "git",
        &git_client,
        &args,
        "git shallow-since branchless http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin shallow-since branchless http",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "smart-http shallow-since branchless",
        &git_client,
        &zmin_client,
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "since-branchless-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git shallow-since branchless ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin shallow-since branchless ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "ssh shallow-since branchless",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "since-branchless-daemon", url.as_str());
    command_output(
        "git",
        &git_client,
        &args,
        "git shallow-since branchless daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin shallow-since branchless daemon",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "git-daemon shallow-since branchless",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_shallow_exclude_network_branch_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_shallow_exclude_remote(dir.path());

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "exclude-http", url.as_str());
    command_output(
        "git",
        &git_client,
        &[
            "fetch",
            "--quiet",
            "--shallow-exclude=refs/heads/base",
            "origin",
            "main",
        ],
        "git shallow-exclude http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &[
            "fetch",
            "--quiet",
            "--shallow-exclude=refs/heads/base",
            "origin",
            "main",
        ],
        "zmin shallow-exclude http",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "smart-http shallow-exclude",
        &git_client,
        &zmin_client,
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "exclude-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &[
            "fetch",
            "--quiet",
            "--shallow-exclude=refs/heads/base",
            "origin",
            "main",
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git shallow-exclude ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &[
            "fetch",
            "--quiet",
            "--shallow-exclude=refs/heads/base",
            "origin",
            "main",
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin shallow-exclude ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "ssh shallow-exclude",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "exclude-daemon", url.as_str());
    command_output(
        "git",
        &git_client,
        &[
            "fetch",
            "--quiet",
            "--shallow-exclude=refs/heads/base",
            "origin",
            "main",
        ],
        "git shallow-exclude daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &[
            "fetch",
            "--quiet",
            "--shallow-exclude=refs/heads/base",
            "origin",
            "main",
        ],
        "zmin shallow-exclude daemon",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "git-daemon shallow-exclude",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_shallow_exclude_network_multiple_refspecs_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_two_branch_shallow_exclude_remote(dir.path());
    let args = [
        "fetch",
        "--quiet",
        "--shallow-exclude=refs/heads/base",
        "origin",
        "refs/heads/main:refs/remotes/origin/main",
        "refs/heads/feature:refs/remotes/origin/feature",
    ];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "exclude-multi-http", url.as_str());
    command_output("git", &git_client, &args, "git shallow-exclude multi http");
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin shallow-exclude multi http",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "smart-http shallow-exclude multi",
        &git_client,
        &zmin_client,
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        )
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "exclude-multi-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git shallow-exclude multi ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin shallow-exclude multi ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "ssh shallow-exclude multi",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "exclude-multi-daemon", url.as_str());
    command_output(
        "git",
        &git_client,
        &args,
        "git shallow-exclude multi daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin shallow-exclude multi daemon",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "git-daemon shallow-exclude multi",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_shallow_exclude_network_branchless_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_shallow_exclude_remote(dir.path());
    let args = [
        "fetch",
        "--quiet",
        "--shallow-exclude=refs/heads/base",
        "origin",
    ];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "exclude-branchless-http", url.as_str());
    command_output(
        "git",
        &git_client,
        &args,
        "git shallow-exclude branchless http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin shallow-exclude branchless http",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "smart-http shallow-exclude branchless",
        &git_client,
        &zmin_client,
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "exclude-branchless-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git shallow-exclude branchless ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin shallow-exclude branchless ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "ssh shallow-exclude branchless",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "exclude-branchless-daemon", url.as_str());
    command_output(
        "git",
        &git_client,
        &args,
        "git shallow-exclude branchless daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin shallow-exclude branchless daemon",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "git-daemon shallow-exclude branchless",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_shallow_exclude_repeated_network_branch_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let (remote, left_tip, right_tip) = prepare_repeated_shallow_exclude_remote(dir.path());
    let args = [
        "fetch",
        "--quiet",
        "--shallow-exclude=refs/heads/left",
        "--shallow-exclude",
        "refs/heads/right",
        "origin",
        "main",
    ];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "exclude-repeated-http", url.as_str());
    command_output(
        "git",
        &git_client,
        &args,
        "git shallow-exclude repeated http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin shallow-exclude repeated http",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "smart-http shallow-exclude repeated",
        &git_client,
        &zmin_client,
    );
    for excluded_tip in [&left_tip, &right_tip] {
        assert_eq!(
            git_status_args(&zmin_client, &["cat-file", "-e", excluded_tip]),
            git_status_args(&git_client, &["cat-file", "-e", excluded_tip]),
            "smart-http shallow-exclude repeated"
        );
    }

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "exclude-repeated-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git shallow-exclude repeated ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin shallow-exclude repeated ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "ssh shallow-exclude repeated",
        &git_client,
        &zmin_client,
    );
    for excluded_tip in [&left_tip, &right_tip] {
        assert_eq!(
            git_status_args(&zmin_client, &["cat-file", "-e", excluded_tip]),
            git_status_args(&git_client, &["cat-file", "-e", excluded_tip]),
            "ssh shallow-exclude repeated"
        );
    }

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "exclude-repeated-daemon", url.as_str());
    command_output(
        "git",
        &git_client,
        &args,
        "git shallow-exclude repeated daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin shallow-exclude repeated daemon",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "git-daemon shallow-exclude repeated",
        &git_client,
        &zmin_client,
    );
    for excluded_tip in [&left_tip, &right_tip] {
        assert_eq!(
            git_status_args(&zmin_client, &["cat-file", "-e", excluded_tip]),
            git_status_args(&git_client, &["cat-file", "-e", excluded_tip]),
            "git-daemon shallow-exclude repeated"
        );
    }
}

#[test]
fn fetch_deepen_network_branch_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_shallow_since_remote(dir.path());

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "deepen-http", url.as_str());
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--depth=1", "origin", "main"],
        "git depth http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--depth=1", "origin", "main"],
        "zmin depth http",
    );
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--deepen=1", "origin", "main"],
        "git deepen http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--deepen=1", "origin", "main"],
        "zmin deepen http",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "smart-http deepen",
        &git_client,
        &zmin_client,
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "deepen-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &["fetch", "--quiet", "--depth=1", "origin", "main"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git depth ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--depth=1", "origin", "main"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin depth ssh",
    );
    command_output_with_env(
        "git",
        &git_client,
        &["fetch", "--quiet", "--deepen=1", "origin", "main"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git deepen ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--deepen=1", "origin", "main"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin deepen ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git("ssh deepen", &git_client, &zmin_client);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "deepen-daemon", url.as_str());
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--depth=1", "origin", "main"],
        "git depth daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--depth=1", "origin", "main"],
        "zmin depth daemon",
    );
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--deepen=1", "origin", "main"],
        "git deepen daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--deepen=1", "origin", "main"],
        "zmin deepen daemon",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "git-daemon deepen",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_deepen_network_multiple_refspecs_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let (remote, _, _) = prepare_two_branch_shallow_remote(dir.path());
    let depth_args = [
        "fetch",
        "--quiet",
        "--depth=1",
        "origin",
        "refs/heads/main:refs/remotes/origin/main",
        "refs/heads/feature:refs/remotes/origin/feature",
    ];
    let deepen_args = [
        "fetch",
        "--quiet",
        "--deepen=1",
        "origin",
        "refs/heads/main:refs/remotes/origin/main",
        "refs/heads/feature:refs/remotes/origin/feature",
    ];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "deepen-multi-http", url.as_str());
    command_output("git", &git_client, &depth_args, "git depth multi http");
    command_output(
        zmin_bin(),
        &zmin_client,
        &depth_args,
        "zmin depth multi http",
    );
    command_output("git", &git_client, &deepen_args, "git deepen multi http");
    command_output(
        zmin_bin(),
        &zmin_client,
        &deepen_args,
        "zmin deepen multi http",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "smart-http deepen multi",
        &git_client,
        &zmin_client,
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        )
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "deepen-multi-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &depth_args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git depth multi ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &depth_args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin depth multi ssh",
    );
    command_output_with_env(
        "git",
        &git_client,
        &deepen_args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git deepen multi ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &deepen_args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin deepen multi ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "ssh deepen multi",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "deepen-multi-daemon", url.as_str());
    command_output("git", &git_client, &depth_args, "git depth multi daemon");
    command_output(
        zmin_bin(),
        &zmin_client,
        &depth_args,
        "zmin depth multi daemon",
    );
    command_output("git", &git_client, &deepen_args, "git deepen multi daemon");
    command_output(
        zmin_bin(),
        &zmin_client,
        &deepen_args,
        "zmin deepen multi daemon",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "git-daemon deepen multi",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_deepen_network_branchless_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_shallow_since_remote(dir.path());

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "deepen-branchless-http", url.as_str());
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--depth=1", "origin"],
        "git depth branchless http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--depth=1", "origin"],
        "zmin depth branchless http",
    );
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--deepen=1", "origin"],
        "git deepen branchless http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--deepen=1", "origin"],
        "zmin deepen branchless http",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "smart-http deepen branchless",
        &git_client,
        &zmin_client,
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "deepen-branchless-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &["fetch", "--quiet", "--depth=1", "origin"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git depth branchless ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--depth=1", "origin"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin depth branchless ssh",
    );
    command_output_with_env(
        "git",
        &git_client,
        &["fetch", "--quiet", "--deepen=1", "origin"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git deepen branchless ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--deepen=1", "origin"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin deepen branchless ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "ssh deepen branchless",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "deepen-branchless-daemon", url.as_str());
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--depth=1", "origin"],
        "git depth branchless daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--depth=1", "origin"],
        "zmin depth branchless daemon",
    );
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--deepen=1", "origin"],
        "git deepen branchless daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--deepen=1", "origin"],
        "zmin deepen branchless daemon",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "git-daemon deepen branchless",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_unshallow_network_branch_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_shallow_since_remote(dir.path());

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "unshallow-http", url.as_str());
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--depth=1", "origin", "main"],
        "git depth http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--depth=1", "origin", "main"],
        "zmin depth http",
    );
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--unshallow", "origin", "main"],
        "git unshallow http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--unshallow", "origin", "main"],
        "zmin unshallow http",
    );
    assert_network_branch_unshallow_fetch_matches_stock_git(
        "smart-http unshallow",
        &git_client,
        &zmin_client,
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "unshallow-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &["fetch", "--quiet", "--depth=1", "origin", "main"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git depth ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--depth=1", "origin", "main"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin depth ssh",
    );
    command_output_with_env(
        "git",
        &git_client,
        &["fetch", "--quiet", "--unshallow", "origin", "main"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git unshallow ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--unshallow", "origin", "main"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin unshallow ssh",
    );
    assert_network_branch_unshallow_fetch_matches_stock_git(
        "ssh unshallow",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "unshallow-daemon", url.as_str());
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--depth=1", "origin", "main"],
        "git depth daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--depth=1", "origin", "main"],
        "zmin depth daemon",
    );
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--unshallow", "origin", "main"],
        "git unshallow daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--unshallow", "origin", "main"],
        "zmin unshallow daemon",
    );
    assert_network_branch_unshallow_fetch_matches_stock_git(
        "git-daemon unshallow",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_unshallow_network_multiple_refspecs_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let (remote, _, _) = prepare_two_branch_shallow_remote(dir.path());
    let depth_args = [
        "fetch",
        "--quiet",
        "--depth=1",
        "origin",
        "refs/heads/main:refs/remotes/origin/main",
        "refs/heads/feature:refs/remotes/origin/feature",
    ];
    let unshallow_args = [
        "fetch",
        "--quiet",
        "--unshallow",
        "origin",
        "refs/heads/main:refs/remotes/origin/main",
        "refs/heads/feature:refs/remotes/origin/feature",
    ];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "unshallow-multi-http", url.as_str());
    command_output("git", &git_client, &depth_args, "git depth multi http");
    command_output(
        zmin_bin(),
        &zmin_client,
        &depth_args,
        "zmin depth multi http",
    );
    command_output(
        "git",
        &git_client,
        &unshallow_args,
        "git unshallow multi http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &unshallow_args,
        "zmin unshallow multi http",
    );
    assert_network_branch_unshallow_fetch_matches_stock_git(
        "smart-http unshallow multi",
        &git_client,
        &zmin_client,
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        )
    );
    assert_eq!(
        git(&zmin_client, ["rev-list", "--count", "origin/feature"]),
        git(&git_client, ["rev-list", "--count", "origin/feature"])
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "unshallow-multi-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &depth_args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git depth multi ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &depth_args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin depth multi ssh",
    );
    command_output_with_env(
        "git",
        &git_client,
        &unshallow_args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git unshallow multi ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &unshallow_args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin unshallow multi ssh",
    );
    assert_network_branch_unshallow_fetch_matches_stock_git(
        "ssh unshallow multi",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "unshallow-multi-daemon", url.as_str());
    command_output("git", &git_client, &depth_args, "git depth multi daemon");
    command_output(
        zmin_bin(),
        &zmin_client,
        &depth_args,
        "zmin depth multi daemon",
    );
    command_output(
        "git",
        &git_client,
        &unshallow_args,
        "git unshallow multi daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &unshallow_args,
        "zmin unshallow multi daemon",
    );
    assert_network_branch_unshallow_fetch_matches_stock_git(
        "git-daemon unshallow multi",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_upload_pack_ssh_shallow_modes_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_shallow_since_remote(dir.path());
    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let (wrapper, log) = write_upload_pack_wrapper(dir.path(), "ssh-shallow");
    let wrapper_command = wrapper.to_str().expect("wrapper path");
    let upload_pack_arg = format!("--upload-pack={wrapper_command}");
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "upload-pack-ssh-shallow", url.as_str());
    let ssh_env = [("GIT_SSH_COMMAND", fake_ssh_arg.as_str())];

    command_output_with_env(
        "git",
        &git_client,
        &[
            "fetch",
            "--quiet",
            "--depth=1",
            &upload_pack_arg,
            "origin",
            "main",
        ],
        &ssh_env,
        "git upload-pack depth ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &[
            "fetch",
            "--quiet",
            "--depth=1",
            &upload_pack_arg,
            "origin",
            "main",
        ],
        &ssh_env,
        "zmin upload-pack depth ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "ssh upload-pack depth",
        &git_client,
        &zmin_client,
    );

    command_output_with_env(
        "git",
        &git_client,
        &[
            "fetch",
            "--quiet",
            "--deepen=1",
            &upload_pack_arg,
            "origin",
            "main",
        ],
        &ssh_env,
        "git upload-pack deepen ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &[
            "fetch",
            "--quiet",
            "--deepen=1",
            &upload_pack_arg,
            "origin",
            "main",
        ],
        &ssh_env,
        "zmin upload-pack deepen ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "ssh upload-pack deepen",
        &git_client,
        &zmin_client,
    );

    command_output_with_env(
        "git",
        &git_client,
        &[
            "fetch",
            "--quiet",
            "--unshallow",
            &upload_pack_arg,
            "origin",
            "main",
        ],
        &ssh_env,
        "git upload-pack unshallow ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &[
            "fetch",
            "--quiet",
            "--unshallow",
            &upload_pack_arg,
            "origin",
            "main",
        ],
        &ssh_env,
        "zmin upload-pack unshallow ssh",
    );
    assert_network_branch_unshallow_fetch_matches_stock_git(
        "ssh upload-pack unshallow",
        &git_client,
        &zmin_client,
    );

    let log_contents = fs::read_to_string(&log).expect("upload-pack wrapper log");
    assert!(
        log_contents.lines().count() >= 6,
        "expected stock Git and Zmin to invoke upload-pack wrapper for each fetch:\n{log_contents}"
    );
    assert!(
        log_contents.contains(remote.to_str().expect("remote path")),
        "expected wrapper log to include remote path:\n{log_contents}"
    );
}

#[test]
fn fetch_unshallow_network_branchless_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_shallow_since_remote(dir.path());

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "unshallow-branchless-http", url.as_str());
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--depth=1", "origin"],
        "git depth branchless http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--depth=1", "origin"],
        "zmin depth branchless http",
    );
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--unshallow", "origin"],
        "git unshallow branchless http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--unshallow", "origin"],
        "zmin unshallow branchless http",
    );
    assert_network_branch_unshallow_fetch_matches_stock_git(
        "smart-http unshallow branchless",
        &git_client,
        &zmin_client,
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "unshallow-branchless-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &["fetch", "--quiet", "--depth=1", "origin"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git depth branchless ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--depth=1", "origin"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin depth branchless ssh",
    );
    command_output_with_env(
        "git",
        &git_client,
        &["fetch", "--quiet", "--unshallow", "origin"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git unshallow branchless ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--unshallow", "origin"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin unshallow branchless ssh",
    );
    assert_network_branch_unshallow_fetch_matches_stock_git(
        "ssh unshallow branchless",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "unshallow-branchless-daemon", url.as_str());
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--depth=1", "origin"],
        "git depth branchless daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--depth=1", "origin"],
        "zmin depth branchless daemon",
    );
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--unshallow", "origin"],
        "git unshallow branchless daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--unshallow", "origin"],
        "zmin unshallow branchless daemon",
    );
    assert_network_branch_unshallow_fetch_matches_stock_git(
        "git-daemon unshallow branchless",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_update_shallow_network_branch_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_update_shallow_remote(dir.path());

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/shallow.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "update-shallow-http", url.as_str());
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--update-shallow", "origin", "main"],
        "git update-shallow http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--update-shallow", "origin", "main"],
        "zmin update-shallow http",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "smart-http update-shallow",
        &git_client,
        &zmin_client,
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "update-shallow-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &["fetch", "--quiet", "--update-shallow", "origin", "main"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git update-shallow ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--update-shallow", "origin", "main"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin update-shallow ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "ssh update-shallow",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/shallow.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "update-shallow-daemon", url.as_str());
    command_output(
        "git",
        &git_client,
        &["fetch", "--quiet", "--update-shallow", "origin", "main"],
        "git update-shallow daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--quiet", "--update-shallow", "origin", "main"],
        "zmin update-shallow daemon",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "git-daemon update-shallow",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_update_shallow_network_multiple_refspecs_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_two_branch_update_shallow_remote(dir.path());
    let args = [
        "fetch",
        "--quiet",
        "--update-shallow",
        "origin",
        "refs/heads/main:refs/remotes/origin/main",
        "refs/heads/feature:refs/remotes/origin/feature",
    ];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/shallow-two-branch.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "update-shallow-multi-http", url.as_str());
    command_output("git", &git_client, &args, "git update-shallow multi http");
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin update-shallow multi http",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "smart-http update-shallow multi",
        &git_client,
        &zmin_client,
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        )
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "update-shallow-multi-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git update-shallow multi ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin update-shallow multi ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "ssh update-shallow multi",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/shallow-two-branch.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "update-shallow-multi-daemon", url.as_str());
    command_output("git", &git_client, &args, "git update-shallow multi daemon");
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin update-shallow multi daemon",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "git-daemon update-shallow multi",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_update_shallow_network_branchless_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_two_branch_update_shallow_remote(dir.path());
    let args = ["fetch", "--quiet", "--update-shallow", "origin"];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/shallow-two-branch.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "update-shallow-branchless-http", url.as_str());
    command_output(
        "git",
        &git_client,
        &args,
        "git update-shallow branchless http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin update-shallow branchless http",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "smart-http update-shallow branchless",
        &git_client,
        &zmin_client,
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        )
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "update-shallow-branchless-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git update-shallow branchless ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin update-shallow branchless ssh",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "ssh update-shallow branchless",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/shallow-two-branch.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "update-shallow-branchless-daemon", url.as_str());
    command_output(
        "git",
        &git_client,
        &args,
        "git update-shallow branchless daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin update-shallow branchless daemon",
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "git-daemon update-shallow branchless",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_filter_blob_none_network_branch_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_filter_remote(dir.path());
    let args = ["fetch", "--quiet", "--filter=blob:none", "origin", "main"];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-http", url.as_str());
    let stock_filter_output =
        pinned_command_any_output(&git_client, &args, "pinned Git filter http");
    assert_eq!(stock_filter_output.0, 0, "pinned Git filter http");
    command_output(zmin_bin(), &zmin_client, &args, "zmin filter http");
    assert_filtered_fetch_matches_stock_git("smart-http filter", &git_client, &zmin_client);
    assert_promisor_marker_roles_match("smart-http filter", &git_client, &zmin_client);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git filter ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin filter ssh",
    );
    assert_filtered_fetch_matches_stock_git("ssh filter", &git_client, &zmin_client);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/filter.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-daemon", url.as_str());
    command_output("git", &git_client, &args, "git filter daemon");
    command_output(zmin_bin(), &zmin_client, &args, "zmin filter daemon");
    assert_filtered_fetch_matches_stock_git("git-daemon filter", &git_client, &zmin_client);
}

#[test]
fn fetch_filter_blob_none_with_depth_network_branch_transports_match_stock_git() {
    let dir = TempDir::new().expect("filter depth temp dir");
    let remote = prepare_filter_remote(dir.path());
    let args = [
        "fetch",
        "--quiet",
        "--depth=1",
        "--filter=blob:none",
        "origin",
        "main",
    ];

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let ssh_url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-depth-ssh", ssh_url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git filter depth ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin filter depth ssh",
    );
    assert_filter_fetch_common_matches_stock_git("ssh filter depth", &git_client, &zmin_client);
    assert_promisor_marker_roles_match("ssh filter depth", &git_client, &zmin_client);
    assert_eq!(
        fs::read(git_client.join(".git/shallow")).expect("stock ssh shallow"),
        fs::read(zmin_client.join(".git/shallow")).expect("zmin ssh shallow"),
        "ssh filter depth shallow state"
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let daemon_url = format!("git://127.0.0.1:{port}/filter.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-depth-daemon", &daemon_url);
    command_output("git", &git_client, &args, "git filter depth daemon");
    command_output(zmin_bin(), &zmin_client, &args, "zmin filter depth daemon");
    assert_filter_fetch_common_matches_stock_git(
        "git-daemon filter depth",
        &git_client,
        &zmin_client,
    );
    assert_promisor_marker_roles_match("git-daemon filter depth", &git_client, &zmin_client);
    assert_eq!(
        fs::read(git_client.join(".git/shallow")).expect("stock daemon shallow"),
        fs::read(zmin_client.join(".git/shallow")).expect("zmin daemon shallow"),
        "git-daemon filter depth shallow state"
    );
}

#[test]
fn fetch_filter_blob_none_with_depth_unsupported_ssh_matches_stock_git() {
    let dir = TempDir::new().expect("unsupported filter depth temp dir");
    let remote = prepare_filter_remote(dir.path());
    pinned_git_args(&remote, ["config", "uploadpack.allowFilter", "false"]);
    let args = [
        "fetch",
        "--quiet",
        "--depth=1",
        "--filter=blob:none",
        "origin",
        "main",
    ];
    let warning = "warning: filtering not recognized by server, ignoring";
    let pinned_helper_path = pinned_git_helper_path(&pinned_http_bundle_root());
    let pinned_helper_path = pinned_helper_path
        .to_str()
        .expect("pinned Git helper path is UTF-8");

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let ssh_url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_pinned_network_fetch_clients(dir.path(), "filter-depth-unsupported-ssh", &ssh_url);
    let stock_output = pinned_command_any_output_with_env(
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "stock unsupported filter depth ssh",
    );
    let zmin_output = command_any_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[
            ("GIT_SSH_COMMAND", fake_ssh_arg.as_str()),
            ("PATH", pinned_helper_path),
        ],
        "Zmin unsupported filter depth ssh",
    );
    assert_eq!(
        zmin_output.0, stock_output.0,
        "unsupported SSH filter depth status"
    );
    assert_eq!(stock_output.2.matches(warning).count(), 2);
    assert_eq!(zmin_output.2.matches(warning).count(), 2);
    assert_filter_fetch_common_matches_stock_git(
        "unsupported SSH filter depth",
        &git_client,
        &zmin_client,
    );
    assert_promisor_marker_roles_match("unsupported SSH filter depth", &git_client, &zmin_client);
    assert_eq!(
        fs::read(git_client.join(".git/shallow")).expect("stock unsupported ssh shallow"),
        fs::read(zmin_client.join(".git/shallow")).expect("zmin unsupported ssh shallow"),
    );
}

#[test]
fn fetch_filter_unsupported_ssh_and_daemon_matches_promisor_state() {
    let dir = TempDir::new().expect("unsupported filter temp dir");
    let remote = prepare_filter_remote(dir.path());
    pinned_git_args(&remote, ["config", "uploadpack.allowFilter", "false"]);
    let args = ["fetch", "--filter=blob:none", "origin", "main"];

    let assert_fallback = |label: &str,
                           git_client: &std::path::Path,
                           zmin_client: &std::path::Path,
                           stock_output: &(i32, String, String),
                           zmin_output: &(i32, String, String)| {
        assert_eq!(zmin_output.0, stock_output.0, "{label}: exit status");
        assert_eq!(
            pinned_git_args(zmin_client, ["show-ref"]),
            pinned_git_args(git_client, ["show-ref"]),
            "{label}: refs"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("stock FETCH_HEAD"),
            "{label}: FETCH_HEAD"
        );
        for key in [
            "remote.origin.promisor",
            "remote.origin.partialclonefilter",
            "extensions.partialclone",
        ] {
            let stock =
                pinned_command_any_output(git_client, ["config", "--get", key].as_ref(), label);
            let zmin = command_any_output(
                zmin_bin(),
                zmin_client,
                ["config", "--get", key].as_ref(),
                label,
            );
            if key == "extensions.partialclone" {
                assert_ne!(stock.0, 0, "{label}: stock unexpectedly set {key}");
                assert_ne!(zmin.0, 0, "{label}: Zmin unexpectedly set {key}");
            } else {
                assert_eq!(stock.0, 0, "{label}: stock missing {key}");
                assert_eq!(zmin.0, 0, "{label}: Zmin missing {key}");
                assert_eq!(zmin.1, stock.1, "{label}: {key} value");
            }
        }
        assert_promisor_marker_roles_match(label, git_client, zmin_client);
        let warning = "warning: filtering not recognized by server, ignoring";
        assert!(
            stock_output.2.matches(warning).count() == 2,
            "{label}: stock warning missing: {}",
            stock_output.2
        );
        assert_eq!(
            zmin_output.2.matches(warning).count(),
            2,
            "{label}: Zmin warning count: {}",
            zmin_output.2
        );
    };

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let pinned_helper_path = pinned_git_helper_path(&pinned_http_bundle_root());
    let pinned_helper_path = pinned_helper_path
        .to_str()
        .expect("pinned Git helper path is UTF-8");
    let ssh_url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_pinned_network_fetch_clients(dir.path(), "filter-unsupported-ssh", ssh_url.as_str());
    let stock_output = pinned_command_any_output_with_env(
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "stock unsupported SSH filter",
    );
    let zmin_output = command_any_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[
            ("GIT_SSH_COMMAND", fake_ssh_arg.as_str()),
            ("PATH", pinned_helper_path),
        ],
        "Zmin unsupported SSH filter",
    );
    assert_fallback(
        "unsupported SSH filter",
        &git_client,
        &zmin_client,
        &stock_output,
        &zmin_output,
    );

    let configured_args = ["fetch", "--filter=blob:none", "origin"];
    let (git_client, zmin_client) = init_pinned_network_fetch_clients(
        dir.path(),
        "filter-unsupported-ssh-configured",
        ssh_url.as_str(),
    );
    let stock_output = pinned_command_any_output_with_env(
        &git_client,
        &configured_args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "stock unsupported configured SSH filter",
    );
    let zmin_output = command_any_output_with_env(
        zmin_bin(),
        &zmin_client,
        &configured_args,
        &[
            ("GIT_SSH_COMMAND", fake_ssh_arg.as_str()),
            ("PATH", pinned_helper_path),
        ],
        "Zmin unsupported configured SSH filter",
    );
    assert_fallback(
        "unsupported configured SSH filter",
        &git_client,
        &zmin_client,
        &stock_output,
        &zmin_output,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn_pinned(dir.path(), port);
    let daemon_url = format!("git://127.0.0.1:{port}/filter.git");
    let (git_client, zmin_client) =
        init_pinned_network_fetch_clients(dir.path(), "filter-unsupported-daemon", &daemon_url);
    let stock_output =
        pinned_command_any_output(&git_client, &args, "stock unsupported daemon filter");
    let zmin_output = command_any_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "Zmin unsupported daemon filter",
    );
    assert_fallback(
        "unsupported daemon filter",
        &git_client,
        &zmin_client,
        &stock_output,
        &zmin_output,
    );

    let (git_client, zmin_client) = init_pinned_network_fetch_clients(
        dir.path(),
        "filter-unsupported-daemon-configured",
        &daemon_url,
    );
    let stock_output = pinned_command_any_output(
        &git_client,
        &configured_args,
        "stock unsupported configured daemon filter",
    );
    let zmin_output = command_any_output(
        zmin_bin(),
        &zmin_client,
        &configured_args,
        "Zmin unsupported configured daemon filter",
    );
    assert_fallback(
        "unsupported configured daemon filter",
        &git_client,
        &zmin_client,
        &stock_output,
        &zmin_output,
    );
}

#[test]
fn smart_http_filter_capability_v0_v1_fetch_and_clone_matches_pinned_git() {
    let dir = TempDir::new().expect("unsupported HTTP filter temp dir");
    let remote = prepare_filter_remote(dir.path());
    pinned_git_args(&remote, ["config", "uploadpack.allowFilter", "false"]);
    let warning = "warning: filtering not recognized by server, ignoring";

    for &(protocol, protocol_label) in &[(0, "v0"), (1, "v1")] {
        let server = SmartHttpServer::new(dir.path().to_path_buf());
        let url = format!("http://127.0.0.1:{}/filter.git", server.port);
        let (stock_client, zmin_client) = init_pinned_network_fetch_clients(
            dir.path(),
            &format!("filter-unsupported-http-{protocol_label}"),
            &url,
        );
        let protocol_value = protocol.to_string();
        pinned_git_args(
            &stock_client,
            ["config", "protocol.version", protocol_value.as_str()],
        );
        run_zmin(
            &zmin_client,
            ["config", "protocol.version", protocol_value.as_str()],
        );
        let args = ["fetch", "--quiet", "--filter=blob:none", "origin", "main"];
        let stock_output = pinned_command_any_output(
            &stock_client,
            &args,
            &format!("pinned unsupported HTTP filter {protocol_label} fetch"),
        );
        let zmin_output = command_any_output(
            zmin_bin(),
            &zmin_client,
            &args,
            &format!("Zmin unsupported HTTP filter {protocol_label} fetch"),
        );
        assert_eq!(
            zmin_output, stock_output,
            "unsupported HTTP {protocol_label} fetch"
        );
        assert_eq!(
            stock_output.2.matches(warning).count(),
            2,
            "stock warning count"
        );
        assert_eq!(
            zmin_output.2.matches(warning).count(),
            2,
            "Zmin warning count"
        );
        assert_eq!(
            server.upload_pack_requests(),
            1,
            "unsupported HTTP upload-pack count: {:?}",
            server.upload_pack_bodies_text()
        );
        let bodies = server.upload_pack_bodies_text();
        assert_eq!(bodies.len(), 1, "unsupported HTTP request count");
        assert!(
            bodies[0]
                .lines()
                .any(|line| line.starts_with("want ") && line.ends_with(" filter")),
            "filter capability missing from unsupported wire"
        );
        assert!(
            !bodies[0].contains("filter blob:none\n"),
            "filter spec leaked onto unsupported wire"
        );
        assert_filtered_fetch_matches_stock_git(
            &format!("unsupported HTTP {protocol_label} fetch"),
            &stock_client,
            &zmin_client,
        );
        assert_promisor_marker_roles_match(
            &format!("unsupported HTTP {protocol_label} fetch"),
            &stock_client,
            &zmin_client,
        );

        let stock_depth_server = SmartHttpServer::new(dir.path().to_path_buf());
        let depth_url = format!("http://127.0.0.1:{}/filter.git", stock_depth_server.port);
        let (stock_depth, zmin_depth) = init_pinned_network_fetch_clients(
            dir.path(),
            &format!("filter-unsupported-http-{protocol_label}-depth"),
            &depth_url,
        );
        pinned_git_args(
            &stock_depth,
            ["config", "protocol.version", protocol_value.as_str()],
        );
        run_zmin(
            &zmin_depth,
            ["config", "protocol.version", protocol_value.as_str()],
        );
        let depth_args = [
            "fetch",
            "--quiet",
            "--depth=1",
            "--filter=blob:none",
            "origin",
            "main",
        ];
        let stock_depth_output = pinned_command_any_output(
            &stock_depth,
            &depth_args,
            &format!("pinned unsupported HTTP {protocol_label} depth fetch"),
        );
        let zmin_depth_output = command_any_output(
            zmin_bin(),
            &zmin_depth,
            &depth_args,
            &format!("Zmin unsupported HTTP {protocol_label} depth fetch"),
        );
        assert_eq!(
            zmin_depth_output, stock_depth_output,
            "unsupported HTTP depth fetch"
        );
        assert_eq!(
            stock_depth_output.2.matches(warning).count(),
            2,
            "stock depth warning count"
        );
        assert_eq!(
            zmin_depth_output.2.matches(warning).count(),
            2,
            "Zmin depth warning count"
        );
        assert_eq!(
            stock_depth_server.upload_pack_requests(),
            1,
            "unsupported HTTP depth upload-pack count"
        );
        let depth_bodies = stock_depth_server.upload_pack_bodies_text();
        assert_eq!(
            depth_bodies.len(),
            1,
            "unsupported HTTP depth request count"
        );
        assert!(
            depth_bodies[0].contains("deepen 1"),
            "depth missing from fallback wire"
        );
        assert!(
            depth_bodies[0]
                .lines()
                .any(|line| line.starts_with("want ") && line.ends_with(" filter")),
            "filter capability missing from depth fallback wire"
        );
        assert!(
            !depth_bodies[0].contains("filter blob:none\n"),
            "filter spec leaked onto depth fallback wire"
        );
        assert_network_branch_shallow_fetch_matches_stock_git(
            &format!("unsupported HTTP {protocol_label} depth fetch"),
            &stock_depth,
            &zmin_depth,
        );
        assert_promisor_marker_roles_match(
            &format!("unsupported HTTP {protocol_label} depth fetch"),
            &stock_depth,
            &zmin_depth,
        );

        let stock_clone = dir
            .path()
            .join(format!("stock-filter-unsupported-http-{protocol_label}"));
        let zmin_clone = dir
            .path()
            .join(format!("zmin-filter-unsupported-http-{protocol_label}"));
        let clone_server = SmartHttpServer::new(dir.path().to_path_buf());
        let clone_url = format!("http://127.0.0.1:{}/filter.git", clone_server.port);
        let stock_protocol_config = format!("protocol.version={protocol}");
        let zmin_protocol_config = format!("--config=protocol.version={protocol}");
        let mut stock_clone_args = vec!["clone", "-q", "--filter=blob:none"];
        let mut zmin_clone_args = vec!["clone", "-q", "--filter=blob:none"];
        stock_clone_args.splice(0..0, ["-c", stock_protocol_config.as_str()]);
        zmin_clone_args.splice(0..0, [zmin_protocol_config.as_str()]);
        stock_clone_args.extend([
            clone_url.as_str(),
            stock_clone.to_str().expect("stock clone"),
        ]);
        zmin_clone_args.extend([clone_url.as_str(), zmin_clone.to_str().expect("Zmin clone")]);
        let stock_clone_output = pinned_command_any_output(
            dir.path(),
            &stock_clone_args,
            &format!("pinned unsupported HTTP {protocol_label} clone"),
        );
        let zmin_clone_output = command_any_output(
            zmin_bin(),
            dir.path(),
            &zmin_clone_args,
            &format!("Zmin unsupported HTTP {protocol_label} clone"),
        );
        assert_eq!(
            zmin_clone_output, stock_clone_output,
            "unsupported HTTP clone"
        );
        assert_eq!(
            stock_clone_output.2.matches(warning).count(),
            2,
            "stock clone warning count"
        );
        assert_eq!(
            zmin_clone_output.2.matches(warning).count(),
            2,
            "Zmin clone warning count"
        );
        assert_eq!(
            clone_server.upload_pack_requests(),
            1,
            "unsupported HTTP clone upload-pack"
        );
        let clone_bodies = clone_server.upload_pack_bodies_text();
        assert_eq!(
            clone_bodies.len(),
            1,
            "unsupported HTTP clone request count"
        );
        assert!(
            clone_bodies[0]
                .lines()
                .any(|line| line.starts_with("want ") && line.ends_with(" filter")),
            "filter capability missing from clone wire"
        );
        assert!(
            !clone_bodies[0].contains("filter blob:none\n"),
            "filter spec leaked onto clone wire"
        );
        for repository in [&stock_clone, &zmin_clone] {
            assert_eq!(
                pinned_git_args(repository, ["config", "--get", "remote.origin.promisor"]),
                "true"
            );
            assert_eq!(
                pinned_git_args(
                    repository,
                    ["config", "--get", "remote.origin.partialclonefilter"]
                ),
                "blob:none"
            );
            assert_no_http_temporary_pack_entries(repository, "unsupported HTTP clone");
        }
        assert_eq!(
            http_object_file_snapshot(&zmin_clone),
            http_object_file_snapshot(&stock_clone),
            "unsupported HTTP clone storage"
        );
        assert_promisor_marker_roles_match("unsupported HTTP clone", &stock_clone, &zmin_clone);
    }
}

#[test]
fn fetch_filter_blob_none_smart_http_marker_matches_pinned_git() {
    let dir = TempDir::new().expect("smart HTTP filter marker temp dir");
    prepare_filter_remote(dir.path());
    let args = ["fetch", "--quiet", "--filter=blob:none", "origin", "main"];

    let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
    let stock_url = format!("http://127.0.0.1:{}/filter.git", stock_server.port);
    let (git_client, _) =
        init_network_fetch_clients(dir.path(), "filter-marker-stock", stock_url.as_str());
    let stock_output = pinned_command_any_output(
        &git_client,
        &args,
        "pinned Git smart HTTP filter marker fetch",
    );
    assert_eq!(stock_output.0, 0, "pinned Git filter marker fetch");

    let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
    let zmin_url = format!("http://127.0.0.1:{}/filter.git", zmin_server.port);
    let (_, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-marker-zmin", zmin_url.as_str());
    let zmin_output = command_any_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "Zmin smart HTTP filter marker fetch",
    );
    assert_eq!(zmin_output, stock_output, "filter marker fetch tuple");
    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"]),
        "smart-http filter marker refs"
    );
    assert_eq!(
        run_zmin(&zmin_client, ["config", "--get", "remote.origin.promisor"]),
        git(&git_client, ["config", "--get", "remote.origin.promisor"]),
        "smart-http filter marker promisor config"
    );
    assert_eq!(
        run_zmin(
            &zmin_client,
            ["config", "--get", "remote.origin.partialclonefilter"]
        ),
        git(
            &git_client,
            ["config", "--get", "remote.origin.partialclonefilter"]
        ),
        "smart-http filter marker filter config"
    );
    assert_eq!(
        filtered_blob_local_presence(zmin_bin(), &zmin_client, "a.txt"),
        filtered_blob_local_presence(
            required_pinned_stock_git()
                .to_str()
                .expect("pinned Git path"),
            &git_client,
            "a.txt",
        ),
        "smart-http filter marker blob presence"
    );
    assert_promisor_marker_roles_match("smart-http filter marker", &git_client, &zmin_client);
}

#[test]
fn fetch_filter_blob_none_network_branchless_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_filter_remote(dir.path());
    let args = ["fetch", "--quiet", "--filter=blob:none", "origin"];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-branchless-http", url.as_str());
    let stock_filter_output =
        pinned_command_any_output(&git_client, &args, "pinned Git filter branchless http");
    assert_eq!(
        stock_filter_output.0, 0,
        "pinned Git filter branchless http"
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter branchless http",
    );
    assert_filtered_fetch_matches_stock_git(
        "smart-http filter branchless",
        &git_client,
        &zmin_client,
    );
    assert_promisor_marker_roles_match("smart-http filter branchless", &git_client, &zmin_client);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-branchless-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git filter branchless ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin filter branchless ssh",
    );
    assert_filtered_fetch_matches_stock_git("ssh filter branchless", &git_client, &zmin_client);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/filter.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-branchless-daemon", url.as_str());
    command_output("git", &git_client, &args, "git filter branchless daemon");
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter branchless daemon",
    );
    assert_filtered_fetch_matches_stock_git(
        "git-daemon filter branchless",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_filter_blob_limit_network_branch_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_filter_remote(dir.path());
    let args = [
        "fetch",
        "--quiet",
        "--filter=blob:limit=8",
        "origin",
        "main",
    ];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-limit-http", url.as_str());
    command_output("git", &git_client, &args, "git filter limit http");
    command_output(zmin_bin(), &zmin_client, &args, "zmin filter limit http");
    assert_blob_limit_filter_fetch_matches_stock_git(
        "smart-http filter blob limit",
        &git_client,
        &zmin_client,
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-limit-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git filter limit ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin filter limit ssh",
    );
    assert_blob_limit_filter_fetch_matches_stock_git(
        "ssh filter blob limit",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/filter.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-limit-daemon", url.as_str());
    command_output("git", &git_client, &args, "git filter limit daemon");
    command_output(zmin_bin(), &zmin_client, &args, "zmin filter limit daemon");
    assert_blob_limit_filter_fetch_matches_stock_git(
        "git-daemon filter blob limit",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_filter_blob_limit_network_branchless_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_filter_remote(dir.path());
    let args = ["fetch", "--quiet", "--filter=blob:limit=8", "origin"];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-limit-branchless-http", url.as_str());
    command_output(
        "git",
        &git_client,
        &args,
        "git filter limit branchless http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter limit branchless http",
    );
    assert_blob_limit_filter_fetch_matches_stock_git(
        "smart-http filter blob limit branchless",
        &git_client,
        &zmin_client,
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-limit-branchless-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git filter limit branchless ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin filter limit branchless ssh",
    );
    assert_blob_limit_filter_fetch_matches_stock_git(
        "ssh filter blob limit branchless",
        &git_client,
        &zmin_client,
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/filter.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-limit-branchless-daemon", url.as_str());
    command_output(
        "git",
        &git_client,
        &args,
        "git filter limit branchless daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter limit branchless daemon",
    );
    assert_blob_limit_filter_fetch_matches_stock_git(
        "git-daemon filter blob limit branchless",
        &git_client,
        &zmin_client,
    );
}

#[test]
fn fetch_filter_object_type_blob_network_branch_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_filter_remote(dir.path());
    let blob = git(&remote, ["rev-parse", "main:small.txt"]);
    let tree = git(&remote, ["rev-parse", "main^{tree}"]);
    let args = [
        "fetch",
        "--quiet",
        "--filter=object:type=blob",
        "origin",
        "main",
    ];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-object-type-http", url.as_str());
    command_output("git", &git_client, &args, "git filter object type http");
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter object type http",
    );
    assert_object_type_blob_filter_fetch_matches_stock_git(
        "smart-http filter object:type=blob",
        &git_client,
        &zmin_client,
        blob.as_str(),
        tree.as_str(),
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-object-type-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git filter object type ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin filter object type ssh",
    );
    assert_object_type_blob_filter_fetch_matches_stock_git(
        "ssh filter object:type=blob",
        &git_client,
        &zmin_client,
        blob.as_str(),
        tree.as_str(),
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/filter.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-object-type-daemon", url.as_str());
    command_output("git", &git_client, &args, "git filter object type daemon");
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter object type daemon",
    );
    assert_object_type_blob_filter_fetch_matches_stock_git(
        "git-daemon filter object:type=blob",
        &git_client,
        &zmin_client,
        blob.as_str(),
        tree.as_str(),
    );
}

#[test]
fn fetch_filter_object_type_blob_network_branchless_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_filter_remote(dir.path());
    let blob = git(&remote, ["rev-parse", "main:small.txt"]);
    let tree = git(&remote, ["rev-parse", "main^{tree}"]);
    let args = ["fetch", "--quiet", "--filter=object:type=blob", "origin"];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (git_client, zmin_client) = init_network_fetch_clients(
        dir.path(),
        "filter-object-type-branchless-http",
        url.as_str(),
    );
    command_output(
        "git",
        &git_client,
        &args,
        "git filter object type branchless http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter object type branchless http",
    );
    assert_object_type_blob_filter_fetch_matches_stock_git(
        "smart-http filter object:type=blob branchless",
        &git_client,
        &zmin_client,
        blob.as_str(),
        tree.as_str(),
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) = init_network_fetch_clients(
        dir.path(),
        "filter-object-type-branchless-ssh",
        url.as_str(),
    );
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git filter object type branchless ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin filter object type branchless ssh",
    );
    assert_object_type_blob_filter_fetch_matches_stock_git(
        "ssh filter object:type=blob branchless",
        &git_client,
        &zmin_client,
        blob.as_str(),
        tree.as_str(),
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/filter.git");
    let (git_client, zmin_client) = init_network_fetch_clients(
        dir.path(),
        "filter-object-type-branchless-daemon",
        url.as_str(),
    );
    command_output(
        "git",
        &git_client,
        &args,
        "git filter object type branchless daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter object type branchless daemon",
    );
    assert_object_type_blob_filter_fetch_matches_stock_git(
        "git-daemon filter object:type=blob branchless",
        &git_client,
        &zmin_client,
        blob.as_str(),
        tree.as_str(),
    );
}

#[test]
fn fetch_filter_tree_depth_network_branch_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_filter_remote(dir.path());
    let root_blob = git(&remote, ["rev-parse", "main:small.txt"]);
    let dir_tree = git(&remote, ["rev-parse", "main:dir"]);
    let child_blob = git(&remote, ["rev-parse", "main:dir/b.txt"]);
    let sub_tree = git(&remote, ["rev-parse", "main:dir/sub"]);
    let args = ["fetch", "--quiet", "--filter=tree:2", "origin", "main"];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-tree-depth-http", url.as_str());
    command_output("git", &git_client, &args, "git filter tree depth http");
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter tree depth http",
    );
    assert_tree_depth_filter_fetch_matches_stock_git(
        "smart-http filter tree:2",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        dir_tree.as_str(),
        child_blob.as_str(),
        sub_tree.as_str(),
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-tree-depth-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git filter tree depth ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin filter tree depth ssh",
    );
    assert_tree_depth_filter_fetch_matches_stock_git(
        "ssh filter tree:2",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        dir_tree.as_str(),
        child_blob.as_str(),
        sub_tree.as_str(),
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/filter.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-tree-depth-daemon", url.as_str());
    command_output("git", &git_client, &args, "git filter tree depth daemon");
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter tree depth daemon",
    );
    assert_tree_depth_filter_fetch_matches_stock_git(
        "git-daemon filter tree:2",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        dir_tree.as_str(),
        child_blob.as_str(),
        sub_tree.as_str(),
    );
}

#[test]
fn fetch_filter_tree_depth_network_branchless_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_filter_remote(dir.path());
    let root_blob = git(&remote, ["rev-parse", "main:small.txt"]);
    let dir_tree = git(&remote, ["rev-parse", "main:dir"]);
    let child_blob = git(&remote, ["rev-parse", "main:dir/b.txt"]);
    let sub_tree = git(&remote, ["rev-parse", "main:dir/sub"]);
    let args = ["fetch", "--quiet", "--filter=tree:2", "origin"];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (git_client, zmin_client) = init_network_fetch_clients(
        dir.path(),
        "filter-tree-depth-branchless-http",
        url.as_str(),
    );
    command_output(
        "git",
        &git_client,
        &args,
        "git filter tree depth branchless http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter tree depth branchless http",
    );
    assert_tree_depth_filter_fetch_matches_stock_git(
        "smart-http filter tree:2 branchless",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        dir_tree.as_str(),
        child_blob.as_str(),
        sub_tree.as_str(),
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-tree-depth-branchless-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git filter tree depth branchless ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin filter tree depth branchless ssh",
    );
    assert_tree_depth_filter_fetch_matches_stock_git(
        "ssh filter tree:2 branchless",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        dir_tree.as_str(),
        child_blob.as_str(),
        sub_tree.as_str(),
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/filter.git");
    let (git_client, zmin_client) = init_network_fetch_clients(
        dir.path(),
        "filter-tree-depth-branchless-daemon",
        url.as_str(),
    );
    command_output(
        "git",
        &git_client,
        &args,
        "git filter tree depth branchless daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter tree depth branchless daemon",
    );
    assert_tree_depth_filter_fetch_matches_stock_git(
        "git-daemon filter tree:2 branchless",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        dir_tree.as_str(),
        child_blob.as_str(),
        sub_tree.as_str(),
    );
}

#[test]
fn fetch_filter_combine_network_branch_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_filter_remote(dir.path());
    let root_blob = git(&remote, ["rev-parse", "main:small.txt"]);
    let child_blob = git(&remote, ["rev-parse", "main:dir/b.txt"]);
    let dir_tree = git(&remote, ["rev-parse", "main:dir"]);
    let args = [
        "fetch",
        "--quiet",
        "--filter=combine:object%3Atype%3Dblob+tree%3A2",
        "origin",
        "main",
    ];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-combine-http", url.as_str());
    command_output("git", &git_client, &args, "git filter combine http");
    command_output(zmin_bin(), &zmin_client, &args, "zmin filter combine http");
    assert_combined_filter_fetch_matches_stock_git(
        "smart-http filter combine",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        child_blob.as_str(),
        dir_tree.as_str(),
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-combine-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git filter combine ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin filter combine ssh",
    );
    assert_combined_filter_fetch_matches_stock_git(
        "ssh filter combine",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        child_blob.as_str(),
        dir_tree.as_str(),
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/filter.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-combine-daemon", url.as_str());
    command_output("git", &git_client, &args, "git filter combine daemon");
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter combine daemon",
    );
    assert_combined_filter_fetch_matches_stock_git(
        "git-daemon filter combine",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        child_blob.as_str(),
        dir_tree.as_str(),
    );
}

#[test]
fn fetch_filter_combine_network_branchless_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_filter_remote(dir.path());
    let root_blob = git(&remote, ["rev-parse", "main:small.txt"]);
    let child_blob = git(&remote, ["rev-parse", "main:dir/b.txt"]);
    let dir_tree = git(&remote, ["rev-parse", "main:dir"]);
    let args = [
        "fetch",
        "--quiet",
        "--filter=combine:object%3Atype%3Dblob+tree%3A2",
        "origin",
    ];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-combine-branchless-http", url.as_str());
    command_output(
        "git",
        &git_client,
        &args,
        "git filter combine branchless http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter combine branchless http",
    );
    assert_combined_filter_fetch_matches_stock_git(
        "smart-http filter combine branchless",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        child_blob.as_str(),
        dir_tree.as_str(),
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-combine-branchless-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git filter combine branchless ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin filter combine branchless ssh",
    );
    assert_combined_filter_fetch_matches_stock_git(
        "ssh filter combine branchless",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        child_blob.as_str(),
        dir_tree.as_str(),
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/filter.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-combine-branchless-daemon", url.as_str());
    command_output(
        "git",
        &git_client,
        &args,
        "git filter combine branchless daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter combine branchless daemon",
    );
    assert_combined_filter_fetch_matches_stock_git(
        "git-daemon filter combine branchless",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        child_blob.as_str(),
        dir_tree.as_str(),
    );
}

#[test]
fn fetch_filter_sparse_oid_network_branch_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_filter_remote(dir.path());
    let spec = git(&remote, ["rev-parse", "main:sparse-spec"]);
    let root_blob = git(&remote, ["rev-parse", "main:small.txt"]);
    let keep_blob = git(&remote, ["rev-parse", "main:keep/a.txt"]);
    let drop_blob = git(&remote, ["rev-parse", "main:drop/b.txt"]);
    let filter = format!("--filter=sparse:oid={spec}");
    let args = ["fetch", "--quiet", filter.as_str(), "origin", "main"];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-sparse-oid-http", url.as_str());
    command_output("git", &git_client, &args, "git filter sparse oid http");
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter sparse oid http",
    );
    assert_sparse_oid_filter_fetch_matches_stock_git(
        "smart-http filter sparse:oid",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        keep_blob.as_str(),
        drop_blob.as_str(),
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-sparse-oid-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git filter sparse oid ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin filter sparse oid ssh",
    );
    assert_sparse_oid_filter_fetch_matches_stock_git(
        "ssh filter sparse:oid",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        keep_blob.as_str(),
        drop_blob.as_str(),
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/filter.git");
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-sparse-oid-daemon", url.as_str());
    command_output("git", &git_client, &args, "git filter sparse oid daemon");
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter sparse oid daemon",
    );
    assert_sparse_oid_filter_fetch_matches_stock_git(
        "git-daemon filter sparse:oid",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        keep_blob.as_str(),
        drop_blob.as_str(),
    );
}

#[test]
fn fetch_filter_sparse_oid_network_branchless_transports_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = prepare_filter_remote(dir.path());
    let spec = git(&remote, ["rev-parse", "main:sparse-spec"]);
    let root_blob = git(&remote, ["rev-parse", "main:small.txt"]);
    let keep_blob = git(&remote, ["rev-parse", "main:keep/a.txt"]);
    let drop_blob = git(&remote, ["rev-parse", "main:drop/b.txt"]);
    let filter = format!("--filter=sparse:oid={spec}");
    let args = ["fetch", "--quiet", filter.as_str(), "origin"];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (git_client, zmin_client) = init_network_fetch_clients(
        dir.path(),
        "filter-sparse-oid-branchless-http",
        url.as_str(),
    );
    command_output(
        "git",
        &git_client,
        &args,
        "git filter sparse oid branchless http",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter sparse oid branchless http",
    );
    assert_sparse_oid_filter_fetch_matches_stock_git(
        "smart-http filter sparse:oid branchless",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        keep_blob.as_str(),
        drop_blob.as_str(),
    );

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    let (git_client, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-sparse-oid-branchless-ssh", url.as_str());
    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git filter sparse oid branchless ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin filter sparse oid branchless ssh",
    );
    assert_sparse_oid_filter_fetch_matches_stock_git(
        "ssh filter sparse:oid branchless",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        keep_blob.as_str(),
        drop_blob.as_str(),
    );

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/filter.git");
    let (git_client, zmin_client) = init_network_fetch_clients(
        dir.path(),
        "filter-sparse-oid-branchless-daemon",
        url.as_str(),
    );
    command_output(
        "git",
        &git_client,
        &args,
        "git filter sparse oid branchless daemon",
    );
    command_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "zmin filter sparse oid branchless daemon",
    );
    assert_sparse_oid_filter_fetch_matches_stock_git(
        "git-daemon filter sparse:oid branchless",
        &git_client,
        &zmin_client,
        root_blob.as_str(),
        keep_blob.as_str(),
        drop_blob.as_str(),
    );
}

fn prepare_filter_remote(root: &std::path::Path) -> std::path::PathBuf {
    let remote = root.join("filter.git");
    let work = root.join("filter-work");
    git(root, ["init", "--bare", "filter.git"]);
    git(&remote, ["config", "uploadpack.allowFilter", "true"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(root, ["init", "-b", "main", "filter-work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    fs::write(work.join("small.txt"), b"tiny\n").expect("write small");
    fs::write(work.join("large.txt"), b"large blob payload\n").expect("write large");
    fs::create_dir_all(work.join("dir/sub")).expect("create dir");
    fs::create_dir_all(work.join("keep")).expect("create keep");
    fs::create_dir_all(work.join("drop")).expect("create drop");
    fs::write(work.join("dir/b.txt"), b"nested\n").expect("write b");
    fs::write(work.join("dir/sub/deep.txt"), b"deep\n").expect("write deep");
    fs::write(work.join("keep/a.txt"), b"keep\n").expect("write keep");
    fs::write(work.join("drop/b.txt"), b"drop\n").expect("write drop");
    fs::write(work.join("sparse-spec"), b"/keep/\nsmall.txt\n").expect("write sparse spec");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);
    remote
}

#[test]
fn fetch_recurse_submodules_smart_http_parent_local_submodule_matches_stock_git() {
    let cases: [(&str, &[&str], bool); 11] = [
        ("implicit-yes", &["--recurse-submodules"], true),
        ("explicit-yes", &["--recurse-submodules=yes"], true),
        ("explicit-true", &["--recurse-submodules=true"], true),
        ("explicit-one", &["--recurse-submodules=1"], true),
        ("on-demand", &["--recurse-submodules=on-demand"], true),
        (
            "jobs-equals-two",
            &["--jobs=2", "--recurse-submodules"],
            true,
        ),
        (
            "jobs-short-negative",
            &["-j", "-1", "--recurse-submodules=on-demand"],
            true,
        ),
        ("explicit-no", &["--recurse-submodules=no"], false),
        ("explicit-false", &["--recurse-submodules=false"], false),
        ("explicit-zero", &["--recurse-submodules=0"], false),
        ("no-recurse", &["--no-recurse-submodules"], false),
    ];

    for (label, mode_args, expect_submodule_fetch) in cases {
        assert_fetch_recurse_submodules_smart_http_parent_local_submodule_matches_stock_git(
            label,
            mode_args,
            true,
            expect_submodule_fetch,
        );
    }
}

#[test]
fn fetch_recurse_submodules_smart_http_parent_uninitialized_submodule_matches_stock_git() {
    let cases: [(&str, &[&str]); 9] = [
        ("implicit-yes", &["--recurse-submodules"]),
        ("explicit-yes", &["--recurse-submodules=yes"]),
        ("explicit-true", &["--recurse-submodules=true"]),
        ("explicit-one", &["--recurse-submodules=1"]),
        ("on-demand", &["--recurse-submodules=on-demand"]),
        ("explicit-no", &["--recurse-submodules=no"]),
        ("explicit-false", &["--recurse-submodules=false"]),
        ("explicit-zero", &["--recurse-submodules=0"]),
        ("no-recurse", &["--no-recurse-submodules"]),
    ];

    for (label, mode_args) in cases {
        assert_fetch_recurse_submodules_smart_http_parent_local_submodule_matches_stock_git(
            label, mode_args, false, false,
        );
    }
}

#[test]
fn fetch_recurse_submodules_ssh_parent_local_submodule_matches_stock_git() {
    assert_fetch_recurse_submodules_network_parent_local_submodule_matches_stock_git(
        "ssh-on-demand",
        &["--recurse-submodules=on-demand"],
        true,
        true,
        FetchRecurseSubmodulesParentTransport::Ssh,
    );
}

#[test]
fn fetch_recurse_submodules_git_daemon_parent_local_submodule_matches_stock_git() {
    assert_fetch_recurse_submodules_network_parent_local_submodule_matches_stock_git(
        "git-daemon-on-demand",
        &["--recurse-submodules=on-demand"],
        true,
        true,
        FetchRecurseSubmodulesParentTransport::GitDaemon,
    );
}

#[test]
fn fetch_recurse_submodules_smart_http_parent_smart_http_submodule_matches_stock_git() {
    assert_fetch_recurse_submodules_smart_http_parent_network_submodule_matches_stock_git(
        "smart-http-submodule",
        FetchRecurseSubmodulesSubmoduleTransport::SmartHttp,
    );
}

#[test]
fn fetch_recurse_submodules_smart_http_parent_ssh_submodule_matches_stock_git() {
    assert_fetch_recurse_submodules_smart_http_parent_network_submodule_matches_stock_git(
        "ssh-submodule",
        FetchRecurseSubmodulesSubmoduleTransport::Ssh,
    );
}

#[test]
fn fetch_recurse_submodules_smart_http_parent_git_daemon_submodule_matches_stock_git() {
    assert_fetch_recurse_submodules_smart_http_parent_network_submodule_matches_stock_git(
        "git-daemon-submodule",
        FetchRecurseSubmodulesSubmoduleTransport::GitDaemon,
    );
}

#[derive(Clone, Copy)]
enum FetchRecurseSubmodulesSubmoduleTransport {
    SmartHttp,
    Ssh,
    GitDaemon,
}

fn assert_fetch_recurse_submodules_smart_http_parent_network_submodule_matches_stock_git(
    label: &str,
    submodule_transport: FetchRecurseSubmodulesSubmoduleTransport,
) {
    let dir = TempDir::new().expect("temp dir");
    let submodule_remote = dir.path().join("submodule.git");
    let submodule_work = dir.path().join("submodule-work");
    let source = dir.path().join("source");
    let parent_remote = dir.path().join("parent.git");
    let git_client = dir
        .path()
        .join(format!("git-client-submodule-http-parent-{label}"));
    let zmin_client = dir
        .path()
        .join(format!("zmin-client-submodule-http-parent-{label}"));

    git(dir.path(), ["init", "--bare", "submodule.git"]);
    fs::write(submodule_remote.join("git-daemon-export-ok"), "").expect("submodule export marker");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            submodule_work.to_str().expect("submodule work path"),
        ],
    );
    configure_identity(&submodule_work);
    fs::write(submodule_work.join("lib.txt"), b"one\n").expect("write submodule one");
    git(&submodule_work, ["add", "-A"]);
    git_with_env(&submodule_work, ["commit", "-m", "submodule one"]);
    let first_submodule_head = git(&submodule_work, ["rev-parse", "HEAD"]);
    git(
        &submodule_work,
        [
            "remote",
            "add",
            "origin",
            submodule_remote.to_str().expect("submodule remote path"),
        ],
    );
    git(&submodule_work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&submodule_remote);

    git(dir.path(), ["init", "--bare", "parent.git"]);
    fs::write(parent_remote.join("git-daemon-export-ok"), "").expect("parent export marker");
    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let mut _submodule_daemon = None;
    let mut command_envs = Vec::<(String, String)>::new();
    let submodule_url = match submodule_transport {
        FetchRecurseSubmodulesSubmoduleTransport::SmartHttp => {
            format!("http://127.0.0.1:{}/submodule.git", server.port)
        }
        FetchRecurseSubmodulesSubmoduleTransport::Ssh => {
            let fake_ssh = write_fake_ssh(dir.path());
            command_envs.push((
                "GIT_SSH_COMMAND".to_owned(),
                fake_ssh_command_arg(&fake_ssh),
            ));
            ssh_url_for_remote(&submodule_remote)
        }
        FetchRecurseSubmodulesSubmoduleTransport::GitDaemon => {
            let port = unused_local_port();
            _submodule_daemon = Some(StockGitDaemon::spawn(dir.path(), port));
            format!("git://127.0.0.1:{port}/submodule.git")
        }
    };
    let parent_url = format!("http://127.0.0.1:{}/parent.git", server.port);
    let command_envs = command_envs
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect::<Vec<_>>();

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    command_output_with_env(
        "git",
        &source,
        &["submodule", "add", &submodule_url, "deps/sub"],
        &command_envs,
        "git submodule add network",
    );
    git_with_env(&source, ["commit", "-m", "add submodule"]);
    git(
        &source,
        [
            "remote",
            "add",
            "origin",
            parent_remote.to_str().expect("parent remote path"),
        ],
    );
    git(&source, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&parent_remote);

    for (client, label) in [(&git_client, "git"), (&zmin_client, "zmin")] {
        command_output_with_env(
            "git",
            dir.path(),
            &[
                "clone",
                "--recurse-submodules",
                &parent_url,
                client.to_str().expect("client path"),
            ],
            &command_envs,
            &format!("{label} recursive clone"),
        );
    }

    fs::write(submodule_work.join("lib.txt"), b"two\n").expect("write submodule two");
    git(&submodule_work, ["add", "-A"]);
    git_with_env(&submodule_work, ["commit", "-m", "submodule two"]);
    let second_submodule_head = git(&submodule_work, ["rev-parse", "HEAD"]);
    git(&submodule_work, ["push", "-q", "origin", "main"]);
    command_output_with_env(
        "git",
        &source.join("deps/sub"),
        &["fetch", "origin"],
        &command_envs,
        "git source submodule fetch",
    );
    git(
        &source.join("deps/sub"),
        ["checkout", &second_submodule_head],
    );
    git(&source, ["add", "deps/sub"]);
    git_with_env(&source, ["commit", "-m", "update submodule"]);
    git(&source, ["push", "-q", "origin", "main"]);

    let args = [
        "fetch",
        "--quiet",
        "--recurse-submodules=on-demand",
        "origin",
    ];
    let git_output = command_output_with_env("git", &git_client, &args, &command_envs, "git fetch");
    let zmin_output =
        command_output_with_env(zmin_bin(), &zmin_client, &args, &command_envs, "zmin fetch");
    assert_eq!(zmin_output, git_output);
    assert_eq!(
        git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
        git(&git_client, ["rev-parse", "refs/remotes/origin/main"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert_eq!(
        git(
            &zmin_client.join("deps/sub"),
            ["cat-file", "-t", &second_submodule_head]
        ),
        git(
            &git_client.join("deps/sub"),
            ["cat-file", "-t", &second_submodule_head]
        )
    );
    assert_eq!(
        git(&zmin_client.join("deps/sub"), ["rev-parse", "HEAD"]),
        first_submodule_head
    );
    assert_eq!(
        git(&git_client.join("deps/sub"), ["rev-parse", "HEAD"]),
        first_submodule_head
    );
}

#[test]
fn fetch_jobs_invalid_value_matches_stock_git_failure() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    for args in [
        ["fetch", "-j", "bad", "origin"].as_slice(),
        ["fetch", "--jobs=bad", "origin"].as_slice(),
        ["fetch", "--jobs", "bad", "origin"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), &repo, args, "zmin"),
            command_any_output("git", &repo, args, "git"),
            "fetch jobs validation mismatch for {args:?}"
        );
    }
}

#[test]
fn fetch_dry_run_submodule_smart_http_parent_modes_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let submodule = dir.path().join("submodule");
    let source = dir.path().join("source");
    let parent_remote = dir.path().join("parent.git");
    let git_default_client = dir
        .path()
        .join("git-client-dry-run-default-submodule-http-parent");
    let zmin_default_client = dir
        .path()
        .join("zmin-client-dry-run-default-submodule-http-parent");
    let git_recurse_client = dir
        .path()
        .join("git-client-dry-run-recurse-submodule-http-parent");
    let zmin_recurse_client = dir
        .path()
        .join("zmin-client-dry-run-recurse-submodule-http-parent");

    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            submodule.to_str().expect("submodule path"),
        ],
    );
    configure_identity(&submodule);
    fs::write(submodule.join("lib.txt"), b"one\n").expect("write submodule one");
    git(&submodule, ["add", "-A"]);
    git_with_env(&submodule, ["commit", "-m", "submodule one"]);
    let first_submodule_head = git(&submodule, ["rev-parse", "HEAD"]);

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    command_output_with_env(
        "git",
        &source,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            submodule.to_str().expect("submodule path"),
            "deps/sub",
        ],
        &[],
        "git submodule add",
    );
    git_with_env(&source, ["commit", "-m", "add submodule"]);

    git(dir.path(), ["init", "--bare", "parent.git"]);
    fs::write(parent_remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(
        &source,
        [
            "remote",
            "add",
            "origin",
            parent_remote.to_str().expect("parent remote path"),
        ],
    );
    git(&source, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&parent_remote);

    for (client, label, command) in [
        (&git_default_client, "git default", "git"),
        (&zmin_default_client, "zmin default", zmin_bin()),
        (&git_recurse_client, "git recurse", "git"),
        (&zmin_recurse_client, "zmin recurse", zmin_bin()),
    ] {
        command_output_with_env(
            command,
            dir.path(),
            &[
                "-c",
                "protocol.file.allow=always",
                "clone",
                "--recurse-submodules",
                source.to_str().expect("source path"),
                client.to_str().expect("client path"),
            ],
            &[],
            &format!("{label} recursive clone"),
        );
    }

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let parent_url = format!("http://127.0.0.1:{}/parent.git", server.port);
    for client in [
        &git_default_client,
        &zmin_default_client,
        &git_recurse_client,
        &zmin_recurse_client,
    ] {
        git(client, ["remote", "set-url", "origin", &parent_url]);
    }
    let git_default_before_remote = git(
        &git_default_client,
        ["rev-parse", "refs/remotes/origin/main"],
    );
    let zmin_default_before_remote = git(
        &zmin_default_client,
        ["rev-parse", "refs/remotes/origin/main"],
    );
    let git_recurse_before_remote = git(
        &git_recurse_client,
        ["rev-parse", "refs/remotes/origin/main"],
    );
    let zmin_recurse_before_remote = git(
        &zmin_recurse_client,
        ["rev-parse", "refs/remotes/origin/main"],
    );
    let git_default_before_fetch_head =
        fs::read_to_string(git_default_client.join(".git/FETCH_HEAD")).ok();
    let zmin_default_before_fetch_head =
        fs::read_to_string(zmin_default_client.join(".git/FETCH_HEAD")).ok();
    let git_recurse_before_fetch_head =
        fs::read_to_string(git_recurse_client.join(".git/FETCH_HEAD")).ok();
    let zmin_recurse_before_fetch_head =
        fs::read_to_string(zmin_recurse_client.join(".git/FETCH_HEAD")).ok();

    fs::write(submodule.join("lib.txt"), b"two\n").expect("write submodule two");
    git(&submodule, ["add", "-A"]);
    git_with_env(&submodule, ["commit", "-m", "submodule two"]);
    let second_submodule_head = git(&submodule, ["rev-parse", "HEAD"]);
    command_output_with_env(
        "git",
        &source.join("deps/sub"),
        &["-c", "protocol.file.allow=always", "fetch", "origin"],
        &[],
        "git submodule source fetch",
    );
    git(
        &source.join("deps/sub"),
        ["checkout", &second_submodule_head],
    );
    git(&source, ["add", "deps/sub"]);
    git_with_env(&source, ["commit", "-m", "update submodule"]);
    git(&source, ["push", "-q", "origin", "main"]);

    let default_args = ["fetch", "--quiet", "--dry-run", "origin"];
    assert_eq!(
        command_any_output(
            zmin_bin(),
            &zmin_default_client,
            &default_args,
            "zmin default dry-run fetch",
        ),
        command_any_output(
            "git",
            &git_default_client,
            &default_args,
            "git default dry-run fetch",
        )
    );
    assert_eq!(
        git(
            &zmin_default_client,
            ["rev-parse", "refs/remotes/origin/main"]
        ),
        zmin_default_before_remote
    );
    assert_eq!(
        git(
            &git_default_client,
            ["rev-parse", "refs/remotes/origin/main"]
        ),
        git_default_before_remote
    );
    assert_eq!(
        fs::read_to_string(zmin_default_client.join(".git/FETCH_HEAD")).ok(),
        zmin_default_before_fetch_head
    );
    assert_eq!(
        fs::read_to_string(git_default_client.join(".git/FETCH_HEAD")).ok(),
        git_default_before_fetch_head
    );
    assert_eq!(
        git(&zmin_default_client.join("deps/sub"), ["rev-parse", "HEAD"]),
        first_submodule_head
    );
    assert_eq!(
        git(&git_default_client.join("deps/sub"), ["rev-parse", "HEAD"]),
        first_submodule_head
    );
    let args = ["cat-file", "-e", &second_submodule_head];
    assert_eq!(
        git_status_args(&zmin_default_client.join("deps/sub"), &args),
        git_status_args(&git_default_client.join("deps/sub"), &args)
    );

    let recurse_args = [
        "-c",
        "protocol.file.allow=always",
        "fetch",
        "--quiet",
        "--dry-run",
        "--recurse-submodules",
        "origin",
    ];
    assert_eq!(
        command_any_output(
            zmin_bin(),
            &zmin_recurse_client,
            &recurse_args,
            "zmin dry-run fetch with submodule recursion",
        ),
        command_any_output(
            "git",
            &git_recurse_client,
            &recurse_args,
            "git dry-run fetch with submodule recursion",
        )
    );
    assert_eq!(
        git(
            &zmin_recurse_client,
            ["rev-parse", "refs/remotes/origin/main"]
        ),
        zmin_recurse_before_remote
    );
    assert_eq!(
        git(
            &git_recurse_client,
            ["rev-parse", "refs/remotes/origin/main"]
        ),
        git_recurse_before_remote
    );
    assert_eq!(
        fs::read_to_string(zmin_recurse_client.join(".git/FETCH_HEAD")).ok(),
        zmin_recurse_before_fetch_head
    );
    assert_eq!(
        fs::read_to_string(git_recurse_client.join(".git/FETCH_HEAD")).ok(),
        git_recurse_before_fetch_head
    );
    assert_eq!(
        git(&zmin_recurse_client.join("deps/sub"), ["rev-parse", "HEAD"]),
        first_submodule_head
    );
    assert_eq!(
        git(&git_recurse_client.join("deps/sub"), ["rev-parse", "HEAD"]),
        first_submodule_head
    );
    assert_eq!(
        git_status_args(&zmin_recurse_client.join("deps/sub"), &args),
        git_status_args(&git_recurse_client.join("deps/sub"), &args)
    );
}

#[test]
fn fetch_recurse_submodules_smart_http_parent_nested_submodule_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let grandchild = dir.path().join("grandchild");
    let submodule = dir.path().join("submodule");
    let source = dir.path().join("source");
    let parent_remote = dir.path().join("parent.git");
    let git_client = dir.path().join("git-client-nested-submodule-http-parent");
    let zmin_client = dir.path().join("zmin-client-nested-submodule-http-parent");

    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            grandchild.to_str().expect("grandchild path"),
        ],
    );
    configure_identity(&grandchild);
    fs::write(grandchild.join("grand.txt"), b"one\n").expect("write grandchild one");
    git(&grandchild, ["add", "-A"]);
    git_with_env(&grandchild, ["commit", "-m", "grandchild one"]);
    let first_grandchild_head = git(&grandchild, ["rev-parse", "HEAD"]);

    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            submodule.to_str().expect("submodule path"),
        ],
    );
    configure_identity(&submodule);
    command_output_with_env(
        "git",
        &submodule,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            grandchild.to_str().expect("grandchild path"),
            "nested/grand",
        ],
        &[],
        "git nested submodule add",
    );
    git_with_env(&submodule, ["commit", "-m", "add nested submodule"]);

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    command_output_with_env(
        "git",
        &source,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            submodule.to_str().expect("submodule path"),
            "deps/sub",
        ],
        &[],
        "git parent submodule add",
    );
    git_with_env(&source, ["commit", "-m", "add submodule"]);

    git(dir.path(), ["init", "--bare", "parent.git"]);
    fs::write(parent_remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(
        &source,
        [
            "remote",
            "add",
            "origin",
            parent_remote.to_str().expect("parent remote path"),
        ],
    );
    git(&source, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&parent_remote);

    for (client, label) in [(&git_client, "git"), (&zmin_client, "zmin")] {
        command_output_with_env(
            "git",
            dir.path(),
            &[
                "-c",
                "protocol.file.allow=always",
                "clone",
                "--recurse-submodules",
                source.to_str().expect("source path"),
                client.to_str().expect("client path"),
            ],
            &[],
            &format!("{label} recursive clone"),
        );
    }

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let parent_url = format!("http://127.0.0.1:{}/parent.git", server.port);
    git(&git_client, ["remote", "set-url", "origin", &parent_url]);
    git(&zmin_client, ["remote", "set-url", "origin", &parent_url]);

    fs::write(grandchild.join("grand.txt"), b"two\n").expect("write grandchild two");
    git(&grandchild, ["add", "-A"]);
    git_with_env(&grandchild, ["commit", "-m", "grandchild two"]);
    let second_grandchild_head = git(&grandchild, ["rev-parse", "HEAD"]);
    command_output_with_env(
        "git",
        &submodule.join("nested/grand"),
        &["-c", "protocol.file.allow=always", "fetch", "origin"],
        &[],
        "git nested submodule source fetch",
    );
    git(
        &submodule.join("nested/grand"),
        ["checkout", &second_grandchild_head],
    );
    git(&submodule, ["add", "nested/grand"]);
    git_with_env(&submodule, ["commit", "-m", "update nested submodule"]);
    let second_submodule_head = git(&submodule, ["rev-parse", "HEAD"]);
    command_output_with_env(
        "git",
        &source.join("deps/sub"),
        &["-c", "protocol.file.allow=always", "fetch", "origin"],
        &[],
        "git submodule source fetch",
    );
    git(
        &source.join("deps/sub"),
        ["checkout", &second_submodule_head],
    );
    git(&source, ["add", "deps/sub"]);
    git_with_env(&source, ["commit", "-m", "update submodule"]);
    git(&source, ["push", "-q", "origin", "main"]);

    let args = [
        "-c",
        "protocol.file.allow=always",
        "fetch",
        "--quiet",
        "--recurse-submodules",
        "origin",
    ];
    let git_output = command_output_with_env("git", &git_client, &args, &[], "git fetch");
    let zmin_output = command_output_with_env(zmin_bin(), &zmin_client, &args, &[], "zmin fetch");
    assert_eq!(zmin_output, git_output);
    assert_eq!(
        git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
        git(&git_client, ["rev-parse", "refs/remotes/origin/main"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert_eq!(
        git(
            &zmin_client.join("deps/sub"),
            ["cat-file", "-t", &second_submodule_head]
        ),
        git(
            &git_client.join("deps/sub"),
            ["cat-file", "-t", &second_submodule_head]
        )
    );
    assert_eq!(
        git(
            &zmin_client.join("deps/sub/nested/grand"),
            ["cat-file", "-t", &second_grandchild_head]
        ),
        git(
            &git_client.join("deps/sub/nested/grand"),
            ["cat-file", "-t", &second_grandchild_head]
        )
    );
    assert_eq!(
        git(
            &zmin_client.join("deps/sub/nested/grand"),
            ["rev-parse", "HEAD"]
        ),
        first_grandchild_head
    );
    assert_eq!(
        git(
            &git_client.join("deps/sub/nested/grand"),
            ["rev-parse", "HEAD"]
        ),
        first_grandchild_head
    );
}

fn assert_fetch_recurse_submodules_smart_http_parent_local_submodule_matches_stock_git(
    label: &str,
    mode_args: &[&str],
    initialize_submodule: bool,
    expect_submodule_fetch: bool,
) {
    assert_fetch_recurse_submodules_network_parent_local_submodule_matches_stock_git(
        label,
        mode_args,
        initialize_submodule,
        expect_submodule_fetch,
        FetchRecurseSubmodulesParentTransport::SmartHttp,
    );
}

#[derive(Clone, Copy)]
enum FetchRecurseSubmodulesParentTransport {
    SmartHttp,
    Ssh,
    GitDaemon,
}

fn assert_fetch_recurse_submodules_network_parent_local_submodule_matches_stock_git(
    label: &str,
    mode_args: &[&str],
    initialize_submodule: bool,
    expect_submodule_fetch: bool,
    parent_transport: FetchRecurseSubmodulesParentTransport,
) {
    let dir = TempDir::new().expect("temp dir");
    let submodule = dir.path().join("submodule");
    let source = dir.path().join("source");
    let parent_remote = dir.path().join("parent.git");
    let git_client = dir
        .path()
        .join(format!("git-client-submodule-http-parent-{label}"));
    let zmin_client = dir
        .path()
        .join(format!("zmin-client-submodule-http-parent-{label}"));

    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            submodule.to_str().expect("submodule path"),
        ],
    );
    configure_identity(&submodule);
    fs::write(submodule.join("lib.txt"), b"one\n").expect("write submodule one");
    git(&submodule, ["add", "-A"]);
    git_with_env(&submodule, ["commit", "-m", "submodule one"]);
    let first_submodule_head = git(&submodule, ["rev-parse", "HEAD"]);

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    command_output_with_env(
        "git",
        &source,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            submodule.to_str().expect("submodule path"),
            "deps/sub",
        ],
        &[],
        "git submodule add",
    );
    git_with_env(&source, ["commit", "-m", "add submodule"]);

    git(dir.path(), ["init", "--bare", "parent.git"]);
    fs::write(parent_remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(
        &source,
        [
            "remote",
            "add",
            "origin",
            parent_remote.to_str().expect("parent remote path"),
        ],
    );
    git(&source, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&parent_remote);

    if initialize_submodule {
        command_output_with_env(
            "git",
            dir.path(),
            &[
                "-c",
                "protocol.file.allow=always",
                "clone",
                "--recurse-submodules",
                source.to_str().expect("source path"),
                git_client.to_str().expect("git client path"),
            ],
            &[],
            "git recursive clone",
        );
        command_output_with_env(
            zmin_bin(),
            dir.path(),
            &[
                "clone",
                "--recurse-submodules",
                source.to_str().expect("source path"),
                zmin_client.to_str().expect("zmin client path"),
            ],
            &[],
            "zmin recursive clone",
        );
    } else {
        command_output_with_env(
            "git",
            dir.path(),
            &[
                "clone",
                source.to_str().expect("source path"),
                git_client.to_str().expect("git client path"),
            ],
            &[],
            "git clone",
        );
        command_output_with_env(
            zmin_bin(),
            dir.path(),
            &[
                "clone",
                source.to_str().expect("source path"),
                zmin_client.to_str().expect("zmin client path"),
            ],
            &[],
            "zmin clone",
        );
    }

    let mut _server = None;
    let mut _daemon = None;
    let mut command_envs = Vec::<(String, String)>::new();
    let parent_url = match parent_transport {
        FetchRecurseSubmodulesParentTransport::SmartHttp => {
            let server = SmartHttpServer::new(dir.path().to_path_buf());
            let url = format!("http://127.0.0.1:{}/parent.git", server.port);
            _server = Some(server);
            url
        }
        FetchRecurseSubmodulesParentTransport::Ssh => {
            let fake_ssh = write_fake_ssh(dir.path());
            command_envs.push((
                "GIT_SSH_COMMAND".to_owned(),
                fake_ssh_command_arg(&fake_ssh),
            ));
            ssh_url_for_remote(&parent_remote)
        }
        FetchRecurseSubmodulesParentTransport::GitDaemon => {
            let port = unused_local_port();
            _daemon = Some(StockGitDaemon::spawn(dir.path(), port));
            format!("git://127.0.0.1:{port}/parent.git")
        }
    };
    git(&git_client, ["remote", "set-url", "origin", &parent_url]);
    git(&zmin_client, ["remote", "set-url", "origin", &parent_url]);

    fs::write(submodule.join("lib.txt"), b"two\n").expect("write submodule two");
    git(&submodule, ["add", "-A"]);
    git_with_env(&submodule, ["commit", "-m", "submodule two"]);
    let second_submodule_head = git(&submodule, ["rev-parse", "HEAD"]);
    command_output_with_env(
        "git",
        &source.join("deps/sub"),
        &["-c", "protocol.file.allow=always", "fetch", "origin"],
        &[],
        "git submodule source fetch",
    );
    git(
        &source.join("deps/sub"),
        ["checkout", &second_submodule_head],
    );
    git(&source, ["add", "deps/sub"]);
    git_with_env(&source, ["commit", "-m", "update submodule"]);
    git(&source, ["push", "-q", "origin", "main"]);

    let mut args = vec!["-c", "protocol.file.allow=always", "fetch", "--quiet"];
    args.extend_from_slice(mode_args);
    args.push("origin");
    let command_envs = command_envs
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect::<Vec<_>>();
    let git_output = command_output_with_env("git", &git_client, &args, &command_envs, "git fetch");
    let zmin_output =
        command_output_with_env(zmin_bin(), &zmin_client, &args, &command_envs, "zmin fetch");
    assert_eq!(zmin_output, git_output);
    assert_eq!(
        git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
        git(&git_client, ["rev-parse", "refs/remotes/origin/main"]),
        "{label}"
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
        "{label}"
    );
    if initialize_submodule {
        if expect_submodule_fetch {
            assert_eq!(
                git(
                    &zmin_client.join("deps/sub"),
                    ["cat-file", "-t", &second_submodule_head]
                ),
                git(
                    &git_client.join("deps/sub"),
                    ["cat-file", "-t", &second_submodule_head]
                ),
                "{label}"
            );
        } else {
            let args = ["cat-file", "-e", &second_submodule_head];
            assert_eq!(
                git_status_args(&zmin_client.join("deps/sub"), &args),
                git_status_args(&git_client.join("deps/sub"), &args),
                "{label}"
            );
        }
        assert_eq!(
            git(&zmin_client.join("deps/sub"), ["rev-parse", "HEAD"]),
            first_submodule_head,
            "{label}"
        );
        assert_eq!(
            git(&git_client.join("deps/sub"), ["rev-parse", "HEAD"]),
            first_submodule_head,
            "{label}"
        );
    } else {
        assert!(
            !zmin_client.join(".git/modules/deps/sub").exists(),
            "{label}"
        );
        assert!(
            !git_client.join(".git/modules/deps/sub").exists(),
            "{label}"
        );
        assert_eq!(
            git(&zmin_client, ["submodule", "status"]),
            git(&git_client, ["submodule", "status"]),
            "{label}"
        );
    }
}

fn assert_filtered_fetch_matches_stock_git(
    label: &str,
    git_client: &std::path::Path,
    zmin_client: &std::path::Path,
) {
    assert_filter_fetch_common_matches_stock_git(label, git_client, zmin_client);
    assert_eq!(
        filtered_blob_local_presence(zmin_bin(), zmin_client, "a.txt"),
        filtered_blob_local_presence(
            required_pinned_stock_git()
                .to_str()
                .expect("pinned Git path"),
            git_client,
            "a.txt",
        ),
        "{label}"
    );
}

fn assert_smart_http_filter_v2_wire(
    server: &SmartHttpServer,
    label: &str,
    body_start: usize,
    header_start: usize,
    protocol_before: usize,
    upload_pack_before: usize,
) {
    assert_eq!(
        server.git_protocol_requests() - protocol_before,
        3,
        "{label}: expected one v2 capabilities, ls-refs, and fetch request"
    );
    assert_eq!(
        server.upload_pack_requests() - upload_pack_before,
        2,
        "{label}: expected one v2 ls-refs and one v2 fetch POST"
    );
    let headers = server.request_headers_text();
    let headers = &headers[header_start..];
    assert_eq!(
        headers
            .iter()
            .filter(|headers| headers.contains("Git-Protocol: version=2\r\n"))
            .count(),
        3,
        "{label}: every v2 request must carry the protocol header"
    );
    let bodies = server.upload_pack_bodies_text();
    let bodies = &bodies[body_start..];
    assert_eq!(
        bodies
            .iter()
            .filter(|body| body.contains("command=ls-refs"))
            .count(),
        1,
        "{label}: expected exactly one v2 ls-refs command"
    );
    assert_eq!(
        bodies
            .iter()
            .filter(|body| body.contains("command=fetch"))
            .count(),
        1,
        "{label}: expected exactly one v2 fetch command"
    );
    assert!(
        bodies
            .iter()
            .any(|body| body.contains("command=ls-refs") && body.contains("symrefs\n")),
        "{label}: v2 ls-refs request did not ask for symrefs: {bodies:?}"
    );
    assert!(
        bodies
            .iter()
            .any(|body| body.contains("command=ls-refs") && body.contains("unborn\n")),
        "{label}: v2 ls-refs request did not ask for unborn: {bodies:?}"
    );
    assert!(
        bodies
            .iter()
            .any(|body| body.contains("command=fetch") && body.contains("filter blob:none")),
        "{label}: v2 fetch request did not carry filter blob:none: {bodies:?}"
    );
}

fn assert_filter_fetch_common_matches_stock_git(
    label: &str,
    git_client: &std::path::Path,
    zmin_client: &std::path::Path,
) {
    assert_eq!(
        git(zmin_client, ["show-ref"]),
        git(git_client, ["show-ref"]),
        "{label}"
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
        "{label}"
    );
    assert_eq!(
        run_zmin(zmin_client, ["config", "--get", "remote.origin.promisor"]),
        git(git_client, ["config", "--get", "remote.origin.promisor"]),
        "{label}"
    );
    assert_eq!(
        run_zmin(
            zmin_client,
            ["config", "--get", "remote.origin.partialclonefilter"]
        ),
        git(
            git_client,
            ["config", "--get", "remote.origin.partialclonefilter"]
        ),
        "{label}"
    );
}

fn assert_promisor_marker_roles_match(
    label: &str,
    git_client: &std::path::Path,
    zmin_client: &std::path::Path,
) {
    let stock_promisor_roles = http_pack_role_snapshot(git_client)
        .into_iter()
        .filter(|name| name.ends_with(".promisor"))
        .collect::<Vec<_>>();
    let zmin_promisor_roles = http_pack_role_snapshot(zmin_client)
        .into_iter()
        .filter(|name| name.ends_with(".promisor"))
        .collect::<Vec<_>>();
    assert!(
        !stock_promisor_roles.is_empty(),
        "{label}: pinned Git did not create a promisor marker"
    );
    assert!(
        !zmin_promisor_roles.is_empty(),
        "{label}: Zmin did not create a promisor marker"
    );
    assert_eq!(zmin_promisor_roles, stock_promisor_roles, "{label}");
    for role in stock_promisor_roles {
        let stock_content = fs::read(git_client.join(".git/objects/pack").join(&role))
            .expect("stock promisor marker");
        let zmin_content = fs::read(zmin_client.join(".git/objects/pack").join(&role))
            .expect("Zmin promisor marker");
        assert_eq!(
            zmin_content, stock_content,
            "{label}: promisor role {role:?} content differs"
        );
    }
}

fn assert_blob_limit_filter_fetch_matches_stock_git(
    label: &str,
    git_client: &std::path::Path,
    zmin_client: &std::path::Path,
) {
    assert_filter_fetch_common_matches_stock_git(label, git_client, zmin_client);
    assert_eq!(
        filtered_blob_local_presence(zmin_bin(), zmin_client, "small.txt"),
        0,
        "{label}"
    );
    assert_eq!(
        filtered_blob_local_presence(
            required_pinned_stock_git()
                .to_str()
                .expect("pinned Git path"),
            git_client,
            "small.txt",
        ),
        0,
        "{label}"
    );
    assert_ne!(
        filtered_blob_local_presence(zmin_bin(), zmin_client, "large.txt"),
        0,
        "{label}"
    );
    assert_eq!(
        filtered_blob_local_presence(zmin_bin(), zmin_client, "large.txt"),
        filtered_blob_local_presence(
            required_pinned_stock_git()
                .to_str()
                .expect("pinned Git path"),
            git_client,
            "large.txt",
        ),
        "{label}"
    );
}

fn assert_object_type_blob_filter_fetch_matches_stock_git(
    label: &str,
    git_client: &std::path::Path,
    zmin_client: &std::path::Path,
    blob: &str,
    tree: &str,
) {
    assert_filter_fetch_common_matches_stock_git(label, git_client, zmin_client);
    assert_eq!(
        filtered_object_local_presence(zmin_bin(), zmin_client, blob),
        0,
        "{label}"
    );
    assert_eq!(
        filtered_object_local_presence(
            required_pinned_stock_git()
                .to_str()
                .expect("pinned Git path"),
            git_client,
            blob,
        ),
        0,
        "{label}"
    );
    assert_ne!(
        filtered_object_local_presence(zmin_bin(), zmin_client, tree),
        0,
        "{label}"
    );
    assert_eq!(
        filtered_object_local_presence(zmin_bin(), zmin_client, tree),
        filtered_object_local_presence(
            required_pinned_stock_git()
                .to_str()
                .expect("pinned Git path"),
            git_client,
            tree,
        ),
        "{label}"
    );
}

fn assert_tree_depth_filter_fetch_matches_stock_git(
    label: &str,
    git_client: &std::path::Path,
    zmin_client: &std::path::Path,
    root_blob: &str,
    dir_tree: &str,
    child_blob: &str,
    sub_tree: &str,
) {
    assert_filter_fetch_common_matches_stock_git(label, git_client, zmin_client);
    for object in [root_blob, dir_tree] {
        assert_eq!(
            filtered_object_local_presence(zmin_bin(), zmin_client, object),
            0,
            "{label}"
        );
        assert_eq!(
            filtered_object_local_presence(
                required_pinned_stock_git()
                    .to_str()
                    .expect("pinned Git path"),
                git_client,
                object,
            ),
            0,
            "{label}"
        );
    }
    for object in [child_blob, sub_tree] {
        assert_ne!(
            filtered_object_local_presence(zmin_bin(), zmin_client, object),
            0,
            "{label}"
        );
        assert_eq!(
            filtered_object_local_presence(zmin_bin(), zmin_client, object),
            filtered_object_local_presence(
                required_pinned_stock_git()
                    .to_str()
                    .expect("pinned Git path"),
                git_client,
                object,
            ),
            "{label}"
        );
    }
}

fn assert_combined_filter_fetch_matches_stock_git(
    label: &str,
    git_client: &std::path::Path,
    zmin_client: &std::path::Path,
    root_blob: &str,
    child_blob: &str,
    dir_tree: &str,
) {
    assert_filter_fetch_common_matches_stock_git(label, git_client, zmin_client);
    assert_eq!(
        filtered_object_local_presence(zmin_bin(), zmin_client, root_blob),
        0,
        "{label}"
    );
    assert_eq!(
        filtered_object_local_presence(
            required_pinned_stock_git()
                .to_str()
                .expect("pinned Git path"),
            git_client,
            root_blob,
        ),
        0,
        "{label}"
    );
    for object in [child_blob, dir_tree] {
        assert_ne!(
            filtered_object_local_presence(zmin_bin(), zmin_client, object),
            0,
            "{label}"
        );
        assert_eq!(
            filtered_object_local_presence(zmin_bin(), zmin_client, object),
            filtered_object_local_presence(
                required_pinned_stock_git()
                    .to_str()
                    .expect("pinned Git path"),
                git_client,
                object,
            ),
            "{label}"
        );
    }
}

fn assert_sparse_oid_filter_fetch_matches_stock_git(
    label: &str,
    git_client: &std::path::Path,
    zmin_client: &std::path::Path,
    root_blob: &str,
    keep_blob: &str,
    drop_blob: &str,
) {
    assert_filter_fetch_common_matches_stock_git(label, git_client, zmin_client);
    for object in [root_blob, keep_blob] {
        assert_eq!(
            filtered_object_local_presence(zmin_bin(), zmin_client, object),
            0,
            "{label}"
        );
        assert_eq!(
            filtered_object_local_presence(
                required_pinned_stock_git()
                    .to_str()
                    .expect("pinned Git path"),
                git_client,
                object,
            ),
            0,
            "{label}"
        );
    }
    assert_ne!(
        filtered_object_local_presence(zmin_bin(), zmin_client, drop_blob),
        0,
        "{label}"
    );
    assert_eq!(
        filtered_object_local_presence(zmin_bin(), zmin_client, drop_blob),
        filtered_object_local_presence(
            required_pinned_stock_git()
                .to_str()
                .expect("pinned Git path"),
            git_client,
            drop_blob,
        ),
        "{label}"
    );
}

fn filtered_blob_local_presence(command: &str, repo: &std::path::Path, path: &str) -> i32 {
    let blobish = format!("origin/main:{path}");
    let blob = git(repo, ["rev-parse", blobish.as_str()]);
    filtered_object_local_presence(command, repo, blob.as_str())
}

fn filtered_object_local_presence(command: &str, repo: &std::path::Path, object: &str) -> i32 {
    Command::new(command)
        .current_dir(repo)
        .env("GIT_NO_LAZY_FETCH", "1")
        .args(["cat-file", "-e", object])
        .output()
        .expect("cat-file local blob presence")
        .status
        .code()
        .expect("cat-file exit code")
}

#[test]
fn fetch_reads_shallow_ssh_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    for idx in 1..=3 {
        fs::write(work.join("a.txt"), format!("commit {idx}\n")).expect("write a");
        git(&work, ["add", "-A"]);
        git_with_env(&work, ["commit", "-m", &format!("commit {idx}")]);
    }
    let parent = git(&work, ["rev-parse", "HEAD^"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    for client in [&git_client, &zmin_client] {
        git(dir.path(), ["init", client.to_str().expect("client path")]);
        git(client, ["remote", "add", "origin", url.as_str()]);
    }

    command_output_with_env(
        "git",
        &git_client,
        &["fetch", "--depth=1", "origin", "main"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git shallow fetch ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--depth=1", "origin", "main"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin shallow fetch ssh",
    );

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_matching_shallow_state(&zmin_client, &git_client, &parent);
}

#[test]
fn fetch_depth_ssh_multiple_explicit_refspecs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let (remote, main_parent, feature_parent) = prepare_two_branch_shallow_remote(dir.path());
    let git_client = dir.path().join("git-depth-multi-ssh");
    let zmin_client = dir.path().join("zmin-depth-multi-ssh");
    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    for client in [&git_client, &zmin_client] {
        git(dir.path(), ["init", client.to_str().expect("client path")]);
        git(client, ["remote", "add", "origin", url.as_str()]);
    }
    let args = [
        "fetch",
        "--depth=1",
        "origin",
        "refs/heads/main:refs/remotes/origin/main",
        "refs/heads/feature:refs/remotes/origin/feature",
    ];

    command_output_with_env(
        "git",
        &git_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git shallow multi-refspec ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin shallow multi-refspec ssh",
    );

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_matching_shallow_state_for_missing_objects(
        &zmin_client,
        &git_client,
        &[main_parent, feature_parent],
    );
}

#[test]
fn clone_reads_ssh_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::create_dir_all(work.join("dir")).expect("create dir");
    fs::write(work.join("dir/a.txt"), b"hello\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature"]);
    set_bare_head_to_main(&remote);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    command_output_with_env(
        "git",
        dir.path(),
        &[
            "clone",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git clone",
    );
    command_output_with_env(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin clone",
    );
    assert_eq!(
        fs::read_to_string(zmin_clone.join("dir/a.txt")).expect("read zmin a"),
        fs::read_to_string(git_clone.join("dir/a.txt")).expect("read git a")
    );
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&git_clone, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&zmin_clone, ["show-ref"]),
        git(&git_clone, ["show-ref"])
    );
}

#[test]
fn ssh_clone_filter_rejects_before_ssh_or_destination() {
    let dir = TempDir::new().expect("temp dir");
    let (fake_ssh, request_log) = write_logging_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let destination = dir.path().join("ssh-filter-destination");
    let output = command_any_output_with_env(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--filter=blob:none",
            "ssh://example.test/repo.git",
            destination.to_str().expect("destination path"),
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "ssh clone filter preflight",
    );

    assert_eq!(output.0, 129);
    assert!(output.1.is_empty());
    assert_eq!(
        output.2,
        "fatal: clone --filter over SSH and git daemon requires --no-checkout or --bare"
    );
    assert!(!destination.exists());
    assert!(
        !request_log.exists(),
        "SSH was invoked before preflight rejection"
    );
}

#[test]
fn clone_filter_v0_no_checkout_ssh_and_daemon_preserve_promisor_state() {
    let dir = TempDir::new().expect("clone filter v0 temp dir");
    let remote = prepare_filter_remote(dir.path());
    let (fake_ssh, request_log) = write_logging_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let helper_path = pinned_git_helper_path(&pinned_http_bundle_root());
    let helper_path = helper_path
        .to_str()
        .expect("pinned Git helper path is UTF-8");
    let ssh_url = ssh_url_for_remote(&remote);
    let stock_ssh = dir.path().join("stock-ssh-filter-clone");
    let zmin_ssh = dir.path().join("zmin-ssh-filter-clone");
    let stock_ssh_args = [
        "clone",
        "-q",
        "--no-checkout",
        "--depth=1",
        "--filter=blob:none",
        ssh_url.as_str(),
        stock_ssh.to_str().expect("stock SSH clone"),
    ];
    let zmin_ssh_args = [
        "clone",
        "-q",
        "--no-checkout",
        "--depth=1",
        "--filter=blob:none",
        ssh_url.as_str(),
        zmin_ssh.to_str().expect("Zmin SSH clone"),
    ];
    let stock_output = pinned_command_any_output_with_env(
        dir.path(),
        &stock_ssh_args,
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "pinned SSH filtered clone",
    );
    let zmin_output = command_any_output_with_env(
        zmin_bin(),
        dir.path(),
        &zmin_ssh_args,
        &[
            ("GIT_SSH_COMMAND", fake_ssh_arg.as_str()),
            ("PATH", helper_path),
        ],
        "Zmin SSH filtered clone",
    );
    assert_eq!(zmin_output.0, stock_output.0, "SSH filtered clone status");
    assert_eq!(zmin_output.2.matches("warning:").count(), 0);
    assert_eq!(stock_output.2.matches("warning:").count(), 0);
    assert_eq!(git(&zmin_ssh, ["show-ref"]), git(&stock_ssh, ["show-ref"]));
    assert_eq!(
        run_zmin(&zmin_ssh, ["config", "--get", "remote.origin.promisor"]),
        pinned_git_args(&stock_ssh, ["config", "--get", "remote.origin.promisor"]),
    );
    assert_promisor_marker_roles_match("SSH filtered clone", &stock_ssh, &zmin_ssh);
    assert_ne!(
        filtered_blob_local_presence(zmin_bin(), &zmin_ssh, "a.txt"),
        0,
        "SSH filtered clone unexpectedly hydrated blob"
    );
    assert_eq!(
        request_log
            .exists()
            .then(|| fs::read_to_string(&request_log).expect("SSH request log"))
            .unwrap_or_default()
            .matches("REMOTE_COMMAND=")
            .count(),
        2,
        "SSH clones must use one advertised session each"
    );

    let daemon_port = unused_local_port();
    let _daemon = StockGitDaemon::spawn_pinned(dir.path(), daemon_port);
    let daemon_url = format!("git://127.0.0.1:{daemon_port}/filter.git");
    let stock_daemon = dir.path().join("stock-daemon-filter-clone");
    let zmin_daemon = dir.path().join("zmin-daemon-filter-clone");
    let stock_daemon_args = [
        "clone",
        "-q",
        "--no-checkout",
        "--depth=1",
        "--filter=blob:none",
        daemon_url.as_str(),
        stock_daemon.to_str().expect("stock daemon clone"),
    ];
    let zmin_daemon_args = [
        "clone",
        "-q",
        "--no-checkout",
        "--depth=1",
        "--filter=blob:none",
        daemon_url.as_str(),
        zmin_daemon.to_str().expect("Zmin daemon clone"),
    ];
    let stock_output = pinned_command_any_output(
        dir.path(),
        &stock_daemon_args,
        "pinned daemon filtered clone",
    );
    let zmin_output = command_any_output(
        zmin_bin(),
        dir.path(),
        &zmin_daemon_args,
        "Zmin daemon filtered clone",
    );
    assert_eq!(
        zmin_output.0, stock_output.0,
        "daemon filtered clone status"
    );
    assert_eq!(
        git(&zmin_daemon, ["show-ref"]),
        git(&stock_daemon, ["show-ref"])
    );
    assert_promisor_marker_roles_match("daemon filtered clone", &stock_daemon, &zmin_daemon);
    assert_ne!(
        filtered_blob_local_presence(zmin_bin(), &zmin_daemon, "a.txt"),
        0,
        "daemon filtered clone unexpectedly hydrated blob"
    );

    pinned_git_args(&remote, ["config", "uploadpack.allowFilter", "false"]);
    let unsupported_ssh = dir.path().join("zmin-ssh-unsupported-filter-clone");
    let unsupported_args = [
        "clone",
        "-q",
        "--no-checkout",
        "--filter=blob:none",
        ssh_url.as_str(),
        unsupported_ssh.to_str().expect("unsupported SSH clone"),
    ];
    let unsupported_output = command_any_output_with_env(
        zmin_bin(),
        dir.path(),
        &unsupported_args,
        &[
            ("GIT_SSH_COMMAND", fake_ssh_arg.as_str()),
            ("PATH", helper_path),
        ],
        "Zmin unsupported SSH filtered clone",
    );
    assert_eq!(unsupported_output.0, 0);
    assert_eq!(
        unsupported_output
            .2
            .matches("warning: filtering not recognized by server, ignoring")
            .count(),
        2
    );
    assert_eq!(
        filtered_blob_local_presence(zmin_bin(), &unsupported_ssh, "a.txt"),
        0,
        "unsupported SSH fallback must retain full blob"
    );
    assert_eq!(
        run_zmin(
            &unsupported_ssh,
            ["config", "--get", "remote.origin.partialclonefilter"]
        ),
        "blob:none"
    );
    assert!(
        http_pack_role_snapshot(&unsupported_ssh)
            .iter()
            .any(|name| name.ends_with(".promisor")),
        "unsupported SSH fallback must publish a promisor marker"
    );

    let unsupported_daemon = dir.path().join("zmin-daemon-unsupported-filter-clone");
    let unsupported_daemon_args = [
        "clone",
        "-q",
        "--no-checkout",
        "--filter=blob:none",
        daemon_url.as_str(),
        unsupported_daemon
            .to_str()
            .expect("unsupported daemon clone"),
    ];
    let unsupported_daemon_output = command_any_output(
        zmin_bin(),
        dir.path(),
        &unsupported_daemon_args,
        "Zmin unsupported daemon filtered clone",
    );
    assert_eq!(unsupported_daemon_output.0, 0);
    assert_eq!(
        unsupported_daemon_output
            .2
            .matches("warning: filtering not recognized by server, ignoring")
            .count(),
        2
    );
    assert_eq!(
        filtered_blob_local_presence(zmin_bin(), &unsupported_daemon, "a.txt"),
        0,
        "unsupported daemon fallback must retain full blob"
    );
    assert_eq!(
        run_zmin(
            &unsupported_daemon,
            ["config", "--get", "remote.origin.partialclonefilter"]
        ),
        "blob:none"
    );
    assert!(
        http_pack_role_snapshot(&unsupported_daemon)
            .iter()
            .any(|name| name.ends_with(".promisor")),
        "unsupported daemon fallback must publish a promisor marker"
    );
}

#[test]
fn ssh_filtered_default_checkout_hydrates_missing_blobs_once_sha1_sha256_v0_v2() {
    for &(sha256, protocol_v2, label) in &[
        (false, false, "ssh-filter-checkout-sha1-v0"),
        (false, true, "ssh-filter-checkout-sha1-v2"),
        (true, true, "ssh-filter-checkout-sha256-v2"),
    ] {
        let dir = TempDir::new().expect("filtered SSH checkout temp dir");
        let remote = if sha256 {
            let remote = prepare_sha256_smart_http_remote(dir.path());
            pinned_git_args(&remote, ["config", "uploadpack.allowFilter", "true"]);
            remote
        } else {
            let remote = prepare_filter_remote(dir.path());
            if !protocol_v2 {
                pinned_git_args(&remote, ["config", "uploadpack.allowAnySHA1InWant", "true"]);
            }
            remote
        };
        let (clone, request_log) =
            run_logged_ssh_filtered_clone(dir.path(), &remote, protocol_v2, label, 2, 1);
        assert_eq!(
            run_zmin(&clone, ["rev-parse", "--show-object-format"]),
            if sha256 { "sha256" } else { "sha1" },
            "{label}: object format"
        );
        assert_eq!(
            run_zmin(&clone, ["config", "--get", "remote.origin.promisor"]),
            "true",
            "{label}: promisor config"
        );
        assert_eq!(
            run_zmin(
                &clone,
                ["config", "--get", "remote.origin.partialclonefilter"]
            ),
            "blob:none",
            "{label}: partial clone filter config"
        );
        assert!(
            pinned_git_args(&clone, ["status", "--porcelain"]).is_empty(),
            "{label}: checkout left worktree changes"
        );
        let (path, content) = if sha256 {
            ("sha256.txt", b"sha256 smart HTTP\n".as_slice())
        } else {
            ("a.txt", b"hello\n".as_slice())
        };
        assert_eq!(
            fs::read(clone.join(path)).expect("read hydrated checkout file"),
            content,
            "{label}: checkout content"
        );
        assert_eq!(
            filtered_blob_local_presence(zmin_bin(), &clone, path),
            0,
            "{label}: hydrated blob remains missing"
        );
        assert!(
            request_log.matches("REMOTE_COMMAND=").count() == 2,
            "{label}: expected initial plus one hydration session"
        );
    }
}

#[test]
fn ssh_filtered_default_checkout_unsupported_filter_has_no_hydration_session() {
    let dir = TempDir::new().expect("unsupported SSH checkout temp dir");
    let remote = prepare_filter_remote(dir.path());
    pinned_git_args(&remote, ["config", "uploadpack.allowAnySHA1InWant", "true"]);
    pinned_git_args(&remote, ["config", "uploadpack.allowFilter", "false"]);
    let (clone, request_log) = run_logged_ssh_filtered_clone(
        dir.path(),
        &remote,
        false,
        "ssh-filter-checkout-unsupported",
        1,
        0,
    );
    assert!(
        request_log
            .lines()
            .any(|line| line.contains("GIT_PROTOCOL=")),
        "unsupported SSH checkout did not record a v0 session"
    );
    assert_eq!(
        run_zmin(&clone, ["config", "--get", "remote.origin.promisor"]),
        "true"
    );
    assert_eq!(
        filtered_blob_local_presence(zmin_bin(), &clone, "a.txt"),
        0,
        "unsupported filter fallback did not retain the full blob"
    );
}

#[test]
fn ssh_filtered_default_checkout_hydration_failure_removes_destination() {
    let dir = TempDir::new().expect("failed SSH hydration temp dir");
    let remote = prepare_filter_remote(dir.path());
    let destination = dir.path().join("ssh-filter-checkout-failure");
    let (fake_ssh, request_log, counter) = write_pinned_fail_after_first_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let helper_path = pinned_git_helper_path(&pinned_http_bundle_root());
    let helper_path = helper_path
        .to_str()
        .expect("pinned Git helper path is UTF-8");
    let url = ssh_url_for_remote(&remote);
    let output = command_any_output_with_env(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "-q",
            "--filter=blob:none",
            &url,
            destination.to_str().expect("failure destination"),
        ],
        &[
            ("GIT_SSH_COMMAND", fake_ssh_arg.as_str()),
            ("PATH", helper_path),
        ],
        "SSH filtered checkout hydration failure",
    );
    assert_ne!(output.0, 0, "hydration failure unexpectedly succeeded");
    assert!(!destination.exists(), "failed clone left destination state");
    assert_eq!(
        fs::read_to_string(&counter).expect("read SSH failure counter"),
        "2\n",
        "hydration failure did not occur in the second session"
    );
    assert_eq!(
        fs::read_to_string(&request_log)
            .expect("read SSH failure log")
            .matches("REMOTE_COMMAND=")
            .count(),
        2,
        "hydration failure opened an unexpected number of sessions"
    );
}

#[test]
fn clone_instant_ssh_materializes_head_then_fetch_hydrates_refs() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_clone = dir.path().join("git-ssh-clone");
    let zmin_clone = dir.path().join("zmin-ssh-instant");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join(".gitattributes"), b"crlf.txt -text\n").expect("write attributes");
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    fs::write(work.join("crlf.txt"), b"line one\r\nline two\r\n").expect("write crlf");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["switch", "-c", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    git(&work, ["switch", "main"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "release"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    command_output_with_env(
        "git",
        dir.path(),
        &[
            "clone",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git clone ssh",
    );
    command_output_with_env(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--instant",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin clone instant ssh",
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&git_clone, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD^{tree}"]),
        git(&git_clone, ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        fs::read(zmin_clone.join("crlf.txt")).expect("zmin crlf"),
        fs::read(git_clone.join("crlf.txt")).expect("git crlf")
    );
    let initial_refs = git(&zmin_clone, ["show-ref"]);
    assert!(
        initial_refs
            .lines()
            .any(|line| line.ends_with(" refs/remotes/origin/main")),
        "instant clone should write the fetched HEAD branch ref:\n{initial_refs}"
    );
    assert!(
        !initial_refs
            .lines()
            .any(|line| line.ends_with(" refs/remotes/origin/feature")),
        "instant clone should not write refs for objects it did not request:\n{initial_refs}"
    );
    assert!(
        !initial_refs
            .lines()
            .any(|line| line.ends_with(" refs/tags/v1")),
        "instant clone should leave non-target tags for later fetch:\n{initial_refs}"
    );

    command_output_with_env(
        zmin_bin(),
        &zmin_clone,
        &["fetch", "origin"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin fetch ssh",
    );
    let hydrated_refs = git(&zmin_clone, ["show-ref"]);
    assert!(
        hydrated_refs
            .lines()
            .any(|line| line.ends_with(" refs/remotes/origin/feature")),
        "fetch should hydrate additional remote branch refs:\n{hydrated_refs}"
    );
    assert!(
        hydrated_refs
            .lines()
            .any(|line| line.ends_with(" refs/tags/v1")),
        "fetch should hydrate followed tag refs:\n{hydrated_refs}"
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
}

#[test]
fn clone_instant_ssh_demand_hydrate_recovers_missing_head_objects() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let zmin_clone = dir.path().join("zmin-ssh-instant-demand");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    command_output_with_env(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--instant",
            "--demand-hydrate",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin clone instant ssh demand",
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_demand_hydrate_config(&zmin_clone);
    let head = git(&zmin_clone, ["rev-parse", "HEAD"]);
    remove_all_pack_files(&zmin_clone);

    let head_type = command_output_with_env(
        zmin_bin(),
        &zmin_clone,
        &["cat-file", "-t", "HEAD"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin cat-file ssh demand",
    );
    assert_eq!(head_type.1, "commit");
    let object_type = command_output_with_env(
        zmin_bin(),
        &zmin_clone,
        &["cat-file", "-t", &head],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin cat-file ssh demand head",
    );
    assert_eq!(object_type.1, "commit");
    git(&zmin_clone, ["fsck", "--strict"]);
}

#[test]
fn clone_worktree_first_ssh_demand_hydrate_recovers_missing_head_objects() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let zmin_clone = dir.path().join("zmin-ssh-worktree-first-demand");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    command_output_with_env(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--worktree-first",
            "--demand-hydrate",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin clone worktree-first ssh demand",
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_demand_hydrate_config(&zmin_clone);
    let head = git(&zmin_clone, ["rev-parse", "HEAD"]);
    remove_all_pack_files(&zmin_clone);

    let head_type = command_output_with_env(
        zmin_bin(),
        &zmin_clone,
        &["cat-file", "-t", "HEAD"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin cat-file worktree-first ssh demand",
    );
    assert_eq!(head_type.1, "commit");
    let object_type = command_output_with_env(
        zmin_bin(),
        &zmin_clone,
        &["cat-file", "-t", &head],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin cat-file worktree-first ssh demand head",
    );
    assert_eq!(object_type.1, "commit");
    git(&zmin_clone, ["fsck", "--strict"]);
}

#[test]
fn clone_instant_ssh_background_fetch_hydrates_refs() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let zmin_clone = dir.path().join("zmin-ssh-instant-background");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["switch", "-c", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    git(&work, ["switch", "main"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "release"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    command_output_with_env(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--instant",
            "--background-fetch",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin clone instant ssh background",
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_background_fetch_hydrated(&zmin_clone);
}

#[test]
fn clone_worktree_first_ssh_background_fetch_hydrates_refs() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let zmin_clone = dir.path().join("zmin-ssh-worktree-first-background");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["switch", "-c", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    git(&work, ["switch", "main"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "release"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    command_output_with_env(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--worktree-first",
            "--background-fetch",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin clone worktree-first ssh background",
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_background_fetch_hydrated(&zmin_clone);
}

#[test]
fn clone_reads_shallow_ssh_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    for idx in 1..=3 {
        fs::write(work.join("a.txt"), format!("commit {idx}\n")).expect("write a");
        git(&work, ["add", "-A"]);
        git_with_env(&work, ["commit", "-m", &format!("commit {idx}")]);
    }
    let parent = git(&work, ["rev-parse", "HEAD^"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    command_output_with_env(
        "git",
        dir.path(),
        &[
            "clone",
            "--depth=1",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git shallow clone ssh",
    );
    command_output_with_env(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--depth=1",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin shallow clone ssh",
    );

    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&git_clone, ["rev-parse", "HEAD"])
    );
    assert_matching_shallow_state(&zmin_clone, &git_clone, &parent);
}

#[test]
fn clone_shared_is_ignored_for_ssh_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    command_output_with_env(
        "git",
        dir.path(),
        &[
            "clone",
            "--shared",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git shared clone ssh",
    );
    command_output_with_env(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--shared",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin shared clone ssh",
    );
    assert_eq!(
        git(&zmin_clone, ["show-ref"]),
        git(&git_clone, ["show-ref"])
    );
    assert_no_alternates(&git_clone);
    assert_no_alternates(&zmin_clone);
}

#[test]
fn push_writes_ssh_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote_git = dir.path().join("remote-git.git");
    let remote_zmin = dir.path().join("remote-zmin.git");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote-git.git"]);
    git(dir.path(), ["init", "--bare", "remote-zmin.git"]);
    git(dir.path(), ["init", "-b", "main", "git-client"]);
    git(dir.path(), ["init", "-b", "main", "zmin-client"]);
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    for client in [&git_client, &zmin_client] {
        fs::write(client.join("a.txt"), b"hello\n").expect("write a");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "initial"]);
    }

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let git_url = ssh_url_for_remote(&remote_git);
    let zmin_url = ssh_url_for_remote(&remote_zmin);
    git(&git_client, ["remote", "add", "origin", git_url.as_str()]);
    git(&zmin_client, ["remote", "add", "origin", zmin_url.as_str()]);

    command_output_with_env(
        "git",
        &git_client,
        &["push", "-u", "origin", "main"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git push",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["push", "-u", "origin", "main"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin push",
    );
    assert_eq!(
        git(&remote_zmin, ["rev-parse", "refs/heads/main"]),
        git(&remote_git, ["rev-parse", "refs/heads/main"])
    );
    assert_eq!(
        git(&remote_zmin, ["cat-file", "-p", "refs/heads/main:a.txt"]),
        git(&remote_git, ["cat-file", "-p", "refs/heads/main:a.txt"])
    );
    assert_eq!(
        git(&zmin_client, ["config", "--get", "branch.main.remote"]),
        git(&git_client, ["config", "--get", "branch.main.remote"])
    );
    assert_eq!(
        git(&zmin_client, ["config", "--get", "branch.main.merge"]),
        git(&git_client, ["config", "--get", "branch.main.merge"])
    );

    for client in [&git_client, &zmin_client] {
        git(client, ["checkout", "-b", "feature"]);
        fs::write(client.join("feature.txt"), b"feature\n").expect("write feature");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "feature"]);
    }
    command_output_with_env(
        "git",
        &git_client,
        &["push", "origin", "feature"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git push feature",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["push", "origin", "feature"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin push feature",
    );
    assert_eq!(
        git(&remote_zmin, ["rev-parse", "refs/heads/feature"]),
        git(&remote_git, ["rev-parse", "refs/heads/feature"])
    );

    command_output_with_env(
        "git",
        &git_client,
        &["push", "origin", ":feature"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git push delete",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["push", "origin", ":feature"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin push delete",
    );
    assert_eq!(
        git(&remote_zmin, ["show-ref"]),
        git(&remote_git, ["show-ref"])
    );
}

#[test]
fn push_writes_smart_http_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote_git = dir.path().join("remote-git.git");
    let remote_zmin = dir.path().join("remote-zmin.git");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote-git.git"]);
    git(dir.path(), ["init", "--bare", "remote-zmin.git"]);
    fs::write(remote_git.join("git-daemon-export-ok"), "").expect("export git");
    fs::write(remote_zmin.join("git-daemon-export-ok"), "").expect("export zmin");
    git(&remote_git, ["config", "http.receivepack", "true"]);
    git(&remote_zmin, ["config", "http.receivepack", "true"]);
    git(dir.path(), ["init", "-b", "main", "git-client"]);
    git(dir.path(), ["init", "-b", "main", "zmin-client"]);
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    for client in [&git_client, &zmin_client] {
        fs::write(client.join("a.txt"), b"hello\n").expect("write a");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "initial"]);
    }

    let git_server = BackendHttpServer::new(
        required_pinned_stock_git().display().to_string(),
        dir.path().to_path_buf(),
    );
    let zmin_server = BackendHttpServer::new(zmin_bin().to_owned(), dir.path().to_path_buf());
    let git_url = format!("http://127.0.0.1:{}/remote-git.git", git_server.port);
    let zmin_url = format!("http://127.0.0.1:{}/remote-zmin.git", zmin_server.port);
    git(&git_client, ["remote", "add", "origin", zmin_url.as_str()]);
    git(&zmin_client, ["remote", "add", "origin", git_url.as_str()]);

    command_output_with_env(
        "git",
        &git_client,
        &["push", "-u", "origin", "main"],
        &[],
        "git push http",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["push", "-u", "origin", "main"],
        &[],
        "zmin push http",
    );
    assert_eq!(
        git(&remote_zmin, ["rev-parse", "refs/heads/main"]),
        git(&remote_git, ["rev-parse", "refs/heads/main"])
    );
    assert_eq!(
        git(&remote_zmin, ["cat-file", "-p", "refs/heads/main:a.txt"]),
        git(&remote_git, ["cat-file", "-p", "refs/heads/main:a.txt"])
    );

    for client in [&git_client, &zmin_client] {
        git(client, ["checkout", "-b", "feature"]);
        fs::write(client.join("feature.txt"), b"feature\n").expect("write feature");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "feature"]);
    }
    command_output_with_env(
        "git",
        &git_client,
        &["push", "origin", "feature"],
        &[],
        "git push feature http",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["push", "origin", "feature"],
        &[],
        "zmin push feature http",
    );
    assert_eq!(
        git(&remote_zmin, ["rev-parse", "refs/heads/feature"]),
        git(&remote_git, ["rev-parse", "refs/heads/feature"])
    );

    command_output_with_env(
        "git",
        &git_client,
        &["push", "origin", ":feature"],
        &[],
        "git push delete http",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["push", "origin", ":feature"],
        &[],
        "zmin push delete http",
    );
    assert_eq!(
        git(&remote_zmin, ["show-ref"]),
        git(&remote_git, ["show-ref"])
    );
}

#[test]
#[cfg(not(windows))]
fn push_writes_git_daemon_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote_git = dir.path().join("remote-git.git");
    let remote_zmin = dir.path().join("remote-zmin.git");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote-git.git"]);
    git(dir.path(), ["init", "--bare", "remote-zmin.git"]);
    git(dir.path(), ["init", "-b", "main", "git-client"]);
    git(dir.path(), ["init", "-b", "main", "zmin-client"]);
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    for client in [&git_client, &zmin_client] {
        fs::write(client.join("a.txt"), b"hello\n").expect("write a");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "initial"]);
    }

    let git_port = unused_local_port();
    let zmin_port = unused_local_port();
    let _git_daemon =
        StockGitDaemon::spawn_with_args(dir.path(), git_port, &["--enable=receive-pack"]);
    let _zmin_daemon =
        StockGitDaemon::spawn_with_args(dir.path(), zmin_port, &["--enable=receive-pack"]);
    let git_url = format!("git://127.0.0.1:{git_port}/remote-git.git");
    let zmin_url = format!("git://127.0.0.1:{zmin_port}/remote-zmin.git");
    git(&git_client, ["remote", "add", "origin", git_url.as_str()]);
    git(&zmin_client, ["remote", "add", "origin", zmin_url.as_str()]);

    command_output_with_env(
        "git",
        &git_client,
        &["push", "-u", "origin", "main"],
        &[],
        "git push daemon",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["push", "-u", "origin", "main"],
        &[],
        "zmin push daemon",
    );
    assert_eq!(
        git(&remote_zmin, ["rev-parse", "refs/heads/main"]),
        git(&remote_git, ["rev-parse", "refs/heads/main"])
    );
    assert_eq!(
        git(&remote_zmin, ["cat-file", "-p", "refs/heads/main:a.txt"]),
        git(&remote_git, ["cat-file", "-p", "refs/heads/main:a.txt"])
    );

    for client in [&git_client, &zmin_client] {
        git(client, ["checkout", "-b", "feature"]);
        fs::write(client.join("feature.txt"), b"feature\n").expect("write feature");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "feature"]);
    }
    command_output_with_env(
        "git",
        &git_client,
        &["push", "origin", "feature"],
        &[],
        "git push feature daemon",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["push", "origin", "feature"],
        &[],
        "zmin push feature daemon",
    );
    assert_eq!(
        git(&remote_zmin, ["rev-parse", "refs/heads/feature"]),
        git(&remote_git, ["rev-parse", "refs/heads/feature"])
    );

    command_output_with_env(
        "git",
        &git_client,
        &["push", "origin", ":feature"],
        &[],
        "git push delete daemon",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["push", "origin", ":feature"],
        &[],
        "zmin push delete daemon",
    );
    assert_eq!(
        git(&remote_zmin, ["show-ref"]),
        git(&remote_git, ["show-ref"])
    );
}

#[test]
fn http_backend_info_refs_matches_stock_git_smart_discovery_refs() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "tag message"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let zmin = http_backend_response(zmin_bin(), dir.path());
    let git = http_backend_response("git", dir.path());
    assert!(
        String::from_utf8_lossy(&zmin)
            .contains("Content-Type: application/x-git-upload-pack-advertisement")
    );
    assert_eq!(smart_http_ref_lines(&zmin), smart_http_ref_lines(&git));
}

#[test]
fn http_backend_filter_capability_follows_server_policy_v0_and_v2() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    git(&remote, ["config", "uploadpack.allowFilter", "false"]);
    let disabled_v0 = http_backend_response_with_body_and_protocol(
        zmin_bin(),
        dir.path(),
        "/remote.git/info/refs",
        "service=git-upload-pack",
        "GET",
        &[],
        None,
    );
    assert!(
        !disabled_v0
            .windows(b" filter object-format=".len())
            .any(|window| window == b" filter object-format=")
    );
    let disabled_v2 = http_backend_response_with_body_and_protocol(
        zmin_bin(),
        dir.path(),
        "/remote.git/info/refs",
        "service=git-upload-pack",
        "GET",
        &[],
        Some("version=2"),
    );
    assert!(
        !disabled_v2
            .windows(b"fetch=shallow wait-for-done filter\n".len())
            .any(|window| window == b"fetch=shallow wait-for-done filter\n")
    );

    git(&remote, ["config", "uploadpack.allowFilter", "true"]);
    let enabled_v0 = http_backend_response_with_body_and_protocol(
        zmin_bin(),
        dir.path(),
        "/remote.git/info/refs",
        "service=git-upload-pack",
        "GET",
        &[],
        None,
    );
    assert_eq!(
        enabled_v0
            .windows(b" filter object-format=".len())
            .filter(|window| *window == b" filter object-format=")
            .count(),
        1
    );
    let enabled_v2 = http_backend_response_with_body_and_protocol(
        zmin_bin(),
        dir.path(),
        "/remote.git/info/refs",
        "service=git-upload-pack",
        "GET",
        &[],
        Some("version=2"),
    );
    assert_eq!(
        enabled_v2
            .windows(b"fetch=shallow wait-for-done filter\n".len())
            .filter(|window| *window == b"fetch=shallow wait-for-done filter\n")
            .count(),
        1
    );
}

#[test]
fn http_backend_resolves_scriptalias_path_translated_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let zmin = http_backend_response_with_translated_path(zmin_bin(), dir.path());
    let git = http_backend_response_with_translated_path("git", dir.path());
    assert_eq!(smart_http_ref_lines(&zmin), smart_http_ref_lines(&git));
}

#[test]
fn http_backend_serves_scriptalias_non_bare_repo_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let work = dir.path().join("server");
    git(dir.path(), ["init", "-b", "main", "server"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);

    let zmin =
        http_backend_response_with_translated_path_at(zmin_bin(), dir.path(), "/server/info/refs");
    let git = http_backend_response_with_translated_path_at("git", dir.path(), "/server/info/refs");
    assert_eq!(smart_http_ref_lines(&zmin), smart_http_ref_lines(&git));
}

#[test]
fn http_backend_upload_pack_post_returns_stock_readable_pack() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(
        work.join("a.txt"),
        format!("{}\nbase\n", "shared line\n".repeat(2_000)),
    )
    .expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    let base = git(&work, ["rev-parse", "HEAD"]);
    let base_blob = git(&work, ["rev-parse", "HEAD:a.txt"]);
    fs::write(
        work.join("a.txt"),
        format!("{}\nchanged\n", "shared line\n".repeat(2_000)),
    )
    .expect("rewrite fixture");
    fs::write(
        work.join("b.txt"),
        format!("{}\nchanged sibling\n", "shared line\n".repeat(2_000)),
    )
    .expect("write sibling fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "changed"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);
    let head = git(&work, ["rev-parse", "HEAD"]);

    let mut request = Vec::new();
    request.extend(pkt_line_bytes(
        format!("want {head} side-band-64k thin-pack ofs-delta\n").as_bytes(),
    ));
    request.extend_from_slice(b"0000");
    request.extend(pkt_line_bytes(format!("have {base}\n").as_bytes()));
    request.extend(pkt_line_bytes(b"done\n"));

    let response = http_backend_response_with_body(
        zmin_bin(),
        dir.path(),
        "/remote.git/git-upload-pack",
        "",
        "POST",
        &request,
    );
    assert!(
        String::from_utf8_lossy(&response)
            .contains("Content-Type: application/x-git-upload-pack-result")
    );
    assert!(
        String::from_utf8_lossy(&response).contains(&format!("ACK {base}\n")),
        "expected upload-pack to ACK common have"
    );
    let pack = sideband_pack_from_http_response(&response);
    assert_eq!(&pack[..4], b"PACK");

    let verify = git_init();
    let index_output = git_with_stdin_bytes(verify.path(), ["index-pack", "--stdin"], &pack);
    let pack_id = index_output
        .strip_prefix("pack\t")
        .expect("index-pack output pack id");
    let idx = verify
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{pack_id}.idx"));
    let verify_pack = git_args(
        verify.path(),
        &["verify-pack", "-v", idx.to_str().expect("idx path")],
    );
    assert!(
        verify_pack
            .lines()
            .any(|line| line.contains(" blob ") && line.split_whitespace().count() >= 7),
        "expected upload-pack response to contain a delta:\n{verify_pack}"
    );
    assert!(
        !verify_pack.lines().any(|line| line.starts_with(&base)),
        "pack should not resend common base commit:\n{verify_pack}"
    );
    assert!(
        !verify_pack.lines().any(|line| line.starts_with(&base_blob)),
        "pack should not resend common base blob:\n{verify_pack}"
    );
}

#[test]
fn http_backend_upload_pack_deepen_emits_shallow_boundary_and_depth_limited_pack() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    for idx in 1..=3 {
        fs::write(work.join("a.txt"), format!("commit {idx}\n")).expect("write fixture");
        git(&work, ["add", "-A"]);
        git_with_env(&work, ["commit", "-m", &format!("commit {idx}")]);
    }
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);
    let head = git(&work, ["rev-parse", "HEAD"]);
    let parent = git(&work, ["rev-parse", "HEAD^"]);

    let mut request = Vec::new();
    request.extend(pkt_line_bytes(
        format!("want {head} side-band-64k thin-pack ofs-delta\n").as_bytes(),
    ));
    request.extend(pkt_line_bytes(b"deepen 1\n"));
    request.extend_from_slice(b"0000");
    request.extend(pkt_line_bytes(b"done\n"));

    let response = http_backend_response_with_body(
        zmin_bin(),
        dir.path(),
        "/remote.git/git-upload-pack",
        "",
        "POST",
        &request,
    );
    let control = upload_pack_control_lines(&response);
    assert!(
        control
            .iter()
            .any(|line| line == &format!("shallow {head}")),
        "expected shallow boundary for wanted head, got {control:?}"
    );
    let pack = sideband_pack_from_http_response(&response);
    let verify = git_init();
    let pack_id = git_with_stdin_bytes(verify.path(), ["index-pack", "--stdin"], &pack)
        .strip_prefix("pack\t")
        .expect("index-pack output pack id")
        .to_owned();
    let idx = verify
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{pack_id}.idx"));
    let verify_pack = git_args(
        verify.path(),
        &["verify-pack", "-v", idx.to_str().expect("idx path")],
    );
    assert!(
        verify_pack.lines().any(|line| line.starts_with(&head)),
        "pack should include wanted head:\n{verify_pack}"
    );
    assert!(
        !verify_pack.lines().any(|line| line.starts_with(&parent)),
        "depth-1 pack should not include parent commit:\n{verify_pack}"
    );
}

#[test]
fn http_backend_upload_pack_deepen_since_emits_time_limited_pack() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    for (idx, timestamp) in [(1, 1700000100), (2, 1700000200), (3, 1700000300)] {
        fs::write(work.join("a.txt"), format!("commit {idx}\n")).expect("write fixture");
        git(&work, ["add", "-A"]);
        let output = Command::new(stock_git_bin())
            .args(["commit", "-m", &format!("commit {idx}")])
            .current_dir(&work)
            .env("GIT_AUTHOR_DATE", format!("{timestamp} +0000"))
            .env("GIT_COMMITTER_DATE", format!("{timestamp} +0000"))
            .output()
            .expect("commit dated fixture");
        assert!(
            output.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);
    let head = git(&work, ["rev-parse", "HEAD"]);
    let parent = git(&work, ["rev-parse", "HEAD^"]);

    let mut request = Vec::new();
    request.extend(pkt_line_bytes(
        format!("want {head} side-band-64k thin-pack ofs-delta\n").as_bytes(),
    ));
    request.extend(pkt_line_bytes(b"deepen-since 1700000250\n"));
    request.extend_from_slice(b"0000");
    request.extend(pkt_line_bytes(b"done\n"));

    let response = http_backend_response_with_body(
        zmin_bin(),
        dir.path(),
        "/remote.git/git-upload-pack",
        "",
        "POST",
        &request,
    );
    let control = upload_pack_control_lines(&response);
    assert!(
        control
            .iter()
            .any(|line| line == &format!("shallow {head}")),
        "expected deepen-since shallow boundary for wanted head, got {control:?}"
    );
    let pack = sideband_pack_from_http_response(&response);
    let verify = git_init();
    let pack_id = git_with_stdin_bytes(verify.path(), ["index-pack", "--stdin"], &pack)
        .strip_prefix("pack\t")
        .expect("index-pack output pack id")
        .to_owned();
    let idx = verify
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{pack_id}.idx"));
    let verify_pack = git_args(
        verify.path(),
        &["verify-pack", "-v", idx.to_str().expect("idx path")],
    );
    assert!(
        verify_pack.lines().any(|line| line.starts_with(&head)),
        "deepen-since pack should include wanted head:\n{verify_pack}"
    );
    assert!(
        !verify_pack.lines().any(|line| line.starts_with(&parent)),
        "deepen-since pack should omit older parent:\n{verify_pack}"
    );
}

#[test]
fn http_backend_upload_pack_deepen_not_excludes_named_ref_history() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), "base\n").expect("write base");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "base"]);
    git(&work, ["branch", "base"]);
    fs::write(work.join("a.txt"), "main\n").expect("write main");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "main"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "base"]);
    set_bare_head_to_main(&remote);
    let head = git(&work, ["rev-parse", "main"]);
    let base = git(&work, ["rev-parse", "base"]);

    let mut request = Vec::new();
    request.extend(pkt_line_bytes(
        format!("want {head} side-band-64k thin-pack ofs-delta\n").as_bytes(),
    ));
    request.extend(pkt_line_bytes(b"deepen-not refs/heads/base\n"));
    request.extend_from_slice(b"0000");
    request.extend(pkt_line_bytes(b"done\n"));

    let response = http_backend_response_with_body(
        zmin_bin(),
        dir.path(),
        "/remote.git/git-upload-pack",
        "",
        "POST",
        &request,
    );
    let control = upload_pack_control_lines(&response);
    assert!(
        control
            .iter()
            .any(|line| line == &format!("shallow {head}")),
        "expected deepen-not shallow boundary for wanted head, got {control:?}"
    );
    let pack = sideband_pack_from_http_response(&response);
    let verify = git_init();
    let pack_id = git_with_stdin_bytes(verify.path(), ["index-pack", "--stdin"], &pack)
        .strip_prefix("pack\t")
        .expect("index-pack output pack id")
        .to_owned();
    let idx = verify
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{pack_id}.idx"));
    let verify_pack = git_args(
        verify.path(),
        &["verify-pack", "-v", idx.to_str().expect("idx path")],
    );
    assert!(
        verify_pack.lines().any(|line| line.starts_with(&head)),
        "deepen-not pack should include wanted head:\n{verify_pack}"
    );
    assert!(
        !verify_pack.lines().any(|line| line.starts_with(&base)),
        "deepen-not pack should omit excluded ref history:\n{verify_pack}"
    );
}

#[test]
fn http_backend_upload_pack_deepen_relative_extends_existing_shallow_boundary() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    for idx in 1..=3 {
        fs::write(work.join("a.txt"), format!("commit {idx}\n")).expect("write fixture");
        git(&work, ["add", "-A"]);
        git_with_env(&work, ["commit", "-m", &format!("commit {idx}")]);
    }
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);
    let head = git(&work, ["rev-parse", "HEAD"]);
    let parent = git(&work, ["rev-parse", "HEAD^"]);
    let grandparent = git(&work, ["rev-parse", "HEAD^^"]);

    let mut request = Vec::new();
    request.extend(pkt_line_bytes(
        format!("want {head} side-band-64k thin-pack ofs-delta\n").as_bytes(),
    ));
    request.extend(pkt_line_bytes(format!("shallow {head}\n").as_bytes()));
    request.extend(pkt_line_bytes(b"deepen 1\n"));
    request.extend(pkt_line_bytes(b"deepen-relative\n"));
    request.extend_from_slice(b"0000");
    request.extend(pkt_line_bytes(b"done\n"));

    let response = http_backend_response_with_body(
        zmin_bin(),
        dir.path(),
        "/remote.git/git-upload-pack",
        "",
        "POST",
        &request,
    );
    let control = upload_pack_control_lines(&response);
    assert!(
        control
            .iter()
            .any(|line| line == &format!("shallow {parent}")),
        "expected relative deepen shallow boundary at parent, got {control:?}"
    );
    let pack = sideband_pack_from_http_response(&response);
    let verify = git_init();
    let pack_id = git_with_stdin_bytes(verify.path(), ["index-pack", "--stdin"], &pack)
        .strip_prefix("pack\t")
        .expect("index-pack output pack id")
        .to_owned();
    let idx = verify
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{pack_id}.idx"));
    let verify_pack = git_args(
        verify.path(),
        &["verify-pack", "-v", idx.to_str().expect("idx path")],
    );
    assert!(
        verify_pack.lines().any(|line| line.starts_with(&parent)),
        "deepen-relative pack should include newly reachable parent:\n{verify_pack}"
    );
    assert!(
        !verify_pack
            .lines()
            .any(|line| line.starts_with(&grandparent)),
        "deepen-relative pack should not include commits beyond the requested increment:\n{verify_pack}"
    );
}

#[test]
fn http_backend_upload_pack_filter_blob_none_omits_blob_objects() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::create_dir_all(work.join("dir")).expect("create dir");
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    fs::write(work.join("dir/b.txt"), b"world\n").expect("write b");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);
    let head = git(&work, ["rev-parse", "HEAD"]);

    let mut request = Vec::new();
    request.extend(pkt_line_bytes(
        format!("want {head} side-band-64k thin-pack ofs-delta no-progress filter\n").as_bytes(),
    ));
    request.extend(pkt_line_bytes(b"filter blob:none\n"));
    request.extend_from_slice(b"0000");
    request.extend(pkt_line_bytes(b"done\n"));

    let response = http_backend_response_with_body(
        zmin_bin(),
        dir.path(),
        "/remote.git/git-upload-pack",
        "",
        "POST",
        &request,
    );
    let pack = sideband_pack_from_http_response(&response);
    let verify = git_init();
    let pack_id = git_with_stdin_bytes(verify.path(), ["index-pack", "--stdin"], &pack)
        .strip_prefix("pack\t")
        .expect("index-pack output pack id")
        .to_owned();
    let idx = verify
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{pack_id}.idx"));
    let verify_pack = git_args(
        verify.path(),
        &["verify-pack", "-v", idx.to_str().expect("idx path")],
    );
    assert!(
        verify_pack.lines().any(|line| line.contains(" tree ")),
        "blob:none pack should keep tree objects:\n{verify_pack}"
    );
    assert!(
        !verify_pack.lines().any(|line| line.contains(" blob ")),
        "blob:none pack should omit blob objects:\n{verify_pack}"
    );
}

#[test]
fn http_backend_upload_pack_invalid_filters_match_stock_git_failures() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(&remote, ["config", "uploadpack.allowFilter", "true"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);
    let head = git(&work, ["rev-parse", "HEAD"]);

    for filter in [
        "bad",
        "blob:limit=abc",
        "object:type=bad",
        "tree:abc",
        "combine:",
    ] {
        let mut request = Vec::new();
        request.extend(pkt_line_bytes(
            format!("want {head} side-band-64k thin-pack ofs-delta no-progress filter\n")
                .as_bytes(),
        ));
        request.extend(pkt_line_bytes(format!("filter {filter}\n").as_bytes()));
        request.extend_from_slice(b"0000");
        request.extend(pkt_line_bytes(b"done\n"));

        let zmin = http_backend_failure_with_body(
            zmin_bin(),
            dir.path(),
            "/remote.git/git-upload-pack",
            "",
            "POST",
            &request,
        );
        let git = http_backend_failure_with_body(
            "git",
            dir.path(),
            "/remote.git/git-upload-pack",
            "",
            "POST",
            &request,
        );
        assert_eq!(zmin.0, git.0, "exit code for filter {filter}");
        assert_eq!(zmin.2, git.2, "stderr for filter {filter}");
        assert!(
            !zmin.2.contains("not supported yet"),
            "filter {filter} should not report an implementation gap: {}",
            zmin.2
        );
    }
}

#[test]
fn http_backend_upload_pack_filter_blob_limit_omits_large_blobs() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("small.txt"), b"small\n").expect("write small");
    fs::write(
        work.join("large.txt"),
        b"this blob is larger than the limit\n",
    )
    .expect("write large");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);
    let head = git(&work, ["rev-parse", "HEAD"]);
    let small = git(&work, ["rev-parse", "HEAD:small.txt"]);
    let large = git(&work, ["rev-parse", "HEAD:large.txt"]);

    let mut request = Vec::new();
    request.extend(pkt_line_bytes(
        format!("want {head} side-band-64k thin-pack ofs-delta no-progress filter\n").as_bytes(),
    ));
    request.extend(pkt_line_bytes(b"filter blob:limit=10\n"));
    request.extend_from_slice(b"0000");
    request.extend(pkt_line_bytes(b"done\n"));

    let response = http_backend_response_with_body(
        zmin_bin(),
        dir.path(),
        "/remote.git/git-upload-pack",
        "",
        "POST",
        &request,
    );
    let pack = sideband_pack_from_http_response(&response);
    let verify = git_init();
    let pack_id = git_with_stdin_bytes(verify.path(), ["index-pack", "--stdin"], &pack)
        .strip_prefix("pack\t")
        .expect("index-pack output pack id")
        .to_owned();
    let idx = verify
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{pack_id}.idx"));
    let verify_pack = git_args(
        verify.path(),
        &["verify-pack", "-v", idx.to_str().expect("idx path")],
    );
    assert!(
        verify_pack.lines().any(|line| line.starts_with(&small)),
        "blob:limit pack should keep small blob:\n{verify_pack}"
    );
    assert!(
        !verify_pack.lines().any(|line| line.starts_with(&large)),
        "blob:limit pack should omit large blob:\n{verify_pack}"
    );
}

#[test]
fn http_backend_upload_pack_filter_object_type_blob_omits_trees() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::create_dir_all(work.join("dir")).expect("create dir");
    fs::write(work.join("root.txt"), b"root\n").expect("write root");
    fs::write(work.join("dir/child.txt"), b"child\n").expect("write child");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);
    let head = git(&work, ["rev-parse", "HEAD"]);
    let root_blob = git(&work, ["rev-parse", "HEAD:root.txt"]);
    let child_blob = git(&work, ["rev-parse", "HEAD:dir/child.txt"]);

    let mut request = Vec::new();
    request.extend(pkt_line_bytes(
        format!("want {head} side-band-64k thin-pack ofs-delta no-progress filter\n").as_bytes(),
    ));
    request.extend(pkt_line_bytes(b"filter object:type=blob\n"));
    request.extend_from_slice(b"0000");
    request.extend(pkt_line_bytes(b"done\n"));

    let response = http_backend_response_with_body(
        zmin_bin(),
        dir.path(),
        "/remote.git/git-upload-pack",
        "",
        "POST",
        &request,
    );
    let pack = sideband_pack_from_http_response(&response);
    let verify = git_init();
    let pack_id = git_with_stdin_bytes(verify.path(), ["index-pack", "--stdin"], &pack)
        .strip_prefix("pack\t")
        .expect("index-pack output pack id")
        .to_owned();
    let idx = verify
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{pack_id}.idx"));
    let verify_pack = git_args(
        verify.path(),
        &["verify-pack", "-v", idx.to_str().expect("idx path")],
    );
    assert!(
        verify_pack.lines().any(|line| line.starts_with(&root_blob)),
        "object:type=blob pack should include root blob:\n{verify_pack}"
    );
    assert!(
        verify_pack
            .lines()
            .any(|line| line.starts_with(&child_blob)),
        "object:type=blob pack should include nested blob:\n{verify_pack}"
    );
    assert!(
        !verify_pack.lines().any(|line| line.contains(" tree ")),
        "object:type=blob pack should omit trees:\n{verify_pack}"
    );
}

#[test]
fn http_backend_upload_pack_filter_tree_depth_limits_tree_walk() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::create_dir_all(work.join("dir/sub")).expect("create dirs");
    fs::write(work.join("root.txt"), b"root\n").expect("write root");
    fs::write(work.join("dir/child.txt"), b"child\n").expect("write child");
    fs::write(work.join("dir/sub/deep.txt"), b"deep\n").expect("write deep");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);
    let head = git(&work, ["rev-parse", "HEAD"]);
    let root_blob = git(&work, ["rev-parse", "HEAD:root.txt"]);
    let child_blob = git(&work, ["rev-parse", "HEAD:dir/child.txt"]);
    let dir_tree = git(&work, ["rev-parse", "HEAD:dir"]);
    let sub_tree = git(&work, ["rev-parse", "HEAD:dir/sub"]);

    let mut request = Vec::new();
    request.extend(pkt_line_bytes(
        format!("want {head} side-band-64k thin-pack ofs-delta no-progress filter\n").as_bytes(),
    ));
    request.extend(pkt_line_bytes(b"filter tree:2\n"));
    request.extend_from_slice(b"0000");
    request.extend(pkt_line_bytes(b"done\n"));

    let response = http_backend_response_with_body(
        zmin_bin(),
        dir.path(),
        "/remote.git/git-upload-pack",
        "",
        "POST",
        &request,
    );
    let pack = sideband_pack_from_http_response(&response);
    let verify = git_init();
    let pack_id = git_with_stdin_bytes(verify.path(), ["index-pack", "--stdin"], &pack)
        .strip_prefix("pack\t")
        .expect("index-pack output pack id")
        .to_owned();
    let idx = verify
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{pack_id}.idx"));
    let verify_pack = git_args(
        verify.path(),
        &["verify-pack", "-v", idx.to_str().expect("idx path")],
    );
    assert!(
        verify_pack.lines().any(|line| line.starts_with(&root_blob)),
        "tree:2 pack should include root-level blob:\n{verify_pack}"
    );
    assert!(
        verify_pack.lines().any(|line| line.starts_with(&dir_tree)),
        "tree:2 pack should include first-level tree:\n{verify_pack}"
    );
    assert!(
        !verify_pack
            .lines()
            .any(|line| line.starts_with(&child_blob)),
        "tree:2 pack should omit second-level blob:\n{verify_pack}"
    );
    assert!(
        !verify_pack.lines().any(|line| line.starts_with(&sub_tree)),
        "tree:2 pack should omit second-level tree:\n{verify_pack}"
    );
}

#[test]
fn http_backend_upload_pack_filter_combine_applies_all_filters() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::create_dir_all(work.join("dir")).expect("create dir");
    fs::write(work.join("root.txt"), b"root\n").expect("write root");
    fs::write(work.join("dir/child.txt"), b"child\n").expect("write child");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);
    let head = git(&work, ["rev-parse", "HEAD"]);
    let root_blob = git(&work, ["rev-parse", "HEAD:root.txt"]);
    let child_blob = git(&work, ["rev-parse", "HEAD:dir/child.txt"]);

    let mut request = Vec::new();
    request.extend(pkt_line_bytes(
        format!("want {head} side-band-64k thin-pack ofs-delta no-progress filter\n").as_bytes(),
    ));
    request.extend(pkt_line_bytes(
        b"filter combine:object%3Atype%3Dblob+tree%3A2\n",
    ));
    request.extend_from_slice(b"0000");
    request.extend(pkt_line_bytes(b"done\n"));

    let response = http_backend_response_with_body(
        zmin_bin(),
        dir.path(),
        "/remote.git/git-upload-pack",
        "",
        "POST",
        &request,
    );
    let pack = sideband_pack_from_http_response(&response);
    let verify = git_init();
    let pack_id = git_with_stdin_bytes(verify.path(), ["index-pack", "--stdin"], &pack)
        .strip_prefix("pack\t")
        .expect("index-pack output pack id")
        .to_owned();
    let idx = verify
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{pack_id}.idx"));
    let verify_pack = git_args(
        verify.path(),
        &["verify-pack", "-v", idx.to_str().expect("idx path")],
    );
    assert!(
        verify_pack.lines().any(|line| line.starts_with(&root_blob)),
        "combined filter should include root-level blob:\n{verify_pack}"
    );
    assert!(
        !verify_pack
            .lines()
            .any(|line| line.starts_with(&child_blob)),
        "combined filter should omit nested blob:\n{verify_pack}"
    );
    assert!(
        !verify_pack.lines().any(|line| line.contains(" tree ")),
        "combined filter should omit trees through object:type=blob:\n{verify_pack}"
    );
}

#[test]
fn http_backend_upload_pack_filter_sparse_oid_omits_unmatched_blobs() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::create_dir_all(work.join("keep")).expect("create keep");
    fs::create_dir_all(work.join("drop")).expect("create drop");
    fs::write(work.join("root.txt"), b"root\n").expect("write root");
    fs::write(work.join("keep/a.txt"), b"keep\n").expect("write keep");
    fs::write(work.join("drop/b.txt"), b"drop\n").expect("write drop");
    fs::write(work.join("sparse-spec"), b"/keep/\nroot.txt\n").expect("write sparse spec");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);
    let head = git(&work, ["rev-parse", "HEAD"]);
    let spec = git(&work, ["rev-parse", "HEAD:sparse-spec"]);
    let root_blob = git(&work, ["rev-parse", "HEAD:root.txt"]);
    let keep_blob = git(&work, ["rev-parse", "HEAD:keep/a.txt"]);
    let drop_blob = git(&work, ["rev-parse", "HEAD:drop/b.txt"]);

    let mut request = Vec::new();
    request.extend(pkt_line_bytes(
        format!("want {head} side-band-64k thin-pack ofs-delta no-progress filter\n").as_bytes(),
    ));
    request.extend(pkt_line_bytes(
        format!("filter sparse:oid={spec}\n").as_bytes(),
    ));
    request.extend_from_slice(b"0000");
    request.extend(pkt_line_bytes(b"done\n"));

    let response = http_backend_response_with_body(
        zmin_bin(),
        dir.path(),
        "/remote.git/git-upload-pack",
        "",
        "POST",
        &request,
    );
    let pack = sideband_pack_from_http_response(&response);
    let verify = git_init();
    let pack_id = git_with_stdin_bytes(verify.path(), ["index-pack", "--stdin"], &pack)
        .strip_prefix("pack\t")
        .expect("index-pack output pack id")
        .to_owned();
    let idx = verify
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{pack_id}.idx"));
    let verify_pack = git_args(
        verify.path(),
        &["verify-pack", "-v", idx.to_str().expect("idx path")],
    );
    assert!(
        verify_pack.lines().any(|line| line.starts_with(&root_blob)),
        "sparse:oid pack should include root matched blob:\n{verify_pack}"
    );
    assert!(
        verify_pack.lines().any(|line| line.starts_with(&keep_blob)),
        "sparse:oid pack should include directory matched blob:\n{verify_pack}"
    );
    assert!(
        !verify_pack.lines().any(|line| line.starts_with(&drop_blob)),
        "sparse:oid pack should omit unmatched blob:\n{verify_pack}"
    );
}

#[test]
fn http_fetch_fetches_dumb_http_objects_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::create_dir_all(source.join("dir")).expect("create dir");
    fs::write(source.join("dir/a.txt"), b"hello\n").expect("write a");
    fs::write(source.join("root.txt"), b"root\n").expect("write root");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    fs::write(source.join("dir/a.txt"), b"hello again\n").expect("rewrite a");
    fs::write(source.join("second.txt"), b"second\n").expect("write second");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "second"]);
    git(&source, ["update-server-info"]);
    let head = git(&source, ["rev-parse", "HEAD"]);
    let server = StaticHttpServer::new(source.clone());
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "zmin-client"]);

    git(
        &git_client,
        [
            "http-fetch",
            "-a",
            "-w",
            "refs/heads/main",
            head.as_str(),
            url.as_str(),
        ],
    );
    run_zmin(
        &zmin_client,
        [
            "http-fetch",
            "-a",
            "-w",
            "refs/heads/main",
            head.as_str(),
            url.as_str(),
        ],
    );
    assert_eq!(
        git(&zmin_client, ["rev-parse", "refs/heads/main"]),
        git(&git_client, ["rev-parse", "refs/heads/main"])
    );
    assert_eq!(
        git(&zmin_client, ["log", "--format=%s", "refs/heads/main"]),
        git(&git_client, ["log", "--format=%s", "refs/heads/main"])
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", &format!("{head}:dir/a.txt")]
        ),
        git(
            &git_client,
            ["cat-file", "-p", &format!("{head}:dir/a.txt")]
        )
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", &format!("{head}:root.txt")]
        ),
        git(&git_client, ["cat-file", "-p", &format!("{head}:root.txt")])
    );

    let git_stdin_client = dir.path().join("git-stdin-client");
    let zmin_stdin_client = dir.path().join("zmin-stdin-client");
    git(dir.path(), ["init", "git-stdin-client"]);
    git(dir.path(), ["init", "zmin-stdin-client"]);
    let stdin = format!("{head}\n");
    assert_eq!(
        run_zmin_with_stdin_args(
            &zmin_stdin_client,
            &["http-fetch", "--stdin", url.as_str()],
            &stdin,
        ),
        git_with_stdin_args(
            &git_stdin_client,
            &["http-fetch", "--stdin", url.as_str()],
            &stdin,
        )
    );
    assert_eq!(
        git(
            &zmin_stdin_client,
            ["cat-file", "-p", &format!("{head}:second.txt")]
        ),
        git(
            &git_stdin_client,
            ["cat-file", "-p", &format!("{head}:second.txt")]
        )
    );
}

#[test]
fn ls_remote_sends_basic_auth_from_url_userinfo() {
    let dir = TempDir::new().expect("temp dir");
    let server = AuthorizationCaptureHttpServer::new();
    let url = format!("http://user:p%40ss@127.0.0.1:{}/repo.git", server.port);

    let (_code, _stdout, _stderr) =
        run_zmin_failure_output(dir.path(), &["ls-remote", url.as_str()]);

    let request = server.request_text();
    assert!(
        request.contains("Authorization: Basic dXNlcjpwQHNz\r\n"),
        "request did not include decoded URL userinfo auth header:\n{request}"
    );
    assert!(
        request.starts_with("GET /repo.git/info/refs?service=git-upload-pack "),
        "request path should not include URL userinfo:\n{request}"
    );
}

#[test]
fn ls_remote_sends_basic_auth_from_credential_store_helper() {
    let dir = TempDir::new().expect("temp dir");
    git(dir.path(), ["init", "client"]);
    let client = dir.path().join("client");
    let credentials = dir.path().join("credentials");
    let server = AuthorizationCaptureHttpServer::new();
    let url = format!("http://127.0.0.1:{}/repo.git", server.port);
    std::fs::write(
        &credentials,
        format!("http://user:p%40ss@127.0.0.1:{}\n", server.port),
    )
    .expect("credentials");
    git(
        &client,
        [
            "config",
            "credential.helper",
            &format!("store --file {}", credentials.display()),
        ],
    );

    let (_code, _stdout, _stderr) = run_zmin_failure_output(&client, &["ls-remote", url.as_str()]);

    let request = server.request_text();
    assert!(
        request.contains("Authorization: Basic dXNlcjpwQHNz\r\n"),
        "request did not include credential-store auth header:\n{request}"
    );
}

#[test]
fn ls_remote_sends_basic_auth_from_credential_store_helper_with_quoted_file_path() {
    let dir = TempDir::new().expect("temp dir");
    git(dir.path(), ["init", "client"]);
    let client = dir.path().join("client");
    let credentials_dir = dir.path().join("folder with spaces");
    std::fs::create_dir_all(&credentials_dir).expect("credentials dir");
    let credentials = credentials_dir.join("quoted credentials");
    let server = AuthorizationCaptureHttpServer::new();
    let url = format!("http://127.0.0.1:{}/repo.git", server.port);
    std::fs::write(
        &credentials,
        format!("http://user:p%40ss@127.0.0.1:{}\n", server.port),
    )
    .expect("credentials");
    git(
        &client,
        [
            "config",
            "credential.helper",
            &format!("store --file '{}'", credentials.display()),
        ],
    );

    let (_code, _stdout, _stderr) = run_zmin_failure_output(&client, &["ls-remote", url.as_str()]);

    let request = server.request_text();
    assert!(
        request.contains("Authorization: Basic dXNlcjpwQHNz\r\n"),
        "request did not include credential-store auth header from quoted path:\n{request}"
    );
}

#[test]
fn ls_remote_follows_http_redirect_to_location() {
    let dir = TempDir::new().expect("temp dir");
    let target = AuthorizationCaptureHttpServer::new();
    let redirect = OneShotRedirectHttpServer::new(format!("http://127.0.0.1:{}", target.port));
    let url = format!("http://127.0.0.1:{}/repo.git", redirect.port);

    let (_code, _stdout, _stderr) =
        run_zmin_failure_output(dir.path(), &["ls-remote", url.as_str()]);

    let request = target.request_text();
    assert!(
        request.starts_with("GET /repo.git/info/refs?service=git-upload-pack "),
        "redirect target did not receive smart discovery request:\n{request}"
    );
}

#[test]
fn ls_remote_strips_authorization_on_cross_origin_redirect() {
    let dir = TempDir::new().expect("temp dir");
    let target = AuthorizationCaptureHttpServer::new();
    let redirect = OneShotRedirectHttpServer::new(format!("http://127.0.0.1:{}", target.port));
    let url = format!("http://user:pass@127.0.0.1:{}/repo.git", redirect.port);

    let (_code, _stdout, _stderr) =
        run_zmin_failure_output(dir.path(), &["ls-remote", url.as_str()]);

    let request = target.request_text();
    assert!(
        request.starts_with("GET /repo.git/info/refs?service=git-upload-pack "),
        "redirect target did not receive smart discovery request:\n{request}"
    );
    assert!(
        !request.contains("\r\nAuthorization:"),
        "cross-origin redirect leaked Authorization header:\n{request}"
    );
}

#[test]
fn ls_remote_strips_configured_authorization_header_on_cross_origin_redirect() {
    let dir = TempDir::new().expect("temp dir");
    git(dir.path(), ["init", "client"]);
    let client = dir.path().join("client");
    git(
        &client,
        [
            "config",
            "--add",
            "http.extraHeader",
            "Authorization: Bearer scoped",
        ],
    );
    git(
        &client,
        ["config", "--add", "http.extraHeader", "X-Zmin-Trace: keep"],
    );
    let target = AuthorizationCaptureHttpServer::new();
    let redirect = OneShotRedirectHttpServer::new(format!("http://127.0.0.1:{}", target.port));
    let url = format!("http://127.0.0.1:{}/repo.git", redirect.port);

    let (_code, _stdout, _stderr) = run_zmin_failure_output(&client, &["ls-remote", url.as_str()]);

    let request = target.request_text();
    assert!(
        request.starts_with("GET /repo.git/info/refs?service=git-upload-pack "),
        "redirect target did not receive smart discovery request:\n{request}"
    );
    assert!(
        !request.contains("\r\nAuthorization:"),
        "cross-origin redirect leaked configured Authorization header:\n{request}"
    );
    assert!(
        request.contains("X-Zmin-Trace: keep\r\n"),
        "cross-origin redirect dropped non-credential extra header:\n{request}"
    );
}

#[test]
fn ls_remote_sends_configured_http_extra_header() {
    let dir = TempDir::new().expect("temp dir");
    git(dir.path(), ["init", "client"]);
    let client = dir.path().join("client");
    git(
        &client,
        ["config", "--add", "http.extraHeader", "X-Zmin-Token: local"],
    );
    let server = AuthorizationCaptureHttpServer::new();
    let url = format!("http://127.0.0.1:{}/repo.git", server.port);

    let (_code, _stdout, _stderr) = run_zmin_failure_output(&client, &["ls-remote", url.as_str()]);

    let request = server.request_text();
    assert!(
        request.contains("X-Zmin-Token: local\r\n"),
        "request did not include http.extraHeader value:\n{request}"
    );
}

#[test]
fn fetch_sends_configured_http_extra_header() {
    let dir = TempDir::new().expect("temp dir");
    git(dir.path(), ["init", "client"]);
    let client = dir.path().join("client");
    let server = AuthorizationCaptureHttpServer::new();
    let url = format!("http://127.0.0.1:{}/repo.git", server.port);
    git(&client, ["remote", "add", "origin", url.as_str()]);
    git(
        &client,
        ["config", "--add", "http.extraHeader", "X-Zmin-Token: fetch"],
    );

    let (_code, _stdout, _stderr) = run_zmin_failure_output(&client, &["fetch", "origin"]);

    let request = server.request_text();
    assert!(
        request.contains("X-Zmin-Token: fetch\r\n"),
        "fetch request did not include http.extraHeader value:\n{request}"
    );
}

#[test]
fn fetch_sends_configured_http_user_agent() {
    let dir = TempDir::new().expect("temp dir");
    git(dir.path(), ["init", "client"]);
    let client = dir.path().join("client");
    let server = AuthorizationCaptureHttpServer::new();
    let url = format!("http://127.0.0.1:{}/repo.git", server.port);
    git(&client, ["remote", "add", "origin", url.as_str()]);
    git(&client, ["config", "http.userAgent", "zmin-test/1"]);

    let (_code, _stdout, _stderr) = run_zmin_failure_output(&client, &["fetch", "origin"]);

    let request = server.request_text();
    assert!(
        request.contains("User-Agent: zmin-test/1\r\n"),
        "fetch request did not include http.userAgent value:\n{request}"
    );
}

#[test]
fn ls_remote_sends_git_http_user_agent_env_without_repo() {
    let dir = TempDir::new().expect("temp dir");
    let server = AuthorizationCaptureHttpServer::new();
    let url = format!("http://127.0.0.1:{}/repo.git", server.port);

    let (_code, _stdout, _stderr) = command_failure_output_with_env(
        zmin_bin(),
        dir.path(),
        &["ls-remote", url.as_str()],
        &[("GIT_HTTP_USER_AGENT", "zmin-env/1")],
        "zmin ls-remote user agent",
    );

    let request = server.request_text();
    assert!(
        request.contains("User-Agent: zmin-env/1\r\n"),
        "request did not include GIT_HTTP_USER_AGENT value:\n{request}"
    );
}

#[test]
fn http_fetch_packfile_downloads_and_indexes_pack() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let zmin_client = dir.path().join("zmin-client");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"hello\n").expect("write a");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    fs::write(source.join("b.txt"), b"second\n").expect("write b");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "second"]);
    git(&source, ["repack", "-ad"]);
    git(&source, ["update-server-info"]);

    let pack_dir = source.join(".git/objects/pack");
    let pack_path = fs::read_dir(&pack_dir)
        .expect("read source pack dir")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("pack"))
        .expect("source pack");
    let pack_name = pack_path
        .file_stem()
        .and_then(|name| name.to_str())
        .expect("pack file stem");
    let pack_hash = pack_name
        .strip_prefix("pack-")
        .expect("pack hash from file name");
    let head = git(&source, ["rev-parse", "HEAD"]);
    let server = StaticHttpServer::new(source.clone());
    let url = format!(
        "http://127.0.0.1:{}/.git/objects/pack/pack-{pack_hash}.pack",
        server.port
    );
    let git_client = dir.path().join("git-client");
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "zmin-client"]);

    let pack_args = [
        "http-fetch",
        &format!("--packfile={pack_hash}"),
        "--index-pack-arg=index-pack",
        "--index-pack-arg=--stdin",
        "--index-pack-arg=--keep",
        url.as_str(),
    ];
    let git_pack_output = command_output("git", &git_client, &pack_args, "git");
    let zmin_pack_output = command_output(zmin_bin(), &zmin_client, &pack_args, "zmin");
    assert_eq!(zmin_pack_output.0, git_pack_output.0);
    assert_eq!(zmin_pack_output.1, git_pack_output.1);
    assert_eq!(zmin_pack_output.1, format!("keep\t{pack_hash}"));
    assert_eq!(
        run_zmin(&zmin_client, ["cat-file", "-p", &format!("{head}:b.txt")]),
        git(&git_client, ["cat-file", "-p", &format!("{head}:b.txt")])
    );
    assert!(
        zmin_client
            .join(format!(".git/objects/pack/pack-{pack_hash}.pack"))
            .exists()
    );
    assert!(
        zmin_client
            .join(format!(".git/objects/pack/pack-{pack_hash}.idx"))
            .exists()
    );
    assert!(
        zmin_client
            .join(format!(".git/objects/pack/pack-{pack_hash}.rev"))
            .exists()
    );
    assert!(
        zmin_client
            .join(format!(".git/objects/pack/pack-{pack_hash}.keep"))
            .exists()
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", &format!("{head}:b.txt")]),
        "second"
    );

    let zmin_no_rev_client = dir.path().join("zmin-no-rev-client");
    git(dir.path(), ["init", "zmin-no-rev-client"]);
    assert_eq!(
        run_zmin(
            &zmin_no_rev_client,
            [
                "http-fetch",
                &format!("--packfile={pack_hash}"),
                "--index-pack-arg=index-pack",
                "--index-pack-arg=--stdin",
                "--index-pack-arg=--keep=manual-keep",
                "--index-pack-arg=--no-rev-index",
                url.as_str(),
            ],
        ),
        format!("keep\t{pack_hash}")
    );
    assert!(
        !zmin_no_rev_client
            .join(format!(".git/objects/pack/pack-{pack_hash}.rev"))
            .exists()
    );
    assert_eq!(
        fs::read_to_string(
            zmin_no_rev_client.join(format!(".git/objects/pack/pack-{pack_hash}.keep"))
        )
        .expect("read keep"),
        "manual-keep\n"
    );
}

#[test]
fn http_fetch_packfile_accepts_arbitrary_requested_seed_sha1_sha256() {
    for &(sha256, hash_label) in &[(false, "sha1"), (true, "sha256")] {
        let dir = TempDir::new().expect("arbitrary packfile seed temp dir");
        let source = dir.path().join("source");
        let mut init_args = vec!["init".to_owned()];
        if sha256 {
            init_args.push("--object-format=sha256".to_owned());
        }
        init_args.push(source.display().to_string());
        let init_args = init_args.iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(
            pinned_command_any_output(dir.path(), &init_args, "arbitrary seed init").0,
            0
        );
        configure_identity(&source);
        fs::write(source.join("a.txt"), b"arbitrary seed\n").expect("write source file");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", "initial"]);
        pinned_git_args(&source, ["repack", "-ad"]);

        let pack_dir = source.join(".git/objects/pack");
        let pack_path = fs::read_dir(&pack_dir)
            .expect("read arbitrary seed source pack directory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("pack"))
            .expect("arbitrary seed source pack");
        let pack_hash = pack_path
            .file_stem()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix("pack-"))
            .expect("arbitrary seed source pack hash");
        let requested_seed = "1".repeat(if sha256 { 64 } else { 40 });
        assert_ne!(
            requested_seed, pack_hash,
            "seed must differ for {hash_label}"
        );
        let server = StaticHttpServer::new(source);
        let url = format!(
            "http://127.0.0.1:{}/.git/objects/pack/pack-{pack_hash}.pack",
            server.port
        );

        let zmin_client = dir.path().join("zmin-client");
        let mut client_init_args = vec!["init".to_owned()];
        if sha256 {
            client_init_args.push("--object-format=sha256".to_owned());
        }
        client_init_args.push(zmin_client.display().to_string());
        let client_init_args = client_init_args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            pinned_command_any_output(dir.path(), &client_init_args, "arbitrary seed client init")
                .0,
            0
        );

        let packfile_arg = format!("--packfile={requested_seed}");
        let args = [
            "http-fetch",
            packfile_arg.as_str(),
            "--index-pack-arg=index-pack",
            "--index-pack-arg=--stdin",
            url.as_str(),
        ];
        let output = command_any_output(zmin_bin(), &zmin_client, &args, "Zmin arbitrary seed");
        assert_eq!(
            output.0, 0,
            "arbitrary seed should succeed for {hash_label}"
        );
        assert_eq!(output.1, format!("pack\t{pack_hash}"));
        assert!(
            output.2.is_empty(),
            "unexpected stderr for {hash_label}: {:?}",
            output.2
        );
        assert!(
            zmin_client
                .join(format!(".git/objects/pack/pack-{pack_hash}.pack"))
                .exists()
        );
        assert!(
            !zmin_client
                .join(format!(".git/objects/pack/pack-{requested_seed}.pack"))
                .exists(),
            "requested seed must not become the installed pack ID for {hash_label}"
        );
    }
}

#[test]
fn http_fetch_packfile_fix_thin_accepts_changed_final_id_sha1_sha256() {
    for &(sha256, hash_label) in &[(false, "sha1"), (true, "sha256")] {
        let dir = TempDir::new().expect("thin arbitrary packfile seed temp dir");
        let source = dir.path().join("source");
        let mut init_args = vec!["init".to_owned()];
        if sha256 {
            init_args.push("--object-format=sha256".to_owned());
        }
        init_args.push(source.display().to_string());
        let init_args = init_args.iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(
            pinned_command_any_output(dir.path(), &init_args, "thin arbitrary seed init").0,
            0
        );
        configure_identity(&source);
        fs::write(
            source.join("delta.txt"),
            format!("{}\nbase\n", "shared line\n".repeat(4_000)),
        )
        .expect("write thin base");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", "base"]);
        let base = pinned_git_args(&source, ["rev-parse", "HEAD"]);
        fs::write(
            source.join("delta.txt"),
            format!("{}\nchanged\n", "shared line\n".repeat(4_000)),
        )
        .expect("write thin replacement");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", "changed"]);
        let pack_input = format!("HEAD\n^{base}\n");
        let pinned_git = required_pinned_stock_git();
        let pinned_git = pinned_git.to_str().expect("pinned Git path");
        let thin_pack = common::command_stdout_bytes_with_stdin(
            pinned_git,
            &source,
            &[
                "pack-objects",
                "--stdout",
                "--thin",
                "--window=50",
                "--depth=50",
                "--revs",
            ],
            pack_input.as_bytes(),
        );
        let algorithm = if sha256 {
            GitHashAlgorithm::Sha256
        } else {
            GitHashAlgorithm::Sha1
        };
        let digest_len = algorithm.digest_len();
        let thin_pack_id = ObjectId::new(algorithm, &thin_pack[thin_pack.len() - digest_len..]);
        fs::write(dir.path().join("thin.pack"), &thin_pack).expect("write thin HTTP pack");

        let git_client = dir.path().join("git-client");
        let zmin_client = dir.path().join("zmin-client");
        for client in [&git_client, &zmin_client] {
            let mut client_init_args = vec!["init".to_owned()];
            if sha256 {
                client_init_args.push("--object-format=sha256".to_owned());
            }
            client_init_args.push(client.display().to_string());
            let client_init_args = client_init_args
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            assert_eq!(
                pinned_command_any_output(
                    dir.path(),
                    &client_init_args,
                    "thin arbitrary seed client init",
                )
                .0,
                0
            );
            let source_path = source.display().to_string();
            let fetch_ref = format!("{base}:refs/heads/base");
            pinned_git_args(client, ["fetch", source_path.as_str(), fetch_ref.as_str()]);
        }
        let stock_output = common::command_stdout_bytes_with_stdin(
            pinned_git,
            &git_client,
            &["index-pack", "--stdin", "--fix-thin"],
            &thin_pack,
        );
        let stock_output = String::from_utf8(stock_output)
            .expect("pinned thin index output")
            .trim()
            .to_owned();
        assert!(
            stock_output.starts_with("pack\t"),
            "pinned thin index output"
        );
        let requested_seed = "2".repeat(if sha256 { 64 } else { 40 });

        let server = StaticHttpServer::new(dir.path().to_path_buf());
        let url = format!("http://127.0.0.1:{}/thin.pack", server.port);
        let packfile_arg = format!("--packfile={requested_seed}");
        let args = [
            "http-fetch",
            packfile_arg.as_str(),
            "--index-pack-arg=index-pack",
            "--index-pack-arg=--stdin",
            "--index-pack-arg=--fix-thin",
            url.as_str(),
        ];
        let output =
            command_any_output(zmin_bin(), &zmin_client, &args, "Zmin arbitrary thin seed");
        assert_eq!(
            output.0, 0,
            "thin arbitrary seed should succeed for {hash_label}"
        );
        assert!(
            output.2.is_empty(),
            "unexpected thin stderr for {hash_label}: {:?}",
            output.2
        );
        let final_pack_id = output
            .1
            .strip_prefix("pack\t")
            .expect("Zmin thin index pack output");
        assert_ne!(
            requested_seed, final_pack_id,
            "seed must not be the repaired pack ID for {hash_label}"
        );
        assert_ne!(
            thin_pack_id.to_hex(),
            final_pack_id,
            "repair must change final ID for {hash_label}"
        );
        assert!(
            zmin_client
                .join(format!(".git/objects/pack/pack-{final_pack_id}.pack"))
                .exists()
        );
        assert!(
            !zmin_client
                .join(format!(".git/objects/pack/pack-{requested_seed}.pack"))
                .exists(),
            "requested seed must not become the repaired pack ID for {hash_label}"
        );
    }
}

#[test]
fn http_fetch_packfile_strict_semantic_failure_is_atomic_sha1_sha256() {
    for &(sha256, hash_label) in &[(false, "sha1"), (true, "sha256")] {
        let dir = TempDir::new().expect("direct semantic corruption temp dir");
        let remote = prepare_semantic_corruption_remote(dir.path(), sha256);
        let algorithm = if sha256 {
            GitHashAlgorithm::Sha256
        } else {
            GitHashAlgorithm::Sha1
        };
        let pack_dir = remote.join("objects/pack");
        pinned_git_args(&remote, ["repack", "-ad"]);
        let source_pack = fs::read_dir(&pack_dir)
            .expect("read direct semantic source pack directory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("pack"))
            .expect("direct semantic source pack");
        let corrupted_pack = corrupt_undeltified_commit_pack(
            &fs::read(&source_pack).expect("read direct semantic source pack"),
            algorithm,
        );
        let pack_id = ObjectId::new(
            algorithm,
            &corrupted_pack[corrupted_pack.len() - algorithm.digest_len()..],
        );
        let corrupted_path = pack_dir.join(format!("pack-{}.pack", pack_id.to_hex()));
        fs::write(&corrupted_path, corrupted_pack).expect("write direct semantic pack");

        let zmin_client = dir.path().join("zmin-client");
        let mut zmin_init = vec!["init".to_owned(), "--bare".to_owned()];
        if sha256 {
            zmin_init.push("--object-format=sha256".to_owned());
        }
        zmin_init.push(zmin_client.display().to_string());
        let zmin_init_refs = zmin_init.iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(
            command_any_output(
                zmin_bin(),
                dir.path(),
                &zmin_init_refs,
                "Zmin direct semantic init",
            )
            .0,
            0
        );
        let server = StaticHttpServer::new(remote.clone());
        let url = format!(
            "http://127.0.0.1:{}/objects/pack/pack-{}.pack",
            server.port,
            pack_id.to_hex()
        );
        let args = [
            "http-fetch",
            &format!("--packfile={}", pack_id.to_hex()),
            "--index-pack-arg=index-pack",
            "--index-pack-arg=--stdin",
            "--index-pack-arg=--strict",
            url.as_str(),
        ];
        let zmin_output =
            command_any_output(zmin_bin(), &zmin_client, &args, "Zmin direct semantic");
        assert_eq!(
            zmin_output.0, 128,
            "Zmin direct semantic failure {hash_label}"
        );
        let expected_id = expected_semantic_corrupt_commit_id(&remote, algorithm);
        assert!(
            zmin_output
                .2
                .contains(&format!("error: bogus commit object {expected_id}")),
            "Zmin direct semantic diagnostic {hash_label}: {zmin_output:?}"
        );
        let zmin_pack_dir = zmin_client.join("objects/pack");
        let zmin_entries = fs::read_dir(&zmin_pack_dir)
            .expect("read Zmin direct semantic pack directory")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            zmin_entries.is_empty(),
            "Zmin direct semantic failure must not publish pack roles {hash_label}: {zmin_entries:?}"
        );
    }
}

#[test]
fn http_fetch_packfile_requires_index_pack_args_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "zmin-client"]);
    let hash = "1111111111111111111111111111111111111111";
    let url = "http://127.0.0.1/repo.git";

    let git_args = ["http-fetch", &format!("--packfile={hash}"), url];
    let zmin_args = ["http-fetch", &format!("--packfile={hash}"), url];
    assert_eq!(
        command_failure_output("git", &git_client, &git_args, "git"),
        run_zmin_failure_output(&zmin_client, &zmin_args)
    );
}

#[test]
fn http_fetch_packfile_rejects_bad_index_pack_arg_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"hello\n").expect("write a");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["repack", "-ad"]);
    git(&source, ["update-server-info"]);

    let pack_dir = source.join(".git/objects/pack");
    let pack_path = fs::read_dir(&pack_dir)
        .expect("read source pack dir")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("pack"))
        .expect("source pack");
    let pack_name = pack_path
        .file_stem()
        .and_then(|name| name.to_str())
        .expect("pack file stem");
    let pack_hash = pack_name
        .strip_prefix("pack-")
        .expect("pack hash from file name");
    let server = StaticHttpServer::new(source);
    let url = format!(
        "http://127.0.0.1:{}/.git/objects/pack/pack-{pack_hash}.pack",
        server.port
    );
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "zmin-client"]);

    let args = [
        "http-fetch",
        &format!("--packfile={pack_hash}"),
        "--index-pack-arg=index-pack",
        "--index-pack-arg=--bad",
        url.as_str(),
    ];
    assert_eq!(
        command_failure_output("git", &git_client, &args, "git"),
        run_zmin_failure_output(&zmin_client, &args)
    );
}

#[test]
fn http_fetch_documented_plural_index_pack_args_matches_stock_git_rejections() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"hello\n").expect("write a");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["repack", "-ad"]);
    git(&source, ["update-server-info"]);

    let pack_dir = source.join(".git/objects/pack");
    let pack_path = fs::read_dir(&pack_dir)
        .expect("read source pack dir")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("pack"))
        .expect("source pack");
    let pack_name = pack_path
        .file_stem()
        .and_then(|name| name.to_str())
        .expect("pack file stem");
    let pack_hash = pack_name
        .strip_prefix("pack-")
        .expect("pack hash from file name");
    let server = StaticHttpServer::new(source);
    let pack_url = format!(
        "http://127.0.0.1:{}/.git/objects/pack/pack-{pack_hash}.pack",
        server.port
    );
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "zmin-client"]);

    let packfile_args = [
        "http-fetch",
        &format!("--packfile={pack_hash}"),
        "--index-pack-args=index-pack --stdin --keep",
        pack_url.as_str(),
    ];
    assert_eq!(
        command_failure_output("git", &git_client, &packfile_args, "git"),
        run_zmin_failure_output(&zmin_client, &packfile_args)
    );

    let usage_args = [
        "http-fetch",
        "--index-pack-args=index-pack --stdin --keep",
        "http://127.0.0.1/repo.git",
    ];
    assert_eq!(
        command_failure_output("git", &git_client, &usage_args, "git"),
        run_zmin_failure_output(&zmin_client, &usage_args)
    );
}

#[test]
fn ls_remote_reads_dumb_http_info_refs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"hello\n").expect("write a");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["branch", "feature"]);
    git_with_env(&source, ["tag", "-a", "v1", "-m", "tag message"]);
    git(&source, ["update-server-info"]);

    let server = StaticHttpServer::new(source);
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    for args in [
        vec!["ls-remote", url.as_str()],
        vec!["ls-remote", "--heads", url.as_str()],
        vec!["ls-remote", "--tags", url.as_str()],
        vec!["ls-remote", "--refs", url.as_str()],
        vec!["ls-remote", url.as_str(), "v*"],
    ] {
        assert_eq!(
            run_zmin_args(dir.path(), &args),
            git_args(dir.path(), &args),
            "args: {args:?}"
        );
    }
}

#[test]
fn ls_remote_option_family_matches_stock_git_for_dumb_http_remote() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"hello\n").expect("write a");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["branch", "feature"]);
    git_with_env(&source, ["tag", "-a", "v1", "-m", "tag message"]);
    git(&source, ["update-server-info"]);

    let server = StaticHttpServer::new(source);
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    for (label, args) in [
        (
            "ls-remote --branches dumb-http",
            vec!["ls-remote", "--branches", url.as_str()],
        ),
        (
            "ls-remote -b dumb-http",
            vec!["ls-remote", "-b", url.as_str()],
        ),
        (
            "ls-remote --quiet dumb-http",
            vec!["ls-remote", "--quiet", url.as_str()],
        ),
        (
            "ls-remote -q dumb-http",
            vec!["ls-remote", "-q", url.as_str()],
        ),
        (
            "ls-remote --get-url dumb-http",
            vec!["ls-remote", "--get-url", url.as_str()],
        ),
        (
            "ls-remote --symref dumb-http",
            vec!["ls-remote", "--symref", url.as_str()],
        ),
        (
            "ls-remote --exit-code match dumb-http",
            vec!["ls-remote", "--exit-code", url.as_str(), "main"],
        ),
        (
            "ls-remote --exit-code miss dumb-http",
            vec!["ls-remote", "--exit-code", url.as_str(), "no-such*"],
        ),
        (
            "ls-remote --server-option=foo dumb-http",
            vec!["ls-remote", "--server-option=foo", url.as_str()],
        ),
        (
            "ls-remote -o foo dumb-http",
            vec!["ls-remote", "-o", "foo", url.as_str()],
        ),
        (
            "ls-remote --sort=refname dumb-http",
            vec!["ls-remote", "--sort=refname", url.as_str()],
        ),
        (
            "ls-remote --sort=-refname dumb-http",
            vec!["ls-remote", "--sort=-refname", url.as_str()],
        ),
        (
            "ls-remote -t dumb-http",
            vec!["ls-remote", "-t", url.as_str()],
        ),
    ] {
        assert_any_ls_remote_output_matches_stock_git(dir.path(), &args, label);
    }
}

#[test]
fn ls_remote_reads_smart_http_info_refs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "tag message"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    for args in [
        vec!["ls-remote", url.as_str()],
        vec!["ls-remote", "--heads", url.as_str()],
        vec!["ls-remote", "--tags", url.as_str()],
        vec!["ls-remote", "--refs", url.as_str()],
        vec!["ls-remote", url.as_str(), "v*"],
    ] {
        assert_eq!(
            run_zmin_args(dir.path(), &args),
            git_args(dir.path(), &args),
            "args: {args:?}"
        );
    }
}

#[test]
fn zmin_git_http_version_controls_real_local_helper_requests() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"hello\n").expect("write source file");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["update-server-info"]);
    let head = git(&source, ["rev-parse", "HEAD"]);

    let server = StaticHttpServer::new(source);
    wait_for_tcp_port(server.port);
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    let mut observed_requests = 0;

    let helper = pinned_http_helper_for_evidence();
    assert!(
        helper.is_file(),
        "remote HTTP helper is missing: {}",
        helper.display()
    );

    for (label, http_version) in [
        ("unset", None),
        ("auto", Some("auto")),
        ("http1", Some("http1")),
        ("http2", Some("http2")),
        ("http3", Some("http3")),
    ] {
        let client = dir.path().join(format!("client-{label}"));
        git(dir.path(), ["init", client.to_str().expect("client path")]);
        let output = run_hermetic_zmin_http_fetch(&client, &head, &url, helper, http_version);
        assert!(
            output.status.success(),
            "{label} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty(), "{label} emitted stderr");
        assert_eq!(
            git(&client, ["rev-parse", "refs/heads/main"]),
            head,
            "{label}"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let requests = loop {
            let requests = server.request_headers_text();
            if requests.len() > observed_requests {
                break requests;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{label} did not produce a bounded HTTP request"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert!(
            requests
                .last()
                .expect("latest HTTP request")
                .starts_with("GET /.git/objects/"),
            "{label} used an unexpected HTTP request: {:?}",
            requests.last()
        );
        observed_requests = requests.len();
    }

    let invalid_client = dir.path().join("client-invalid");
    git(
        dir.path(),
        [
            "init",
            invalid_client.to_str().expect("invalid client path"),
        ],
    );
    let output = run_hermetic_zmin_http_fetch(&invalid_client, &head, &url, helper, Some("h1"));
    assert_eq!(output.status.code(), Some(128));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).expect("invalid-value stderr is UTF-8"),
        "fatal: unsupported ZMIN_GIT_HTTP_VERSION 'h1'; expected auto, http1, http2, or http3\n"
    );
    assert_eq!(
        server.request_headers_text().len(),
        observed_requests,
        "invalid configuration must fail before making an HTTP request"
    );
}

#[cfg(unix)]
#[test]
fn zmin_git_http_version_http2_negotiates_alpn_and_fetches_pack() {
    let dir = TempDir::new().expect("H2 HTTP temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"hello H2\n").expect("write source file");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["repack", "-ad"]);
    let pack = fs::read_dir(source.join(".git/objects/pack"))
        .expect("read source pack directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|extension| extension.to_str()) == Some("pack"))
        .expect("source pack");
    let pack_hash = pack
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.strip_prefix("pack-"))
        .expect("source pack hash")
        .to_owned();

    let client = dir.path().join("client");
    git(dir.path(), ["init", client.to_str().expect("client path")]);
    let server = H2TlsServer::new(dir.path(), &pack);
    let helper = pinned_http_helper_for_evidence();
    assert!(
        helper.is_file(),
        "remote HTTP helper is missing: {}",
        helper.display()
    );
    let url = format!(
        "https://127.0.0.1:{}/objects/pack/pack-{pack_hash}.pack",
        server.port
    );
    let pack_arg = format!("--packfile={pack_hash}");
    let mut command = hermetic_zmin_command(&client, helper, Some("http2"), true);
    command.args([
        "http-fetch",
        pack_arg.as_str(),
        "--index-pack-arg=index-pack",
        "--index-pack-arg=--stdin",
        url.as_str(),
    ]);
    let output = command.output().expect("run hermetic H2 http-fetch");
    assert!(
        output.status.success(),
        "H2 http-fetch failed: stdout={} stderr={} server-marker={} server-stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        fs::read_to_string(&server.marker).unwrap_or_default(),
        server.stderr()
    );
    assert!(output.stderr.is_empty(), "H2 http-fetch emitted stderr");
    assert!(
        String::from_utf8_lossy(&output.stdout).starts_with("pack\t"),
        "H2 http-fetch did not index the fetched pack: {:?}",
        output.stdout
    );
    server.wait_for_h2_request();
}

#[test]
fn ls_remote_option_family_matches_stock_git_for_smart_http_remote() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "tag message"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    for (label, args) in [
        (
            "ls-remote --branches smart-http",
            vec!["ls-remote", "--branches", url.as_str()],
        ),
        (
            "ls-remote -b smart-http",
            vec!["ls-remote", "-b", url.as_str()],
        ),
        (
            "ls-remote --quiet smart-http",
            vec!["ls-remote", "--quiet", url.as_str()],
        ),
        (
            "ls-remote -q smart-http",
            vec!["ls-remote", "-q", url.as_str()],
        ),
        (
            "ls-remote --get-url smart-http",
            vec!["ls-remote", "--get-url", url.as_str()],
        ),
        (
            "ls-remote --symref smart-http",
            vec!["ls-remote", "--symref", url.as_str()],
        ),
        (
            "ls-remote --exit-code match smart-http",
            vec!["ls-remote", "--exit-code", url.as_str(), "main"],
        ),
        (
            "ls-remote --exit-code miss smart-http",
            vec!["ls-remote", "--exit-code", url.as_str(), "no-such*"],
        ),
        (
            "ls-remote --server-option=foo smart-http",
            vec!["ls-remote", "--server-option=foo", url.as_str()],
        ),
        (
            "ls-remote -o foo smart-http",
            vec!["ls-remote", "-o", "foo", url.as_str()],
        ),
        (
            "ls-remote --sort=refname smart-http",
            vec!["ls-remote", "--sort=refname", url.as_str()],
        ),
        (
            "ls-remote --sort=-refname smart-http",
            vec!["ls-remote", "--sort=-refname", url.as_str()],
        ),
        (
            "ls-remote -t smart-http",
            vec!["ls-remote", "-t", url.as_str()],
        ),
    ] {
        assert_any_ls_remote_output_matches_stock_git(dir.path(), &args, label);
    }
}

#[test]
fn ls_remote_accepts_smart_http_service_advertisement_without_newline() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
    let stock_url = format!("http://127.0.0.1:{}/remote.git", stock_server.port);
    let stock = git_args(dir.path(), &["ls-remote", "--refs", &stock_url]);
    drop(stock_server);

    let server = SmartHttpServer::bitbucket_style(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    assert_eq!(run_zmin(dir.path(), ["ls-remote", "--refs", &url]), stock);
}

#[test]
fn http_transport_rejects_truncated_content_length() {
    let dir = TempDir::new().expect("temp dir");
    let server = TruncatedHttpServer::new();
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (code, _stdout, stderr) = run_zmin_failure_output(dir.path(), &["ls-remote", &url]);
    assert_eq!(code, 128);
    assert!(
        stderr.contains("HTTP response ended early"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn http_transport_rejects_conflicting_content_length() {
    let dir = TempDir::new().expect("temp dir");
    let server = ConflictingLengthHttpServer::new();
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let (code, _stdout, stderr) = run_zmin_failure_output(dir.path(), &["ls-remote", &url]);
    assert_eq!(code, 128);
    assert!(
        stderr.contains("conflicting Content-Length"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn http_transport_decodes_chunked_info_refs() {
    let dir = TempDir::new().expect("temp dir");
    let server = ChunkedHttpServer::new();
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let stdout = run_zmin(dir.path(), ["ls-remote", "--refs", &url]);
    assert!(
        stdout.contains("1111111111111111111111111111111111111111\trefs/heads/main"),
        "unexpected stdout: {stdout}"
    );
}

#[test]
fn ls_remote_accepts_non_chunked_transfer_encoding_like_stock_git() {
    let git_dir = TempDir::new().expect("git temp dir");
    let zmin_dir = TempDir::new().expect("zmin temp dir");
    let git_server = NonChunkedTransferEncodingHttpServer::new();
    let zmin_server = NonChunkedTransferEncodingHttpServer::new();
    let git_url = format!("http://127.0.0.1:{}/remote.git", git_server.port);
    let zmin_url = format!("http://127.0.0.1:{}/remote.git", zmin_server.port);

    assert_eq!(
        command_any_output(
            zmin_bin(),
            zmin_dir.path(),
            &["ls-remote", "--refs", &zmin_url],
            "zmin ls-remote transfer encoding",
        ),
        command_any_output(
            "git",
            git_dir.path(),
            &["ls-remote", "--refs", &git_url],
            "git ls-remote transfer encoding",
        )
    );
}

fn prepare_sha256_smart_http_remote(root: &std::path::Path) -> std::path::PathBuf {
    let remote = root.join("remote.git");
    let work = root.join("sha256-work");
    pinned_git_args(
        root,
        ["init", "--bare", "--object-format=sha256", "remote.git"],
    );
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    pinned_git_args(
        root,
        [
            "init",
            "--object-format=sha256",
            "-b",
            "main",
            "sha256-work",
        ],
    );
    pinned_git_args(&work, ["config", "user.name", "Bench"]);
    pinned_git_args(&work, ["config", "user.email", "bench@example.test"]);
    pinned_git_args(&work, ["config", "commit.gpgsign", "false"]);
    fs::write(work.join("sha256.txt"), b"sha256 smart HTTP\n").expect("write sha256 file");
    pinned_git_args(&work, ["add", "-A"]);
    pinned_git_with_env(
        &work,
        ["commit", "-m", "sha256 initial"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
    );
    fs::write(work.join("second.txt"), b"second object\n").expect("write second file");
    for index in 0..128 {
        fs::write(
            work.join(format!("bulk-{index:03}.txt")),
            format!("bulk object {index}\n"),
        )
        .expect("write bulk file");
    }
    pinned_git_args(&work, ["add", "-A"]);
    pinned_git_with_env(
        &work,
        ["commit", "-m", "sha256 second"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
    );
    pinned_git_args(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    pinned_git_args(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main_pinned(&remote);
    remote
}

fn assert_sha256_http_repository_state(
    repository: &std::path::Path,
    expected_head: &str,
    expected_refs: &str,
) {
    assert_eq!(
        pinned_git_args(repository, ["rev-parse", "--show-object-format"]),
        "sha256"
    );
    assert_eq!(
        pinned_git_args(repository, ["rev-parse", "HEAD"]),
        expected_head
    );
    assert_eq!(pinned_git_args(repository, ["show-ref"]), expected_refs);
    assert_eq!(
        fs::read(repository.join("sha256.txt")).expect("read sha256 file"),
        b"sha256 smart HTTP\n"
    );
    assert_eq!(
        fs::read(repository.join("second.txt")).expect("read second file"),
        b"second object\n"
    );
    let (pack_count, loose_count) = sha256_http_storage_shape(repository);
    assert!(
        pack_count > 0 || loose_count > 0,
        "SHA-256 smart HTTP should materialize objects"
    );
}

#[derive(Debug, PartialEq, Eq)]
struct HttpRepositorySnapshot(Vec<(String, Vec<u8>)>);

fn snapshot_http_repository(repository: &std::path::Path) -> HttpRepositorySnapshot {
    fn collect(path: &std::path::Path, root: &std::path::Path, files: &mut Vec<(String, Vec<u8>)>) {
        let metadata = fs::symlink_metadata(path).expect("snapshot metadata");
        let relative = path
            .strip_prefix(root)
            .expect("snapshot relative path")
            .to_string_lossy()
            .into_owned();
        if metadata.is_dir() {
            let mut entries = fs::read_dir(path)
                .expect("snapshot directory")
                .map(|entry| entry.expect("snapshot entry").path())
                .collect::<Vec<_>>();
            entries.sort();
            for entry in entries {
                collect(&entry, root, files);
            }
        } else if metadata.file_type().is_symlink() {
            files.push((
                relative,
                fs::read_link(path)
                    .expect("snapshot link")
                    .to_string_lossy()
                    .as_bytes()
                    .to_vec(),
            ));
        } else {
            files.push((relative, fs::read(path).expect("snapshot file")));
        }
    }

    let mut files = Vec::new();
    collect(repository, repository, &mut files);
    HttpRepositorySnapshot(files)
}

fn prepare_sha1_smart_http_mismatch_remote(root: &std::path::Path) -> std::path::PathBuf {
    let remote = root.join("sha1-remote.git");
    let work = root.join("sha1-work");
    pinned_git_args(root, ["init", "--bare", "sha1-remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("SHA-1 export marker");
    pinned_git_args(root, ["init", "-b", "main", "sha1-work"]);
    pinned_git_args(&work, ["config", "user.name", "Bench"]);
    pinned_git_args(&work, ["config", "user.email", "bench@example.test"]);
    fs::write(work.join("sha1.txt"), b"sha1 smart HTTP\n").expect("write SHA-1 file");
    pinned_git_args(&work, ["add", "-A"]);
    pinned_git_with_env(
        &work,
        ["commit", "-m", "sha1 initial"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
    );
    pinned_git_args(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("SHA-1 remote path"),
        ],
    );
    pinned_git_args(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main_pinned(&remote);
    remote
}

fn prepare_semantic_corruption_remote(root: &std::path::Path, sha256: bool) -> std::path::PathBuf {
    let remote = if sha256 {
        prepare_sha256_smart_http_remote(root)
    } else {
        prepare_sha1_smart_http_mismatch_remote(root)
    };
    pinned_git_args(&remote, ["config", "pack.window", "0"]);
    pinned_git_args(&remote, ["config", "pack.depth", "0"]);
    remote
}

fn expected_semantic_corrupt_commit_id(
    remote: &std::path::Path,
    algorithm: GitHashAlgorithm,
) -> ObjectId {
    let head = pinned_git_args(remote, ["rev-parse", "HEAD"]);
    let output = pinned_git_command(remote, &["cat-file", "commit", head.as_str()], &[])
        .output()
        .expect("read semantic corruption source commit");
    assert!(
        output.status.success(),
        "pinned Git failed to read semantic corruption source commit: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut content = output.stdout;
    let tree_header = content
        .windows(b"tree ".len())
        .position(|window| window == b"tree ")
        .expect("semantic corruption source tree header");
    content[tree_header] = b'x';
    hash_object(algorithm, GitObjectKind::Commit, &content)
}

fn sha256_http_storage_shape(repository: &std::path::Path) -> (usize, usize) {
    let objects = repository.join(".git/objects");
    let pack_count = fs::read_dir(objects.join("pack"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry.path().extension().and_then(|ext| ext.to_str()) == Some("pack")
                })
                .count()
        })
        .unwrap_or(0);
    let loose_count = fs::read_dir(objects)
        .expect("read SHA-256 object directory")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.len() == 2 && name.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        })
        .map(|entry| {
            fs::read_dir(entry.path())
                .map(|children| children.filter_map(Result::ok).count())
                .unwrap_or(0)
        })
        .sum();
    (pack_count, loose_count)
}

#[derive(Debug, PartialEq, Eq)]
struct HttpObjectFileSnapshot {
    relative_path: std::path::PathBuf,
    content: Vec<u8>,
}

fn http_object_file_snapshot(repository: &std::path::Path) -> Vec<HttpObjectFileSnapshot> {
    let objects = repository.join(".git/objects");
    let mut files = Vec::new();
    let mut pending = vec![objects.clone()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("read object directory {directory:?}: {error}"))
        {
            let entry = entry.expect("object directory entry");
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).expect("object entry metadata");
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            if metadata.is_file() {
                let relative_path = path
                    .strip_prefix(&objects)
                    .expect("object relative path")
                    .to_owned();
                files.push(HttpObjectFileSnapshot {
                    relative_path,
                    content: fs::read(path).expect("object file content"),
                });
            }
        }
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    files
}

fn assert_no_http_temporary_pack_entries(repository: &std::path::Path, label: &str) {
    let pack_dir = repository.join(".git/objects/pack");
    for entry in fs::read_dir(&pack_dir)
        .unwrap_or_else(|error| panic!("read pack directory {pack_dir:?}: {error}"))
    {
        let entry = entry.expect("pack directory entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            !name.starts_with("tmp_http_pack_")
                && !name.starts_with("http-fetch-thin-repaired")
                && !name.starts_with("tmp_pack_"),
            "temporary HTTP role leaked in {label}: {name}"
        );
    }
}

fn remove_http_failure_temporary_pack_entries(repository: &std::path::Path) {
    let pack_dir = repository.join(".git/objects/pack");
    for name in http_failure_temporary_pack_entries(repository) {
        fs::remove_file(pack_dir.join(name)).expect("remove owned failure pack temporary");
    }
}

fn http_failure_temporary_pack_entries(repository: &std::path::Path) -> Vec<String> {
    let pack_dir = repository.join(".git/objects/pack");
    let mut names = fs::read_dir(&pack_dir)
        .expect("read failure pack directory")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (name.starts_with("tmp_http_pack_")
                || name.starts_with("http-fetch-thin-repaired")
                || name.starts_with("tmp_pack_"))
            .then_some(name)
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn http_pack_role_snapshot(repository: &std::path::Path) -> Vec<String> {
    let pack_dir = repository.join(".git/objects/pack");
    let mut roles = fs::read_dir(pack_dir)
        .expect("read HTTP pack roles")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("pack-").then_some(name)
        })
        .collect::<Vec<_>>();
    roles.sort();
    roles
}

#[test]
fn fetch_smart_http_fsck_validation_and_selected_promisor_match_stock_sha1_sha256_v0_v2() {
    for &(sha256, hash_label) in &[(false, "sha1"), (true, "sha256")] {
        for &(protocol, protocol_label) in &[(0, "v0"), (2, "v2")] {
            for promisor_configuration in [
                FsckPromisorConfiguration::Ordinary,
                FsckPromisorConfiguration::RemotePromisor,
                FsckPromisorConfiguration::PartialCloneRemote,
                FsckPromisorConfiguration::UnrelatedPromisor,
            ] {
                let dir = TempDir::new().expect("fsck HTTP temp dir");
                let remote = if sha256 {
                    prepare_sha256_smart_http_remote(dir.path())
                } else {
                    prepare_sha1_smart_http_mismatch_remote(dir.path())
                };
                let promisor_label = promisor_configuration.label();
                let label = format!("{hash_label}-{protocol_label}-{promisor_label}");
                let stock_client = dir.path().join(format!("stock-{label}"));
                let zmin_client = dir.path().join(format!("zmin-{label}"));
                let mut stock_init = vec!["init".to_owned()];
                if sha256 {
                    stock_init.push("--object-format=sha256".to_owned());
                }
                stock_init.push(stock_client.display().to_string());
                let stock_init_refs = stock_init.iter().map(String::as_str).collect::<Vec<_>>();
                assert_eq!(
                    pinned_command_any_output(
                        dir.path(),
                        &stock_init_refs,
                        &format!("stock fsck init {label}"),
                    )
                    .0,
                    0
                );
                let mut zmin_init = vec!["init".to_owned()];
                if sha256 {
                    zmin_init.push("--object-format=sha256".to_owned());
                }
                zmin_init.push(zmin_client.display().to_string());
                let zmin_init_refs = zmin_init.iter().map(String::as_str).collect::<Vec<_>>();
                assert_eq!(
                    command_any_output(
                        zmin_bin(),
                        dir.path(),
                        &zmin_init_refs,
                        &format!("Zmin fsck init {label}"),
                    )
                    .0,
                    0
                );

                let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
                let stock_url = format!(
                    "http://127.0.0.1:{}/{}",
                    stock_server.port,
                    remote
                        .file_name()
                        .expect("stock remote name")
                        .to_string_lossy()
                );
                let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
                let zmin_url = format!(
                    "http://127.0.0.1:{}/{}",
                    zmin_server.port,
                    remote
                        .file_name()
                        .expect("Zmin remote name")
                        .to_string_lossy()
                );
                pinned_git_args(
                    &stock_client,
                    ["remote", "add", "origin", stock_url.as_str()],
                );
                run_zmin(&zmin_client, ["remote", "add", "origin", zmin_url.as_str()]);
                let protocol_value = protocol.to_string();
                pinned_git_args(
                    &stock_client,
                    ["config", "protocol.version", protocol_value.as_str()],
                );
                run_zmin(
                    &zmin_client,
                    ["config", "protocol.version", protocol_value.as_str()],
                );
                pinned_git_args(&stock_client, ["config", "fetch.fsckObjects", "true"]);
                run_zmin(&zmin_client, ["config", "fetch.fsckObjects", "true"]);
                match promisor_configuration {
                    FsckPromisorConfiguration::Ordinary => {}
                    FsckPromisorConfiguration::RemotePromisor => {
                        pinned_git_args(
                            &stock_client,
                            ["config", "remote.origin.promisor", "true"],
                        );
                        run_zmin(&zmin_client, ["config", "remote.origin.promisor", "true"]);
                    }
                    FsckPromisorConfiguration::PartialCloneRemote => {
                        pinned_git_args(
                            &stock_client,
                            ["config", "core.repositoryformatversion", "1"],
                        );
                        run_zmin(
                            &zmin_client,
                            ["config", "core.repositoryformatversion", "1"],
                        );
                        pinned_git_args(
                            &stock_client,
                            ["config", "extensions.partialclone", "origin"],
                        );
                        run_zmin(
                            &zmin_client,
                            ["config", "extensions.partialclone", "origin"],
                        );
                    }
                    FsckPromisorConfiguration::UnrelatedPromisor => {
                        pinned_git_args(
                            &stock_client,
                            ["config", "remote.unrelated.promisor", "true"],
                        );
                        run_zmin(
                            &zmin_client,
                            ["config", "remote.unrelated.promisor", "true"],
                        );
                    }
                }

                let stock_output = pinned_command_any_output(
                    &stock_client,
                    ["fetch", "-q", "origin", "main"].as_slice(),
                    &format!("stock fsck fetch {label}"),
                );
                let zmin_output = command_any_output(
                    zmin_bin(),
                    &zmin_client,
                    ["fetch", "-q", "origin", "main"].as_slice(),
                    &format!("Zmin fsck fetch {label}"),
                );
                assert_eq!(zmin_output, stock_output, "fsck fetch tuple {label}");
                assert_eq!(
                    pinned_git_args(&stock_client, ["show-ref"]),
                    pinned_git_args(&zmin_client, ["show-ref"]),
                    "fsck fetch refs {label}"
                );
                assert_eq!(
                    pinned_git_args(&stock_client, ["ls-files", "--stage"]),
                    pinned_git_args(&zmin_client, ["ls-files", "--stage"]),
                    "fsck fetch index {label}"
                );
                assert_eq!(
                    pinned_git_args(&stock_client, ["status", "--porcelain"]),
                    pinned_git_args(&zmin_client, ["status", "--porcelain"]),
                    "fsck fetch status {label}"
                );
                let stock_marker_count = http_pack_role_snapshot(&stock_client)
                    .iter()
                    .filter(|name| name.ends_with(".promisor"))
                    .count();
                let zmin_marker_count = http_pack_role_snapshot(&zmin_client)
                    .iter()
                    .filter(|name| name.ends_with(".promisor"))
                    .count();
                assert_eq!(
                    stock_marker_count, 0,
                    "configured ordinary/promisor fsck fetch must not mark packs {label}"
                );
                assert_eq!(
                    zmin_marker_count, 0,
                    "Zmin configured ordinary/promisor fsck fetch must not mark packs {label}"
                );
                assert_eq!(
                    stock_marker_count, zmin_marker_count,
                    "promisor marker role {label}"
                );
                let stock_promisor = http_pack_role_snapshot(&stock_client)
                    .iter()
                    .any(|name| name.ends_with(".promisor"));
                let zmin_promisor = http_pack_role_snapshot(&zmin_client)
                    .iter()
                    .any(|name| name.ends_with(".promisor"));
                assert_eq!(zmin_promisor, stock_promisor, "promisor marker {label}");
                assert_eq!(
                    http_pack_role_snapshot(&zmin_client),
                    http_pack_role_snapshot(&stock_client),
                    "full pack role set {label}"
                );
                assert_eq!(
                    sha256_http_storage_shape(&stock_client),
                    sha256_http_storage_shape(&zmin_client),
                    "fsck storage shape {label}"
                );
                for repository in [&stock_client, &zmin_client] {
                    assert_no_http_temporary_pack_entries(repository, &label);
                }
                assert_eq!(stock_server.git_protocol_requests() > 0, protocol == 2);
                assert_eq!(zmin_server.git_protocol_requests() > 0, protocol == 2);
            }
        }
    }
}

#[test]
fn fetch_smart_http_checksum_valid_semantic_corruption_fsck_is_atomic_sha1_sha256() {
    for &(sha256, hash_label) in &[(false, "sha1"), (true, "sha256")] {
        let dir = TempDir::new().expect("semantic corruption HTTP temp dir");
        let case_root = dir.path().join(hash_label);
        fs::create_dir_all(&case_root).expect("semantic corruption case root");
        let algorithm = if sha256 {
            GitHashAlgorithm::Sha256
        } else {
            GitHashAlgorithm::Sha1
        };
        let remote = prepare_semantic_corruption_remote(&case_root, sha256);
        let expected_corrupt_commit_id = expected_semantic_corrupt_commit_id(&remote, algorithm);
        let stock_client = case_root.join("stock-client");
        let zmin_client = case_root.join("zmin-client");
        let mut stock_init = vec!["init".to_owned()];
        if sha256 {
            stock_init.push("--object-format=sha256".to_owned());
        }
        stock_init.push(stock_client.display().to_string());
        let stock_init_refs = stock_init.iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(
            pinned_command_any_output(
                &case_root,
                &stock_init_refs,
                "pinned semantic corruption init",
            )
            .0,
            0
        );
        let mut zmin_init = vec!["init".to_owned()];
        if sha256 {
            zmin_init.push("--object-format=sha256".to_owned());
        }
        zmin_init.push(zmin_client.display().to_string());
        let zmin_init_refs = zmin_init.iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(
            command_any_output(
                zmin_bin(),
                &case_root,
                &zmin_init_refs,
                "Zmin semantic corruption init",
            )
            .0,
            0
        );

        let stock_server = SmartHttpServer::corrupt_commit_pack(case_root.clone(), algorithm);
        let stock_url = format!(
            "http://127.0.0.1:{}/{}",
            stock_server.port,
            remote
                .file_name()
                .expect("semantic remote name")
                .to_string_lossy()
        );
        let zmin_server = SmartHttpServer::corrupt_commit_pack(case_root.clone(), algorithm);
        let zmin_url = format!(
            "http://127.0.0.1:{}/{}",
            zmin_server.port,
            remote
                .file_name()
                .expect("semantic remote name")
                .to_string_lossy()
        );
        pinned_git_args(
            &stock_client,
            ["remote", "add", "origin", stock_url.as_str()],
        );
        run_zmin(&zmin_client, ["remote", "add", "origin", zmin_url.as_str()]);
        pinned_git_args(&stock_client, ["config", "fetch.fsckObjects", "true"]);
        run_zmin(&zmin_client, ["config", "fetch.fsckObjects", "true"]);
        fs::write(stock_client.join(".git/FETCH_HEAD"), []).expect("seed stock FETCH_HEAD");
        fs::write(zmin_client.join(".git/FETCH_HEAD"), []).expect("seed Zmin FETCH_HEAD");
        let stock_before = snapshot_http_repository(&stock_client);
        let zmin_before = snapshot_http_repository(&zmin_client);
        let stock_output = pinned_command_any_output(
            &stock_client,
            ["fetch", "-q", "origin", "main"].as_slice(),
            "pinned checksum-valid semantic corruption fetch",
        );
        let zmin_output = command_any_output(
            zmin_bin(),
            &zmin_client,
            ["fetch", "-q", "origin", "main"].as_slice(),
            "Zmin checksum-valid semantic corruption fetch",
        );
        assert_eq!(
            zmin_output.0, stock_output.0,
            "semantic corruption exit status {hash_label}"
        );
        assert_eq!(
            zmin_output.1, stock_output.1,
            "semantic corruption stdout {hash_label}"
        );
        assert_eq!(stock_output.0, 128, "pinned semantic corruption must fail");
        assert!(
            stock_output.2.contains(&format!(
                "error: bogus commit object {}",
                expected_corrupt_commit_id
            )),
            "pinned semantic corruption diagnostic {hash_label}: {}",
            stock_output.2
        );
        assert!(
            zmin_output.2.contains(&format!(
                "error: bogus commit object {}",
                expected_corrupt_commit_id
            )),
            "Zmin semantic corruption semantic diagnostic {hash_label}: {}",
            zmin_output.2
        );
        let stock_failure_temps = http_failure_temporary_pack_entries(&stock_client);
        let zmin_failure_temps = http_failure_temporary_pack_entries(&zmin_client);
        assert!(
            !stock_failure_temps.is_empty(),
            "pinned semantic corruption must leave its owned temporary pack artifact {hash_label}"
        );
        assert!(
            stock_failure_temps
                .iter()
                .all(|name| name.starts_with("tmp_pack_")),
            "unexpected pinned semantic corruption temporary names {hash_label}: {stock_failure_temps:?}"
        );
        assert!(
            zmin_failure_temps.is_empty(),
            "Zmin semantic corruption must leave no temporary pack artifacts {hash_label}: {zmin_failure_temps:?}"
        );
        remove_http_failure_temporary_pack_entries(&stock_client);
        assert_eq!(snapshot_http_repository(&stock_client), stock_before);
        assert_eq!(snapshot_http_repository(&zmin_client), zmin_before);
        assert_no_http_temporary_pack_entries(&stock_client, hash_label);
        assert_no_http_temporary_pack_entries(&zmin_client, hash_label);
    }
}

#[test]
fn fetch_all_and_multiple_partial_clone_filter_have_explicit_boundary() {
    let dir = TempDir::new().expect("partial clone fetch-set temp dir");
    let repo = dir.path().join("client");
    run_zmin(dir.path(), ["init", repo.to_str().expect("client path")]);
    run_zmin(
        &repo,
        ["config", "remote.origin.url", "http://127.0.0.1/unused.git"],
    );
    run_zmin(
        &repo,
        [
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    );
    run_zmin(
        &repo,
        ["config", "remote.origin.partialclonefilter", "blob:none"],
    );
    let config_before = fs::read(repo.join(".git/config")).expect("read partial clone config");
    for args in [
        &["fetch", "--all"][..],
        &["fetch", "--multiple", "origin"][..],
    ] {
        let failure = run_zmin_failure_output(&repo, &args);
        assert_eq!(failure.0, 128);
        assert_eq!(
            failure.2,
            "fatal: fetch --all/--multiple currently does not support configured partial clone filters"
        );
        assert_eq!(
            fs::read(repo.join(".git/config")).expect("read unchanged partial clone config"),
            config_before,
            "partial-clone fetch-set boundary must not mutate config"
        );
    }
}

#[test]
fn clone_reads_smart_http_indexed_pack_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_clone = dir.path().join("git-smart-clone");
    let zmin_clone = dir.path().join("zmin-smart-clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::create_dir_all(work.join("dir")).expect("create dir");
    fs::write(work.join("dir/a.txt"), b"hello\n").expect("write a");
    fs::write(work.join("root.txt"), b"root\n").expect("write root");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    fs::write(work.join("dir/a.txt"), b"hello again\n").expect("rewrite a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "second"]);
    git(&work, ["branch", "feature"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let stock_clone_args = [
        "-c",
        "fetch.unpackLimit=1",
        "clone",
        url.as_str(),
        git_clone.to_str().expect("git clone path"),
    ];
    let stock_output = pinned_command_any_output(
        dir.path(),
        &stock_clone_args,
        "stock indexed smart HTTP clone",
    );
    assert_eq!(stock_output.0, 0, "stock indexed smart HTTP clone");
    run_zmin(
        dir.path(),
        [
            "clone",
            "--config=fetch.unpackLimit=1",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        fs::read_to_string(zmin_clone.join("dir/a.txt")).expect("read zmin a"),
        fs::read_to_string(git_clone.join("dir/a.txt")).expect("read git a")
    );
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&git_clone, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&zmin_clone, ["show-ref"]),
        git(&git_clone, ["show-ref"])
    );
    let stock_packs = fs::read_dir(git_clone.join(".git/objects/pack"))
        .expect("read stock pack dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("pack"))
        .count();
    let zmin_packs = fs::read_dir(zmin_clone.join(".git/objects/pack"))
        .expect("read zmin pack dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("pack"))
        .count();
    assert_eq!(zmin_packs, stock_packs, "smart HTTP clone pack cardinality");
    assert_eq!(
        http_object_file_snapshot(&zmin_clone),
        http_object_file_snapshot(&git_clone),
        "smart HTTP clone object storage"
    );
    assert_no_http_temporary_pack_entries(&git_clone, "stock smart HTTP clone");
    assert_no_http_temporary_pack_entries(&zmin_clone, "zmin smart HTTP clone");
}

#[test]
fn clone_smart_http_filter_records_remote_filter_and_promisor_like_stock_git() {
    let dir = TempDir::new().expect("smart HTTP filtered clone temp dir");
    prepare_filter_remote(dir.path());
    let stock_clone = dir.path().join("stock-filter-clone");
    let zmin_clone = dir.path().join("zmin-filter-clone");

    let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
    let stock_url = format!("http://127.0.0.1:{}/filter.git", stock_server.port);
    let stock_args = [
        "clone",
        "-q",
        "--filter=blob:none",
        stock_url.as_str(),
        stock_clone.to_str().expect("stock filtered clone path"),
    ];
    let stock_output = pinned_command_any_output(
        dir.path(),
        &stock_args,
        "pinned Git smart HTTP filtered clone",
    );
    assert_eq!(stock_output.0, 0, "stock filtered clone: {stock_output:?}");

    let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
    let zmin_url = format!("http://127.0.0.1:{}/filter.git", zmin_server.port);
    let zmin_output = command_any_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "-q",
            "--filter=blob:none",
            zmin_url.as_str(),
            zmin_clone.to_str().expect("Zmin filtered clone path"),
        ],
        "Zmin smart HTTP filtered clone",
    );
    assert_eq!(zmin_output, stock_output, "filtered clone tuple");
    for repository in [&stock_clone, &zmin_clone] {
        assert_eq!(
            pinned_git_args(repository, ["config", "--get", "remote.origin.promisor"]),
            "true"
        );
        assert_eq!(
            pinned_git_args(
                repository,
                ["config", "--get", "remote.origin.partialclonefilter"]
            ),
            "blob:none"
        );
    }
    let stock_roles = http_pack_role_snapshot(&stock_clone);
    let zmin_roles = http_pack_role_snapshot(&zmin_clone);
    assert_eq!(
        zmin_roles, stock_roles,
        "filtered clone roles: stock={stock_roles:?} zmin={zmin_roles:?}"
    );
    assert_promisor_marker_roles_match("filtered clone", &stock_clone, &zmin_clone);
    assert_eq!(
        filtered_blob_local_presence(zmin_bin(), &zmin_clone, "a.txt"),
        filtered_blob_local_presence(
            required_pinned_stock_git()
                .to_str()
                .expect("pinned Git path"),
            &stock_clone,
            "a.txt"
        )
    );
}

#[test]
fn clone_smart_http_filter_protocol_v2_matches_pinned_git() {
    let dir = TempDir::new().expect("smart HTTP filtered v2 clone temp dir");
    prepare_filter_remote(dir.path());
    let stock_clone = dir.path().join("stock-filter-v2-clone");
    let zmin_clone = dir.path().join("zmin-filter-v2-clone");

    let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
    let stock_url = format!("http://127.0.0.1:{}/filter.git", stock_server.port);
    let stock_output = pinned_command_any_output(
        dir.path(),
        &[
            "-c",
            "protocol.version=2",
            "clone",
            "-q",
            "--filter=blob:none",
            stock_url.as_str(),
            stock_clone.to_str().expect("stock filtered v2 clone path"),
        ],
        "pinned Git smart HTTP filtered v2 clone",
    );
    assert_eq!(
        stock_output.0, 0,
        "pinned filtered v2 clone: {stock_output:?}"
    );

    let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
    let zmin_url = format!("http://127.0.0.1:{}/filter.git", zmin_server.port);
    let zmin_output = command_any_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--config=protocol.version=2",
            "-q",
            "--filter=blob:none",
            zmin_url.as_str(),
            zmin_clone.to_str().expect("Zmin filtered v2 clone path"),
        ],
        "Zmin smart HTTP filtered v2 clone",
    );
    assert_eq!(zmin_output, stock_output, "filtered v2 clone tuple");
    assert!(stock_server.git_protocol_requests() > 0);
    assert!(
        zmin_server.git_protocol_requests() > 0,
        "Zmin filtered v2 clone headers: {:?}",
        zmin_server.request_headers_text()
    );
    for repository in [&stock_clone, &zmin_clone] {
        assert_eq!(
            pinned_git_args(repository, ["config", "--get", "remote.origin.promisor"]),
            "true"
        );
        assert_eq!(
            pinned_git_args(
                repository,
                ["config", "--get", "remote.origin.partialclonefilter"]
            ),
            "blob:none"
        );
    }
    assert_eq!(
        http_pack_role_snapshot(&zmin_clone),
        http_pack_role_snapshot(&stock_clone),
        "filtered v2 clone roles"
    );
    assert_promisor_marker_roles_match("filtered v2 clone", &stock_clone, &zmin_clone);
    assert_eq!(
        filtered_blob_local_presence(zmin_bin(), &zmin_clone, "a.txt"),
        filtered_blob_local_presence(
            required_pinned_stock_git()
                .to_str()
                .expect("pinned Git path"),
            &stock_clone,
            "a.txt",
        )
    );
}

#[test]
fn smart_http_filter_capability_v2_unsupported_fetch_and_clone_matches_pinned_git() {
    let dir = TempDir::new().expect("unsupported v2 HTTP filter temp dir");
    let remote = prepare_filter_remote(dir.path());
    pinned_git_args(&remote, ["config", "uploadpack.allowFilter", "false"]);
    let warning = "warning: filtering not recognized by server, ignoring";

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (stock_client, _) =
        init_network_fetch_clients(dir.path(), "filter-fetch-v2-unsupported-stock", &url);
    let (_, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-fetch-v2-unsupported-zmin", &url);
    pinned_git_args(&stock_client, ["config", "protocol.version", "2"]);
    run_zmin(&zmin_client, ["config", "protocol.version", "2"]);
    let args = ["fetch", "--quiet", "--filter=blob:none", "origin", "main"];
    let stock_output = pinned_command_any_output(
        &stock_client,
        &args,
        "pinned unsupported HTTP v2 filter fetch",
    );
    let stock_body_count = server.upload_pack_bodies_text().len();
    let zmin_output = command_any_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "Zmin unsupported HTTP v2 filter fetch",
    );
    assert_eq!(zmin_output, stock_output, "unsupported HTTP v2 fetch tuple");
    assert_eq!(stock_output.2.matches(warning).count(), 1);
    assert_eq!(zmin_output.2.matches(warning).count(), 1);
    let zmin_bodies = server.upload_pack_bodies_text();
    assert!(
        zmin_bodies[stock_body_count..]
            .iter()
            .filter(|body| body.contains("command=fetch"))
            .all(|body| !body.contains("filter blob:none")),
        "unsupported v2 fetch sent filter: {zmin_bodies:?}"
    );
    assert_filtered_fetch_matches_stock_git(
        "unsupported HTTP v2 fetch",
        &stock_client,
        &zmin_client,
    );
    assert_promisor_marker_roles_match("unsupported HTTP v2 fetch", &stock_client, &zmin_client);

    let stock_clone = dir.path().join("stock-filter-v2-unsupported-clone");
    let zmin_clone = dir.path().join("zmin-filter-v2-unsupported-clone");
    let clone_server = SmartHttpServer::new(dir.path().to_path_buf());
    let clone_url = format!("http://127.0.0.1:{}/filter.git", clone_server.port);
    let stock_clone_output = pinned_command_any_output(
        dir.path(),
        &[
            "-c",
            "protocol.version=2",
            "clone",
            "-q",
            "--filter=blob:none",
            &clone_url,
            stock_clone.to_str().expect("stock v2 clone path"),
        ],
        "pinned unsupported HTTP v2 filter clone",
    );
    let zmin_clone_output = command_any_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--config=protocol.version=2",
            "-q",
            "--filter=blob:none",
            &clone_url,
            zmin_clone.to_str().expect("Zmin v2 clone path"),
        ],
        "Zmin unsupported HTTP v2 filter clone",
    );
    assert_eq!(zmin_clone_output, stock_clone_output);
    assert_eq!(stock_clone_output.2.matches(warning).count(), 1);
    assert_eq!(zmin_clone_output.2.matches(warning).count(), 1);
    let clone_bodies = clone_server.upload_pack_bodies_text();
    assert!(
        clone_bodies
            .iter()
            .filter(|body| body.contains("command=fetch"))
            .all(|body| !body.contains("filter blob:none")),
        "unsupported v2 clone sent filter: {clone_bodies:?}"
    );
    assert_promisor_marker_roles_match("unsupported HTTP v2 clone", &stock_clone, &zmin_clone);
}

#[test]
fn fetch_smart_http_filter_protocol_v2_depth_matches_pinned_git_and_wire() {
    let dir = TempDir::new().expect("smart HTTP filtered v2 depth temp dir");
    prepare_filter_remote(dir.path());
    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (stock_client, _) =
        init_network_fetch_clients(dir.path(), "filter-fetch-v2-depth-stock", &url);
    let (_, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-fetch-v2-depth-zmin", &url);
    pinned_git_args(&stock_client, ["config", "protocol.version", "2"]);
    run_zmin(&zmin_client, ["config", "protocol.version", "2"]);
    let args = [
        "fetch",
        "--quiet",
        "--depth=1",
        "--filter=blob:none",
        "origin",
        "main",
    ];
    let stock_output =
        pinned_command_any_output(&stock_client, &args, "pinned filtered HTTP v2 depth fetch");
    let stock_body_count = server.upload_pack_bodies_text().len();
    let zmin_output = command_any_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "Zmin filtered HTTP v2 depth fetch",
    );
    assert_eq!(zmin_output, stock_output, "filtered v2 depth tuple");
    let zmin_bodies = server.upload_pack_bodies_text();
    assert!(
        zmin_bodies[stock_body_count..].iter().any(|body| {
            body.contains("command=fetch")
                && body.contains("deepen 1")
                && body.contains("filter blob:none")
        }),
        "v2 depth/filter missing from wire: {zmin_bodies:?}"
    );
    assert_network_branch_shallow_fetch_matches_stock_git(
        "filtered HTTP v2 depth fetch",
        &stock_client,
        &zmin_client,
    );
    assert_promisor_marker_roles_match("filtered HTTP v2 depth fetch", &stock_client, &zmin_client);
}

#[test]
fn fetch_smart_http_filter_protocol_v2_branch_matches_pinned_git_and_wire() {
    let dir = TempDir::new().expect("smart HTTP filtered v2 fetch temp dir");
    prepare_filter_remote(dir.path());
    let args = ["fetch", "--quiet", "--filter=blob:none", "origin", "main"];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (stock_client, _) =
        init_network_fetch_clients(dir.path(), "filter-fetch-v2-stock", url.as_str());
    pinned_git_args(&stock_client, ["config", "protocol.version", "2"]);
    let stock_output = pinned_command_any_output(
        &stock_client,
        &args,
        "pinned Git smart HTTP filtered v2 fetch",
    );
    assert_eq!(
        stock_output.0, 0,
        "stock filtered v2 fetch: {stock_output:?}"
    );
    let stock_protocol_count = server.git_protocol_requests();
    let stock_upload_pack_count = server.upload_pack_requests();
    let stock_body_count = server.upload_pack_bodies_text().len();
    let stock_header_count = server.request_headers_text().len();

    let (_, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-fetch-v2-zmin", url.as_str());
    run_zmin(&zmin_client, ["config", "protocol.version", "2"]);
    let zmin_output = command_any_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "Zmin smart HTTP filtered v2 fetch",
    );
    assert_eq!(zmin_output, stock_output, "filtered v2 fetch tuple");
    assert_filtered_fetch_matches_stock_git(
        "smart-http filtered v2 fetch",
        &stock_client,
        &zmin_client,
    );
    assert_promisor_marker_roles_match("smart-http filtered v2 fetch", &stock_client, &zmin_client);
    assert_smart_http_filter_v2_wire(
        &server,
        "branch",
        stock_body_count,
        stock_header_count,
        stock_protocol_count,
        stock_upload_pack_count,
    );
}

#[test]
fn fetch_smart_http_filter_protocol_v2_configured_matches_pinned_git_and_wire() {
    let dir = TempDir::new().expect("smart HTTP configured filtered v2 fetch temp dir");
    prepare_filter_remote(dir.path());
    let args = ["fetch", "--quiet", "--filter=blob:none", "origin"];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (stock_client, _) =
        init_network_fetch_clients(dir.path(), "filter-fetch-v2-configured-stock", url.as_str());
    pinned_git_args(&stock_client, ["config", "protocol.version", "2"]);
    let stock_output = pinned_command_any_output(
        &stock_client,
        &args,
        "pinned Git smart HTTP configured filtered v2 fetch",
    );
    assert_eq!(
        stock_output.0, 0,
        "stock configured filtered v2 fetch: {stock_output:?}"
    );
    let stock_protocol_count = server.git_protocol_requests();
    let stock_upload_pack_count = server.upload_pack_requests();
    let stock_body_count = server.upload_pack_bodies_text().len();
    let stock_header_count = server.request_headers_text().len();

    let (_, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-fetch-v2-configured-zmin", url.as_str());
    run_zmin(&zmin_client, ["config", "protocol.version", "2"]);
    let zmin_output = command_any_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "Zmin smart HTTP configured filtered v2 fetch",
    );
    assert_eq!(
        zmin_output, stock_output,
        "configured filtered v2 fetch tuple"
    );
    assert_filtered_fetch_matches_stock_git(
        "smart-http configured filtered v2 fetch",
        &stock_client,
        &zmin_client,
    );
    assert_promisor_marker_roles_match(
        "smart-http configured filtered v2 fetch",
        &stock_client,
        &zmin_client,
    );
    assert_smart_http_filter_v2_wire(
        &server,
        "configured",
        stock_body_count,
        stock_header_count,
        stock_protocol_count,
        stock_upload_pack_count,
    );
}

#[test]
fn fetch_sha256_smart_http_filter_protocol_v2_matches_pinned_git_and_wire() {
    let dir = TempDir::new().expect("SHA-256 filtered v2 fetch temp dir");
    let remote = prepare_sha256_smart_http_remote(dir.path());
    pinned_git_args(&remote, ["config", "uploadpack.allowFilter", "true"]);
    let stock_client = dir.path().join("stock-filter-v2-sha256");
    let zmin_client = dir.path().join("zmin-filter-v2-sha256");
    pinned_git_args(
        dir.path(),
        [
            "init",
            "--object-format=sha256",
            stock_client.to_str().expect("stock SHA-256 client path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "init",
            "--object-format=sha256",
            zmin_client.to_str().expect("Zmin SHA-256 client path"),
        ],
    );

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    pinned_git_args(&stock_client, ["remote", "add", "origin", &url]);
    pinned_git_args(&stock_client, ["config", "protocol.version", "2"]);
    run_zmin(&zmin_client, ["remote", "add", "origin", &url]);
    run_zmin(&zmin_client, ["config", "protocol.version", "2"]);

    let args = ["fetch", "--quiet", "--filter=blob:none", "origin", "main"];
    let stock_output = pinned_command_any_output(
        &stock_client,
        &args,
        "pinned Git SHA-256 smart HTTP filtered v2 fetch",
    );
    assert_eq!(
        stock_output.0, 0,
        "stock SHA-256 filtered v2 fetch: {stock_output:?}"
    );
    let stock_protocol_count = server.git_protocol_requests();
    let stock_upload_pack_count = server.upload_pack_requests();
    let stock_body_count = server.upload_pack_bodies_text().len();
    let stock_header_count = server.request_headers_text().len();
    let zmin_output = command_any_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "Zmin SHA-256 smart HTTP filtered v2 fetch",
    );
    assert_eq!(zmin_output, stock_output, "SHA-256 filtered v2 fetch tuple");
    assert_filter_fetch_common_matches_stock_git(
        "SHA-256 smart HTTP filtered v2 fetch",
        &stock_client,
        &zmin_client,
    );
    let stock_blob = pinned_git_args(&stock_client, ["rev-parse", "origin/main:sha256.txt"]);
    let zmin_blob = git(&zmin_client, ["rev-parse", "origin/main:sha256.txt"]);
    assert_eq!(
        filtered_object_local_presence(zmin_bin(), &zmin_client, &zmin_blob),
        filtered_object_local_presence(
            required_pinned_stock_git()
                .to_str()
                .expect("pinned Git path"),
            &stock_client,
            &stock_blob,
        ),
        "SHA-256 filtered blob presence"
    );
    assert_promisor_marker_roles_match(
        "SHA-256 smart HTTP filtered v2 fetch",
        &stock_client,
        &zmin_client,
    );
    assert_eq!(
        pinned_git_args(&stock_client, ["rev-parse", "--show-object-format"]),
        "sha256"
    );
    assert_eq!(
        pinned_git_args(&zmin_client, ["rev-parse", "--show-object-format"]),
        "sha256"
    );
    assert_smart_http_filter_v2_wire(
        &server,
        "SHA-256",
        stock_body_count,
        stock_header_count,
        stock_protocol_count,
        stock_upload_pack_count,
    );
}

#[test]
fn fetch_smart_http_filter_protocol_v0_control_has_no_v2_wire() {
    let dir = TempDir::new().expect("smart HTTP filtered v0 fetch temp dir");
    prepare_filter_remote(dir.path());
    let args = ["fetch", "--quiet", "--filter=blob:none", "origin", "main"];

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/filter.git", server.port);
    let (stock_client, _) =
        init_network_fetch_clients(dir.path(), "filter-fetch-v0-stock", url.as_str());
    pinned_git_args(&stock_client, ["config", "protocol.version", "0"]);
    let stock_output = pinned_command_any_output(
        &stock_client,
        &args,
        "pinned Git smart HTTP filtered v0 fetch",
    );
    assert_eq!(
        stock_output.0, 0,
        "stock filtered v0 fetch: {stock_output:?}"
    );
    let stock_protocol_count = server.git_protocol_requests();
    let stock_upload_pack_count = server.upload_pack_requests();
    let stock_body_count = server.upload_pack_bodies_text().len();

    let (_, zmin_client) =
        init_network_fetch_clients(dir.path(), "filter-fetch-v0-zmin", url.as_str());
    run_zmin(&zmin_client, ["config", "protocol.version", "0"]);
    let zmin_output = command_any_output(
        zmin_bin(),
        &zmin_client,
        &args,
        "Zmin smart HTTP filtered v0 fetch",
    );
    assert_eq!(zmin_output, stock_output, "filtered v0 fetch tuple");
    assert_filtered_fetch_matches_stock_git(
        "smart-http filtered v0 fetch",
        &stock_client,
        &zmin_client,
    );
    assert_eq!(server.git_protocol_requests() - stock_protocol_count, 0);
    assert_eq!(server.upload_pack_requests() - stock_upload_pack_count, 1);
    let bodies = server.upload_pack_bodies_text();
    let bodies = &bodies[stock_body_count..];
    assert!(bodies.iter().any(|body| body.contains("filter blob:none")));
    assert!(!bodies.iter().any(|body| body.contains("command=ls-refs")));
    assert!(!bodies.iter().any(|body| body.contains("command=fetch")));
}

#[test]
fn clone_sha256_smart_http_protocol_v0_matches_stock_git() {
    let dir = TempDir::new().expect("SHA-256 v0 clone temp dir");
    prepare_sha256_smart_http_remote(dir.path());
    let expected_head = pinned_git_args(&dir.path().join("sha256-work"), ["rev-parse", "HEAD"]);
    let git_clone = dir.path().join("git-sha256-v0");
    let zmin_clone = dir.path().join("zmin-sha256-v0");
    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    let args = [
        "-c",
        "protocol.version=0",
        "clone",
        "-q",
        url.as_str(),
        git_clone.to_str().expect("git clone path"),
    ];
    assert_eq!(
        pinned_command_any_output(dir.path(), &args, "stock SHA-256 v0 clone"),
        command_any_output(
            zmin_bin(),
            dir.path(),
            &[
                "-c",
                "protocol.version=0",
                "clone",
                "-q",
                url.as_str(),
                zmin_clone.to_str().expect("zmin clone path"),
            ],
            "zmin SHA-256 v0 clone"
        )
    );
    assert_eq!(server.git_protocol_requests(), 0);
    let expected_refs = pinned_git_args(&git_clone, ["show-ref"]);
    assert_sha256_http_repository_state(&git_clone, &expected_head, &expected_refs);
    assert_sha256_http_repository_state(&zmin_clone, &expected_head, &expected_refs);
    assert_eq!(
        sha256_http_storage_shape(&git_clone),
        sha256_http_storage_shape(&zmin_clone),
        "SHA-256 v0 object storage layout"
    );
}

#[test]
fn clone_smart_http_low_unpack_limit_sha1_sha256_v0_v2() {
    // Explicit below/equal thresholds force the indexed path for both hash
    // widths on a small v0 fixture; the ordinary-fetch matrix below covers
    // all threshold decisions without conflating clone behavior.
    let limits: &[(&str, Option<&str>)] = &[("below", Some("1")), ("equal", Some("3"))];
    for &(sha256, hash_label) in &[(false, "sha1"), (true, "sha256")] {
        for &(protocol, protocol_label) in &[(0, "v0"), (2, "v2")] {
            for &(limit_label, limit) in limits {
                let dir = TempDir::new().expect("unpack-limit temp dir");
                let remote = if sha256 {
                    prepare_sha256_smart_http_remote(dir.path())
                } else {
                    prepare_sha1_smart_http_mismatch_remote(dir.path())
                };
                let label = format!("{hash_label}-{protocol_label}-{limit_label}");
                let stock_clone = dir.path().join(format!("stock-{label}"));
                let zmin_clone = dir.path().join(format!("zmin-{label}"));
                let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
                let stock_url = format!(
                    "http://127.0.0.1:{}/{}",
                    stock_server.port,
                    remote.file_name().expect("remote name").to_string_lossy()
                );
                let mut stock_args = vec!["-c".to_owned(), format!("protocol.version={protocol}")];
                if let Some(limit) = limit {
                    stock_args.extend(["-c".to_owned(), format!("fetch.unpackLimit={limit}")]);
                }
                stock_args.extend([
                    "clone".to_owned(),
                    "-q".to_owned(),
                    stock_url,
                    stock_clone.display().to_string(),
                ]);
                let stock_refs = stock_args.iter().map(String::as_str).collect::<Vec<_>>();
                let stock_output = pinned_command_any_output(
                    dir.path(),
                    &stock_refs,
                    &format!("stock smart HTTP unpack limit {label}"),
                );
                assert_eq!(stock_output.0, 0, "stock {label}: {stock_output:?}");

                let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
                let zmin_url = format!(
                    "http://127.0.0.1:{}/{}",
                    zmin_server.port,
                    remote.file_name().expect("remote name").to_string_lossy()
                );
                let mut zmin_args = vec!["clone".to_owned()];
                if protocol == 2 {
                    zmin_args.push("--config=protocol.version=2".to_owned());
                }
                if let Some(limit) = limit {
                    zmin_args.push(format!("--config=fetch.unpackLimit={limit}"));
                }
                zmin_args.extend(["-q".to_owned(), zmin_url, zmin_clone.display().to_string()]);
                let zmin_refs = zmin_args.iter().map(String::as_str).collect::<Vec<_>>();
                let zmin_output = command_any_output(
                    zmin_bin(),
                    dir.path(),
                    &zmin_refs,
                    &format!("zmin smart HTTP unpack limit {label}"),
                );
                assert_eq!(zmin_output, stock_output, "smart HTTP tuple {label}");

                let expected_refs = pinned_git_args(&stock_clone, ["show-ref"]);
                let expected_stage = pinned_git_args(&stock_clone, ["ls-files", "--stage"]);
                for repository in [&stock_clone, &zmin_clone] {
                    assert_eq!(pinned_git_args(repository, ["show-ref"]), expected_refs);
                    assert_eq!(
                        pinned_git_args(repository, ["ls-files", "--stage"]),
                        expected_stage
                    );
                    assert_eq!(pinned_git_args(repository, ["status", "--porcelain"]), "");
                    assert_eq!(
                        pinned_git_args(repository, ["rev-parse", "--show-object-format"]),
                        hash_label
                    );
                    assert_no_http_temporary_pack_entries(repository, &label);
                }
                let stock_shape = sha256_http_storage_shape(&stock_clone);
                let zmin_shape = sha256_http_storage_shape(&zmin_clone);
                assert_eq!(zmin_shape, stock_shape, "storage shape {label}");
                assert!(
                    stock_shape.0 > 0,
                    "stock clone must keep an indexed pack {label}"
                );
                assert!(
                    zmin_shape.0 > 0,
                    "Zmin clone must keep an indexed pack {label}"
                );
                if protocol == 2 {
                    assert!(stock_server.git_protocol_requests() > 0, "stock v2 {label}");
                    assert!(zmin_server.git_protocol_requests() > 0, "Zmin v2 {label}");
                } else {
                    assert_eq!(stock_server.git_protocol_requests(), 0, "stock v0 {label}");
                    assert_eq!(zmin_server.git_protocol_requests(), 0, "Zmin v0 {label}");
                }
            }
        }
    }
}

#[test]
fn clone_smart_http_no_checkout_default_high_files_reftable_sha1_sha256_v0_v2() {
    let limits: &[(&str, Option<&str>)] = &[("default", None), ("high", Some("1000"))];
    let ref_formats: &[(&str, Option<&str>)] = &[("files", None), ("reftable", Some("reftable"))];
    for &(sha256, hash_label) in &[(false, "sha1"), (true, "sha256")] {
        for &(protocol, protocol_label) in &[(0, "v0"), (2, "v2")] {
            for &(ref_label, ref_format) in ref_formats {
                for &(limit_label, limit) in limits {
                    run_no_checkout_clone_case(
                        sha256,
                        hash_label,
                        protocol,
                        protocol_label,
                        ref_label,
                        ref_format,
                        limit_label,
                        limit,
                    );
                }
            }
        }
    }
}

fn run_no_checkout_clone_case(
    sha256: bool,
    hash_label: &str,
    protocol: u8,
    protocol_label: &str,
    ref_label: &str,
    ref_format: Option<&str>,
    limit_label: &str,
    limit: Option<&str>,
) {
    let dir = TempDir::new().expect("no-checkout clone temp dir");
    let remote = if sha256 {
        prepare_sha256_smart_http_remote(dir.path())
    } else {
        prepare_sha1_smart_http_mismatch_remote(dir.path())
    };
    let reachable_object_count = pinned_git_args(&remote, ["rev-list", "--objects", "--all"])
        .lines()
        .count();
    let label = format!("{hash_label}-{protocol_label}-{ref_label}-{limit_label}");
    let stock_clone = dir.path().join(format!("stock-{label}"));
    let zmin_clone = dir.path().join(format!("zmin-{label}"));
    let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
    let stock_url = format!(
        "http://127.0.0.1:{}/{}",
        stock_server.port,
        remote
            .file_name()
            .expect("stock remote name")
            .to_string_lossy()
    );
    let mut stock_args = vec!["-c".to_owned(), format!("protocol.version={protocol}")];
    if let Some(limit) = limit {
        stock_args.extend(["-c".to_owned(), format!("fetch.unpackLimit={limit}")]);
    }
    stock_args.push("clone".to_owned());
    if let Some(ref_format) = ref_format {
        stock_args.push(format!("--ref-format={ref_format}"));
    }
    stock_args.extend([
        "--no-checkout".to_owned(),
        "-q".to_owned(),
        stock_url,
        stock_clone.display().to_string(),
    ]);
    let stock_refs = stock_args.iter().map(String::as_str).collect::<Vec<_>>();
    let stock_output = pinned_command_any_output(
        dir.path(),
        &stock_refs,
        &format!("stock no-checkout clone {label}"),
    );
    assert_eq!(
        stock_output.0, 0,
        "stock no-checkout {label}: {stock_output:?}"
    );

    let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
    let zmin_url = format!(
        "http://127.0.0.1:{}/{}",
        zmin_server.port,
        remote
            .file_name()
            .expect("Zmin remote name")
            .to_string_lossy()
    );
    let mut zmin_args = vec![
        "clone".to_owned(),
        format!("--config=protocol.version={protocol}"),
    ];
    if let Some(limit) = limit {
        zmin_args.push(format!("--config=fetch.unpackLimit={limit}"));
    }
    if let Some(ref_format) = ref_format {
        zmin_args.push(format!("--ref-format={ref_format}"));
    }
    zmin_args.extend([
        "--no-checkout".to_owned(),
        "-q".to_owned(),
        zmin_url,
        zmin_clone.display().to_string(),
    ]);
    let zmin_refs = zmin_args.iter().map(String::as_str).collect::<Vec<_>>();
    let zmin_output = command_any_output(
        zmin_bin(),
        dir.path(),
        &zmin_refs,
        &format!("Zmin no-checkout clone {label}"),
    );
    assert_eq!(zmin_output, stock_output, "no-checkout clone tuple {label}");

    assert_eq!(
        pinned_git_args(&stock_clone, ["show-ref"]),
        pinned_git_args(&zmin_clone, ["show-ref"]),
        "no-checkout refs {label}"
    );
    assert_eq!(
        pinned_git_args(&stock_clone, ["ls-files", "--stage"]),
        pinned_git_args(&zmin_clone, ["ls-files", "--stage"]),
        "no-checkout index {label}"
    );
    assert_eq!(
        pinned_git_args(&stock_clone, ["status", "--porcelain"]),
        pinned_git_args(&zmin_clone, ["status", "--porcelain"]),
        "no-checkout status {label}"
    );
    assert_eq!(
        pinned_git_args(&stock_clone, ["rev-parse", "--show-object-format"]),
        hash_label,
        "stock no-checkout object format {label}"
    );
    assert_eq!(
        pinned_git_args(&zmin_clone, ["rev-parse", "--show-object-format"]),
        hash_label,
        "Zmin no-checkout object format {label}"
    );
    assert_eq!(
        pinned_git_args(&stock_clone, ["rev-parse", "--show-ref-format"]),
        ref_format.unwrap_or("files"),
        "stock no-checkout ref format {label}"
    );
    assert_eq!(
        pinned_git_args(&zmin_clone, ["rev-parse", "--show-ref-format"]),
        ref_format.unwrap_or("files"),
        "Zmin no-checkout ref format {label}"
    );
    let stock_shape = sha256_http_storage_shape(&stock_clone);
    let zmin_shape = sha256_http_storage_shape(&zmin_clone);
    assert_eq!(
        stock_shape, zmin_shape,
        "no-checkout object storage {label}"
    );
    let configured_limit = limit
        .map(|value| value.parse::<usize>().expect("numeric test unpack limit"))
        .unwrap_or(100);
    let expected_indexed =
        protocol == 2 || (configured_limit > 0 && reachable_object_count >= configured_limit);
    assert_eq!(
        stock_shape.0 > 0,
        expected_indexed,
        "stock receive mode disagrees with raw object count {reachable_object_count}, limit {configured_limit}, and protocol {protocol} for {label}"
    );
    assert_eq!(
        stock_shape.1 > 0,
        !expected_indexed,
        "stock loose/indexed shape for {label}"
    );
    assert_eq!(
        http_pack_role_snapshot(&stock_clone),
        http_pack_role_snapshot(&zmin_clone),
        "no-checkout pack roles {label}"
    );
    assert!(
        http_pack_role_snapshot(&stock_clone)
            .iter()
            .all(|role| !role.ends_with(".keep")),
        "no-checkout clone must not retain .keep {label}"
    );
    assert_eq!(
        http_object_file_snapshot(&stock_clone),
        http_object_file_snapshot(&zmin_clone),
        "no-checkout object files {label}"
    );
    assert_no_http_temporary_pack_entries(&stock_clone, &label);
    assert_no_http_temporary_pack_entries(&zmin_clone, &label);
    if protocol == 2 {
        assert!(stock_server.git_protocol_requests() > 0, "stock v2 {label}");
        assert!(zmin_server.git_protocol_requests() > 0, "Zmin v2 {label}");
    } else {
        assert_eq!(stock_server.git_protocol_requests(), 0, "stock v0 {label}");
        assert_eq!(zmin_server.git_protocol_requests(), 0, "Zmin v0 {label}");
    }
}

#[test]
fn clone_sha256_smart_http_reftable_matches_stock_git() {
    let dir = TempDir::new().expect("SHA-256 reftable clone temp dir");
    prepare_sha256_smart_http_remote(dir.path());
    let expected_head = pinned_git_args(&dir.path().join("sha256-work"), ["rev-parse", "HEAD"]);
    let git_clone = dir.path().join("git-sha256-reftable");
    let zmin_clone = dir.path().join("zmin-sha256-reftable");
    let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
    let stock_url = format!("http://127.0.0.1:{}/remote.git", stock_server.port);
    let stock_args = [
        "clone",
        "--ref-format=reftable",
        "--no-checkout",
        "-q",
        stock_url.as_str(),
        git_clone.to_str().expect("stock reftable clone path"),
    ];
    let stock_output =
        pinned_command_any_output(dir.path(), &stock_args, "stock SHA-256 reftable clone");
    let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
    let zmin_url = format!("http://127.0.0.1:{}/remote.git", zmin_server.port);
    let zmin_args = [
        "clone",
        "--ref-format=reftable",
        "--no-checkout",
        "-q",
        zmin_url.as_str(),
        zmin_clone.to_str().expect("zmin reftable clone path"),
    ];
    let zmin_output = command_any_output(
        zmin_bin(),
        dir.path(),
        &zmin_args,
        "zmin SHA-256 reftable clone",
    );
    assert_eq!(zmin_output, stock_output);
    let expected_refs = pinned_git_args(&git_clone, ["show-ref"]);
    for repository in [&git_clone, &zmin_clone] {
        assert_eq!(
            pinned_git_args(repository, ["rev-parse", "--show-object-format"]),
            "sha256"
        );
        assert_eq!(pinned_git_args(repository, ["show-ref"]), expected_refs);
        assert_eq!(
            pinned_git_args(repository, ["symbolic-ref", "HEAD"]),
            "refs/heads/main"
        );
        assert_eq!(
            pinned_git_args(repository, ["rev-parse", "HEAD"]),
            expected_head
        );
    }
    for repository in [&git_clone, &zmin_clone] {
        assert_eq!(
            pinned_git_args(repository, ["rev-parse", "--show-ref-format"]),
            "reftable"
        );
        assert!(repository.join(".git/reftable/tables.list").is_file());
    }
    assert_eq!(
        http_pack_role_snapshot(&git_clone),
        http_pack_role_snapshot(&zmin_clone),
        "SHA-256 reftable clone object roles"
    );
    assert_eq!(
        http_object_file_snapshot(&git_clone),
        http_object_file_snapshot(&zmin_clone),
        "SHA-256 reftable clone object files"
    );
    assert!(
        http_pack_role_snapshot(&git_clone)
            .iter()
            .all(|role| !role.ends_with(".keep")),
        "SHA-256 reftable clone must not retain a .keep file"
    );
}

#[test]
fn clone_populated_smart_http_reftable_sha1_sha256_and_tag_matches_stock_git() {
    run_populated_reftable_clone_case(false, false, false);
    run_populated_reftable_clone_case(true, false, false);
    run_populated_reftable_clone_case(true, true, false);
    run_populated_reftable_clone_case(true, true, true);
}

fn run_populated_reftable_clone_case(sha256: bool, protocol_v2: bool, annotated_tag: bool) {
    let dir = TempDir::new().expect("populated reftable clone temp dir");
    let remote = if sha256 {
        let remote = prepare_sha256_smart_http_remote(dir.path());
        if annotated_tag {
            let work = dir.path().join("sha256-work");
            pinned_git_args(
                &work,
                ["tag", "-a", "v-sha256", "-m", "sha256 release", "HEAD"],
            );
            pinned_git_args(&work, ["push", "-q", "origin", "refs/tags/v-sha256"]);
        }
        remote
    } else {
        assert!(!annotated_tag);
        prepare_sha1_smart_http_mismatch_remote(dir.path())
    };
    let format = if sha256 { "sha256" } else { "sha1" };
    let label = match (sha256, protocol_v2, annotated_tag) {
        (false, false, false) => "sha1-v0-branch",
        (false, true, false) => "sha1-v2-branch",
        (true, false, false) => "sha256-v0-branch",
        (true, true, false) => "sha256-v2-branch",
        (true, true, true) => "sha256-v2-tag",
        _ => unreachable!("unsupported populated reftable case"),
    };
    let git_clone = dir.path().join(format!("git-{label}"));
    let zmin_clone = dir.path().join(format!("zmin-{label}"));
    let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
    let stock_url = format!(
        "http://127.0.0.1:{}/{}",
        stock_server.port,
        remote.file_name().unwrap().to_string_lossy()
    );
    let mut stock_args = Vec::new();
    if protocol_v2 {
        stock_args.extend(["-c".to_owned(), "protocol.version=2".to_owned()]);
    } else {
        stock_args.extend(["-c".to_owned(), "protocol.version=0".to_owned()]);
    }
    stock_args.extend(["clone".to_owned(), "--ref-format=reftable".to_owned()]);
    if annotated_tag {
        stock_args.push("--branch=v-sha256".to_owned());
    }
    stock_args.extend(["-q".to_owned(), stock_url, git_clone.display().to_string()]);
    let stock_arg_refs = stock_args.iter().map(String::as_str).collect::<Vec<_>>();
    let stock_output = if protocol_v2 {
        let exec_path = pinned_http_v2_exec_path(dir.path());
        pinned_command_any_output_with_env(
            dir.path(),
            &stock_arg_refs,
            [(
                "GIT_EXEC_PATH",
                exec_path.to_str().expect("pinned HTTP exec path"),
            )]
            .as_slice(),
            &format!("stock populated {label} reftable clone"),
        )
    } else {
        pinned_command_any_output(
            dir.path(),
            &stock_arg_refs,
            &format!("stock populated {label} reftable clone"),
        )
    };
    assert_eq!(
        stock_output.0, 0,
        "stock populated {label} clone: {stock_output:?}"
    );

    let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
    let zmin_url = format!(
        "http://127.0.0.1:{}/{}",
        zmin_server.port,
        remote.file_name().unwrap().to_string_lossy()
    );
    let mut zmin_args = vec!["clone".to_owned()];
    if protocol_v2 {
        zmin_args.extend(["-c".to_owned(), "protocol.version=2".to_owned()]);
    } else {
        zmin_args.extend(["-c".to_owned(), "protocol.version=0".to_owned()]);
    }
    zmin_args.extend(["--ref-format=reftable".to_owned()]);
    if annotated_tag {
        zmin_args.push("--branch=v-sha256".to_owned());
    }
    zmin_args.extend(["-q".to_owned(), zmin_url, zmin_clone.display().to_string()]);
    let zmin_arg_refs = zmin_args.iter().map(String::as_str).collect::<Vec<_>>();
    let zmin_output = command_any_output(
        zmin_bin(),
        dir.path(),
        &zmin_arg_refs,
        &format!("zmin populated {label} reftable clone"),
    );
    assert_eq!(zmin_output, stock_output, "populated {label} clone tuple");

    if protocol_v2 {
        for server in [&stock_server, &zmin_server] {
            assert!(
                server
                    .request_headers_text()
                    .iter()
                    .any(|headers| headers.contains("Git-Protocol: version=2\r\n")),
                "populated {label} clone did not send Git-Protocol: version=2"
            );
        }
    }
    assert_populated_reftable_clone_state(&git_clone, &zmin_clone, format, annotated_tag);
}

fn ls_files_stage_object_id(line: &str) -> &str {
    // `ls-files --stage` is `mode OID stage<TAB>path`; OID is field 1.
    line.split_whitespace()
        .nth(1)
        .expect("ls-files --stage object id")
}

fn assert_populated_reftable_clone_state(
    stock: &std::path::Path,
    zmin: &std::path::Path,
    format: &str,
    annotated_tag: bool,
) {
    let expected_refs = pinned_git_args(stock, ["show-ref"]);
    let expected_stage = pinned_git_args(stock, ["ls-files", "--stage"]);
    let expected_paths = pinned_git_args(stock, ["ls-files"]);
    for repository in [stock, zmin] {
        assert_eq!(
            pinned_git_args(repository, ["rev-parse", "--show-object-format"]),
            format
        );
        assert_eq!(
            pinned_git_args(repository, ["rev-parse", "--show-ref-format"]),
            "reftable"
        );
        assert_eq!(pinned_git_args(repository, ["show-ref"]), expected_refs);
        assert_eq!(
            pinned_git_args(repository, ["ls-files", "--stage"]),
            expected_stage
        );
        assert_eq!(pinned_git_args(repository, ["ls-files"]), expected_paths);
        assert_eq!(
            pinned_git_args(repository, ["status", "--porcelain"]),
            "",
            "populated reftable clone must be clean"
        );
        assert_eq!(
            pinned_git_args(repository, ["rev-parse", "--is-bare-repository"]),
            "false"
        );
        assert!(repository.join(".git/reftable/tables.list").is_file());
        let tables = fs::read_to_string(repository.join(".git/reftable/tables.list"))
            .expect("read reftable table list");
        assert!(
            !tables.trim().is_empty(),
            "populated reftable clone has no table entries"
        );
        for table in tables.lines().filter(|line| !line.is_empty()) {
            assert!(
                repository.join(".git/reftable").join(table).is_file(),
                "missing reftable table {}",
                table
            );
        }
    }
    if annotated_tag {
        for repository in [stock, zmin] {
            let head = pinned_command_any_output(
                repository,
                &["symbolic-ref", "-q", "HEAD"],
                "annotated tag symbolic HEAD",
            );
            assert_eq!(head, (1, String::new(), String::new()));
        }
    } else {
        assert_eq!(
            pinned_git_args(stock, ["symbolic-ref", "HEAD"]),
            "refs/heads/main"
        );
        assert_eq!(
            pinned_git_args(zmin, ["symbolic-ref", "HEAD"]),
            "refs/heads/main"
        );
    }
    assert_eq!(
        pinned_git_args(stock, ["rev-parse", "HEAD"]),
        pinned_git_args(zmin, ["rev-parse", "HEAD"])
    );
    for line in expected_stage.lines() {
        let oid = ls_files_stage_object_id(line);
        assert_eq!(oid.len(), if format == "sha256" { 64 } else { 40 });
    }
    for path in expected_paths.lines() {
        assert_eq!(
            fs::read(stock.join(path)).expect("stock tracked file"),
            fs::read(zmin.join(path)).expect("zmin tracked file"),
            "tracked content mismatch for {path}"
        );
    }
    assert_eq!(
        sha256_http_storage_shape(stock),
        sha256_http_storage_shape(zmin),
        "populated {format} reftable object storage layout"
    );
    assert_eq!(
        http_pack_role_snapshot(stock),
        http_pack_role_snapshot(zmin),
        "populated {format} reftable pack roles"
    );
    assert_eq!(
        http_object_file_snapshot(stock),
        http_object_file_snapshot(zmin),
        "populated {format} reftable object files"
    );
    assert!(
        http_pack_role_snapshot(stock)
            .iter()
            .all(|role| !role.ends_with(".keep")),
        "populated {format} reftable clone must not retain a .keep file"
    );
}

#[test]
fn clone_sha256_smart_http_protocol_v2_matches_stock_git() {
    let dir = TempDir::new().expect("SHA-256 v2 clone temp dir");
    prepare_sha256_smart_http_remote(dir.path());
    let expected_head = pinned_git_args(&dir.path().join("sha256-work"), ["rev-parse", "HEAD"]);
    let git_clone = dir.path().join("git-sha256-v2");
    let zmin_clone = dir.path().join("zmin-sha256-v2");
    let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
    let stock_url = format!("http://127.0.0.1:{}/remote.git", stock_server.port);
    let stock_exec_path = pinned_http_v2_exec_path(dir.path());
    let stock_args = [
        "-c",
        "protocol.version=2",
        "clone",
        "-q",
        "--server-option=protocol-test",
        stock_url.as_str(),
        git_clone.to_str().expect("git clone path"),
    ];
    let stock_output = pinned_command_any_output_with_env(
        dir.path(),
        &stock_args,
        &[(
            "GIT_EXEC_PATH",
            stock_exec_path.to_str().expect("HTTP helper path"),
        )],
        "stock SHA-256 v2 clone",
    );
    let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
    let zmin_url = format!("http://127.0.0.1:{}/remote.git", zmin_server.port);
    let zmin_args = [
        "clone",
        "--config=protocol.version=2",
        "-q",
        "--server-option=protocol-test",
        zmin_url.as_str(),
        zmin_clone.to_str().expect("zmin clone path"),
    ];
    let zmin_output =
        command_any_output(zmin_bin(), dir.path(), &zmin_args, "zmin SHA-256 v2 clone");
    assert_eq!(zmin_output, stock_output);
    for server in [&stock_server, &zmin_server] {
        assert!(
            server.git_protocol_requests() > 0,
            "protocol-v2 clone must send Git-Protocol: {:?}",
            server.request_headers_text()
        );
        assert!(
            server
                .request_headers_text()
                .iter()
                .any(|headers| headers.contains("Git-Protocol: version=2\r\n")),
            "protocol-v2 clone must send the exact Git-Protocol header"
        );
        assert!(
            server.request_headers_text().iter().any(|headers| {
                headers.contains("Content-Type: application/x-git-upload-pack-request\r\n")
                    && headers.contains("Accept: application/x-git-upload-pack-result\r\n")
            }),
            "protocol-v2 clone must send upload-pack content negotiation headers"
        );
        assert!(
            server.upload_pack_bodies_text().iter().any(|body| {
                body.contains("object-format=sha256")
                    && body.contains("server-option=protocol-test")
            }),
            "protocol-v2 clone must send object-format=sha256 and server-option=protocol-test"
        );
    }
    let expected_refs = pinned_git_args(&git_clone, ["show-ref"]);
    assert_sha256_http_repository_state(&git_clone, &expected_head, &expected_refs);
    assert_sha256_http_repository_state(&zmin_clone, &expected_head, &expected_refs);
    assert_eq!(
        sha256_http_storage_shape(&git_clone),
        sha256_http_storage_shape(&zmin_clone),
        "SHA-256 v2 object storage layout"
    );
}

#[test]
fn fetch_sha256_smart_http_protocol_v2_matches_stock_git() {
    let dir = TempDir::new().expect("SHA-256 v2 fetch temp dir");
    prepare_sha256_smart_http_remote(dir.path());
    let expected_head = pinned_git_args(&dir.path().join("sha256-work"), ["rev-parse", "HEAD"]);
    let git_client = dir.path().join("git-sha256-fetch");
    let zmin_client = dir.path().join("zmin-sha256-fetch");
    pinned_git_args(
        dir.path(),
        ["init", "--object-format=sha256", "git-sha256-fetch"],
    );
    run_zmin(
        dir.path(),
        ["init", "--object-format=sha256", "zmin-sha256-fetch"],
    );

    let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
    let stock_url = format!("http://127.0.0.1:{}/remote.git", stock_server.port);
    pinned_git_args(&git_client, ["remote", "add", "origin", stock_url.as_str()]);
    let stock_args = ["-c", "protocol.version=2", "fetch", "-q", "origin", "main"];
    let stock_output =
        pinned_command_any_output(&git_client, &stock_args, "stock SHA-256 v2 fetch");

    let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
    let zmin_url = format!("http://127.0.0.1:{}/remote.git", zmin_server.port);
    run_zmin(&zmin_client, ["remote", "add", "origin", zmin_url.as_str()]);
    run_zmin(&zmin_client, ["config", "protocol.version", "2"]);
    let zmin_args = ["fetch", "-q", "origin", "main"];
    let zmin_output = command_any_output(
        zmin_bin(),
        &zmin_client,
        &zmin_args,
        "zmin SHA-256 v2 fetch",
    );
    assert_eq!(zmin_output, stock_output);
    assert!(
        stock_server.git_protocol_requests() > 0 && zmin_server.git_protocol_requests() > 0,
        "both v2 fetches must send Git-Protocol"
    );
    for server in [&stock_server, &zmin_server] {
        assert!(
            server
                .request_headers_text()
                .iter()
                .any(|headers| headers.contains("Git-Protocol: version=2\r\n")),
            "both v2 fetches must send the exact Git-Protocol header"
        );
        assert!(
            server.request_headers_text().iter().any(|headers| {
                headers.contains("Content-Type: application/x-git-upload-pack-request\r\n")
                    && headers.contains("Accept: application/x-git-upload-pack-result\r\n")
            }),
            "both v2 fetches must send upload-pack content negotiation headers"
        );
        assert!(
            server
                .upload_pack_bodies_text()
                .iter()
                .any(|body| body.contains("object-format=sha256")),
            "v2 fetch must send object-format=sha256"
        );
    }
    assert_eq!(
        pinned_git_args(&git_client, ["rev-parse", "--show-object-format"]),
        "sha256"
    );
    assert_eq!(
        pinned_git_args(&zmin_client, ["rev-parse", "--show-object-format"]),
        "sha256"
    );
    assert_eq!(
        pinned_git_args(&git_client, ["rev-parse", "refs/remotes/origin/main"]),
        expected_head
    );
    assert_eq!(
        pinned_git_args(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
        expected_head
    );
    assert_eq!(
        pinned_git_args(&zmin_client, ["show-ref"]),
        pinned_git_args(&git_client, ["show-ref"])
    );
    assert_eq!(
        pinned_git_args(&zmin_client, ["cat-file", "-p", "origin/main:sha256.txt"]),
        pinned_git_args(&git_client, ["cat-file", "-p", "origin/main:sha256.txt"])
    );
    assert_eq!(
        sha256_http_storage_shape(&git_client),
        sha256_http_storage_shape(&zmin_client),
        "SHA-256 v2 fetch object storage layout"
    );
}

#[test]
fn fetch_smart_http_unpack_limit_matches_stock_sha1_sha256_v0_v2() {
    for &(sha256, hash_label) in &[(false, "sha1"), (true, "sha256")] {
        for &(protocol, protocol_label) in &[(0, "v0"), (2, "v2")] {
            let dir = TempDir::new().expect("unpack-limit fetch temp dir");
            let remote = if sha256 {
                prepare_sha256_smart_http_remote(dir.path())
            } else {
                prepare_sha1_smart_http_mismatch_remote(dir.path())
            };
            let object_count = pinned_git_args(&remote, ["rev-list", "--objects", "--all"])
                .lines()
                .count();
            assert!(object_count > 1, "fixture pack count must be nontrivial");
            let thresholds = [
                ("default", None, object_count >= 100),
                ("zero", Some(0_usize), false),
                ("below", Some(object_count.saturating_sub(1)), true),
                ("equal", Some(object_count), true),
                ("above", Some(object_count + 1), false),
            ];
            for &(limit_label, limit, expect_indexed) in &thresholds {
                let label = format!("{hash_label}-{protocol_label}-{limit_label}");
                let stock_client = dir.path().join(format!("stock-fetch-{label}"));
                let zmin_client = dir.path().join(format!("zmin-fetch-{label}"));
                if sha256 {
                    pinned_git_args(
                        dir.path(),
                        [
                            "init",
                            "--object-format=sha256",
                            stock_client.to_str().expect("stock fetch client path"),
                        ],
                    );
                } else {
                    pinned_git_args(
                        dir.path(),
                        [
                            "init",
                            stock_client.to_str().expect("stock fetch client path"),
                        ],
                    );
                }
                if sha256 {
                    run_zmin(
                        dir.path(),
                        [
                            "init",
                            "--object-format=sha256",
                            zmin_client.to_str().expect("Zmin fetch client path"),
                        ],
                    );
                } else {
                    run_zmin(
                        dir.path(),
                        [
                            "init",
                            zmin_client.to_str().expect("Zmin fetch client path"),
                        ],
                    );
                }
                let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
                let stock_url = format!(
                    "http://127.0.0.1:{}/{}",
                    stock_server.port,
                    remote.file_name().expect("remote name").to_string_lossy()
                );
                pinned_git_args(
                    &stock_client,
                    ["remote", "add", "origin", stock_url.as_str()],
                );
                let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
                let zmin_url = format!(
                    "http://127.0.0.1:{}/{}",
                    zmin_server.port,
                    remote.file_name().expect("remote name").to_string_lossy()
                );
                run_zmin(&zmin_client, ["remote", "add", "origin", zmin_url.as_str()]);
                if protocol == 2 {
                    pinned_git_args(&stock_client, ["config", "protocol.version", "2"]);
                    run_zmin(&zmin_client, ["config", "protocol.version", "2"]);
                } else {
                    pinned_git_args(&stock_client, ["config", "protocol.version", "0"]);
                    run_zmin(&zmin_client, ["config", "protocol.version", "0"]);
                }
                if let Some(limit) = limit {
                    let value = limit.to_string();
                    pinned_git_args(&stock_client, ["config", "fetch.unpackLimit", &value]);
                    run_zmin(&zmin_client, ["config", "fetch.unpackLimit", &value]);
                }
                let stock_output = pinned_command_any_output(
                    &stock_client,
                    &["fetch", "-q", "origin", "main"],
                    &format!("stock unpack-limit fetch {label}"),
                );
                let zmin_output = command_any_output(
                    zmin_bin(),
                    &zmin_client,
                    &["fetch", "-q", "origin", "main"],
                    &format!("Zmin unpack-limit fetch {label}"),
                );
                assert_eq!(zmin_output, stock_output, "fetch tuple {label}");
                assert_eq!(
                    pinned_git_args(&stock_client, ["show-ref"]),
                    pinned_git_args(&zmin_client, ["show-ref"]),
                    "fetch refs {label}"
                );
                assert_eq!(
                    pinned_git_args(&stock_client, ["rev-parse", "refs/remotes/origin/main"]),
                    pinned_git_args(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
                    "fetch target {label}"
                );
                for repository in [&stock_client, &zmin_client] {
                    assert_eq!(
                        pinned_git_args(repository, ["rev-parse", "--show-object-format"]),
                        hash_label
                    );
                    let pack_dir = repository.join(".git/objects/pack");
                    assert!(
                        fs::read_dir(&pack_dir)
                            .expect("read fetch pack directory")
                            .filter_map(Result::ok)
                            .all(|entry| {
                                let name = entry.file_name();
                                let name = name.to_string_lossy();
                                !name.starts_with("tmp_http_pack_")
                                    && !name.starts_with("http-fetch-thin-repaired")
                            }),
                        "fetch temporary pack leaked in {label}"
                    );
                }
                let stock_shape = sha256_http_storage_shape(&stock_client);
                let zmin_shape = sha256_http_storage_shape(&zmin_client);
                assert_eq!(zmin_shape, stock_shape, "fetch storage shape {label}");
                assert_eq!(
                    http_object_file_snapshot(&zmin_client),
                    http_object_file_snapshot(&stock_client),
                    "fetch object roles/content {label}"
                );
                assert_eq!(stock_shape.0 > 0, expect_indexed, "stock mode {label}");
                assert_eq!(zmin_shape.0 > 0, expect_indexed, "Zmin mode {label}");
                if protocol == 2 {
                    assert!(stock_server.git_protocol_requests() > 0, "stock v2 {label}");
                    assert!(zmin_server.git_protocol_requests() > 0, "Zmin v2 {label}");
                } else {
                    assert_eq!(stock_server.git_protocol_requests(), 0, "stock v0 {label}");
                    assert_eq!(zmin_server.git_protocol_requests(), 0, "Zmin v0 {label}");
                }
            }
        }
    }
}

#[test]
fn fetch_smart_http_reftable_unpack_limit_matches_stock_sha1_sha256() {
    for &(sha256, hash_label) in &[(false, "sha1"), (true, "sha256")] {
        for &(protocol, protocol_label) in &[(0, "v0"), (2, "v2")] {
            let dir = TempDir::new().expect("reftable fetch temp dir");
            let remote = if sha256 {
                prepare_sha256_smart_http_remote(dir.path())
            } else {
                prepare_sha1_smart_http_mismatch_remote(dir.path())
            };
            let object_count = pinned_git_args(&remote, ["rev-list", "--objects", "--all"])
                .lines()
                .count();
            assert!(
                object_count > 1,
                "reftable fixture pack count must be nontrivial"
            );
            let thresholds = [
                ("zero", 0, false),
                ("below", object_count - 1, true),
                ("default", 100, object_count >= 100),
                ("equal", object_count, true),
                ("above", object_count + 1, false),
            ];
            for &(limit_label, limit, expect_indexed) in &thresholds {
                let label = format!("{hash_label}-{protocol_label}-{limit_label}");
                let stock_client = dir.path().join(format!("stock-reftable-fetch-{label}"));
                let zmin_client = dir.path().join(format!("zmin-reftable-fetch-{label}"));
                let mut stock_init = vec![
                    "init".to_owned(),
                    "--ref-format=reftable".to_owned(),
                    "-b".to_owned(),
                    "main".to_owned(),
                ];
                if sha256 {
                    stock_init.push("--object-format=sha256".to_owned());
                }
                stock_init.push(stock_client.display().to_string());
                let stock_init_refs = stock_init.iter().map(String::as_str).collect::<Vec<_>>();
                let stock_init_output = pinned_command_any_output(
                    dir.path(),
                    &stock_init_refs,
                    &format!("stock reftable init {label}"),
                );
                assert_eq!(stock_init_output.0, 0, "stock reftable init {label}");

                let mut zmin_init = vec![
                    "init".to_owned(),
                    "--ref-format=reftable".to_owned(),
                    "-b".to_owned(),
                    "main".to_owned(),
                ];
                if sha256 {
                    zmin_init.push("--object-format=sha256".to_owned());
                }
                zmin_init.push(zmin_client.display().to_string());
                let zmin_init_refs = zmin_init.iter().map(String::as_str).collect::<Vec<_>>();
                let zmin_init_output = command_any_output(
                    zmin_bin(),
                    dir.path(),
                    &zmin_init_refs,
                    &format!("Zmin reftable init {label}"),
                );
                assert_eq!(
                    zmin_init_output.0, stock_init_output.0,
                    "reftable init {label}"
                );
                assert_eq!(
                    zmin_init_output.2, stock_init_output.2,
                    "reftable init stderr {label}"
                );

                let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
                let stock_url = format!(
                    "http://127.0.0.1:{}/{}",
                    stock_server.port,
                    remote.file_name().expect("remote name").to_string_lossy()
                );
                pinned_git_args(
                    &stock_client,
                    ["remote", "add", "origin", stock_url.as_str()],
                );
                let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
                let zmin_url = format!(
                    "http://127.0.0.1:{}/{}",
                    zmin_server.port,
                    remote.file_name().expect("remote name").to_string_lossy()
                );
                run_zmin(&zmin_client, ["remote", "add", "origin", zmin_url.as_str()]);
                pinned_git_args(
                    &stock_client,
                    ["config", "protocol.version", &protocol.to_string()],
                );
                run_zmin(
                    &zmin_client,
                    ["config", "protocol.version", &protocol.to_string()],
                );
                let limit_value = limit.to_string();
                pinned_git_args(
                    &stock_client,
                    ["config", "fetch.unpackLimit", limit_value.as_str()],
                );
                run_zmin(
                    &zmin_client,
                    ["config", "fetch.unpackLimit", limit_value.as_str()],
                );
                let stock_output = pinned_command_any_output(
                    &stock_client,
                    &["fetch", "-q", "origin", "main"],
                    &format!("stock reftable fetch {label}"),
                );
                let zmin_output = command_any_output(
                    zmin_bin(),
                    &zmin_client,
                    &["fetch", "-q", "origin", "main"],
                    &format!("Zmin reftable fetch {label}"),
                );
                assert_eq!(zmin_output, stock_output, "reftable fetch tuple {label}");
                assert_eq!(
                    pinned_git_args(&stock_client, ["show-ref"]),
                    pinned_git_args(&zmin_client, ["show-ref"]),
                    "reftable fetch refs {label}"
                );
                assert_eq!(
                    pinned_git_args(&stock_client, ["rev-parse", "--show-ref-format"]),
                    "reftable"
                );
                assert_eq!(
                    pinned_git_args(&zmin_client, ["rev-parse", "--show-ref-format"]),
                    "reftable"
                );
                for repository in [&stock_client, &zmin_client] {
                    assert_eq!(
                        pinned_git_args(repository, ["rev-parse", "--show-object-format"]),
                        hash_label
                    );
                    assert_eq!(pinned_git_args(repository, ["status", "--porcelain"]), "");
                    assert!(repository.join(".git/reftable/tables.list").is_file());
                    assert_no_http_temporary_pack_entries(repository, &label);
                }
                let stock_shape = sha256_http_storage_shape(&stock_client);
                let zmin_shape = sha256_http_storage_shape(&zmin_client);
                assert_eq!(
                    zmin_shape, stock_shape,
                    "reftable fetch storage shape {label}"
                );
                assert_eq!(
                    http_object_file_snapshot(&zmin_client),
                    http_object_file_snapshot(&stock_client),
                    "reftable fetch object roles/content {label}"
                );
                assert_eq!(
                    stock_shape.0 > 0,
                    expect_indexed,
                    "stock reftable mode {label}"
                );
                assert_eq!(
                    zmin_shape.0 > 0,
                    expect_indexed,
                    "Zmin reftable mode {label}"
                );
                if protocol == 2 {
                    assert!(
                        stock_server.git_protocol_requests() > 0,
                        "stock reftable v2 {label}"
                    );
                    assert!(
                        zmin_server.git_protocol_requests() > 0,
                        "Zmin reftable v2 {label}"
                    );
                } else {
                    assert_eq!(
                        stock_server.git_protocol_requests(),
                        0,
                        "stock reftable v0 {label}"
                    );
                    assert_eq!(
                        zmin_server.git_protocol_requests(),
                        0,
                        "Zmin reftable v0 {label}"
                    );
                }
            }
        }
    }
}

#[test]
fn clone_sha256_stock_client_to_zmin_http_backend_v0_matches_stock_backend() {
    // This fixture has 128 bulk files so the indexed-pack path is exercised;
    // small-pack unpack policy is outside this slice.
    let dir = TempDir::new().expect("SHA-256 backend clone temp dir");
    prepare_sha256_smart_http_remote(dir.path());
    let expected_head = pinned_git_args(&dir.path().join("sha256-work"), ["rev-parse", "HEAD"]);
    let stock_clone = dir.path().join("stock-client-backend-clone");
    let zmin_clone = dir.path().join("zmin-client-backend-clone");
    let stock_backend = BackendHttpServer::new(
        required_pinned_stock_git().display().to_string(),
        dir.path().to_path_buf(),
    );
    let stock_url = format!("http://127.0.0.1:{}/remote.git", stock_backend.port);
    let stock_args = [
        "-c",
        "protocol.version=0",
        "clone",
        "-q",
        stock_url.as_str(),
        stock_clone.to_str().expect("stock backend clone path"),
    ];
    let stock_output = pinned_command_any_output(dir.path(), &stock_args, "stock backend v0 clone");
    let zmin_backend = BackendHttpServer::new(zmin_bin().to_owned(), dir.path().to_path_buf());
    let zmin_url = format!("http://127.0.0.1:{}/remote.git", zmin_backend.port);
    let zmin_args = [
        "-c",
        "protocol.version=0",
        "clone",
        "-q",
        zmin_url.as_str(),
        zmin_clone.to_str().expect("zmin backend clone path"),
    ];
    let zmin_output =
        command_any_output(zmin_bin(), dir.path(), &zmin_args, "zmin backend v0 clone");
    assert_eq!(zmin_output, stock_output);
    let expected_refs = pinned_git_args(&stock_clone, ["show-ref"]);
    assert_sha256_http_repository_state(&stock_clone, &expected_head, &expected_refs);
    assert_sha256_http_repository_state(&zmin_clone, &expected_head, &expected_refs);
    assert_eq!(
        sha256_http_storage_shape(&stock_clone),
        sha256_http_storage_shape(&zmin_clone),
        "SHA-256 backend v0 object storage layout"
    );
    for server in [&stock_backend, &zmin_backend] {
        let headers = server.request_headers_text();
        assert!(
            headers.iter().any(|request| {
                request.contains("Content-Type: application/x-git-upload-pack-request\r\n")
                    && request.contains("Accept: application/x-git-upload-pack-result\r\n")
            }),
            "backend v0 clone must receive upload-pack content type: {headers:?}"
        );
        assert!(
            headers
                .iter()
                .all(|request| !request.contains("Git-Protocol:")),
            "v0 backend clone unexpectedly negotiated protocol v2: {headers:?}"
        );
    }
}

#[test]
fn clone_sha256_stock_client_to_zmin_http_backend_v2_matches_stock_backend() {
    // The large fixture keeps indexed-pack behavior in scope; small-pack
    // unpack policy is intentionally outside this test.
    let dir = TempDir::new().expect("SHA-256 backend v2 clone temp dir");
    prepare_sha256_smart_http_remote(dir.path());
    let expected_head = pinned_git_args(&dir.path().join("sha256-work"), ["rev-parse", "HEAD"]);
    let stock_clone = dir.path().join("stock-client-backend-v2-clone");
    let zmin_clone = dir.path().join("zmin-client-backend-v2-clone");
    let stock_backend = BackendHttpServer::new(
        required_pinned_stock_git().display().to_string(),
        dir.path().to_path_buf(),
    );
    let stock_url = format!("http://127.0.0.1:{}/remote.git", stock_backend.port);
    let stock_args = [
        "-c",
        "protocol.version=2",
        "clone",
        "-q",
        stock_url.as_str(),
        stock_clone.to_str().expect("stock v2 backend clone path"),
    ];
    let stock_exec_path = pinned_http_v2_exec_path(dir.path());
    let stock_output = pinned_command_any_output_with_env(
        dir.path(),
        &stock_args,
        &[(
            "GIT_EXEC_PATH",
            stock_exec_path.to_str().expect("HTTP helper path"),
        )],
        "stock backend v2 clone",
    );
    let zmin_backend = BackendHttpServer::new(zmin_bin().to_owned(), dir.path().to_path_buf());
    let zmin_url = format!("http://127.0.0.1:{}/remote.git", zmin_backend.port);
    let zmin_args = [
        "clone",
        "-c",
        "protocol.version=2",
        "-q",
        zmin_url.as_str(),
        zmin_clone.to_str().expect("zmin v2 backend clone path"),
    ];
    let zmin_output =
        command_any_output(zmin_bin(), dir.path(), &zmin_args, "zmin backend v2 clone");
    assert_eq!(zmin_output, stock_output);
    let expected_refs = pinned_git_args(&stock_clone, ["show-ref"]);
    assert_sha256_http_repository_state(&stock_clone, &expected_head, &expected_refs);
    assert_sha256_http_repository_state(&zmin_clone, &expected_head, &expected_refs);
    assert_eq!(
        sha256_http_storage_shape(&stock_clone),
        sha256_http_storage_shape(&zmin_clone),
        "SHA-256 backend v2 object storage layout"
    );
    for server in [&stock_backend, &zmin_backend] {
        let headers = server.request_headers_text();
        assert!(
            headers
                .iter()
                .any(|request| request.contains("Git-Protocol: version=2\r\n")),
            "backend v2 clone must receive the exact protocol header: {headers:?}"
        );
        assert!(
            headers.iter().any(|request| {
                request.contains("Content-Type: application/x-git-upload-pack-request\r\n")
                    && request.contains("Accept: application/x-git-upload-pack-result\r\n")
            }),
            "backend v2 clone must receive upload-pack content negotiation headers: {headers:?}"
        );
    }
}

fn assert_sha256_backend_fetch_state(repository: &std::path::Path, expected_head: &str) {
    assert_eq!(
        pinned_git_args(repository, ["rev-parse", "--show-object-format"]),
        "sha256"
    );
    assert_eq!(
        pinned_git_args(repository, ["rev-parse", "refs/remotes/origin/main"]),
        expected_head
    );
    let (pack_count, loose_count) = sha256_http_storage_shape(repository);
    assert!(
        pack_count > 0 || loose_count > 0,
        "SHA-256 backend fetch should materialize objects"
    );
}

fn run_sha256_backend_fetch_case(
    root: &std::path::Path,
    expected_head: &str,
    label: &str,
    protocol_v2: bool,
) {
    let stock_client = root.join(format!("stock-backend-fetch-{label}"));
    let zmin_client = root.join(format!("zmin-backend-fetch-{label}"));
    pinned_git_args(
        root,
        [
            "init",
            "--object-format=sha256",
            stock_client.to_str().expect("stock backend fetch path"),
        ],
    );
    run_zmin(
        root,
        [
            "init",
            "--object-format=sha256",
            zmin_client.to_str().expect("zmin backend fetch path"),
        ],
    );
    let stock_backend = BackendHttpServer::new(
        required_pinned_stock_git().display().to_string(),
        root.to_path_buf(),
    );
    let stock_url = format!("http://127.0.0.1:{}/remote.git", stock_backend.port);
    pinned_git_args(
        &stock_client,
        ["remote", "add", "origin", stock_url.as_str()],
    );
    let zmin_backend = BackendHttpServer::new(zmin_bin().to_owned(), root.to_path_buf());
    let zmin_url = format!("http://127.0.0.1:{}/remote.git", zmin_backend.port);
    run_zmin(&zmin_client, ["remote", "add", "origin", zmin_url.as_str()]);
    let version = if protocol_v2 { "2" } else { "0" };
    pinned_git_args(&stock_client, ["config", "protocol.version", version]);
    run_zmin(&zmin_client, ["config", "protocol.version", version]);
    let stock_args = ["fetch", "-q", "origin", "main"];
    let stock_output = if protocol_v2 {
        let stock_exec_path = pinned_http_v2_exec_path(root);
        pinned_command_any_output_with_env(
            &stock_client,
            &stock_args,
            &[(
                "GIT_EXEC_PATH",
                stock_exec_path.to_str().expect("HTTP helper path"),
            )],
            "stock backend SHA-256 v2 fetch",
        )
    } else {
        pinned_command_any_output(&stock_client, &stock_args, "stock backend SHA-256 v0 fetch")
    };
    let zmin_output = command_any_output(
        zmin_bin(),
        &zmin_client,
        &stock_args,
        "zmin backend SHA-256 fetch",
    );
    assert_eq!(zmin_output, stock_output, "backend SHA-256 {label} fetch");
    assert_sha256_backend_fetch_state(&stock_client, expected_head);
    assert_sha256_backend_fetch_state(&zmin_client, expected_head);
    for server in [&stock_backend, &zmin_backend] {
        let headers = server.request_headers_text();
        if protocol_v2 {
            assert!(
                headers
                    .iter()
                    .any(|request| request.contains("Git-Protocol: version=2\r\n")),
                "backend v2 fetch must receive the exact protocol header: {headers:?}"
            );
        } else {
            assert!(
                headers
                    .iter()
                    .all(|request| !request.contains("Git-Protocol:")),
                "backend v0 fetch unexpectedly negotiated protocol v2: {headers:?}"
            );
        }
        assert!(
            headers.iter().any(|request| {
                request.contains("Content-Type: application/x-git-upload-pack-request\r\n")
                    && request.contains("Accept: application/x-git-upload-pack-result\r\n")
            }),
            "backend SHA-256 fetch must receive upload-pack headers: {headers:?}"
        );
    }
}

#[test]
fn fetch_sha256_stock_client_to_zmin_http_backend_v0_and_v2_matches_stock_backend() {
    let dir = TempDir::new().expect("SHA-256 backend fetch temp dir");
    prepare_sha256_smart_http_remote(dir.path());
    let expected_head = pinned_git_args(&dir.path().join("sha256-work"), ["rev-parse", "HEAD"]);
    run_sha256_backend_fetch_case(dir.path(), &expected_head, "v0", false);
    run_sha256_backend_fetch_case(dir.path(), &expected_head, "v2", true);
}

#[test]
fn clone_sha256_smart_http_depth_v0_and_v2_preserve_shallow_boundaries() {
    // The large remote keeps indexed-pack behavior in scope; small-pack
    // unpack policy is intentionally outside this test.
    let dir = TempDir::new().expect("SHA-256 shallow clone temp dir");
    prepare_sha256_smart_http_remote(dir.path());
    let expected_head = pinned_git_args(&dir.path().join("sha256-work"), ["rev-parse", "HEAD"]);
    let stock_clone = dir.path().join("stock-sha256-depth-v0");
    let zmin_clone = dir.path().join("zmin-sha256-depth-v2");
    let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
    let stock_url = format!("http://127.0.0.1:{}/remote.git", stock_server.port);
    let stock_args = [
        "-c",
        "protocol.version=0",
        "clone",
        "-q",
        "--depth=1",
        stock_url.as_str(),
        stock_clone.to_str().expect("stock shallow clone path"),
    ];
    let stock_output =
        pinned_command_any_output(dir.path(), &stock_args, "stock SHA-256 depth v0 clone");
    let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
    let zmin_url = format!("http://127.0.0.1:{}/remote.git", zmin_server.port);
    let zmin_args = [
        "clone",
        "-c",
        "protocol.version=2",
        "-q",
        "--depth=1",
        "--server-option=protocol-test",
        zmin_url.as_str(),
        zmin_clone.to_str().expect("zmin shallow clone path"),
    ];
    let zmin_output = command_any_output(
        zmin_bin(),
        dir.path(),
        &zmin_args,
        "zmin SHA-256 depth v2 clone",
    );
    assert_eq!(zmin_output, stock_output);
    assert_sha256_http_repository_state(
        &stock_clone,
        &expected_head,
        &pinned_git_args(&stock_clone, ["show-ref"]),
    );
    assert_sha256_http_repository_state(
        &zmin_clone,
        &expected_head,
        &pinned_git_args(&zmin_clone, ["show-ref"]),
    );
    assert_eq!(
        pinned_git_args(&stock_clone, ["rev-parse", "--is-shallow-repository"]),
        "true"
    );
    assert_eq!(
        pinned_git_args(&zmin_clone, ["rev-parse", "--is-shallow-repository"]),
        "true"
    );
    assert_eq!(
        fs::read(stock_clone.join(".git/shallow")).expect("stock shallow file"),
        fs::read(zmin_clone.join(".git/shallow")).expect("zmin shallow file")
    );
    let zmin_headers = zmin_server.request_headers_text();
    assert!(
        zmin_headers
            .iter()
            .any(|headers| headers.contains("Git-Protocol: version=2\r\n")),
        "v2 shallow clone must send the exact Git-Protocol header: {zmin_headers:?}"
    );
    assert!(
        zmin_headers.iter().any(|headers| {
            headers.contains("Content-Type: application/x-git-upload-pack-request\r\n")
                && headers.contains("Accept: application/x-git-upload-pack-result\r\n")
        }),
        "v2 shallow clone must send upload-pack content negotiation headers: {zmin_headers:?}"
    );
    assert!(
        zmin_server.upload_pack_bodies_text().iter().any(|body| {
            body.contains("deepen 1")
                && body.contains("object-format=sha256")
                && body.contains("server-option=protocol-test")
        }),
        "v2 shallow clone must send depth, object format, and server option"
    );
}

#[test]
fn fetch_sha256_smart_http_configured_v2_depth_deepen_unshallow_matches_stock_git() {
    let dir = TempDir::new().expect("SHA-256 configured shallow temp dir");
    prepare_sha256_smart_http_remote(dir.path());
    let stock_client = dir.path().join("stock-configured-sha256");
    let zmin_client = dir.path().join("zmin-configured-sha256");
    pinned_git_args(
        dir.path(),
        [
            "init",
            "--object-format=sha256",
            stock_client.to_str().expect("stock client path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "init",
            "--object-format=sha256",
            zmin_client.to_str().expect("zmin client path"),
        ],
    );
    let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
    let stock_url = format!("http://127.0.0.1:{}/remote.git", stock_server.port);
    pinned_git_args(
        &stock_client,
        ["remote", "add", "origin", stock_url.as_str()],
    );
    let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
    let zmin_url = format!("http://127.0.0.1:{}/remote.git", zmin_server.port);
    run_zmin(&zmin_client, ["remote", "add", "origin", zmin_url.as_str()]);
    for client in [&stock_client, &zmin_client] {
        if client == &stock_client {
            pinned_git_args(client, ["config", "protocol.version", "2"]);
        } else {
            run_zmin(client, ["config", "protocol.version", "2"]);
        }
    }

    let stock_depth = pinned_command_any_output(
        &stock_client,
        &["fetch", "-q", "--depth=1", "origin"],
        "stock configured v2 depth",
    );
    let zmin_depth = command_any_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "-q", "--depth=1", "origin"],
        "zmin configured v2 depth",
    );
    assert_eq!(zmin_depth, stock_depth);

    let stock_deepen = pinned_command_any_output(
        &stock_client,
        &["fetch", "-q", "--deepen=1", "origin"],
        "stock configured v2 deepen",
    );
    let zmin_deepen = command_any_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "-q", "--deepen=1", "origin"],
        "zmin configured v2 deepen",
    );
    assert_eq!(zmin_deepen, stock_deepen);

    let stock_unshallow = pinned_command_any_output(
        &stock_client,
        &["fetch", "-q", "--unshallow", "origin"],
        "stock configured v2 unshallow",
    );
    let zmin_unshallow = command_any_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "-q", "--unshallow", "origin"],
        "zmin configured v2 unshallow",
    );
    assert_eq!(zmin_unshallow, stock_unshallow);
    assert_eq!(
        pinned_git_args(&stock_client, ["show-ref"]),
        pinned_git_args(&zmin_client, ["show-ref"])
    );
    assert_eq!(
        pinned_git_args(&stock_client, ["rev-parse", "--show-object-format"]),
        "sha256"
    );
    assert_eq!(
        pinned_git_args(&zmin_client, ["rev-parse", "--show-object-format"]),
        "sha256"
    );
    assert!(
        stock_client.join(".git/shallow").exists() == zmin_client.join(".git/shallow").exists()
    );
    assert!(stock_server.git_protocol_requests() > 0);
    assert!(zmin_server.git_protocol_requests() > 0);
    assert!(
        stock_server
            .upload_pack_bodies_text()
            .iter()
            .any(|body| body.contains("object-format=sha256") && body.contains("deepen"))
    );
    assert!(
        zmin_server
            .upload_pack_bodies_text()
            .iter()
            .any(|body| body.contains("object-format=sha256") && body.contains("deepen"))
    );
}

#[test]
fn clone_empty_smart_http_reftable_sha1_and_sha256_initializes_matching_stacks() {
    let dir = TempDir::new().expect("empty reftable temp dir");
    for (label, object_format) in [("sha1", None), ("sha256", Some("sha256"))] {
        let remote_name = format!("empty-{label}.git");
        let remote = dir.path().join(&remote_name);
        if let Some(object_format) = object_format {
            pinned_git_args(
                dir.path(),
                [
                    "init",
                    "--bare",
                    &format!("--object-format={object_format}"),
                    &remote_name,
                ],
            );
        } else {
            pinned_git_args(dir.path(), ["init", "--bare", &remote_name]);
        }
        fs::write(remote.join("git-daemon-export-ok"), b"").expect("empty reftable export marker");
        let stock_server = SmartHttpServer::new(dir.path().to_path_buf());
        let stock_url = format!("http://127.0.0.1:{}/{}", stock_server.port, remote_name);
        let stock_clone = dir.path().join(format!("stock-empty-{label}"));
        let mut stock_args = vec![
            "clone".to_owned(),
            "--ref-format=reftable".to_owned(),
            "--no-checkout".to_owned(),
            "-q".to_owned(),
            stock_url.clone(),
            stock_clone.display().to_string(),
        ];
        let stock_output = pinned_command_any_output(
            dir.path(),
            &stock_args.iter().map(String::as_str).collect::<Vec<_>>(),
            &format!("stock empty {label} reftable clone"),
        );
        let zmin_server = SmartHttpServer::new(dir.path().to_path_buf());
        let zmin_url = format!("http://127.0.0.1:{}/{}", zmin_server.port, remote_name);
        stock_args[4] = zmin_url;
        let zmin_clone = dir.path().join(format!("zmin-empty-{label}"));
        stock_args[5] = zmin_clone.display().to_string();
        let zmin_output = command_any_output(
            zmin_bin(),
            dir.path(),
            &stock_args.iter().map(String::as_str).collect::<Vec<_>>(),
            &format!("zmin empty {label} reftable clone"),
        );
        assert_eq!(zmin_output, stock_output, "empty {label} clone tuple");
        for repository in [&stock_clone, &zmin_clone] {
            assert_eq!(
                pinned_git_args(repository, ["rev-parse", "--show-ref-format"]),
                "reftable"
            );
            assert!(
                repository.join(".git/reftable/tables.list").is_file(),
                "missing empty reftable stack at {}",
                repository.display()
            );
            assert!(repository.join(".git/HEAD").is_file());
            if let Some(object_format) = object_format {
                assert_eq!(
                    pinned_git_args(repository, ["rev-parse", "--show-object-format"]),
                    object_format
                );
            }
        }
        drop(remote);
    }
}

#[test]
fn fetch_sha256_smart_http_filter_v0_branch_and_configured_match_stock_git() {
    for (label, branch) in [("branch", Some("main")), ("configured", None)] {
        let dir = TempDir::new().expect("SHA-256 filter temp dir");
        let remote = prepare_sha256_smart_http_remote(dir.path());
        pinned_git_args(&remote, ["config", "uploadpack.allowFilter", "true"]);
        let stock_client = dir.path().join(format!("stock-filter-{label}"));
        let zmin_client = dir.path().join(format!("zmin-filter-{label}"));
        let server = SmartHttpServer::new(dir.path().to_path_buf());
        let url = format!("http://127.0.0.1:{}/remote.git", server.port);
        init_sha256_filter_client(dir.path(), &stock_client, &url, true);
        init_sha256_filter_client(dir.path(), &zmin_client, &url, false);
        let mut args = vec!["fetch", "-q", "--filter=blob:none", "origin"];
        if let Some(branch) = branch {
            args.push(branch);
        }

        let stock_body_start = server.upload_pack_bodies_text().len();
        let stock_header_start = server.request_headers_text().len();
        let stock_protocol_before = server.git_protocol_requests();
        let stock_upload_before = server.upload_pack_requests();
        let stock_output = pinned_command_any_output(
            &stock_client,
            &args,
            &format!("pinned SHA-256 filter v0 {label}"),
        );
        assert_eq!(stock_output.0, 0, "pinned SHA-256 filter v0 {label}");
        assert_sha256_filter_v0_wire(
            &server,
            &format!("stock {label}"),
            stock_body_start,
            stock_header_start,
            stock_protocol_before,
            stock_upload_before,
        );

        let zmin_body_start = server.upload_pack_bodies_text().len();
        let zmin_header_start = server.request_headers_text().len();
        let zmin_protocol_before = server.git_protocol_requests();
        let zmin_upload_before = server.upload_pack_requests();
        let zmin_output = command_any_output(
            zmin_bin(),
            &zmin_client,
            &args,
            &format!("Zmin SHA-256 filter v0 {label}"),
        );
        assert_eq!(zmin_output, stock_output, "SHA-256 filter v0 {label} tuple");
        assert_sha256_filter_v0_wire(
            &server,
            &format!("Zmin {label}"),
            zmin_body_start,
            zmin_header_start,
            zmin_protocol_before,
            zmin_upload_before,
        );
        assert_sha256_filter_fetch_state(
            &format!("SHA-256 filter v0 {label}"),
            &stock_client,
            &zmin_client,
            &remote,
            branch.is_none(),
        );
    }
}

#[test]
fn fetch_sha256_smart_http_filter_v0_rejects_sha1_before_mutation() {
    let dir = TempDir::new().expect("SHA-256 filter mismatch temp dir");
    let _remote = prepare_sha1_smart_http_mismatch_remote(dir.path());
    let client = dir.path().join("sha256-filter-mismatch-client");
    run_zmin(
        dir.path(),
        [
            "init",
            "--object-format=sha256",
            client.to_str().expect("filter mismatch client path"),
        ],
    );
    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/sha1-remote.git", server.port);
    run_zmin(&client, ["remote", "add", "origin", url.as_str()]);
    run_zmin(&client, ["config", "protocol.version", "0"]);
    let before = snapshot_http_repository(&client);
    let output = command_any_output(
        zmin_bin(),
        &client,
        &["fetch", "-q", "--filter=blob:none", "origin", "main"],
        "SHA-256 smart HTTP filter mismatch",
    );
    assert_eq!(output.0, 128);
    assert!(
        output
            .2
            .contains("mismatched algorithms: client sha256; server sha1"),
        "unexpected filter mismatch: {output:?}"
    );
    assert_eq!(snapshot_http_repository(&client), before);
    assert_eq!(server.upload_pack_requests(), 0);
    assert_eq!(server.git_protocol_requests(), 0);
}

fn init_sha256_filter_client(
    root: &std::path::Path,
    client: &std::path::Path,
    url: &str,
    pinned: bool,
) {
    let init_args = [
        "init",
        "--object-format=sha256",
        client.to_str().expect("SHA-256 filter client path"),
    ];
    if pinned {
        pinned_git_args(root, init_args);
        pinned_git_args(client, ["remote", "add", "origin", url]);
        pinned_git_args(client, ["config", "protocol.version", "0"]);
    } else {
        run_zmin(root, init_args);
        run_zmin(client, ["remote", "add", "origin", url]);
        run_zmin(client, ["config", "protocol.version", "0"]);
    }
}

fn assert_sha256_filter_v0_wire(
    server: &SmartHttpServer,
    label: &str,
    body_start: usize,
    header_start: usize,
    protocol_before: usize,
    upload_before: usize,
) {
    assert_eq!(
        server.git_protocol_requests() - protocol_before,
        0,
        "{label}: v0 unexpectedly sent Git-Protocol"
    );
    assert_eq!(
        server.upload_pack_requests() - upload_before,
        1,
        "{label}: expected exactly one v0 upload-pack POST"
    );
    let headers = server.request_headers_text();
    let headers = &headers[header_start..];
    assert_eq!(
        headers.len(),
        2,
        "{label}: expected one discovery GET and one POST"
    );
    assert!(
        headers
            .iter()
            .any(|headers| headers.starts_with("GET /remote.git/info/refs?service=git-upload-pack")),
        "{label}: missing v0 discovery GET: {headers:?}"
    );
    assert!(
        headers
            .iter()
            .any(|headers| headers.starts_with("POST /remote.git/git-upload-pack")),
        "{label}: missing v0 upload-pack POST: {headers:?}"
    );
    assert!(
        headers
            .iter()
            .all(|headers| !headers.contains("Git-Protocol:")),
        "{label}: v0 request unexpectedly carried Git-Protocol: {headers:?}"
    );
    let bodies = server.upload_pack_bodies_text();
    let bodies = &bodies[body_start..];
    assert_eq!(
        bodies.len(),
        1,
        "{label}: expected one captured v0 request body"
    );
    let body = &bodies[0];
    let want = body
        .split("want ")
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .expect("v0 filter request missing want");
    assert_eq!(want.len(), 64, "{label}: v0 want must be SHA-256");
    assert!(
        want.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{label}: v0 want is not hexadecimal: {want:?}"
    );
    assert!(
        body.contains("filter blob:none"),
        "{label}: missing filter: {body:?}"
    );
    assert!(
        !body.contains("command=ls-refs"),
        "{label}: v2 ls-refs leaked: {body:?}"
    );
    assert!(
        !body.contains("command=fetch"),
        "{label}: v2 fetch leaked: {body:?}"
    );
}

fn assert_sha256_filter_fetch_state(
    label: &str,
    stock: &std::path::Path,
    zmin: &std::path::Path,
    remote: &std::path::Path,
    configured: bool,
) {
    let expected_head = pinned_git_args(remote, ["rev-parse", "refs/heads/main"]);
    for repository in [stock, zmin] {
        assert_eq!(
            pinned_git_args(repository, ["rev-parse", "--show-object-format"]),
            "sha256",
            "{label}: object format"
        );
        assert_eq!(
            pinned_git_args(repository, ["rev-parse", "refs/remotes/origin/main"]),
            expected_head,
            "{label}: fetched ref"
        );
        assert_no_http_temporary_pack_entries(repository, label);
    }
    assert_eq!(
        pinned_git_args(stock, ["show-ref"]),
        pinned_git_args(zmin, ["show-ref"]),
        "{label}: refs"
    );
    assert_eq!(
        fs::read(stock.join(".git/FETCH_HEAD")).expect("stock FETCH_HEAD"),
        fs::read(zmin.join(".git/FETCH_HEAD")).expect("Zmin FETCH_HEAD"),
        "{label}: FETCH_HEAD"
    );
    for key in [
        "core.repositoryformatversion",
        "extensions.objectformat",
        "protocol.version",
        "remote.origin.promisor",
        "remote.origin.partialclonefilter",
    ] {
        assert_eq!(
            pinned_git_args(stock, ["config", "--get", key]),
            pinned_git_args(zmin, ["config", "--get", key]),
            "{label}: config {key}"
        );
    }
    assert_eq!(
        sha256_http_storage_shape(stock),
        sha256_http_storage_shape(zmin),
        "{label}: storage shape"
    );
    assert_eq!(
        http_pack_role_snapshot(stock),
        http_pack_role_snapshot(zmin),
        "{label}: pack roles"
    );
    assert_eq!(
        http_object_file_snapshot(stock),
        http_object_file_snapshot(zmin),
        "{label}: object/admin files"
    );
    assert_promisor_marker_roles_match(label, stock, zmin);
    if configured {
        assert!(
            pinned_git_args(stock, ["show-ref"])
                .lines()
                .any(|line| line.ends_with(" refs/remotes/origin/HEAD")),
            "{label}: configured fetch missing remote HEAD"
        );
    }
}

#[test]
fn smart_http_v2_capabilities_reject_invalid_object_formats_before_mutation() {
    let cases: &[(&str, &[&[u8]], &str)] = &[
        (
            "absent",
            &[b"version 2\n"],
            "the server does not support algorithm 'sha256'",
        ),
        (
            "duplicate",
            &[
                b"version 2\n",
                b"object-format=sha256\n",
                b"object-format=sha256\n",
            ],
            "HTTP ref request failed: HTTP/1.1 405 Method Not Allowed",
        ),
        (
            "conflicting",
            &[
                b"version 2\n",
                b"object-format=sha1\n",
                b"object-format=sha256\n",
            ],
            "mismatched algorithms: client sha256; server sha1",
        ),
        (
            "unsupported",
            &[b"version 2\n", b"object-format=sha512\n"],
            "unknown object format 'sha512' specified by server",
        ),
    ];
    for (label, capabilities, expected_message) in cases {
        let dir = TempDir::new().expect("invalid capability temp dir");
        let client = dir.path().join(format!("sha256-{label}-client"));
        pinned_git_args(
            dir.path(),
            [
                "init",
                "--object-format=sha256",
                client.to_str().expect("invalid capability client path"),
            ],
        );
        run_zmin(&client, ["config", "protocol.version", "2"]);
        fs::create_dir_all(dir.path().join("remote.git/info"))
            .expect("invalid capability info directory");
        fs::write(
            dir.path().join("remote.git/info/refs"),
            static_smart_http_v2_capabilities(capabilities),
        )
        .expect("invalid capability advertisement");
        let server = StaticHttpServer::new(dir.path().to_path_buf());
        let url = format!("http://127.0.0.1:{}/remote.git", server.port);
        pinned_git_args(&client, ["remote", "add", "origin", url.as_str()]);
        let before = snapshot_http_repository(&client);
        let output = command_any_output(
            zmin_bin(),
            &client,
            &["fetch", "-q", "origin", "main"],
            "invalid smart HTTP object-format capability",
        );
        assert_eq!(output.0, 128, "{label}");
        assert!(output.1.is_empty(), "{label}: unexpected stdout");
        assert_eq!(output.2, format!("fatal: {expected_message}"), "{label}");
        assert_eq!(
            snapshot_http_repository(&client),
            before,
            "{label}: mutation"
        );
    }
}

#[test]
fn smart_http_fetch_rejects_sha1_sha256_mismatch_before_mutation() {
    let dir = TempDir::new().expect("smart HTTP mismatch temp dir");
    let sha256_remote = prepare_sha256_smart_http_remote(dir.path());
    let sha1_remote = prepare_sha1_smart_http_mismatch_remote(dir.path());
    let sha1_client = dir.path().join("sha1-mismatch-client");
    let sha256_client = dir.path().join("sha256-mismatch-client");
    pinned_git_args(dir.path(), ["init", "sha1-mismatch-client"]);
    pinned_git_args(
        dir.path(),
        ["init", "--object-format=sha256", "sha256-mismatch-client"],
    );

    let sha256_server = SmartHttpServer::new(dir.path().to_path_buf());
    let sha256_url = format!("http://127.0.0.1:{}/remote.git", sha256_server.port);
    pinned_git_args(
        &sha1_client,
        ["remote", "add", "origin", sha256_url.as_str()],
    );
    let sha1_before = snapshot_http_repository(&sha1_client);
    let sha1_refs_before =
        pinned_command_any_output(&sha1_client, &["show-ref"], "SHA-1 refs before mismatch").1;
    let sha1_output = command_any_output(
        zmin_bin(),
        &sha1_client,
        &["fetch", "-q", "origin", "main"],
        "SHA-1 client against SHA-256 HTTP remote",
    );
    assert_eq!(
        sha1_output,
        (
            128,
            String::new(),
            "fatal: mismatched algorithms: client sha1; server sha256".to_owned(),
        )
    );
    assert_eq!(
        pinned_command_any_output(&sha1_client, &["show-ref"], "SHA-1 refs after mismatch").1,
        sha1_refs_before
    );
    assert_eq!(snapshot_http_repository(&sha1_client), sha1_before);

    let sha1_server = SmartHttpServer::new(dir.path().to_path_buf());
    let sha1_url = format!("http://127.0.0.1:{}/sha1-remote.git", sha1_server.port);
    pinned_git_args(
        &sha256_client,
        ["remote", "add", "origin", sha1_url.as_str()],
    );
    let sha256_before = snapshot_http_repository(&sha256_client);
    let sha256_refs_before = pinned_command_any_output(
        &sha256_client,
        &["show-ref"],
        "SHA-256 refs before mismatch",
    )
    .1;
    let sha256_output = command_any_output(
        zmin_bin(),
        &sha256_client,
        &["fetch", "-q", "origin", "main"],
        "SHA-256 client against SHA-1 HTTP remote",
    );
    assert_eq!(
        sha256_output,
        (
            128,
            String::new(),
            "fatal: mismatched algorithms: client sha256; server sha1".to_owned(),
        )
    );
    assert_eq!(
        pinned_command_any_output(&sha256_client, &["show-ref"], "SHA-256 refs after mismatch").1,
        sha256_refs_before
    );
    assert_eq!(snapshot_http_repository(&sha256_client), sha256_before);

    assert!(sha256_remote.exists());
    assert!(sha1_remote.exists());
}

#[test]
fn clone_instant_smart_http_materializes_head_then_fetch_hydrates_refs() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let zmin_clone = dir.path().join("zmin-http-instant");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join(".gitattributes"), b"crlf.txt -text\n").expect("write attributes");
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    fs::write(work.join("crlf.txt"), b"line one\r\nline two\r\n").expect("write crlf");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["switch", "-c", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    git(&work, ["switch", "main"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "release"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    run_zmin(
        dir.path(),
        [
            "clone",
            "--instant",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&work, ["rev-parse", "main"])
    );
    assert_eq!(
        fs::read(zmin_clone.join("crlf.txt")).expect("zmin crlf"),
        fs::read(work.join("crlf.txt")).expect("source crlf")
    );
    let initial_refs = git(&zmin_clone, ["show-ref"]);
    assert!(
        initial_refs
            .lines()
            .any(|line| line.ends_with(" refs/remotes/origin/main")),
        "instant clone should write the fetched HEAD branch ref:\n{initial_refs}"
    );
    assert!(
        !initial_refs
            .lines()
            .any(|line| line.ends_with(" refs/remotes/origin/feature")),
        "instant clone should not write refs for objects it did not request:\n{initial_refs}"
    );
    assert!(
        !initial_refs
            .lines()
            .any(|line| line.ends_with(" refs/tags/v1")),
        "instant clone should leave non-target tags for later fetch:\n{initial_refs}"
    );

    run_zmin(&zmin_clone, ["fetch", "origin"]);
    let hydrated_refs = git(&zmin_clone, ["show-ref"]);
    assert!(
        hydrated_refs
            .lines()
            .any(|line| line.ends_with(" refs/remotes/origin/feature")),
        "fetch should hydrate additional remote branch refs:\n{hydrated_refs}"
    );
    assert!(
        hydrated_refs
            .lines()
            .any(|line| line.ends_with(" refs/tags/v1")),
        "fetch should hydrate followed tag refs:\n{hydrated_refs}"
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
}

#[test]
fn clone_instant_smart_http_demand_hydrate_recovers_missing_head_objects() {
    ensure_remote_http_helper();

    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let zmin_clone = dir.path().join("zmin-http-instant-demand");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    run_zmin(
        dir.path(),
        [
            "clone",
            "--instant",
            "--demand-hydrate",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_demand_hydrate_config(&zmin_clone);
    let head = git(&zmin_clone, ["rev-parse", "HEAD"]);
    remove_all_pack_files(&zmin_clone);

    assert_eq!(run_zmin(&zmin_clone, ["cat-file", "-t", "HEAD"]), "commit");
    assert_eq!(run_zmin(&zmin_clone, ["cat-file", "-t", &head]), "commit");
    git(&zmin_clone, ["fsck", "--strict"]);
}

#[test]
fn clone_worktree_first_smart_http_demand_hydrate_recovers_missing_head_objects() {
    ensure_remote_http_helper();

    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let zmin_clone = dir.path().join("zmin-http-worktree-first-demand");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    run_zmin(
        dir.path(),
        [
            "clone",
            "--worktree-first",
            "--demand-hydrate",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_demand_hydrate_config(&zmin_clone);
    let head = git(&zmin_clone, ["rev-parse", "HEAD"]);
    remove_all_pack_files(&zmin_clone);

    assert_eq!(run_zmin(&zmin_clone, ["cat-file", "-t", "HEAD"]), "commit");
    assert_eq!(run_zmin(&zmin_clone, ["cat-file", "-t", &head]), "commit");
    git(&zmin_clone, ["fsck", "--strict"]);
}

#[test]
fn clone_instant_smart_http_background_fetch_hydrates_refs() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let zmin_clone = dir.path().join("zmin-http-instant-background");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["switch", "-c", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    git(&work, ["switch", "main"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "release"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    run_zmin(
        dir.path(),
        [
            "clone",
            "--instant",
            "--background-fetch",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_background_fetch_hydrated(&zmin_clone);
}

#[test]
fn clone_worktree_first_smart_http_background_fetch_hydrates_refs() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let zmin_clone = dir.path().join("zmin-http-worktree-first-background");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("README.md"), b"main\n").expect("write readme");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["switch", "-c", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    git(&work, ["switch", "main"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "release"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    run_zmin(
        dir.path(),
        [
            "clone",
            "--worktree-first",
            "--background-fetch",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_background_fetch_hydrated(&zmin_clone);
}

#[test]
fn clone_reads_shallow_smart_http_pack_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_clone = dir.path().join("git-smart-clone");
    let zmin_clone = dir.path().join("zmin-smart-clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    for idx in 1..=3 {
        fs::write(work.join("a.txt"), format!("commit {idx}\n")).expect("write a");
        git(&work, ["add", "-A"]);
        git_with_env(&work, ["commit", "-m", &format!("commit {idx}")]);
    }
    let parent = git(&work, ["rev-parse", "HEAD^"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    git(
        dir.path(),
        [
            "clone",
            "--depth=1",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--depth=1",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&git_clone, ["rev-parse", "HEAD"])
    );
    assert_matching_shallow_state(&zmin_clone, &git_clone, &parent);
}

#[test]
fn clone_reads_shallow_smart_http_tags_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_clone = dir.path().join("git-smart-clone");
    let zmin_clone = dir.path().join("zmin-smart-clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    for idx in 1..=3 {
        fs::write(work.join("a.txt"), format!("commit {idx}\n")).expect("write a");
        git(&work, ["add", "-A"]);
        git_with_env(&work, ["commit", "-m", &format!("commit {idx}")]);
    }
    let first = git(&work, ["rev-parse", "HEAD~2"]);
    git_with_env(&work, ["tag", "-a", "v0.1", "-m", "old tag", &first]);
    git_with_env(&work, ["tag", "-a", "v0.2", "-m", "tip tag"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "--tags"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    git(
        dir.path(),
        [
            "clone",
            "--depth=1",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--depth=1",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        git(&zmin_clone, ["show-ref"]),
        git(&git_clone, ["show-ref"])
    );
    assert_eq!(
        git(&zmin_clone, ["tag", "-l"]),
        git(&git_clone, ["tag", "-l"])
    );
}

#[test]
fn clone_shared_is_ignored_for_smart_http_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_clone = dir.path().join("git-smart-clone");
    let zmin_clone = dir.path().join("zmin-smart-clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    git(
        dir.path(),
        [
            "clone",
            "--shared",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--shared",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );
    assert_eq!(
        git(&zmin_clone, ["show-ref"]),
        git(&git_clone, ["show-ref"])
    );
    assert_no_alternates(&git_clone);
    assert_no_alternates(&zmin_clone);
}

#[test]
fn fetch_reads_smart_http_pack_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-smart-fetch");
    let zmin_client = dir.path().join("zmin-smart-fetch");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    fs::write(work.join("b.txt"), b"second\n").expect("write b");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "second"]);
    git(&work, ["branch", "feature"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    git(dir.path(), ["init", "git-smart-fetch"]);
    git(dir.path(), ["init", "zmin-smart-fetch"]);
    git(&git_client, ["remote", "add", "origin", url.as_str()]);
    run_zmin(&zmin_client, ["remote", "add", "origin", url.as_str()]);
    git(&git_client, ["fetch", "origin"]);
    run_zmin(&zmin_client, ["fetch", "origin"]);

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "origin/main:b.txt"]),
        git(&git_client, ["cat-file", "-p", "origin/main:b.txt"])
    );
    assert_eq!(
        sha256_http_storage_shape(&zmin_client),
        sha256_http_storage_shape(&git_client),
        "smart HTTP fetch storage shape"
    );
}

#[test]
fn fetch_smart_http_wildcard_refspec_updates_remote_refs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-smart-fetch-wildcard");
    let zmin_client = dir.path().join("zmin-smart-fetch-wildcard");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("main.txt"), b"main\n").expect("write main");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "main"]);
    git(&work, ["checkout", "-b", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    git(&work, ["checkout", "-b", "topic", "main"]);
    fs::write(work.join("topic.txt"), b"topic\n").expect("write topic");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "topic"]);
    git(&work, ["tag", "v1"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "topic"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    git(dir.path(), ["init", "git-smart-fetch-wildcard"]);
    git(dir.path(), ["init", "zmin-smart-fetch-wildcard"]);
    git(&git_client, ["remote", "add", "origin", url.as_str()]);
    run_zmin(&zmin_client, ["remote", "add", "origin", url.as_str()]);

    let refspec = "+refs/heads/*:refs/remotes/origin/*";
    git(
        &git_client,
        ["fetch", "origin", refspec, "--prune", "--no-tags"],
    );
    run_zmin(
        &zmin_client,
        ["fetch", "origin", refspec, "--prune", "--no-tags"],
    );

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        )
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "origin/topic:topic.txt"]),
        git(&git_client, ["cat-file", "-p", "origin/topic:topic.txt"])
    );
}

#[test]
fn fetch_smart_http_incremental_thin_pack_repairs_existing_bases_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-smart-fetch");
    let zmin_client = dir.path().join("zmin-smart-fetch");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("delta.txt"), b"line 1\nline 2\nline 3\n").expect("write base");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    git(dir.path(), ["init", "git-smart-fetch"]);
    git(dir.path(), ["init", "zmin-smart-fetch"]);
    git(&git_client, ["remote", "add", "origin", url.as_str()]);
    run_zmin(&zmin_client, ["remote", "add", "origin", url.as_str()]);
    git(&git_client, ["fetch", "origin"]);
    run_zmin(&zmin_client, ["fetch", "origin"]);

    fs::write(
        work.join("delta.txt"),
        b"line 1\nline 2 changed over smart http\nline 3\n",
    )
    .expect("write changed base");
    fs::write(work.join("new.txt"), b"new file\n").expect("write new");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "incremental"]);
    git(&work, ["push", "-q", "origin", "main"]);

    git(&git_client, ["fetch", "origin"]);
    run_zmin(&zmin_client, ["fetch", "origin"]);

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "origin/main:delta.txt"]),
        git(&git_client, ["cat-file", "-p", "origin/main:delta.txt"])
    );
    git(&zmin_client, ["fsck", "--strict"]);
}

#[test]
fn fetch_smart_http_noop_skips_upload_pack_when_roots_exist_locally() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let zmin_client = dir.path().join("zmin-smart-fetch");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("alpha.txt"), b"alpha\n").expect("write alpha");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["tag", "-a", "v1", "-m", "v1"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "--tags"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    git(dir.path(), ["init", "zmin-smart-fetch"]);
    run_zmin(&zmin_client, ["remote", "add", "origin", url.as_str()]);

    run_zmin(&zmin_client, ["fetch", "origin"]);
    let first_upload_pack_requests = server.upload_pack_requests();
    assert!(
        first_upload_pack_requests > 0,
        "initial smart HTTP fetch should request a pack"
    );
    let refs_after_first_fetch = git(&zmin_client, ["show-ref"]);

    run_zmin(&zmin_client, ["fetch", "origin"]);
    assert_eq!(
        server.upload_pack_requests(),
        first_upload_pack_requests,
        "noop smart HTTP fetch should not request a pack when advertised roots already exist"
    );
    assert_eq!(git(&zmin_client, ["show-ref"]), refs_after_first_fetch);
    git(&zmin_client, ["fsck", "--strict"]);
}

#[test]
fn fetch_server_option_protocol_v2_smart_http_branch_matches_stock_git() {
    assert_server_option_protocol_v2_smart_http_branch_matches_stock_git(
        "equals",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option=trace",
            "origin",
            "main",
        ],
        &["fetch", "--server-option=trace", "origin", "main"],
        &["server-option=trace"],
    );
}

#[test]
fn fetch_server_option_protocol_v2_smart_http_branchless_matches_stock_git() {
    assert_server_option_protocol_v2_smart_http_branch_matches_stock_git(
        "branchless",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option=trace",
            "origin",
        ],
        &["fetch", "--server-option=trace", "origin"],
        &["server-option=trace"],
    );
}

#[test]
fn fetch_server_option_separate_protocol_v2_smart_http_branch_matches_stock_git() {
    assert_server_option_protocol_v2_smart_http_branch_matches_stock_git(
        "separate",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option",
            "trace",
            "origin",
            "main",
        ],
        &["fetch", "--server-option", "trace", "origin", "main"],
        &["server-option=trace"],
    );
}

#[test]
fn fetch_server_option_separate_protocol_v2_smart_http_branchless_matches_stock_git() {
    assert_server_option_protocol_v2_smart_http_branch_matches_stock_git(
        "separate-branchless",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option",
            "trace",
            "origin",
        ],
        &["fetch", "--server-option", "trace", "origin"],
        &["server-option=trace"],
    );
}

#[test]
fn fetch_server_option_repeated_protocol_v2_smart_http_branch_matches_stock_git() {
    assert_server_option_protocol_v2_smart_http_branch_matches_stock_git(
        "repeated",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option=trace",
            "--server-option=mode=full",
            "origin",
            "main",
        ],
        &[
            "fetch",
            "--server-option=trace",
            "--server-option=mode=full",
            "origin",
            "main",
        ],
        &["server-option=trace", "server-option=mode=full"],
    );
}

#[test]
fn fetch_server_option_repeated_protocol_v2_smart_http_branchless_matches_stock_git() {
    assert_server_option_protocol_v2_smart_http_branch_matches_stock_git(
        "repeated-branchless",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option=trace",
            "--server-option=mode=full",
            "origin",
        ],
        &[
            "fetch",
            "--server-option=trace",
            "--server-option=mode=full",
            "origin",
        ],
        &["server-option=trace", "server-option=mode=full"],
    );
}

#[test]
fn fetch_server_option_protocol_v2_ssh_branch_matches_stock_git() {
    assert_server_option_protocol_v2_ssh_branch_matches_stock_git(
        "equals",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option=trace",
            "origin",
            "main",
        ],
        &["fetch", "--server-option=trace", "origin", "main"],
        &["server-option=trace"],
    );
}

#[test]
fn fetch_server_option_protocol_v2_ssh_branchless_matches_stock_git() {
    assert_server_option_protocol_v2_ssh_branch_matches_stock_git(
        "branchless",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option=trace",
            "origin",
        ],
        &["fetch", "--server-option=trace", "origin"],
        &["server-option=trace"],
    );
}

#[test]
fn fetch_server_option_separate_protocol_v2_ssh_branch_matches_stock_git() {
    assert_server_option_protocol_v2_ssh_branch_matches_stock_git(
        "separate",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option",
            "trace",
            "origin",
            "main",
        ],
        &["fetch", "--server-option", "trace", "origin", "main"],
        &["server-option=trace"],
    );
}

#[test]
fn fetch_server_option_separate_protocol_v2_ssh_branchless_matches_stock_git() {
    assert_server_option_protocol_v2_ssh_branch_matches_stock_git(
        "separate-branchless",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option",
            "trace",
            "origin",
        ],
        &["fetch", "--server-option", "trace", "origin"],
        &["server-option=trace"],
    );
}

#[test]
fn fetch_server_option_repeated_protocol_v2_ssh_branch_matches_stock_git() {
    assert_server_option_protocol_v2_ssh_branch_matches_stock_git(
        "repeated",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option=trace",
            "--server-option=mode=full",
            "origin",
            "main",
        ],
        &[
            "fetch",
            "--server-option=trace",
            "--server-option=mode=full",
            "origin",
            "main",
        ],
        &["server-option=trace", "server-option=mode=full"],
    );
}

#[test]
fn fetch_server_option_repeated_protocol_v2_ssh_branchless_matches_stock_git() {
    assert_server_option_protocol_v2_ssh_branch_matches_stock_git(
        "repeated-branchless",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option=trace",
            "--server-option=mode=full",
            "origin",
        ],
        &[
            "fetch",
            "--server-option=trace",
            "--server-option=mode=full",
            "origin",
        ],
        &["server-option=trace", "server-option=mode=full"],
    );
}

#[test]
fn fetch_server_option_protocol_v2_git_daemon_branchless_matches_stock_git() {
    assert_server_option_protocol_v2_git_daemon_matches_stock_git(
        "branchless",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option=trace",
            "origin",
        ],
        &["fetch", "--server-option=trace", "origin"],
        &["server-option=trace"],
    );
}

#[test]
fn fetch_server_option_protocol_v2_git_daemon_branch_matches_stock_git() {
    assert_server_option_protocol_v2_git_daemon_matches_stock_git(
        "branch",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option=trace",
            "origin",
            "main",
        ],
        &["fetch", "--server-option=trace", "origin", "main"],
        &["server-option=trace"],
    );
}

#[test]
fn fetch_server_option_separate_protocol_v2_git_daemon_branch_matches_stock_git() {
    assert_server_option_protocol_v2_git_daemon_matches_stock_git(
        "separate-branch",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option",
            "trace",
            "origin",
            "main",
        ],
        &["fetch", "--server-option", "trace", "origin", "main"],
        &["server-option=trace"],
    );
}

#[test]
fn fetch_server_option_separate_protocol_v2_git_daemon_branchless_matches_stock_git() {
    assert_server_option_protocol_v2_git_daemon_matches_stock_git(
        "separate-branchless",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option",
            "trace",
            "origin",
        ],
        &["fetch", "--server-option", "trace", "origin"],
        &["server-option=trace"],
    );
}

#[test]
fn fetch_server_option_repeated_protocol_v2_git_daemon_branch_matches_stock_git() {
    assert_server_option_protocol_v2_git_daemon_matches_stock_git(
        "repeated-branch",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option=trace",
            "--server-option=mode=full",
            "origin",
            "main",
        ],
        &[
            "fetch",
            "--server-option=trace",
            "--server-option=mode=full",
            "origin",
            "main",
        ],
        &["server-option=trace", "server-option=mode=full"],
    );
}

#[test]
fn fetch_server_option_repeated_protocol_v2_git_daemon_branchless_matches_stock_git() {
    assert_server_option_protocol_v2_git_daemon_matches_stock_git(
        "repeated-branchless",
        &[
            "-c",
            "protocol.version=2",
            "fetch",
            "--server-option=trace",
            "--server-option=mode=full",
            "origin",
        ],
        &[
            "fetch",
            "--server-option=trace",
            "--server-option=mode=full",
            "origin",
        ],
        &["server-option=trace", "server-option=mode=full"],
    );
}

fn assert_server_option_protocol_v2_git_daemon_matches_stock_git(
    label: &str,
    stock_args: &[&str],
    zmin_args: &[&str],
    expected_options: &[&str],
) {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join(format!("git-server-option-daemon-{label}"));
    let zmin_client = dir
        .path()
        .join(format!("zmin-server-option-daemon-{label}"));
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("alpha.txt"), b"alpha\n").expect("write alpha");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let trace = dir.path().join("daemon-packet.trace");
    let trace_value = trace.to_str().expect("trace path");
    let _daemon = StockGitDaemon::spawn_with_args_and_env(
        dir.path(),
        port,
        &[],
        &[("GIT_TRACE_PACKET", trace_value)],
    );
    let url = format!("git://127.0.0.1:{port}/remote.git");
    git(
        dir.path(),
        ["init", git_client.to_str().expect("git client")],
    );
    git(
        dir.path(),
        ["init", zmin_client.to_str().expect("zmin client")],
    );
    git(&git_client, ["remote", "add", "origin", url.as_str()]);
    run_zmin(&zmin_client, ["remote", "add", "origin", url.as_str()]);

    let stock = command_output(
        stock_git_bin().to_str().expect("stock git path"),
        &git_client,
        stock_args,
        "git fetch --server-option over git-daemon",
    );
    assert_eq!(stock.0, 0);
    let stock_trace = fs::read_to_string(&trace).expect("stock daemon packet trace");
    let mut stock_option_counts = Vec::new();
    for expected in expected_options {
        let count = stock_trace.matches(expected).count();
        assert!(
            count >= 2,
            "stock Git should send {expected} during ls-refs and fetch:\n{stock_trace}"
        );
        stock_option_counts.push((*expected, count));
    }

    let zmin = command_output(
        zmin_bin(),
        &zmin_client,
        zmin_args,
        "zmin fetch --server-option over git-daemon",
    );
    assert_eq!(zmin.0, 0);
    assert_eq!(zmin.1, stock.1);
    let full_trace = fs::read_to_string(&trace).expect("zmin daemon packet trace");
    for (expected, stock_count) in stock_option_counts {
        let full_count = full_trace.matches(expected).count();
        assert!(
            full_count >= stock_count + 2,
            "Zmin should send {expected} during ls-refs and fetch:\n{full_trace}"
        );
    }
    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "origin/main:alpha.txt"]),
        git(&git_client, ["cat-file", "-p", "origin/main:alpha.txt"])
    );
    git(&zmin_client, ["fsck", "--strict"]);
}

fn assert_server_option_protocol_v2_smart_http_branch_matches_stock_git(
    label: &str,
    stock_args: &[&str],
    zmin_args: &[&str],
    expected_options: &[&str],
) {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join(format!("git-server-option-{label}"));
    let zmin_client = dir.path().join(format!("zmin-server-option-{label}"));
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("alpha.txt"), b"alpha\n").expect("write alpha");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    git(
        dir.path(),
        ["init", git_client.to_str().expect("git client")],
    );
    git(
        dir.path(),
        ["init", zmin_client.to_str().expect("zmin client")],
    );
    git(&git_client, ["remote", "add", "origin", url.as_str()]);
    run_zmin(&zmin_client, ["remote", "add", "origin", url.as_str()]);

    let stock = command_output(
        stock_git_bin().to_str().expect("stock git path"),
        &git_client,
        stock_args,
        "git fetch --server-option",
    );
    assert_eq!(stock.0, 0);
    let stock_body_count = server.upload_pack_bodies_text().len();
    let stock_protocol_count = server.git_protocol_requests();
    let stock_bodies = server.upload_pack_bodies_text();
    assert!(
        stock_protocol_count >= 3,
        "stock Git should use protocol v2 for discovery and upload-pack requests"
    );
    for expected in expected_options {
        assert!(
            stock_bodies
                .iter()
                .filter(|body| body.contains(expected))
                .count()
                >= 2,
            "stock Git should send {expected} during ls-refs and fetch"
        );
    }

    let zmin = command_output(
        zmin_bin(),
        &zmin_client,
        zmin_args,
        "zmin fetch --server-option",
    );
    assert_eq!(zmin.0, 0);
    assert_eq!(zmin.1, stock.1);
    assert!(
        server.git_protocol_requests() >= stock_protocol_count + 3,
        "Zmin should use protocol v2 for discovery and upload-pack requests"
    );
    let bodies = server.upload_pack_bodies_text();
    let zmin_bodies = &bodies[stock_body_count..];
    for expected in expected_options {
        assert!(
            zmin_bodies
                .iter()
                .filter(|body| body.contains(expected))
                .count()
                >= 2,
            "Zmin should send {expected} during ls-refs and fetch"
        );
    }
    assert!(
        zmin_bodies
            .iter()
            .any(|body| body.contains("command=ls-refs"))
            && zmin_bodies
                .iter()
                .any(|body| body.contains("command=fetch")),
        "Zmin should issue protocol v2 ls-refs and fetch commands"
    );
    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "origin/main:alpha.txt"]),
        git(&git_client, ["cat-file", "-p", "origin/main:alpha.txt"])
    );
    git(&zmin_client, ["fsck", "--strict"]);
}

fn assert_server_option_protocol_v2_ssh_branch_matches_stock_git(
    label: &str,
    stock_args: &[&str],
    zmin_args: &[&str],
    expected_options: &[&str],
) {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join(format!("git-server-option-ssh-{label}"));
    let zmin_client = dir.path().join(format!("zmin-server-option-ssh-{label}"));
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("alpha.txt"), b"alpha\n").expect("write alpha");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let (fake_ssh, fake_ssh_log) = write_logging_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let stock_envs = [
        ("GIT_SSH_COMMAND", fake_ssh_arg.as_str()),
        ("GIT_PROTOCOL", "version=2"),
    ];
    let zmin_envs = [("GIT_SSH_COMMAND", fake_ssh_arg.as_str())];
    let url = ssh_url_for_remote(&remote);
    git(
        dir.path(),
        ["init", git_client.to_str().expect("git client")],
    );
    git(
        dir.path(),
        ["init", zmin_client.to_str().expect("zmin client")],
    );
    git(&git_client, ["remote", "add", "origin", url.as_str()]);
    run_zmin(&zmin_client, ["remote", "add", "origin", url.as_str()]);

    let stock = command_output_with_env(
        stock_git_bin().to_str().expect("stock git path"),
        &git_client,
        stock_args,
        &stock_envs,
        "git fetch --server-option over ssh",
    );
    assert_eq!(stock.0, 0);
    let stock_log = fs::read_to_string(&fake_ssh_log).expect("stock fake ssh log");
    assert!(
        stock_log.contains("GIT_PROTOCOL=version=2"),
        "stock Git should request protocol v2 over SSH:\n{stock_log}"
    );
    assert!(
        stock_log.contains("command=ls-refs") && stock_log.contains("command=fetch"),
        "stock Git should issue protocol v2 ls-refs and fetch commands:\n{stock_log}"
    );
    for expected in expected_options {
        assert!(
            stock_log.matches(expected).count() >= 2,
            "stock Git should send {expected} during ls-refs and fetch:\n{stock_log}"
        );
    }

    let zmin = command_output_with_env(
        zmin_bin(),
        &zmin_client,
        zmin_args,
        &zmin_envs,
        "zmin fetch --server-option over ssh",
    );
    assert_eq!(zmin.0, 0);
    assert_eq!(zmin.1, stock.1);
    let full_log = fs::read_to_string(&fake_ssh_log).expect("zmin fake ssh log");
    let zmin_log = &full_log[stock_log.len()..];
    assert!(
        zmin_log.contains("GIT_PROTOCOL=version=2"),
        "Zmin should request protocol v2 over SSH:\n{zmin_log}"
    );
    assert!(
        zmin_log.contains("command=ls-refs") && zmin_log.contains("command=fetch"),
        "Zmin should issue protocol v2 ls-refs and fetch commands:\n{zmin_log}"
    );
    for expected in expected_options {
        assert!(
            zmin_log.matches(expected).count() >= 2,
            "Zmin should send {expected} during ls-refs and fetch:\n{zmin_log}"
        );
    }
    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "origin/main:alpha.txt"]),
        git(&git_client, ["cat-file", "-p", "origin/main:alpha.txt"])
    );
    git(&zmin_client, ["fsck", "--strict"]);
}

#[test]
fn fetch_smart_http_multiple_explicit_tags_with_protocol_v2_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let server_work = dir.path().join("server");
    let client = dir.path().join("client");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            server_work.to_str().expect("server path"),
        ],
    );
    configure_identity(&server_work);
    fs::write(server_work.join("alpha.txt"), b"alpha 1\n").expect("write alpha");
    git(&server_work, ["add", "-A"]);
    git_with_env(&server_work, ["commit", "-m", "alpha_1"]);
    git(&server_work, ["tag", "alpha_1"]);
    fs::write(server_work.join("alpha.txt"), b"alpha 2\n").expect("write alpha 2");
    git(&server_work, ["commit", "-am", "alpha_2"]);
    git(&server_work, ["tag", "alpha_2"]);
    git(&server_work, ["checkout", "--orphan", "beta"]);
    fs::write(server_work.join("beta.txt"), b"beta 1\n").expect("write beta");
    git(&server_work, ["add", "-A"]);
    git_with_env(&server_work, ["commit", "-m", "beta_1"]);
    git(&server_work, ["tag", "beta_1"]);
    fs::write(server_work.join("beta.txt"), b"beta 2\n").expect("write beta 2");
    git(&server_work, ["commit", "-am", "beta_2"]);
    git(&server_work, ["tag", "beta_2"]);
    fs::write(server_work.join(".git/git-daemon-export-ok"), "").expect("export marker");

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/server", server.port);
    run_zmin(
        dir.path(),
        ["clone", url.as_str(), client.to_str().expect("client path")],
    );
    run_zmin(&client, ["config", "protocol.version", "2"]);
    git(&server_work, ["config", "protocol.version", "2"]);
    fs::write(server_work.join("beta.txt"), b"beta s\n").expect("write beta s");
    git(&server_work, ["commit", "-am", "beta_s"]);
    git(&server_work, ["tag", "beta_s"]);
    git(&server_work, ["checkout", "main"]);
    fs::write(server_work.join("alpha.txt"), b"alpha s\n").expect("write alpha s");
    git(&server_work, ["commit", "-am", "alpha_s"]);
    git(&server_work, ["tag", "alpha_s"]);
    git(
        &server_work,
        ["tag", "-d", "alpha_1", "alpha_2", "beta_1", "beta_2"],
    );

    let trace = dir.path().join("trace");
    let trace_value = trace.to_str().expect("trace path");
    let output = command_output_with_env(
        zmin_bin(),
        &client,
        &[
            "fetch",
            "--negotiation-tip=alpha_1",
            "--negotiation-tip=beta_1",
            "origin",
            "alpha_s",
            "beta_s",
        ],
        &[("GIT_TRACE_PACKET", trace_value)],
        "zmin",
    );

    assert_eq!(output.0, 0, "fetch failed: {}", output.2);
    assert_eq!(
        git(&client, ["rev-parse", "alpha_s"]),
        git(&server_work, ["rev-parse", "alpha_s"])
    );
    assert_eq!(
        git(&client, ["rev-parse", "beta_s"]),
        git(&server_work, ["rev-parse", "beta_s"])
    );
    let trace_contents = fs::read_to_string(trace).expect("trace file");
    assert!(trace_contents.contains(&format!(
        "fetch> have {}",
        git(&client, ["rev-parse", "alpha_1"])
    )));
    assert!(trace_contents.contains(&format!(
        "fetch> have {}",
        git(&client, ["rev-parse", "beta_1"])
    )));
}

#[test]
fn fetch_reads_shallow_smart_http_pack_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-smart-fetch");
    let zmin_client = dir.path().join("zmin-smart-fetch");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    for idx in 1..=3 {
        fs::write(work.join("a.txt"), format!("commit {idx}\n")).expect("write a");
        git(&work, ["add", "-A"]);
        git_with_env(&work, ["commit", "-m", &format!("commit {idx}")]);
    }
    let parent = git(&work, ["rev-parse", "HEAD^"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    git(dir.path(), ["init", "git-smart-fetch"]);
    git(dir.path(), ["init", "zmin-smart-fetch"]);
    git(&git_client, ["remote", "add", "origin", url.as_str()]);
    run_zmin(&zmin_client, ["remote", "add", "origin", url.as_str()]);
    git(&git_client, ["fetch", "--depth=1", "origin", "main"]);
    run_zmin(&zmin_client, ["fetch", "--depth=1", "origin", "main"]);

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_matching_shallow_state(&zmin_client, &git_client, &parent);
}

#[test]
fn fetch_depth_smart_http_multiple_explicit_refspecs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let (remote, main_parent, feature_parent) = prepare_two_branch_shallow_remote(dir.path());
    let git_client = dir.path().join("git-depth-multi-http");
    let zmin_client = dir.path().join("zmin-depth-multi-http");

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/{}", server.port, "remote.git");
    git(dir.path(), ["init", "git-depth-multi-http"]);
    git(dir.path(), ["init", "zmin-depth-multi-http"]);
    git(&git_client, ["remote", "add", "origin", url.as_str()]);
    run_zmin(&zmin_client, ["remote", "add", "origin", url.as_str()]);
    let args = [
        "fetch",
        "--depth=1",
        "origin",
        "refs/heads/main:refs/remotes/origin/main",
        "refs/heads/feature:refs/remotes/origin/feature",
    ];

    git(&git_client, args);
    run_zmin(&zmin_client, args);

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_matching_shallow_state_for_missing_objects(
        &zmin_client,
        &git_client,
        &[main_parent, feature_parent],
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "origin/feature:feature.txt"]
        )
    );
    assert!(remote.join("git-daemon-export-ok").is_file());
}

#[test]
fn pull_rebase_reads_smart_http_pack_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-smart-pull-rebase");
    let zmin_client = dir.path().join("zmin-smart-pull-rebase");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"base\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "base"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    git(
        dir.path(),
        [
            "clone",
            url.as_str(),
            git_client.to_str().expect("git client path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            url.as_str(),
            zmin_client.to_str().expect("zmin client path"),
        ],
    );
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    fs::write(git_client.join("local.txt"), b"local\n").expect("write git local");
    fs::write(zmin_client.join("local.txt"), b"local\n").expect("write zmin local");
    git(&git_client, ["add", "-A"]);
    git(&zmin_client, ["add", "-A"]);
    git_with_env(&git_client, ["commit", "-m", "local"]);
    git_with_env(&zmin_client, ["commit", "-m", "local"]);

    fs::write(work.join("remote.txt"), b"remote\n").expect("write remote");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "remote"]);
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    git_with_env(&git_client, ["pull", "--rebase"]);
    run_zmin_with_env(&zmin_client, ["pull", "--rebase"]);

    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&zmin_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        run_zmin(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn clone_reads_dumb_http_repository_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::create_dir_all(source.join("dir")).expect("create dir");
    fs::write(source.join("dir/a.txt"), b"hello\n").expect("write a");
    fs::write(source.join("root.txt"), b"root\n").expect("write root");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["branch", "feature"]);
    git(&source, ["update-server-info"]);

    let server = StaticHttpServer::new(source.clone());
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    git(
        dir.path(),
        [
            "clone",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        fs::read_to_string(zmin_clone.join("dir/a.txt")).expect("read zmin a"),
        fs::read_to_string(git_clone.join("dir/a.txt")).expect("read git a")
    );
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&git_clone, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&zmin_clone, ["show-ref"]),
        git(&git_clone, ["show-ref"])
    );

    let git_feature = dir.path().join("git-feature");
    let zmin_feature = dir.path().join("zmin-feature");
    git(
        dir.path(),
        [
            "clone",
            "-b",
            "feature",
            "--single-branch",
            "--no-tags",
            "--no-checkout",
            url.as_str(),
            git_feature.to_str().expect("git feature path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "-b",
            "feature",
            "--single-branch",
            "--no-tags",
            "--no-checkout",
            url.as_str(),
            zmin_feature.to_str().expect("zmin feature path"),
        ],
    );
    assert_eq!(
        git(&zmin_feature, ["show-ref"]),
        git(&git_feature, ["show-ref"])
    );
    assert_eq!(
        git(&zmin_feature, ["rev-parse", "HEAD"]),
        git(&git_feature, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        zmin_feature.join("root.txt").exists(),
        git_feature.join("root.txt").exists()
    );

    let git_bare = dir.path().join("git-bare.git");
    let zmin_bare = dir.path().join("zmin-bare.git");
    git(
        dir.path(),
        [
            "clone",
            "--bare",
            url.as_str(),
            git_bare.to_str().expect("git bare path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--bare",
            url.as_str(),
            zmin_bare.to_str().expect("zmin bare path"),
        ],
    );
    assert_eq!(git(&zmin_bare, ["show-ref"]), git(&git_bare, ["show-ref"]));
    assert_eq!(
        fs::read_to_string(zmin_bare.join("HEAD")).expect("read zmin bare HEAD"),
        fs::read_to_string(git_bare.join("HEAD")).expect("read git bare HEAD")
    );

    let git_mirror = dir.path().join("git-mirror.git");
    let zmin_mirror = dir.path().join("zmin-mirror.git");
    git(
        dir.path(),
        [
            "clone",
            "--mirror",
            url.as_str(),
            git_mirror.to_str().expect("git mirror path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--mirror",
            url.as_str(),
            zmin_mirror.to_str().expect("zmin mirror path"),
        ],
    );
    assert_eq!(
        git(&zmin_mirror, ["show-ref"]),
        git(&git_mirror, ["show-ref"])
    );
    assert_eq!(
        git(&zmin_mirror, ["config", "--get", "remote.origin.mirror"]),
        git(&git_mirror, ["config", "--get", "remote.origin.mirror"])
    );
}

#[test]
fn clone_dumb_http_removes_new_destination_after_checksum_failure_and_preserves_existing() {
    let dir = TempDir::new().expect("dumb HTTP checksum temp dir");
    let source = dir.path().join("source");
    let stock_destination = dir.path().join("stock-failed-clone");
    let zmin_destination = dir.path().join("zmin-failed-clone");
    let existing_destination = dir.path().join("existing-destination");
    pinned_git_args(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    pinned_git_args(&source, ["config", "user.name", "Bench"]);
    pinned_git_args(&source, ["config", "user.email", "bench@example.test"]);
    fs::write(source.join("payload.txt"), b"checksum failure\n").expect("write payload");
    pinned_git_args(&source, ["add", "-A"]);
    pinned_git_with_env(
        &source,
        ["commit", "-m", "checksum fixture"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
    );
    pinned_git_args(&source, ["repack", "-ad"]);
    pinned_git_args(&source, ["update-server-info"]);
    let pack_path = fs::read_dir(source.join(".git/objects/pack"))
        .expect("read source pack directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("pack"))
        .expect("source pack");
    let mut pack = fs::read(&pack_path).expect("read source pack");
    let last = pack.last_mut().expect("non-empty source pack");
    *last ^= 0xff;
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&pack_path)
            .expect("source pack metadata")
            .permissions();
        permissions.set_mode(0o644);
        fs::set_permissions(&pack_path, permissions).expect("make source pack writable");
    }
    fs::write(&pack_path, pack).expect("corrupt source pack checksum");

    let stock_server = StaticHttpServer::new(source.clone());
    let stock_url = format!("http://127.0.0.1:{}/source/.git", stock_server.port);
    let stock_output = pinned_command_any_output(
        dir.path(),
        &[
            "clone",
            "-q",
            &stock_url,
            stock_destination.to_str().expect("stock destination"),
        ],
        "pinned stock dumb HTTP checksum clone",
    );
    assert_eq!(
        stock_output.0, 128,
        "stock checksum clone: {stock_output:?}"
    );
    assert!(!stock_destination.exists(), "stock left a failed clone");

    let zmin_server = StaticHttpServer::new(source);
    let zmin_url = format!("http://127.0.0.1:{}/source/.git", zmin_server.port);
    let zmin_output = command_any_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "-q",
            &zmin_url,
            zmin_destination.to_str().expect("zmin destination"),
        ],
        "Zmin dumb HTTP checksum clone",
    );
    assert_eq!(zmin_output.0, stock_output.0);
    assert!(!zmin_destination.exists(), "Zmin left a failed clone");

    fs::create_dir(&existing_destination).expect("create existing destination");
    fs::write(existing_destination.join("keep.txt"), b"keep\n").expect("existing sentinel");
    let existing_output = command_any_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "-q",
            &zmin_url,
            existing_destination.to_str().expect("existing destination"),
        ],
        "Zmin dumb HTTP pre-existing destination",
    );
    assert_eq!(existing_output.0, 128);
    assert_eq!(
        fs::read(existing_destination.join("keep.txt")).expect("read existing sentinel"),
        b"keep\n"
    );
}

#[test]
fn clone_reads_shallow_dumb_http_repository_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    for idx in 1..=3 {
        fs::write(source.join("a.txt"), format!("commit {idx}\n")).expect("write a");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", &format!("commit {idx}")]);
    }
    git(&source, ["update-server-info"]);

    let server = StaticHttpServer::new(source.clone());
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    let git_failure = command_failure_output(
        "git",
        dir.path(),
        &[
            "clone",
            "--depth=1",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
        "git shallow dumb http clone",
    );
    let zmin_failure = command_failure_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--depth=1",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
        "zmin shallow dumb http clone",
    );
    assert_eq!(git_failure.0, zmin_failure.0);
    assert_eq!(git_failure.1, zmin_failure.1);
    assert!(
        git_failure
            .2
            .ends_with("fatal: dumb http transport does not support shallow capabilities")
    );
    assert!(
        zmin_failure
            .2
            .ends_with("fatal: dumb http transport does not support shallow capabilities")
    );
}

#[test]
fn clone_shared_is_ignored_for_dumb_http_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"hello\n").expect("write a");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["update-server-info"]);

    let server = StaticHttpServer::new(source.clone());
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    git(
        dir.path(),
        [
            "clone",
            "--shared",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--shared",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );
    assert_eq!(
        git(&zmin_clone, ["show-ref"]),
        git(&git_clone, ["show-ref"])
    );
    assert_no_alternates(&git_clone);
    assert_no_alternates(&zmin_clone);
}

#[test]
fn clone_reference_dumb_http_matches_stock_git() {
    ensure_remote_http_helper();
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let reference = dir.path().join("reference");
    let git_clone = dir.path().join("git-reference-clone");
    let zmin_clone = dir.path().join("zmin-reference-clone");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"hello\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["update-server-info"]);

    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            reference.to_str().expect("reference path"),
        ],
    );
    configure_identity(&reference);
    fs::write(reference.join("reference.txt"), b"reference\n").expect("write reference");
    git(&reference, ["add", "-A"]);
    git_with_env(&reference, ["commit", "-m", "reference"]);

    let server = StaticHttpServer::new(source.clone());
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    git(
        dir.path(),
        [
            "clone",
            "--reference",
            reference.to_str().expect("reference path"),
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--reference",
            reference.to_str().expect("reference path"),
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        canonical_alternates(&zmin_clone.join(".git/objects/info/alternates")),
        canonical_alternates(&git_clone.join(".git/objects/info/alternates"))
    );
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&git_clone, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&zmin_clone, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_clone, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["status", "--porcelain=v1", "--branch"]),
        git(&git_clone, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn clone_reference_if_able_dumb_http_matches_stock_git() {
    ensure_remote_http_helper();
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let reference = dir.path().join("reference");
    let git_clone = dir.path().join("git-reference-if-able-clone");
    let zmin_clone = dir.path().join("zmin-reference-if-able-clone");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"hello\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["update-server-info"]);

    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            reference.to_str().expect("reference path"),
        ],
    );
    configure_identity(&reference);
    fs::write(reference.join("reference.txt"), b"reference\n").expect("write reference");
    git(&reference, ["add", "-A"]);
    git_with_env(&reference, ["commit", "-m", "reference"]);

    let server = StaticHttpServer::new(source.clone());
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    git(
        dir.path(),
        [
            "clone",
            "--reference-if-able",
            reference.to_str().expect("reference path"),
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--reference-if-able",
            reference.to_str().expect("reference path"),
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        canonical_alternates(&zmin_clone.join(".git/objects/info/alternates")),
        canonical_alternates(&git_clone.join(".git/objects/info/alternates"))
    );
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&git_clone, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&zmin_clone, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_clone, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["status", "--porcelain=v1", "--branch"]),
        git(&git_clone, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn clone_reject_shallow_allows_non_shallow_dumb_http_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"hello\n").expect("write a");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["update-server-info"]);

    let server = StaticHttpServer::new(source.clone());
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    git(
        dir.path(),
        [
            "clone",
            "--reject-shallow",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--reject-shallow",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );
    assert_eq!(
        git(&zmin_clone, ["show-ref"]),
        git(&git_clone, ["show-ref"])
    );
}

#[test]
fn clone_reject_shallow_rejects_shallow_dumb_http_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let shallow_source = dir.path().join("shallow-source");
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    for idx in 1..=3 {
        fs::write(source.join("a.txt"), format!("commit {idx}\n")).expect("write a");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", &format!("commit {idx}")]);
    }
    let source_url = format!("file://{}", source.display());
    git(
        dir.path(),
        [
            "clone",
            "--depth=1",
            source_url.as_str(),
            shallow_source.to_str().expect("shallow source path"),
        ],
    );
    git(&shallow_source, ["update-server-info"]);

    let server = StaticHttpServer::new(shallow_source.clone());
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    let git_failure = command_failure_output(
        "git",
        dir.path(),
        &[
            "clone",
            "--reject-shallow",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
        "git reject shallow dumb http clone",
    );
    let zmin_failure = command_failure_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--reject-shallow",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
        "zmin reject shallow dumb http clone",
    );
    assert_eq!(git_failure.0, zmin_failure.0);
    assert_eq!(git_failure.1, zmin_failure.1);
    assert!(
        git_failure.2.contains("fetch failed") || git_failure.2.contains("Cannot obtain"),
        "unexpected stock Git stderr: {}",
        git_failure.2
    );
    assert!(
        zmin_failure.2.contains("failed") || zmin_failure.2.contains("Cannot obtain"),
        "unexpected Zmin stderr: {}",
        zmin_failure.2
    );
}

#[test]
fn clone_recurse_submodules_reads_dumb_http_submodule_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let submodule = dir.path().join("submodule");
    let source = dir.path().join("source");
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");
    let server = StaticHttpServer::new(dir.path().to_path_buf());
    let submodule_url = format!("http://127.0.0.1:{}/submodule/.git", server.port);
    let source_url = format!("http://127.0.0.1:{}/source/.git", server.port);

    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            submodule.to_str().expect("submodule path"),
        ],
    );
    configure_identity(&submodule);
    fs::write(submodule.join("lib.txt"), b"submodule\n").expect("write submodule file");
    git(&submodule, ["add", "-A"]);
    git_with_env(&submodule, ["commit", "-m", "submodule"]);
    let submodule_head = git(&submodule, ["rev-parse", "HEAD"]);
    git(&submodule, ["update-server-info"]);

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(
        source.join(".gitmodules"),
        format!("[submodule \"sub\"]\n\tpath = sub\n\turl = {submodule_url}\n"),
    )
    .expect("write gitmodules");
    git(&source, ["add", ".gitmodules"]);
    git(
        &source,
        [
            "update-index",
            "--add",
            "--cacheinfo",
            "160000",
            submodule_head.as_str(),
            "sub",
        ],
    );
    git_with_env(&source, ["commit", "-m", "main with submodule"]);
    git(&source, ["update-server-info"]);

    git(
        dir.path(),
        [
            "clone",
            "--recurse-submodules",
            source_url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--recurse-submodules",
            source_url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&git_clone, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&zmin_clone.join("sub"), ["rev-parse", "HEAD"]),
        git(&git_clone.join("sub"), ["rev-parse", "HEAD"])
    );
    assert_eq!(
        fs::read_to_string(zmin_clone.join("sub/lib.txt")).expect("read zmin submodule file"),
        fs::read_to_string(git_clone.join("sub/lib.txt")).expect("read git submodule file")
    );
}

#[test]
fn fetch_reads_dumb_http_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"one\n").expect("write a");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["branch", "feature"]);
    git_with_env(&source, ["tag", "-a", "v1", "-m", "tag message"]);
    git(&source, ["update-server-info"]);

    let server = StaticHttpServer::new(source.clone());
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    for client in [&git_client, &zmin_client] {
        git(dir.path(), ["init", client.to_str().expect("client path")]);
        git(client, ["remote", "add", "origin", url.as_str()]);
    }

    git(&git_client, ["fetch", "origin"]);
    run_zmin(&zmin_client, ["fetch", "origin"]);
    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "origin/main:a.txt"]),
        git(&git_client, ["cat-file", "-p", "origin/main:a.txt"])
    );

    fs::write(source.join("feature.txt"), b"feature\n").expect("write feature");
    git(&source, ["checkout", "feature"]);
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "feature"]);
    git(&source, ["update-server-info"]);
    git(&git_client, ["fetch", "origin", "feature"]);
    run_zmin(&zmin_client, ["fetch", "origin", "feature"]);
    assert_eq!(
        git(&zmin_client, ["rev-parse", "origin/feature"]),
        git(&git_client, ["rev-parse", "origin/feature"])
    );
}

#[test]
fn fetch_reads_shallow_dumb_http_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    for idx in 1..=3 {
        fs::write(source.join("a.txt"), format!("commit {idx}\n")).expect("write a");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", &format!("commit {idx}")]);
    }
    git(&source, ["update-server-info"]);

    let server = StaticHttpServer::new(source.clone());
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    for client in [&git_client, &zmin_client] {
        git(dir.path(), ["init", client.to_str().expect("client path")]);
        git(client, ["remote", "add", "origin", url.as_str()]);
    }

    assert_eq!(
        command_failure_output(
            "git",
            &git_client,
            &["fetch", "--depth=1", "origin", "main"],
            "git shallow dumb http fetch",
        ),
        command_failure_output(
            zmin_bin(),
            &zmin_client,
            &["fetch", "--depth=1", "origin", "main"],
            "zmin shallow dumb http fetch",
        )
    );
}

#[test]
fn fetch_reads_shallow_git_daemon_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    for idx in 1..=3 {
        fs::write(work.join("a.txt"), format!("commit {idx}\n")).expect("write a");
        git(&work, ["add", "-A"]);
        git_with_env(&work, ["commit", "-m", &format!("commit {idx}")]);
    }
    let parent = git(&work, ["rev-parse", "HEAD^"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    for client in [&git_client, &zmin_client] {
        git(dir.path(), ["init", client.to_str().expect("client path")]);
        git(client, ["remote", "add", "origin", url.as_str()]);
    }

    git(&git_client, ["fetch", "--depth=1", "origin", "main"]);
    run_zmin(&zmin_client, ["fetch", "--depth=1", "origin", "main"]);

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_matching_shallow_state(&zmin_client, &git_client, &parent);
}

#[test]
fn fetch_depth_git_daemon_multiple_explicit_refspecs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let (_remote, main_parent, feature_parent) = prepare_two_branch_shallow_remote(dir.path());
    let git_client = dir.path().join("git-depth-multi-daemon");
    let zmin_client = dir.path().join("zmin-depth-multi-daemon");
    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    for client in [&git_client, &zmin_client] {
        git(dir.path(), ["init", client.to_str().expect("client path")]);
        git(client, ["remote", "add", "origin", url.as_str()]);
    }
    let args = [
        "fetch",
        "--depth=1",
        "origin",
        "refs/heads/main:refs/remotes/origin/main",
        "refs/heads/feature:refs/remotes/origin/feature",
    ];

    git(&git_client, args);
    run_zmin(&zmin_client, args);

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_matching_shallow_state_for_missing_objects(
        &zmin_client,
        &git_client,
        &[main_parent, feature_parent],
    );
}

#[test]
fn clone_reads_shallow_git_daemon_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    for idx in 1..=3 {
        fs::write(work.join("a.txt"), format!("commit {idx}\n")).expect("write a");
        git(&work, ["add", "-A"]);
        git_with_env(&work, ["commit", "-m", &format!("commit {idx}")]);
    }
    let parent = git(&work, ["rev-parse", "HEAD^"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    git(
        dir.path(),
        [
            "clone",
            "--depth=1",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--depth=1",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );

    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&git_clone, ["rev-parse", "HEAD"])
    );
    assert_matching_shallow_state(&zmin_clone, &git_clone, &parent);
}

#[test]
fn clone_shared_is_ignored_for_git_daemon_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    git(
        dir.path(),
        [
            "clone",
            "--shared",
            url.as_str(),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--shared",
            url.as_str(),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );
    assert_eq!(
        git(&zmin_clone, ["show-ref"]),
        git(&git_clone, ["show-ref"])
    );
    assert_no_alternates(&git_clone);
    assert_no_alternates(&zmin_clone);
}

#[test]
fn daemon_unknown_service_matches_stock_git_inetd_failure() {
    let dir = TempDir::new().expect("temp dir");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    let payload = b"git-foo /remote.git\0host=localhost\0";
    let mut request = format!("{:04x}", payload.len() + 4).into_bytes();
    request.extend_from_slice(payload);
    let base_path = format!("--base-path={}", dir.path().display());
    for args in [
        vec!["daemon", "--inetd", "--export-all", base_path.as_str()],
        vec![
            "daemon",
            "--inetd",
            "--informative-errors",
            "--export-all",
            base_path.as_str(),
        ],
    ] {
        let git_output = daemon_inetd_failure("git", &args, &request);
        let zmin_output = daemon_inetd_failure(zmin_bin(), &args, &request);

        assert_eq!(zmin_output.0, git_output.0);
        assert_eq!(zmin_output.1, git_output.1);
        assert_eq!(zmin_output.2, git_output.2);
    }
}

#[test]
fn daemon_informative_errors_match_stock_git_inetd() {
    let dir = TempDir::new().expect("temp dir");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    let base_path = format!("--base-path={}", dir.path().display());
    let cases = [
        (
            "missing repository",
            vec![
                "daemon",
                "--inetd",
                "--informative-errors",
                "--export-all",
                base_path.as_str(),
            ],
            pkt_line_bytes(b"git-upload-pack /missing.git\0host=localhost\0"),
        ),
        (
            "disabled service",
            vec![
                "daemon",
                "--inetd",
                "--informative-errors",
                "--export-all",
                base_path.as_str(),
            ],
            pkt_line_bytes(b"git-receive-pack /remote.git\0host=localhost\0"),
        ),
        (
            "unexported repository",
            vec![
                "daemon",
                "--inetd",
                "--informative-errors",
                base_path.as_str(),
            ],
            pkt_line_bytes(b"git-upload-pack /remote.git\0host=localhost\0"),
        ),
    ];

    for (label, args, request) in cases {
        let git_output = daemon_inetd_result("git", &args, &request);
        let zmin_output = daemon_inetd_result(zmin_bin(), &args, &request);
        assert_eq!(zmin_output, git_output, "{label}");
    }
}

#[test]
fn daemon_interpolated_path_matches_stock_git_inetd() {
    let dir = TempDir::new().expect("interpolated daemon temp dir");
    fs::create_dir(dir.path().join("localhost")).expect("create virtual host directory");
    let remote = dir.path().join("localhost/interp.git");
    git(dir.path(), ["init", "--bare", "localhost/interp.git"]);
    fs::write(remote.join("git-daemon-export-ok"), b"").expect("export marker");

    let work = dir.path().join("work");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("file"), b"content\n").expect("write source file");
    git(&work, ["add", "file"]);
    git_with_env(&work, ["commit", "-m", "one"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);

    let base_path = format!("--base-path={}", dir.path().display());
    let interpolated_path = format!("--interpolated-path={}/%H%D", dir.path().display());
    let args = [
        "daemon",
        "--inetd",
        base_path.as_str(),
        interpolated_path.as_str(),
    ];
    let mut request = pkt_line_bytes(b"git-upload-pack /interp.git\0host=localhost\0");
    request.extend_from_slice(b"0000");

    let stock = daemon_inetd_result("git", &args, &request);
    let zmin = daemon_inetd_result(zmin_bin(), &args, &request);
    assert_eq!(zmin.0, stock.0);
    assert_eq!(zmin.2, stock.2);
    assert_eq!(stock.0, 0);
    assert!(stock.2.is_empty());
    let main_oid = git(&work, ["rev-parse", "main"]);
    let main_ref = format!("{main_oid} refs/heads/main\n");
    assert!(String::from_utf8_lossy(&stock.1).contains(&main_ref));
    assert!(String::from_utf8_lossy(&zmin.1).contains(&main_ref));
}

#[test]
fn git_daemon_override_virtual_host_matches_stock_ls_remote() {
    let dir = TempDir::new().expect("virtual host daemon temp dir");
    let remote = dir.path().join("localhost/interp.git");
    fs::create_dir_all(remote.parent().expect("virtual host parent"))
        .expect("create virtual host directory");
    git(
        dir.path(),
        ["init", "--bare", remote.to_str().expect("remote path")],
    );
    fs::write(remote.join("git-daemon-export-ok"), b"").expect("export marker");

    let work = dir.path().join("work");
    git(
        dir.path(),
        ["init", "-b", "main", work.to_str().expect("work path")],
    );
    configure_identity(&work);
    fs::write(work.join("file"), b"content\n").expect("write file");
    git(&work, ["add", "file"]);
    git_with_env(&work, ["commit", "-m", "one"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);

    let port = unused_local_port();
    let port_arg = format!("--port={port}");
    let interpolated_path = format!("--interpolated-path={}/%H%D", dir.path().display());
    let mut daemon = Command::new(zmin_bin())
        .args([
            "daemon",
            "--listen=127.0.0.1",
            port_arg.as_str(),
            interpolated_path.as_str(),
        ])
        .arg(dir.path().to_str().expect("daemon root"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn interpolated daemon");
    wait_for_tcp_port(port);

    let url = format!("git://127.0.0.1:{port}/interp.git");
    let stock = Command::new(stock_git_bin())
        .env("GIT_OVERRIDE_VIRTUAL_HOST", "localhost")
        .args(["ls-remote", url.as_str()])
        .output()
        .expect("stock ls-remote");
    let zmin = Command::new(zmin_bin())
        .env("GIT_OVERRIDE_VIRTUAL_HOST", "localhost")
        .args(["ls-remote", url.as_str()])
        .output()
        .expect("zmin ls-remote");
    daemon.kill().expect("stop interpolated daemon");
    let _ = daemon.wait().expect("wait interpolated daemon");

    assert_eq!(zmin.status.code(), stock.status.code());
    assert_eq!(zmin.stdout, stock.stdout);
    assert_eq!(zmin.stderr, stock.stderr);
    assert!(stock.status.success());
}

#[cfg(unix)]
#[test]
fn git_daemon_non_utf8_virtual_host_request_matches_stock_bytes() {
    let virtual_host = OsString::from_vec(b"local\xffhost".to_vec());
    let (stock, stock_request) = capture_git_daemon_request(stock_git_bin(), &virtual_host);
    let (zmin, zmin_request) =
        capture_git_daemon_request(std::path::Path::new(zmin_bin()), &virtual_host);

    assert_eq!(zmin.status.code(), stock.status.code());
    assert_eq!(zmin.stdout, stock.stdout);
    assert_eq!(zmin.stderr, stock.stderr);
    assert_eq!(zmin_request, stock_request);
    assert!(stock.status.success());
    assert!(stock_request.starts_with(b"git-upload-pack /repo.git\0host=local\xffhost\0"));
}

#[cfg(unix)]
#[test]
fn git_daemon_virtual_host_newline_matches_stock_rejection() {
    let virtual_host = OsString::from_vec(b"local\nhost".to_vec());
    let url = "git://127.0.0.1:1/repo.git";
    let stock = Command::new(stock_git_bin())
        .env("GIT_OVERRIDE_VIRTUAL_HOST", &virtual_host)
        .args(["-c", "protocol.version=0", "ls-remote", url])
        .output()
        .expect("stock newline virtual host rejection");
    let zmin = Command::new(zmin_bin())
        .env("GIT_OVERRIDE_VIRTUAL_HOST", &virtual_host)
        .args(["-c", "protocol.version=0", "ls-remote", url])
        .output()
        .expect("zmin newline virtual host rejection");

    assert_eq!(zmin.status.code(), stock.status.code());
    assert_eq!(zmin.stdout, stock.stdout);
    assert_eq!(zmin.stderr, stock.stderr);
    assert!(!stock.status.success());
    assert!(
        stock
            .stderr
            .windows(b"newline is forbidden".len())
            .any(|window| { window == b"newline is forbidden" })
    );
}

#[test]
fn daemon_interpolated_host_canonicalization_matches_stock_git_inetd() {
    let dir = TempDir::new().expect("canonicalized daemon temp dir");
    for (host, directory) in [
        ("LOCALHOST", "localhost"),
        ("[::1]:9418", "::1"),
        ("foo/bar", "foobar"),
    ] {
        let remote = dir.path().join(directory).join("interp.git");
        fs::create_dir_all(remote.parent().expect("remote parent"))
            .expect("create canonicalized host directory");
        git(
            dir.path(),
            ["init", "--bare", remote.to_str().expect("remote path")],
        );
        fs::write(remote.join("git-daemon-export-ok"), b"").expect("export marker");

        let base_path = format!("--base-path={}", dir.path().display());
        let interpolated_path = format!("--interpolated-path={}/%H%D", dir.path().display());
        let args = [
            "daemon",
            "--inetd",
            base_path.as_str(),
            interpolated_path.as_str(),
        ];
        let mut request =
            pkt_line_bytes(format!("git-upload-pack /interp.git\0host={host}\0").as_bytes());
        request.extend_from_slice(b"0000");
        let stock = daemon_inetd_result("git", &args, &request);
        let zmin = daemon_inetd_result(zmin_bin(), &args, &request);
        assert_eq!(zmin.0, stock.0, "host={host}");
        assert_eq!(zmin.2, stock.2, "host={host}");
        assert_eq!(stock.0, 0, "host={host}");
        assert!(
            String::from_utf8_lossy(&stock.1).contains("capabilities^{}"),
            "host={host}"
        );
        assert!(
            String::from_utf8_lossy(&zmin.1).contains("capabilities^{}"),
            "host={host}"
        );
    }
}

#[test]
fn daemon_interpolated_allowlist_uses_expanded_path_like_stock_git() {
    let dir = TempDir::new().expect("allowlist daemon temp dir");
    let root = dir.path().join("root");
    let remote = root.join("localhost/interp.git");
    fs::create_dir_all(remote.parent().expect("remote parent"))
        .expect("create allowlist host directory");
    git(
        dir.path(),
        ["init", "--bare", remote.to_str().expect("remote path")],
    );
    fs::write(remote.join("git-daemon-export-ok"), b"").expect("export marker");

    let base_path = format!("--base-path={}", root.display());
    let interpolated_path = format!("--interpolated-path={}/%H%D", root.display());
    let allowed_path = remote.clone();
    let args = [
        "daemon",
        "--inetd",
        "--strict-paths",
        base_path.as_str(),
        interpolated_path.as_str(),
        allowed_path.to_str().expect("allowlist path"),
    ];
    let mut request = pkt_line_bytes(b"git-upload-pack /interp.git\0host=localhost\0");
    request.extend_from_slice(b"0000");
    let stock = daemon_inetd_result("git", &args, &request);
    let zmin = daemon_inetd_result(zmin_bin(), &args, &request);
    assert_eq!(zmin.0, stock.0);
    assert_eq!(zmin.2, stock.2);
    assert_eq!(stock.0, 0);
}

#[test]
fn daemon_strict_allowlist_matches_exact_and_non_strict_descendant_stock_rules() {
    let dir = TempDir::new().expect("strict allowlist daemon temp dir");
    let root = dir.path().join("root");
    let exact = root.join("localhost/exact.git");
    let descendant = root.join("localhost/child.git");
    fs::create_dir_all(exact.parent().expect("exact parent"))
        .expect("create strict allowlist directory");
    for remote in [&exact, &descendant] {
        git(
            dir.path(),
            ["init", "--bare", remote.to_str().expect("remote path")],
        );
        fs::write(remote.join("git-daemon-export-ok"), b"").expect("export marker");
    }

    let base_path = format!("--base-path={}", root.display());
    let interpolated_path = format!("--interpolated-path={}/%H%D", root.display());
    let cases = [
        ("strict exact", true, exact.clone(), "/exact.git", true),
        (
            "strict descendant",
            true,
            exact.parent().expect("allowlist parent").to_path_buf(),
            "/child.git",
            false,
        ),
        (
            "non-strict descendant",
            false,
            exact.parent().expect("allowlist parent").to_path_buf(),
            "/child.git",
            true,
        ),
    ];
    for (label, strict, allowed, request_path, should_succeed) in cases {
        let mut args = vec!["daemon", "--inetd"];
        if strict {
            args.push("--strict-paths");
        }
        args.push(base_path.as_str());
        args.push(interpolated_path.as_str());
        args.push(allowed.to_str().expect("allowlist path"));
        let mut request =
            pkt_line_bytes(format!("git-upload-pack {request_path}\0host=localhost\0").as_bytes());
        request.extend_from_slice(b"0000");
        let stock = daemon_inetd_result("git", &args, &request);
        let zmin = daemon_inetd_result(zmin_bin(), &args, &request);
        assert_eq!(zmin.0, stock.0, "{label} rc");
        assert_eq!(zmin.2, stock.2, "{label} stderr");
        assert_eq!(stock.0 == 0, should_succeed, "{label} stock status");
    }
}

#[test]
fn daemon_raw_repository_errors_match_stock_informative_and_rc_rules() {
    let dir = TempDir::new().expect("raw daemon error temp dir");
    let base_path = format!("--base-path={}", dir.path().display());
    let request = pkt_line_bytes(b"git-upload-pack /missing.git");
    for informative in [false, true] {
        let mut args = vec!["daemon", "--inetd"];
        if informative {
            args.push("--informative-errors");
        }
        args.push(base_path.as_str());
        let stock = daemon_inetd_result("git", &args, &request);
        let zmin = daemon_inetd_result(zmin_bin(), &args, &request);
        assert_eq!(zmin, stock, "informative={informative}");
        assert_eq!(stock.0, 255, "informative={informative}");
        let expected = if informative {
            b"ERR no such repository: /missing.git".as_slice()
        } else {
            b"ERR access denied or repository not exported: /missing.git".as_slice()
        };
        assert_eq!(
            stock.1,
            pkt_line_bytes(expected),
            "informative={informative}"
        );
        assert!(!stock.1.ends_with(b"\n"), "informative={informative}");
    }
}

#[test]
fn daemon_raw_base_path_strict_allowlist_success_matches_stock_without_extended_args() {
    let dir = TempDir::new().expect("raw base-path daemon success temp dir");
    let root = dir.path().join("root");
    let remote = root.join("repo.git");
    fs::create_dir_all(&root).expect("daemon root");
    git(
        dir.path(),
        ["init", "--bare", remote.to_str().expect("remote path")],
    );
    fs::write(remote.join("git-daemon-export-ok"), b"").expect("export marker");
    let base_path = format!("--base-path={}", root.display());
    let args = [
        "daemon",
        "--inetd",
        "--strict-paths",
        base_path.as_str(),
        remote.to_str().expect("strict allowlist path"),
    ];
    let mut request = pkt_line_bytes(b"git-upload-pack /repo.git");
    request.extend_from_slice(b"0000");
    let stock = daemon_inetd_result("git", &args, &request);
    let zmin = daemon_inetd_result(zmin_bin(), &args, &request);
    assert_eq!(zmin.0, stock.0);
    assert_eq!(zmin.2, stock.2);
    assert_eq!(stock.0, 0);
    assert!(String::from_utf8_lossy(&stock.1).contains("capabilities^{}"));
    assert!(String::from_utf8_lossy(&zmin.1).contains("capabilities^{}"));
}

#[test]
fn daemon_raw_interpolated_request_rejection_matches_stock_without_extended_args() {
    let dir = TempDir::new().expect("raw interpolated daemon error temp dir");
    let root = dir.path().join("root");
    let remote = root.join("raw.git");
    let allowed = root.join("allowed");
    fs::create_dir_all(&root).expect("daemon root");
    fs::create_dir_all(&allowed).expect("allowlist directory");
    git(
        dir.path(),
        ["init", "--bare", remote.to_str().expect("remote path")],
    );
    fs::write(remote.join("git-daemon-export-ok"), b"").expect("export marker");
    let base_path = format!("--base-path={}", root.display());
    let interpolated_path = format!("--interpolated-path={}/%H%D", root.display());
    let request = pkt_line_bytes(b"git-upload-pack /raw.git");
    for informative in [false, true] {
        let mut args = vec!["daemon", "--inetd"];
        if informative {
            args.push("--informative-errors");
        }
        args.push("--strict-paths");
        args.push(base_path.as_str());
        args.push(interpolated_path.as_str());
        args.push(allowed.to_str().expect("allowlist path"));
        let stock = daemon_inetd_result("git", &args, &request);
        let zmin = daemon_inetd_result(zmin_bin(), &args, &request);
        assert_eq!(zmin, stock, "informative={informative}");
        assert_eq!(stock.0, 255, "informative={informative}");
        let expected = if informative {
            b"ERR no such repository: /raw.git".as_slice()
        } else {
            b"ERR access denied or repository not exported: /raw.git".as_slice()
        };
        assert_eq!(
            stock.1,
            pkt_line_bytes(expected),
            "informative={informative}"
        );
        assert!(!stock.1.ends_with(b"\n"), "informative={informative}");
    }
}

#[test]
fn daemon_resolver_errors_match_stock_informative_and_rc_rules() {
    let dir = TempDir::new().expect("resolver daemon error temp dir");
    let root = dir.path().join("root");
    let remote = root.join("localhost/interp.git");
    let allowed = root.join("allowed");
    fs::create_dir_all(remote.parent().expect("resolver remote parent"))
        .expect("resolver remote directory");
    fs::create_dir_all(&allowed).expect("resolver allowlist directory");
    git(
        dir.path(),
        ["init", "--bare", remote.to_str().expect("remote path")],
    );
    fs::write(remote.join("git-daemon-export-ok"), b"").expect("export marker");
    let base_path = format!("--base-path={}", root.display());
    let interpolated_path = format!("--interpolated-path={}/%H%D", root.display());
    let mut request = pkt_line_bytes(b"git-upload-pack /interp.git\0host=localhost\0");
    request.extend_from_slice(b"0000");
    for informative in [false, true] {
        let mut args = vec!["daemon", "--inetd"];
        if informative {
            args.push("--informative-errors");
        }
        args.push("--strict-paths");
        args.push(base_path.as_str());
        args.push(interpolated_path.as_str());
        args.push(allowed.to_str().expect("allowlist path"));
        let stock = daemon_inetd_result("git", &args, &request);
        let zmin = daemon_inetd_result(zmin_bin(), &args, &request);
        assert_eq!(zmin, stock, "informative={informative}");
        assert_eq!(stock.0, 255, "informative={informative}");
        let expected = if informative {
            b"ERR no such repository: /interp.git".as_slice()
        } else {
            b"ERR access denied or repository not exported: /interp.git".as_slice()
        };
        assert_eq!(
            stock.1,
            pkt_line_bytes(expected),
            "informative={informative}"
        );
        assert!(!stock.1.ends_with(b"\n"), "informative={informative}");
    }
}

#[test]
fn daemon_interpolated_missing_target_error_matches_stock_without_lf() {
    let dir = TempDir::new().expect("missing interpolated daemon temp dir");
    let base_path = format!("--base-path={}", dir.path().display());
    let interpolated_path = format!("--interpolated-path={}/%H%D", dir.path().display());
    let args = [
        "daemon",
        "--inetd",
        base_path.as_str(),
        interpolated_path.as_str(),
    ];
    let request = pkt_line_bytes(b"git-upload-pack /missing.git\0host=localhost\0");
    let stock = daemon_inetd_result("git", &args, &request);
    let zmin = daemon_inetd_result(zmin_bin(), &args, &request);
    assert_eq!(zmin, stock);
    assert!(stock.1.ends_with(b"/missing.git"));
    assert!(!stock.1.ends_with(b"/missing.git\n"));
}

#[test]
fn daemon_interpolated_hostname_traversal_matches_stock_rejection() {
    let fixture = TempDir::new().expect("traversal daemon fixture");
    let root = fixture.path().join("root");
    fs::create_dir(&root).expect("daemon root");
    let escape = fixture.path().join("escape.git");
    git(
        fixture.path(),
        ["init", "--bare", escape.to_str().expect("escape path")],
    );
    fs::write(escape.join("git-daemon-export-ok"), b"").expect("escape export marker");

    let base_path = format!("--base-path={}", root.display());
    let interpolated_path = format!("--interpolated-path={}/%H%D", root.display());
    let args = [
        "daemon",
        "--inetd",
        base_path.as_str(),
        interpolated_path.as_str(),
    ];
    let request = pkt_line_bytes(b"git-upload-pack /escape.git\0host=..\0");
    let stock = daemon_inetd_result("git", &args, &request);
    let zmin = daemon_inetd_result(zmin_bin(), &args, &request);
    assert_eq!(zmin, stock);
    assert_ne!(stock.0, 0);
}

fn daemon_inetd_failure(command: &str, args: &[&str], stdin: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    let output = daemon_inetd_result(command, args, stdin);
    assert!(
        output.0 != 0,
        "{command} unexpectedly succeeded: {}",
        String::from_utf8_lossy(&output.1)
    );
    output
}

fn daemon_inetd_result(command: &str, args: &[&str], stdin: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    let mut child = backend_command(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(stdin)
        .unwrap_or_else(|err| panic!("write {command} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
    (
        output.status.code().expect("exit code"),
        output.stdout,
        output.stderr,
    )
}

#[test]
fn maintenance_prefetch_reads_dumb_http_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"one\n").expect("write a");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["checkout", "-b", "feature"]);
    fs::write(source.join("feature.txt"), b"feature\n").expect("write feature");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "feature"]);
    git(&source, ["update-server-info"]);

    let server = StaticHttpServer::new(source.clone());
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    for client in [&git_client, &zmin_client] {
        git(dir.path(), ["init", client.to_str().expect("client path")]);
        git(client, ["remote", "add", "origin", url.as_str()]);
    }

    git(&git_client, ["maintenance", "run", "--task=prefetch"]);
    run_zmin(&zmin_client, ["maintenance", "run", "--task=prefetch"]);
    assert_eq!(
        git(
            &zmin_client,
            [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/prefetch",
            ],
        ),
        git(
            &git_client,
            [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/prefetch",
            ],
        )
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", "refs/prefetch/remotes/origin/main:a.txt"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "refs/prefetch/remotes/origin/main:a.txt"]
        )
    );
}

#[test]
fn maintenance_prefetch_reads_smart_http_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"one\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["checkout", "-b", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature"]);
    set_bare_head_to_main(&remote);

    let server = SmartHttpServer::new(dir.path().to_path_buf());
    let url = format!("http://127.0.0.1:{}/remote.git", server.port);
    for client in [&git_client, &zmin_client] {
        git(dir.path(), ["init", client.to_str().expect("client path")]);
        git(client, ["remote", "add", "origin", url.as_str()]);
    }

    git(&git_client, ["maintenance", "run", "--task=prefetch"]);
    run_zmin(&zmin_client, ["maintenance", "run", "--task=prefetch"]);
    assert_eq!(
        git(
            &zmin_client,
            [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/prefetch",
            ],
        ),
        git(
            &git_client,
            [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/prefetch",
            ],
        )
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", "refs/prefetch/remotes/origin/main:a.txt"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "refs/prefetch/remotes/origin/main:a.txt"]
        )
    );
}

#[test]
fn maintenance_prefetch_reads_git_daemon_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    fs::write(remote.join("git-daemon-export-ok"), "").expect("export marker");
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"one\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["checkout", "-b", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature"]);
    set_bare_head_to_main(&remote);

    let port = unused_local_port();
    let _daemon = StockGitDaemon::spawn(dir.path(), port);
    let url = format!("git://127.0.0.1:{port}/remote.git");
    for client in [&git_client, &zmin_client] {
        git(dir.path(), ["init", client.to_str().expect("client path")]);
        git(client, ["remote", "add", "origin", url.as_str()]);
    }

    git(&git_client, ["maintenance", "run", "--task=prefetch"]);
    run_zmin(&zmin_client, ["maintenance", "run", "--task=prefetch"]);
    assert_eq!(
        git(
            &zmin_client,
            [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/prefetch",
            ],
        ),
        git(
            &git_client,
            [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/prefetch",
            ],
        )
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", "refs/prefetch/remotes/origin/main:a.txt"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "refs/prefetch/remotes/origin/main:a.txt"]
        )
    );
}

#[test]
fn maintenance_prefetch_reads_ssh_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"one\n").expect("write a");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["checkout", "-b", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature"]);
    set_bare_head_to_main(&remote);

    let fake_ssh = write_fake_ssh(dir.path());
    let fake_ssh_arg = fake_ssh_command_arg(&fake_ssh);
    let url = ssh_url_for_remote(&remote);
    for client in [&git_client, &zmin_client] {
        git(dir.path(), ["init", client.to_str().expect("client path")]);
        git(client, ["remote", "add", "origin", url.as_str()]);
    }

    command_output_with_env(
        "git",
        &git_client,
        &["maintenance", "run", "--task=prefetch"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "git maintenance prefetch ssh",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["maintenance", "run", "--task=prefetch"],
        &[("GIT_SSH_COMMAND", fake_ssh_arg.as_str())],
        "zmin maintenance prefetch ssh",
    );
    assert_eq!(
        git(
            &zmin_client,
            [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/prefetch",
            ],
        ),
        git(
            &git_client,
            [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/prefetch",
            ],
        )
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", "refs/prefetch/remotes/origin/main:a.txt"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "refs/prefetch/remotes/origin/main:a.txt"]
        )
    );
}

#[test]
fn pull_reads_dumb_http_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"one\n").expect("write a");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["update-server-info"]);

    let server = StaticHttpServer::new(source.clone());
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    for client in [&git_client, &zmin_client] {
        git(
            dir.path(),
            ["init", "-b", "main", client.to_str().expect("client path")],
        );
        configure_identity(client);
        git(client, ["remote", "add", "origin", url.as_str()]);
        git(client, ["config", "branch.main.remote", "origin"]);
        git(client, ["config", "branch.main.merge", "refs/heads/main"]);
    }

    git(&git_client, ["pull", "--ff-only"]);
    run_zmin(&zmin_client, ["pull", "--ff-only"]);
    assert_eq!(
        fs::read_to_string(zmin_client.join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_client.join("a.txt")).expect("read git a")
    );
    assert_eq!(
        git(&zmin_client, ["rev-parse", "HEAD"]),
        git(&git_client, ["rev-parse", "HEAD"])
    );
}

#[test]
fn pull_rebase_reads_dumb_http_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"base\n").expect("write a");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    git(&source, ["update-server-info"]);

    let server = StaticHttpServer::new(source.clone());
    let url = format!("http://127.0.0.1:{}/.git", server.port);
    for client in [&git_client, &zmin_client] {
        git(
            dir.path(),
            ["init", "-b", "main", client.to_str().expect("client path")],
        );
        configure_identity(client);
        git(client, ["remote", "add", "origin", url.as_str()]);
        git(client, ["config", "branch.main.remote", "origin"]);
        git(client, ["config", "branch.main.merge", "refs/heads/main"]);
    }
    git(&git_client, ["pull", "--ff-only"]);
    run_zmin(&zmin_client, ["pull", "--ff-only"]);

    fs::write(git_client.join("local.txt"), b"local\n").expect("write git local");
    fs::write(zmin_client.join("local.txt"), b"local\n").expect("write zmin local");
    git(&git_client, ["add", "-A"]);
    git(&zmin_client, ["add", "-A"]);
    git_with_env(&git_client, ["commit", "-m", "local"]);
    git_with_env(&zmin_client, ["commit", "-m", "local"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);
    git(&source, ["update-server-info"]);

    git_with_env(&git_client, ["pull", "--rebase"]);
    run_zmin_with_env(&zmin_client, ["pull", "--rebase"]);

    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&zmin_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        run_zmin(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn http_push_puts_loose_objects_and_updates_remote_ref() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::create_dir_all(source.join("dir")).expect("create source dir");
    fs::write(source.join("dir/a.txt"), b"hello\n").expect("write a");
    fs::write(source.join("root.txt"), b"root\n").expect("write root");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    let head = git(&source, ["rev-parse", "HEAD"]);
    let server = WritableHttpServer::new();
    let url = server.url();

    assert_eq!(
        run_zmin(&source, ["http-push", url.as_str(), "main"]),
        "main -> main"
    );

    let remote_git = server.remote_git_dir();
    assert_eq!(
        fs::read_to_string(remote_git.join("refs/heads/main"))
            .expect("read pushed main ref")
            .trim(),
        head
    );
    for object in git(&source, ["rev-list", "--objects", "--all"]).lines() {
        let id = object.split_whitespace().next().expect("object id");
        let local = source.join(".git/objects").join(&id[..2]).join(&id[2..]);
        let remote = remote_git.join("objects").join(&id[..2]).join(&id[2..]);
        assert_eq!(
            fs::read(remote).unwrap_or_else(|err| panic!("read remote object {id}: {err}")),
            fs::read(local).unwrap_or_else(|err| panic!("read local object {id}: {err}")),
        );
    }
}

#[test]
fn http_push_deletes_remote_refspec() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("root.txt"), b"root\n").expect("write root");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    let server = WritableHttpServer::new();
    let url = server.url();
    let remote_git = server.remote_git_dir();
    fs::create_dir_all(remote_git.join("refs/heads")).expect("create remote refs");
    fs::write(
        remote_git.join("refs/heads/topic"),
        git(&source, ["rev-parse", "HEAD"]) + "\n",
    )
    .expect("write remote topic ref");

    assert_eq!(
        run_zmin(&source, ["http-push", url.as_str(), ":topic"]),
        "(delete) -> topic"
    );
    assert!(
        !remote_git.join("refs/heads/topic").exists(),
        "remote topic ref should be deleted"
    );

    assert_eq!(
        run_zmin(
            &source,
            ["http-push", "--dry-run", url.as_str(), ":missing"]
        ),
        "(delete) -> missing (dry run)"
    );
}

#[test]
fn http_push_short_delete_aliases_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("root.txt"), b"root\n").expect("write root");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    let head = git(&source, ["rev-parse", "HEAD"]);

    for short_flag in ["-d", "-D"] {
        let server = WritableHttpServer::new();
        let url = server.url();
        let remote_git = server.remote_git_dir();
        fs::create_dir_all(remote_git.join("refs/heads")).expect("create remote refs");
        fs::write(remote_git.join("refs/heads/main"), format!("{head}\n")).expect("write main");
        fs::write(remote_git.join("refs/heads/topic"), format!("{head}\n")).expect("write topic");

        let git_args = ["http-push", short_flag, url.as_str(), "topic"];
        let stock = command_any_output(
            stock_git_bin().to_str().expect("stock git path"),
            &source,
            &git_args,
            "stock git http-push short delete",
        );
        fs::write(remote_git.join("refs/heads/topic"), format!("{head}\n")).expect("restore topic");
        let zmin = command_any_output(
            zmin_bin(),
            &source,
            &git_args,
            "zmin http-push short delete",
        );

        assert_eq!(zmin, stock, "{short_flag}");
        assert!(
            remote_git.join("refs/heads/topic").exists(),
            "{short_flag} should preserve the remote topic ref on the current writable HTTP lane"
        );
    }
}
