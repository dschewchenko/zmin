mod common;

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use common::{configure_identity, git, test_command_program, write_file, zmin_bin};
use tempfile::{Builder, TempDir};

const WAIT_TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

fn guarded_tempdir(label: &str) -> TempDir {
    let root = std::env::temp_dir();
    assert!(
        root.is_dir(),
        "required guarded test root is unavailable: {root:?}"
    );
    let dir = Builder::new()
        .prefix(&format!("zmin-{label}-"))
        .tempdir_in(&root)
        .expect("create guarded temporary fixture");
    assert!(
        dir.path().starts_with(root),
        "fixture escaped guarded root: {}",
        dir.path().display()
    );
    dir
}

fn hermetic_zmin_command(cwd: &Path, home: &Path) -> Command {
    let mut command = Command::new(test_command_program(zmin_bin()));
    command
        .current_dir(cwd)
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("PATH", std::env::var_os("PATH").expect("test PATH"));
    command
}

struct ChildGuard {
    child: Child,
}

impl ChildGuard {
    fn wait_for_socket(&mut self, socket: &Path) {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        while Instant::now() < deadline {
            if socket.exists() {
                return;
            }
            if let Some(status) = self.child.try_wait().expect("poll daemon status") {
                panic!("daemon exited before socket appeared: {status}");
            }
            thread::sleep(POLL_INTERVAL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        panic!("daemon socket did not appear: {}", socket.display());
    }

    fn wait_for_exit(&mut self) -> ExitStatus {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("poll child status") {
                return status;
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                let status = self.child.wait().expect("wait timed-out child");
                panic!("child did not exit within {:?}: {status}", WAIT_TIMEOUT);
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn terminate(&mut self) {
        if self
            .child
            .try_wait()
            .expect("poll child before terminate")
            .is_none()
        {
            self.child.kill().expect("terminate child");
        }
        self.child.wait().expect("reap child");
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

#[cfg(unix)]
fn spawn_credential_daemon(cwd: &Path, home: &Path, socket: &Path) -> ChildGuard {
    let mut command = hermetic_zmin_command(cwd, home);
    let child = command
        .args([
            "credential-cache",
            "--daemon-internal",
            "--socket",
            socket.to_str().expect("socket path UTF-8"),
            "--timeout",
            "60",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn credential-cache internal daemon");
    ChildGuard { child }
}

#[cfg(unix)]
fn credential_request(socket: &Path, request: &[u8]) -> String {
    use std::os::unix::net::UnixStream;

    let deadline = Instant::now() + WAIT_TIMEOUT;
    let mut stream = loop {
        match UnixStream::connect(socket) {
            Ok(stream) => break stream,
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                thread::sleep(POLL_INTERVAL);
            }
            Err(error) => panic!("connect credential daemon socket: {error}"),
        }
    };
    stream
        .set_read_timeout(Some(WAIT_TIMEOUT))
        .expect("set credential socket timeout");
    stream.write_all(request).expect("write credential request");
    stream
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown credential request");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .expect("read credential response");
    String::from_utf8(response).expect("credential response UTF-8")
}

#[cfg(unix)]
#[test]
fn credential_cache_internal_daemon_covers_socket_lifecycle_and_invalid_protocol() {
    let fixture = guarded_tempdir("credential-internal");
    let home = fixture.path().join("home");
    let cwd = fixture.path().join("cwd");
    fs::create_dir_all(&home).expect("create isolated home");
    fs::create_dir_all(&cwd).expect("create isolated cwd");

    let competitor_socket = fixture.path().join("r.sock");
    let competitor = std::os::unix::net::UnixListener::bind(&competitor_socket)
        .expect("bind competing credential socket");
    let mut competing_daemon = spawn_credential_daemon(&cwd, &home, &competitor_socket);
    let competing_status = competing_daemon.wait_for_exit();
    assert!(
        !competing_status.success(),
        "credential daemon unexpectedly replaced competing listener: {competing_status}"
    );
    assert!(
        competitor_socket.exists(),
        "credential daemon removed competing socket on bind failure"
    );
    drop(competitor);
    fs::remove_file(&competitor_socket).expect("remove competing socket fixture");

    let socket = fixture.path().join("c.sock");
    let mut daemon = spawn_credential_daemon(&cwd, &home, &socket);
    daemon.wait_for_socket(&socket);

    assert_eq!(
        credential_request(
            &socket,
            b"store\nprotocol=https\nhost=internal.example\nusername=alice\npassword=secret\n\n",
        ),
        ""
    );
    let stored = credential_request(&socket, b"get\nprotocol=https\nhost=internal.example\n\n");
    assert!(stored.contains("username=alice\n"));
    assert!(stored.contains("password=secret\n"));

    assert_eq!(
        credential_request(&socket, b"erase\nprotocol=https\nhost=internal.example\n\n",),
        ""
    );
    assert_eq!(
        credential_request(&socket, b"get\nprotocol=https\nhost=internal.example\n\n"),
        ""
    );

    assert_eq!(credential_request(&socket, b"exit\n\n"), "");
    let exit_status = daemon.wait_for_exit();
    assert!(
        exit_status.success(),
        "credential daemon exit failed: {exit_status}"
    );
    assert!(!socket.exists(), "credential daemon left its socket behind");

    let invalid_socket = fixture.path().join("i.sock");
    let mut invalid_daemon = spawn_credential_daemon(&cwd, &home, &invalid_socket);
    invalid_daemon.wait_for_socket(&invalid_socket);
    assert_eq!(
        credential_request(&invalid_socket, b"missing-action-newline"),
        ""
    );
    let invalid_status = invalid_daemon.wait_for_exit();
    assert!(
        !invalid_status.success(),
        "invalid daemon request unexpectedly succeeded"
    );
    assert!(
        !invalid_socket.exists(),
        "credential daemon left its socket after malformed protocol"
    );
}

fn unused_local_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("bind local readiness port")
        .local_addr()
        .expect("read local readiness port")
        .port()
}

fn wait_for_http(port: u16, child: &mut ChildGuard) {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    while Instant::now() < deadline {
        if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) {
            stream
                .set_read_timeout(Some(WAIT_TIMEOUT))
                .expect("set readiness timeout");
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return;
        }
        if let Some(status) = child.child.try_wait().expect("poll instaweb status") {
            panic!("instaweb daemon exited before readiness: {status}");
        }
        thread::sleep(POLL_INTERVAL);
    }
    child.terminate();
    panic!("instaweb daemon did not become ready on 127.0.0.1:{port}");
}

fn http_get_local(port: u16) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect instaweb");
    stream
        .set_read_timeout(Some(WAIT_TIMEOUT))
        .expect("set HTTP read timeout");
    write!(
        stream,
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .expect("write HTTP request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read HTTP response");
    response
}

#[test]
fn instaweb_internal_daemon_uses_explicit_paths_and_has_bounded_lifecycle() {
    let fixture = guarded_tempdir("instaweb-internal");
    let repo = fixture.path().join("explicit-repository");
    let cwd = fixture.path().join("cwd-decoy");
    let home = fixture.path().join("home");
    fs::create_dir_all(&cwd).expect("create decoy cwd");
    fs::create_dir_all(&home).expect("create isolated home");
    fs::create_dir_all(&repo).expect("create repository directory");
    git(&repo, ["init", "-q"]);
    configure_identity(&repo);
    write_file(&repo, "README.md", "explicit path fixture\n");
    git(&repo, ["add", "README.md"]);
    git(&repo, ["commit", "-q", "-m", "explicit instaweb paths"]);
    let git_dir = repo.join(".git");
    let port = unused_local_port();

    let mut command = hermetic_zmin_command(&cwd, &home);
    let mut daemon = ChildGuard {
        child: command
            .args([
                "instaweb",
                "--daemon-internal",
                "--local",
                "--port",
                &port.to_string(),
                "--git-dir",
                git_dir.to_str().expect("git dir path UTF-8"),
                "--work-tree",
                repo.to_str().expect("work tree path UTF-8"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn instaweb internal daemon"),
    };
    wait_for_http(port, &mut daemon);
    let response = http_get_local(port);
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "unexpected HTTP response: {response}"
    );
    assert!(response.contains("explicit instaweb paths"));
    assert!(
        response.contains(
            repo.file_name()
                .and_then(|name| name.to_str())
                .expect("repository name UTF-8")
        )
    );
    daemon.terminate();
}

#[test]
fn internal_options_are_schema_accepted_but_hidden_from_help_and_reject_missing_paths() {
    let fixture = guarded_tempdir("internal-schema");
    let cwd = fixture.path().join("cwd");
    let home = fixture.path().join("home");
    fs::create_dir_all(&cwd).expect("create schema cwd");
    fs::create_dir_all(&home).expect("create schema home");
    let socket = fixture.path().join("h.sock");

    let credential_help = hermetic_zmin_command(&cwd, &home)
        .args([
            "credential-cache",
            "--daemon-internal",
            "--socket",
            socket.to_str().expect("socket path UTF-8"),
            "--definitely-invalid",
        ])
        .output()
        .expect("render credential-cache usage");
    assert!(!credential_help.status.success());
    let credential_help =
        String::from_utf8(credential_help.stderr).expect("credential usage UTF-8");
    assert!(credential_help.contains("Usage: zmin credential-cache"));
    assert!(!credential_help.contains("--daemon-internal"));

    let instaweb_help = hermetic_zmin_command(&cwd, &home)
        .args(["instaweb", "--daemon-internal", "--definitely-invalid"])
        .output()
        .expect("render instaweb usage");
    assert!(!instaweb_help.status.success());
    let instaweb_help = String::from_utf8(instaweb_help.stderr).expect("instaweb usage UTF-8");
    assert!(instaweb_help.contains("Usage: zmin instaweb [OPTIONS]"));
    assert!(!instaweb_help.contains("--daemon-internal"));
    assert!(!instaweb_help.contains("--git-dir"));
    assert!(!instaweb_help.contains("--work-tree"));

    let missing_work_tree = hermetic_zmin_command(&cwd, &home)
        .args([
            "instaweb",
            "--daemon-internal",
            "--git-dir",
            fixture
                .path()
                .join("missing-git-dir")
                .to_str()
                .expect("path UTF-8"),
        ])
        .output()
        .expect("run missing work-tree invocation");
    assert!(!missing_work_tree.status.success());
    assert!(String::from_utf8_lossy(&missing_work_tree.stderr).contains("requires --work-tree"));

    let missing_git_dir = hermetic_zmin_command(&cwd, &home)
        .args([
            "instaweb",
            "--daemon-internal",
            "--work-tree",
            cwd.to_str().expect("cwd path UTF-8"),
        ])
        .output()
        .expect("run missing git-dir invocation");
    assert!(!missing_git_dir.status.success());
    assert!(String::from_utf8_lossy(&missing_git_dir.stderr).contains("requires --git-dir"));
}
