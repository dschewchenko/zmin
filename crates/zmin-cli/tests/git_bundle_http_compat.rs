mod common;

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tempfile::TempDir;

use common::{
    command_any_output, configure_identity, ensure_remote_http_helper, git, git_args, git_with_env,
    zmin_bin,
};

fn bundle_http_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("bundle test lock")
}

fn assert_external_fixture_root(dir: &Path) {
    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    let fixture_root = fs::canonicalize(dir).expect("fixture root");
    let repository_root = fs::canonicalize(repository_root).expect("repository root");
    assert!(
        !fixture_root.starts_with(&repository_root),
        "fixture init/commit cwd must stay outside the shared repository: {}",
        fixture_root.display()
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResponseMode {
    Files,
    BundleNotFound,
    BundleUnauthorized,
    BundleOversize,
    BundleTimeout,
}

struct BundleHttpServer {
    root: PathBuf,
    mode: ResponseMode,
    redirect_hops: usize,
    redirect_target: Option<String>,
    smart_origin: bool,
    port: u16,
    stop: Arc<std::sync::atomic::AtomicBool>,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    handle: Option<JoinHandle<()>>,
}

impl BundleHttpServer {
    fn new(root: PathBuf, mode: ResponseMode) -> Self {
        Self::new_with_redirects(root, mode, 0, None)
    }

    fn new_smart(root: PathBuf) -> Self {
        Self::new_with_options(root, ResponseMode::Files, 0, None, true)
    }

    fn new_with_redirects(
        root: PathBuf,
        mode: ResponseMode,
        redirect_hops: usize,
        redirect_target: Option<String>,
    ) -> Self {
        Self::new_with_options(root, mode, redirect_hops, redirect_target, false)
    }

    fn new_with_options(
        root: PathBuf,
        mode: ResponseMode,
        redirect_hops: usize,
        redirect_target: Option<String>,
        smart_origin: bool,
    ) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind bundle HTTP server");
        let port = listener
            .local_addr()
            .expect("bundle HTTP server address")
            .port();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let thread_stop = stop.clone();
        let thread_requests = requests.clone();
        let thread_root = root.clone();
        let thread_redirect_target = redirect_target.clone();
        let thread_smart_origin = smart_origin;
        let handle = thread::spawn(move || {
            while !thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let request = read_headers(&mut stream);
                if let Ok(request) = request {
                    thread_requests
                        .lock()
                        .expect("bundle request lock")
                        .push(request.clone());
                    serve_request(
                        &thread_root,
                        mode,
                        redirect_hops,
                        thread_redirect_target.as_deref(),
                        &request,
                        &mut stream,
                        thread_smart_origin,
                    );
                }
            }
        });
        Self {
            root,
            mode,
            redirect_hops,
            redirect_target,
            smart_origin,
            port,
            stop,
            requests,
            handle: Some(handle),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .expect("bundle request lock")
            .iter()
            .map(|request| String::from_utf8_lossy(request).into_owned())
            .collect()
    }

    fn origin_object_requests(&self) -> usize {
        self.requests()
            .iter()
            .filter(|request| {
                request
                    .lines()
                    .next()
                    .is_some_and(|line| line.contains("/objects/"))
            })
            .count()
    }

    fn origin_upload_pack_requests(&self) -> usize {
        self.requests()
            .iter()
            .filter(|request| {
                request
                    .lines()
                    .next()
                    .is_some_and(|line| line.starts_with("POST /source/.git/git-upload-pack "))
            })
            .count()
    }

    fn bundle_requests(&self) -> usize {
        self.requests()
            .iter()
            .filter(|request| {
                request
                    .lines()
                    .next()
                    .is_some_and(|line| line.starts_with("GET /bundle "))
            })
            .count()
    }
}

impl Drop for BundleHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            handle.join().expect("join bundle HTTP server");
        }
        let _ = (
            &self.root,
            self.mode,
            self.redirect_hops,
            &self.redirect_target,
            self.smart_origin,
        );
    }
}

struct HttpProxyServer {
    target_port: u16,
    port: u16,
    stop: Arc<std::sync::atomic::AtomicBool>,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    handle: Option<JoinHandle<()>>,
}

impl HttpProxyServer {
    fn new(target_port: u16) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind HTTP proxy server");
        let port = listener
            .local_addr()
            .expect("HTTP proxy server address")
            .port();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let thread_stop = stop.clone();
        let thread_requests = requests.clone();
        let handle = thread::spawn(move || {
            while !thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let Ok(request) = read_headers(&mut stream) else {
                    continue;
                };
                if request.is_empty() {
                    continue;
                }
                thread_requests
                    .lock()
                    .expect("HTTP proxy request lock")
                    .push(request.clone());
                serve_proxy_request(target_port, &request, &mut stream);
            }
        });
        Self {
            target_port,
            port,
            stop,
            requests,
            handle: Some(handle),
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .expect("HTTP proxy request lock")
            .iter()
            .map(|request| String::from_utf8_lossy(request).into_owned())
            .collect()
    }

    fn bundle_requests(&self) -> usize {
        self.requests()
            .iter()
            .filter(|request| {
                request.lines().next().is_some_and(|line| {
                    line.split_ascii_whitespace()
                        .nth(1)
                        .is_some_and(|target| target.contains("/bundle"))
                })
            })
            .count()
    }
}

impl Drop for HttpProxyServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            handle.join().expect("join HTTP proxy server");
        }
        let _ = self.target_port;
    }
}

const TLS_SERVER_SCRIPT: &str = r#"
import pathlib
import socket
import ssl
import sys

port = int(sys.argv[1])
root = pathlib.Path(sys.argv[2])
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(root / "server.pem", root / "server.key")
context.verify_mode = ssl.CERT_REQUIRED
context.load_verify_locations(cafile=root / "ca.pem")
with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", port))
    listener.listen(1)
    connection, _ = listener.accept()
    with context.wrap_socket(connection, server_side=True) as stream:
        stream.settimeout(5)
        request = bytearray()
        while b"\r\n\r\n" not in request:
            chunk = stream.recv(4096)
            if not chunk:
                break
            request.extend(chunk)
        body = b"TLS bundle fixture\n"
        response = (
            b"HTTP/1.1 200 OK\r\nContent-Length: "
            + str(len(body)).encode("ascii")
            + b"\r\nConnection: close\r\n\r\n"
            + body
        )
        stream.sendall(response)
"#;

struct TlsTestServer {
    child: Option<Child>,
}

impl TlsTestServer {
    fn start(dir: &Path, port: u16) -> Self {
        let child = Command::new("python3")
            .args([
                "-c",
                TLS_SERVER_SCRIPT,
                &port.to_string(),
                dir.to_str().expect("TLS fixture path is UTF-8"),
            ])
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(
                fs::File::create(dir.join("tls-server.log")).expect("create TLS server log"),
            ))
            .spawn()
            .expect("spawn OpenSSL TLS fixture server");
        Self { child: Some(child) }
    }

    fn wait_until_listening(&mut self) {
        thread::sleep(Duration::from_millis(100));
        assert!(
            self.child
                .as_mut()
                .is_some_and(|child| child.try_wait().ok().flatten().is_none()),
            "OpenSSL TLS fixture server did not start"
        );
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for TlsTestServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn read_headers(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            return Ok(request);
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(request);
        }
    }
}

fn serve_proxy_request(target_port: u16, request: &[u8], stream: &mut TcpStream) {
    let Some(header_end) = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
    else {
        return;
    };
    let request_text = String::from_utf8_lossy(request);
    let content_length = request_text
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap_or(0))
        })
        .unwrap_or(0);
    let request_len = header_end.saturating_add(content_length).min(request.len());
    let Some(request_line_end) = request[..request_len]
        .windows(2)
        .position(|window| window == b"\r\n")
    else {
        return;
    };
    let Some((method, target, version)) = request_text.lines().next().and_then(|line| {
        let mut fields = line.split_ascii_whitespace();
        Some((fields.next()?, fields.next()?, fields.next()?))
    }) else {
        return;
    };
    let path = proxy_request_path(target);
    let replacement = format!("{method} {path} {version}\r\n");
    let mut forwarded = Vec::with_capacity(request_len + replacement.len());
    forwarded.extend_from_slice(replacement.as_bytes());
    forwarded.extend_from_slice(&request[request_line_end..request_len]);
    let Ok(mut upstream) = TcpStream::connect(("127.0.0.1", target_port)) else {
        return;
    };
    let _ = upstream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = upstream.set_write_timeout(Some(Duration::from_secs(5)));
    if upstream.write_all(&forwarded).is_err() {
        return;
    }
    let mut response = Vec::new();
    if upstream.read_to_end(&mut response).is_ok() {
        let _ = stream.write_all(&response);
    }
}

fn proxy_request_path(target: &str) -> &str {
    let authority = target
        .strip_prefix("http://")
        .or_else(|| target.strip_prefix("https://"))
        .unwrap_or(target);
    authority
        .find('/')
        .map(|position| &authority[position..])
        .unwrap_or("/")
}

fn run_openssl_fixture_command(dir: &Path, args: &[&str]) {
    let output = Command::new("openssl")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run OpenSSL TLS fixture command");
    assert!(
        output.status.success(),
        "OpenSSL fixture command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn generate_openssl_tls_fixture(dir: &Path) {
    fs::write(
        dir.join("server.ext"),
        "subjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\n",
    )
    .expect("write OpenSSL server extensions");
    fs::write(dir.join("client.ext"), "extendedKeyUsage=clientAuth\n")
        .expect("write OpenSSL client extensions");
    run_openssl_fixture_command(
        dir,
        &[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "ca.key",
            "-out",
            "ca.pem",
            "-days",
            "1",
            "-subj",
            "/CN=zmin-test-ca",
        ],
    );
    run_openssl_fixture_command(
        dir,
        &[
            "req",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "server.key",
            "-out",
            "server.csr",
            "-subj",
            "/CN=localhost",
        ],
    );
    run_openssl_fixture_command(
        dir,
        &[
            "x509",
            "-req",
            "-in",
            "server.csr",
            "-CA",
            "ca.pem",
            "-CAkey",
            "ca.key",
            "-CAcreateserial",
            "-out",
            "server.pem",
            "-days",
            "1",
            "-extfile",
            "server.ext",
        ],
    );
    run_openssl_fixture_command(
        dir,
        &[
            "req",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "client.key",
            "-out",
            "client.csr",
            "-subj",
            "/CN=zmin-test-client",
        ],
    );
    run_openssl_fixture_command(
        dir,
        &[
            "x509",
            "-req",
            "-in",
            "client.csr",
            "-CA",
            "ca.pem",
            "-CAkey",
            "ca.key",
            "-CAcreateserial",
            "-out",
            "client.pem",
            "-days",
            "1",
            "-extfile",
            "client.ext",
        ],
    );
}

fn serve_request(
    root: &Path,
    mode: ResponseMode,
    redirect_hops: usize,
    redirect_target: Option<&str>,
    request: &[u8],
    stream: &mut TcpStream,
    smart_origin: bool,
) {
    let mut request = request.to_vec();
    let Some(header_end) = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
    else {
        return;
    };
    let headers = String::from_utf8_lossy(&request[..header_end - 4]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap_or(0))
        })
        .unwrap_or(0);
    while request.len().saturating_sub(header_end) < content_length {
        let mut buffer = [0_u8; 4096];
        let Ok(read) = stream.read(&mut buffer) else {
            return;
        };
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    request.truncate(header_end.saturating_add(content_length).min(request.len()));
    let request_text = String::from_utf8_lossy(&request);
    let Some(raw_path) = request_text
        .lines()
        .next()
        .and_then(|line| line.split_ascii_whitespace().nth(1))
    else {
        return;
    };
    let (path, query) = raw_path.split_once('?').unwrap_or((raw_path, ""));
    if smart_origin && path.starts_with("/source/.git/") {
        serve_git_http_backend(
            root,
            path,
            query,
            &request_text,
            &request[header_end..],
            stream,
        );
        return;
    }
    if path.starts_with("/redirect/") {
        let hop = path
            .trim_start_matches("/redirect/")
            .parse::<usize>()
            .unwrap_or(0);
        let location = if hop + 1 < redirect_hops {
            format!("/redirect/{}", hop + 1)
        } else if let Some(target) = redirect_target {
            target.to_owned()
        } else {
            "/bundle".to_owned()
        };
        let response = format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(response.as_bytes());
        return;
    }
    if mode == ResponseMode::BundleNotFound && path == "/bundle" {
        write_response(stream, "404 Not Found", &[], None);
        return;
    }
    if mode == ResponseMode::BundleUnauthorized && path == "/bundle" {
        write_response(stream, "401 Unauthorized", b"unauthorized\n", None);
        return;
    }
    if mode == ResponseMode::BundleOversize && path == "/bundle" {
        write_response(stream, "200 OK", &[], Some(1024_u64 * 1024 * 1024 + 1));
        return;
    }
    if mode == ResponseMode::BundleTimeout && path == "/bundle" {
        thread::sleep(Duration::from_millis(250));
        write_response(stream, "200 OK", b"timeout fixture\n", None);
        return;
    }
    let relative = path.trim_start_matches('/');
    if relative.split('/').any(|component| component == "..") {
        write_response(stream, "400 Bad Request", &[], None);
        return;
    }
    match fs::read(root.join(relative)) {
        Ok(body) => write_response(stream, "200 OK", &body, None),
        Err(_) => write_response(stream, "404 Not Found", &[], None),
    }
}

fn serve_git_http_backend(
    project_root: &Path,
    path: &str,
    query: &str,
    request: &str,
    body: &[u8],
    stream: &mut TcpStream,
) {
    let method = request
        .lines()
        .next()
        .and_then(|line| line.split_ascii_whitespace().next())
        .unwrap_or("GET");
    let git_protocol = request.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("git-protocol")
            .then(|| value.trim().to_owned())
    });
    let mut command = Command::new("git");
    command
        .arg("http-backend")
        .env("GIT_PROJECT_ROOT", project_root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_INFO", path)
        .env("QUERY_STRING", query)
        .env("REQUEST_METHOD", method)
        .env("CONTENT_LENGTH", body.len().to_string())
        .env("CONTENT_TYPE", "application/x-git-upload-pack-request")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    if let Some(git_protocol) = git_protocol {
        command.env("HTTP_GIT_PROTOCOL", git_protocol);
    }
    let output = command
        .spawn()
        .and_then(|mut child| {
            if !body.is_empty() {
                child
                    .stdin
                    .as_mut()
                    .expect("git backend stdin")
                    .write_all(body)?;
            }
            child.wait_with_output()
        })
        .expect("run git http-backend");
    assert!(
        output.status.success(),
        "git http-backend failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    write_backend_response(stream, &output.stdout);
}

fn write_backend_response(stream: &mut TcpStream, response: &[u8]) {
    let (header_end, separator_len) = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| (position, 4))
        .or_else(|| {
            response
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|position| (position, 2))
        })
        .expect("git backend response headers");
    let headers = String::from_utf8_lossy(&response[..header_end]);
    let mut status = "200 OK";
    let mut content_length = None;
    let mut out = Vec::new();
    for line in headers.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("status") {
            status = value.trim();
        } else if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(value.trim().to_owned());
        } else if !name.eq_ignore_ascii_case("connection") {
            out.extend_from_slice(line.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
    }
    if let Some(content_length) = content_length {
        out.extend_from_slice(b"Content-Length: ");
        out.extend_from_slice(content_length.as_bytes());
        out.extend_from_slice(b"\r\n");
    } else {
        out.extend_from_slice(b"Content-Length: ");
        out.extend_from_slice(
            (response.len() - header_end - separator_len)
                .to_string()
                .as_bytes(),
        );
        out.extend_from_slice(b"\r\n");
    }
    let mut http = format!("HTTP/1.1 {status}\r\n").into_bytes();
    http.extend_from_slice(&out);
    http.extend_from_slice(b"Connection: close\r\n\r\n");
    http.extend_from_slice(&response[header_end + separator_len..]);
    let _ = stream.write_all(&http);
}

fn write_response(stream: &mut TcpStream, status: &str, body: &[u8], length: Option<u64>) {
    let content_length = length.unwrap_or(body.len() as u64);
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {content_length}\r\nConnection: close\r\n\r\n"
    );
    let _ = stream.write_all(header.as_bytes());
    if !body.is_empty() {
        let _ = stream.write_all(body);
    }
}

fn prepare_repository(dir: &Path, object_format: Option<&str>) -> (PathBuf, PathBuf) {
    assert_external_fixture_root(dir);
    let source = dir.join("source");
    let init_args = if object_format.is_some() {
        vec![
            "init",
            "-b",
            "main",
            "--object-format=sha256",
            source.to_str().unwrap(),
        ]
    } else {
        vec!["init", "-b", "main", source.to_str().unwrap()]
    };
    git_args(dir, &init_args);
    configure_identity(&source);
    fs::write(source.join("tracked.txt"), b"bundle fixture\n").expect("write fixture");
    git(&source, ["add", "tracked.txt"]);
    git_with_env(&source, ["commit", "-m", "bundle fixture"]);
    git(&source, ["update-server-info"]);
    let bundle = dir.join("bundle");
    git(
        &source,
        [
            "bundle",
            "create",
            bundle.to_str().expect("bundle path"),
            "--all",
        ],
    );
    (source, bundle)
}

fn prepare_partial_repository(dir: &Path, object_format: Option<&str>) -> (PathBuf, PathBuf) {
    assert_external_fixture_root(dir);
    let source = dir.join("source");
    let init_args = if object_format.is_some() {
        vec![
            "init",
            "-b",
            "main",
            "--object-format=sha256",
            source.to_str().unwrap(),
        ]
    } else {
        vec!["init", "-b", "main", source.to_str().unwrap()]
    };
    git_args(dir, &init_args);
    configure_identity(&source);
    fs::write(source.join("tracked.txt"), b"bundle fixture one\n").expect("write first fixture");
    git(&source, ["add", "tracked.txt"]);
    git_with_env(&source, ["commit", "-m", "bundle fixture one"]);
    fs::write(source.join("second.txt"), b"bundle fixture two\n").expect("write second fixture");
    git(&source, ["add", "second.txt"]);
    git_with_env(&source, ["commit", "-m", "bundle fixture two"]);
    git(&source, ["update-server-info"]);
    let bundle = dir.join("bundle");
    git(
        &source,
        [
            "bundle",
            "create",
            bundle.to_str().expect("bundle path"),
            "HEAD~1..HEAD",
        ],
    );
    (source, bundle)
}

fn prepare_filtered_repository(dir: &Path, object_format: Option<&str>) -> (PathBuf, PathBuf) {
    assert_external_fixture_root(dir);
    let source = dir.join("source");
    let init_args = if object_format.is_some() {
        vec![
            "init",
            "-b",
            "main",
            "--object-format=sha256",
            source.to_str().unwrap(),
        ]
    } else {
        vec!["init", "-b", "main", source.to_str().unwrap()]
    };
    git_args(dir, &init_args);
    configure_identity(&source);
    fs::write(source.join("tracked.txt"), vec![b'x'; 4096]).expect("write filtered fixture");
    git(&source, ["add", "tracked.txt"]);
    git_with_env(&source, ["commit", "-m", "filtered bundle fixture"]);
    git(&source, ["update-server-info"]);
    let bundle = dir.join("bundle");
    git(
        &source,
        [
            "bundle",
            "create",
            bundle.to_str().expect("bundle path"),
            "--filter=blob:none",
            "--all",
        ],
    );
    (source, bundle)
}

fn temp_http_entries() -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("zmin-http-body-"))
        })
        .collect()
}

fn assert_no_new_http_temp_entries(before: &[PathBuf]) {
    let after = temp_http_entries();
    assert!(
        after.iter().all(|path| before.contains(path)),
        "HTTP bundle loader left temporary files: {after:?}"
    );
}

fn clone_http_bundle(
    dir: &TempDir,
    server: &BundleHttpServer,
    bundle_uri: &str,
    extra_args: &[&str],
) -> (i32, String, String, PathBuf) {
    let destination = dir.path().join("clone");
    let origin = server.url("/source/.git");
    let mut args = vec!["clone", "--no-checkout"];
    args.extend_from_slice(extra_args);
    let bundle_arg = format!("--bundle-uri={bundle_uri}");
    args.push(&bundle_arg);
    args.push(&origin);
    args.push(destination.to_str().expect("destination path"));
    let output = command_any_output(zmin_bin(), dir.path(), &args, "HTTP bundle clone");
    (output.0, output.1, output.2, destination)
}

fn clone_http_bundle_without_proxy_env(
    dir: &TempDir,
    server: &BundleHttpServer,
    bundle_uri: &str,
    extra_args: &[&str],
) -> (i32, String, String, PathBuf) {
    let destination = dir.path().join("clone");
    let origin = server.url("/source/.git");
    let mut args = vec!["clone", "--no-checkout"];
    args.extend_from_slice(extra_args);
    let bundle_arg = format!("--bundle-uri={bundle_uri}");
    args.push(&bundle_arg);
    args.push(&origin);
    args.push(destination.to_str().expect("destination path"));
    let mut command = Command::new(zmin_bin());
    command
        .args(&args)
        .current_dir(dir.path())
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .env_remove("HTTPS_PROXY")
        .env_remove("https_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy");
    let output = command.output().expect("HTTP bundle clone through proxy");
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
        destination,
    )
}

#[test]
fn direct_http_bundle_complete_sha1_skips_origin_objects_and_cleans_temp_state() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle fixture directory");
    let (source, bundle) = prepare_repository(dir.path(), None);
    let _helper = ensure_remote_http_helper();
    let server = BundleHttpServer::new(dir.path().to_path_buf(), ResponseMode::Files);
    let destination = dir.path().join("clone");
    let origin = server.url("/source/.git");
    let bundle_uri = server.url("/bundle");
    let output = command_any_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--no-checkout",
            &format!("--bundle-uri={bundle_uri}"),
            &origin,
            destination.to_str().expect("destination path"),
        ],
        "complete direct HTTP bundle clone",
    );
    assert_eq!(output.0, 0, "{}", output.2);
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        server.origin_object_requests(),
        0,
        "origin object fetch was not suppressed"
    );
    assert!(destination.join(".git/objects/pack").exists());
    assert!(!destination.join(".git/FETCH_HEAD").exists());
    assert!(bundle.exists());
}

#[test]
fn direct_http_bundle_complete_sha256_skips_origin_objects_and_cleans_temp_state() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle fixture directory");
    let (source, bundle) = prepare_repository(dir.path(), Some("sha256"));
    let _helper = ensure_remote_http_helper();
    let server = BundleHttpServer::new(dir.path().to_path_buf(), ResponseMode::Files);
    let before = temp_http_entries();
    let (code, _stdout, stderr, destination) =
        clone_http_bundle(&dir, &server, &server.url("/bundle"), &[]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    assert_eq!(server.origin_object_requests(), 0);
    assert!(bundle.exists());
    assert_no_new_http_temp_entries(&before);
}

#[test]
fn direct_http_bundle_partial_sha1_falls_back_to_one_origin_fetch() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle fixture directory");
    let (source, _bundle) = prepare_partial_repository(dir.path(), None);
    let _helper = ensure_remote_http_helper();
    let server = BundleHttpServer::new_with_options(
        dir.path().to_path_buf(),
        ResponseMode::Files,
        0,
        None,
        true,
    );
    let before = temp_http_entries();
    let (code, _stdout, stderr, destination) =
        clone_http_bundle(&dir, &server, &server.url("/bundle"), &[]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    assert_eq!(server.origin_upload_pack_requests(), 1);
    assert_no_new_http_temp_entries(&before);
}

#[test]
fn direct_http_bundle_partial_sha256_falls_back_to_one_origin_fetch() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle fixture directory");
    let (source, _bundle) = prepare_partial_repository(dir.path(), Some("sha256"));
    let _helper = ensure_remote_http_helper();
    let server = BundleHttpServer::new_with_options(
        dir.path().to_path_buf(),
        ResponseMode::Files,
        0,
        None,
        true,
    );
    let before = temp_http_entries();
    let (code, _stdout, stderr, destination) =
        clone_http_bundle(&dir, &server, &server.url("/bundle"), &[]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    assert_eq!(server.origin_upload_pack_requests(), 1);
    assert_no_new_http_temp_entries(&before);
}

#[test]
fn direct_http_bundle_filtered_sha1_and_sha256_preserve_clone_state() {
    let _lock = bundle_http_test_lock();
    for object_format in [None, Some("sha256")] {
        let dir = TempDir::new().expect("bundle fixture directory");
        let (source, _bundle) = prepare_filtered_repository(dir.path(), object_format);
        let _helper = ensure_remote_http_helper();
        let server = BundleHttpServer::new_smart(dir.path().to_path_buf());
        let before = temp_http_entries();
        let (code, _stdout, stderr, destination) = clone_http_bundle(
            &dir,
            &server,
            &server.url("/bundle"),
            &["--filter=blob:none"],
        );
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(
            git(&destination, ["rev-parse", "HEAD"]),
            git(&source, ["rev-parse", "HEAD"])
        );
        assert_eq!(server.origin_upload_pack_requests(), 1);
        assert_no_new_http_temp_entries(&before);
    }
}

#[test]
fn direct_http_bundle_404_and_401_each_fall_back_to_origin_without_orphans() {
    let _lock = bundle_http_test_lock();
    for mode in [
        ResponseMode::BundleNotFound,
        ResponseMode::BundleUnauthorized,
    ] {
        let dir = TempDir::new().expect("bundle fixture directory");
        let (source, _bundle) = prepare_repository(dir.path(), None);
        let _helper = ensure_remote_http_helper();
        let server =
            BundleHttpServer::new_with_options(dir.path().to_path_buf(), mode, 0, None, true);
        let before = temp_http_entries();
        let (code, _stdout, stderr, destination) =
            clone_http_bundle(&dir, &server, &server.url("/bundle"), &[]);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(
            git(&destination, ["rev-parse", "HEAD"]),
            git(&source, ["rev-parse", "HEAD"])
        );
        assert_eq!(server.bundle_requests(), 1);
        assert_eq!(server.origin_upload_pack_requests(), 1);
        assert_no_new_http_temp_entries(&before);
    }
}

#[test]
fn direct_http_bundle_uses_live_configured_proxy_for_bundle_and_origin() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle fixture directory");
    let (source, _bundle) = prepare_repository(dir.path(), None);
    let _helper = ensure_remote_http_helper();
    let backend = BundleHttpServer::new_with_options(
        dir.path().to_path_buf(),
        ResponseMode::BundleNotFound,
        0,
        None,
        true,
    );
    let proxy = HttpProxyServer::new(backend.port);
    let proxy_config = format!("--config=http.proxy=http://127.0.0.1:{}", proxy.port);
    let before = temp_http_entries();
    let (code, _stdout, stderr, destination) = clone_http_bundle_without_proxy_env(
        &dir,
        &backend,
        &backend.url("/bundle"),
        &[proxy_config.as_str()],
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    assert_eq!(backend.bundle_requests(), 1);
    assert_eq!(backend.origin_upload_pack_requests(), 1);
    assert_eq!(proxy.bundle_requests(), 1);
    assert!(!proxy.requests().is_empty(), "proxy saw no HTTP requests");
    assert!(
        proxy
            .requests()
            .iter()
            .all(|request| request.lines().next().is_some_and(|line| line
                .split_ascii_whitespace()
                .nth(1)
                .is_some_and(|target| { target.starts_with("http://127.0.0.1:") }))),
        "configured proxy did not receive absolute-form requests: {:?}",
        proxy.requests()
    );
    assert_no_new_http_temp_entries(&before);
}

#[test]
fn direct_http_bundle_helper_completes_tls_ca_and_client_cert_handshake() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("TLS fixture directory");
    generate_openssl_tls_fixture(dir.path());
    fs::write(dir.path().join("bundle"), b"TLS bundle fixture\n")
        .expect("write TLS bundle fixture");
    let port = TcpListener::bind(("127.0.0.1", 0))
        .expect("bind TLS fixture port")
        .local_addr()
        .expect("TLS fixture address")
        .port();
    let mut server = TlsTestServer::start(dir.path(), port);
    server.wait_until_listening();
    let helper = ensure_remote_http_helper();
    let output_path = dir.path().join("tls-response");
    let mut child = Command::new(helper)
        .args([
            "--batch",
            "--http-version",
            "http1",
            "--ca-file",
            "ca.pem",
            "--client-cert-file",
            "client.pem",
            "--client-key-file",
            "client.key",
        ])
        .current_dir(dir.path())
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .env_remove("HTTPS_PROXY")
        .env_remove("https_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy")
        .env_remove("NO_PROXY")
        .env("NO_PROXY", "localhost,127.0.0.1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn TLS-configured HTTP helper");
    child
        .stdin
        .take()
        .expect("TLS helper stdin")
        .write_all(
            format!(
                "REQUEST\nMETHOD GET\nURL https://127.0.0.1:{port}/bundle\nOUTPUT-FILE {}\n\nDONE\n",
                output_path.display()
            )
            .as_bytes(),
        )
        .expect("write TLS helper request");
    thread::sleep(Duration::from_millis(200));
    server.stop();
    let output = child.wait_with_output().expect("wait for TLS helper");
    assert!(
        output.status.success(),
        "TLS CA/client-cert helper failed: stdout={} stderr={} server={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        fs::read_to_string(dir.path().join("tls-server.log")).unwrap_or_default()
    );
    assert!(
        fs::metadata(&output_path).expect("TLS helper output").len() > 0,
        "TLS helper did not receive an HTTPS response"
    );
}

#[test]
fn direct_http_bundle_oversize_falls_back_to_origin_without_orphans() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle fixture directory");
    let (source, _bundle) = prepare_repository(dir.path(), None);
    let _helper = ensure_remote_http_helper();
    let server = BundleHttpServer::new_with_options(
        dir.path().to_path_buf(),
        ResponseMode::BundleOversize,
        0,
        None,
        true,
    );
    let before = temp_http_entries();
    let (code, _stdout, stderr, destination) =
        clone_http_bundle(&dir, &server, &server.url("/bundle"), &[]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    assert_eq!(server.bundle_requests(), 1);
    assert_eq!(server.origin_upload_pack_requests(), 1);
    assert_no_new_http_temp_entries(&before);
}

#[test]
fn direct_http_bundle_timeout_terminates_bounded_helper_without_orphans() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle fixture directory");
    let (source, _bundle) = prepare_repository(dir.path(), None);
    let _helper = ensure_remote_http_helper();
    let server = BundleHttpServer::new(dir.path().to_path_buf(), ResponseMode::BundleTimeout);
    let before = temp_http_entries();
    let mut child = Command::new(_helper)
        .args([
            "--batch",
            "--http-version",
            "http1",
            "--request-timeout-ms",
            "20",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn bounded HTTP helper");
    child
        .stdin
        .take()
        .expect("bounded HTTP helper stdin")
        .write_all(
            format!(
                "REQUEST\nMETHOD GET\nURL {}\nOUTPUT-FILE {}\n\nDONE\n",
                server.url("/bundle"),
                dir.path().join("timeout.bundle").display()
            )
            .as_bytes(),
        )
        .expect("write bounded HTTP helper request");
    let output = child
        .wait_with_output()
        .expect("wait for bounded HTTP helper");
    assert!(
        !output.status.success(),
        "timeout helper unexpectedly succeeded"
    );
    assert_eq!(server.bundle_requests(), 1);
    assert_no_new_http_temp_entries(&before);

    let fallback_server = BundleHttpServer::new_with_options(
        dir.path().to_path_buf(),
        ResponseMode::BundleTimeout,
        0,
        None,
        true,
    );
    let before = temp_http_entries();
    let (code, _stdout, stderr, destination) =
        clone_http_bundle(&dir, &fallback_server, &fallback_server.url("/bundle"), &[]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    assert_eq!(fallback_server.bundle_requests(), 1);
    assert_eq!(fallback_server.origin_upload_pack_requests(), 1);
    assert_no_new_http_temp_entries(&before);
}

#[test]
fn direct_http_bundle_follows_exactly_twenty_redirects_when_enabled() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle fixture directory");
    let (source, _bundle) = prepare_repository(dir.path(), None);
    let _helper = ensure_remote_http_helper();
    let server = BundleHttpServer::new_with_redirects(
        dir.path().to_path_buf(),
        ResponseMode::Files,
        20,
        None,
    );
    let before = temp_http_entries();
    let (code, _stdout, stderr, destination) = clone_http_bundle(
        &dir,
        &server,
        &server.url("/redirect/0"),
        &["--config=http.followRedirects=true"],
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    assert_eq!(server.origin_object_requests(), 0);
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|request| request.starts_with("GET /redirect/"))
            .count(),
        20
    );
    assert_no_new_http_temp_entries(&before);
}

#[test]
fn direct_http_bundle_twenty_first_redirect_falls_back_to_origin() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle fixture directory");
    let (source, _bundle) = prepare_repository(dir.path(), None);
    let _helper = ensure_remote_http_helper();
    let server = BundleHttpServer::new_with_options(
        dir.path().to_path_buf(),
        ResponseMode::Files,
        21,
        None,
        true,
    );
    let before = temp_http_entries();
    let (code, _stdout, stderr, destination) = clone_http_bundle(
        &dir,
        &server,
        &server.url("/redirect/0"),
        &["--config=http.followRedirects=true"],
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    assert_eq!(server.origin_upload_pack_requests(), 1);
    assert_no_new_http_temp_entries(&before);
}

#[test]
fn direct_http_bundle_redirect_policy_initial_and_false_do_not_follow_chain() {
    let _lock = bundle_http_test_lock();
    for policy in ["initial", "false"] {
        let dir = TempDir::new().expect("bundle fixture directory");
        let (source, _bundle) = prepare_repository(dir.path(), None);
        let _helper = ensure_remote_http_helper();
        let server = BundleHttpServer::new_with_options(
            dir.path().to_path_buf(),
            ResponseMode::Files,
            2,
            None,
            true,
        );
        let before = temp_http_entries();
        let config = format!("--config=http.followRedirects={policy}");
        let (code, _stdout, stderr, destination) = clone_http_bundle(
            &dir,
            &server,
            &server.url("/redirect/0"),
            &[config.as_str()],
        );
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(
            git(&destination, ["rev-parse", "HEAD"]),
            git(&source, ["rev-parse", "HEAD"])
        );
        assert_eq!(server.origin_upload_pack_requests(), 1);
        assert_no_new_http_temp_entries(&before);
    }
}

#[test]
fn direct_http_bundle_preserves_same_origin_auth_and_strips_cross_origin_auth() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle fixture directory");
    let (source, _bundle) = prepare_repository(dir.path(), None);
    let _helper = ensure_remote_http_helper();
    let same_origin = BundleHttpServer::new(dir.path().to_path_buf(), ResponseMode::Files);
    let same_bundle = same_origin.url("/bundle");
    let same_uri = same_bundle.replacen("http://", "http://user:pass@", 1);
    let (code, _stdout, stderr, destination) =
        clone_http_bundle(&dir, &same_origin, &same_uri, &[]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        same_origin
            .requests()
            .iter()
            .filter(|request| request.starts_with("GET /bundle "))
            .filter(|request| {
                request
                    .lines()
                    .any(|line| line.eq_ignore_ascii_case("authorization: Basic dXNlcjpwYXNz"))
            })
            .count(),
        1,
        "requests: {:?}",
        same_origin.requests()
    );
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    fs::remove_dir_all(&destination).expect("remove first auth clone");

    let target = BundleHttpServer::new(dir.path().to_path_buf(), ResponseMode::Files);
    let redirect = BundleHttpServer::new_with_redirects(
        dir.path().to_path_buf(),
        ResponseMode::Files,
        1,
        Some(target.url("/bundle")),
    );
    let cross_bundle = redirect.url("/redirect/0");
    let cross_uri = cross_bundle.replacen("http://", "http://user:pass@", 1);
    let (code, _stdout, stderr, destination) = clone_http_bundle(&dir, &redirect, &cross_uri, &[]);
    assert_eq!(code, 0, "{stderr}");
    let target_bundle_requests = target
        .requests()
        .into_iter()
        .filter(|request| request.starts_with("GET /bundle "))
        .collect::<Vec<_>>();
    assert_eq!(target_bundle_requests.len(), 1);
    assert!(
        !target_bundle_requests[0]
            .lines()
            .any(|line| line.to_ascii_lowercase().starts_with("authorization:")),
        "cross-origin bundle redirect leaked credentials: {}",
        target_bundle_requests[0]
    );
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
}

#[test]
fn bundle_list_all_imports_creation_token_order_and_persists_progress() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle list fixture directory");
    let (source, partial_bundle) = prepare_partial_repository(dir.path(), None);
    let full_bundle = dir.path().join("full.bundle");
    git(
        &source,
        [
            "bundle",
            "create",
            full_bundle.to_str().expect("full bundle path"),
            "--all",
        ],
    );
    let list = dir.path().join("bundle-list");
    fs::write(
        &list,
        format!(
            "[bundle]\nversion=1\nmode=all\nheuristic=creationToken\n\
             [bundle \"new\"]\nuri={}\ncreationToken=20\n\
             [bundle \"base\"]\nuri={}\ncreationToken=10\n",
            partial_bundle.file_name().unwrap().to_string_lossy(),
            full_bundle.file_name().unwrap().to_string_lossy(),
        ),
    )
    .expect("write bundle list");
    let server = BundleHttpServer::new_with_options(
        dir.path().to_path_buf(),
        ResponseMode::Files,
        0,
        None,
        true,
    );
    let root_uri = server.url("/bundle-list");
    let (code, _stdout, stderr, destination) =
        clone_http_bundle(&dir, &server, &root_uri, &["--config=protocol.version=2"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&destination, ["config", "--get", "fetch.bundleURI"]),
        root_uri
    );
    assert_eq!(
        git(
            &destination,
            ["config", "--get", "fetch.bundleCreationToken"],
        ),
        "20"
    );
    assert_eq!(
        git(
            &destination,
            ["for-each-ref", "--format=%(refname)", "refs/bundles"],
        )
        .lines()
        .count(),
        1,
        "the incremental bundle must not replace the base ref namespace",
    );
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|request| request.contains("command=fetch"))
            .count(),
        0
    );
    assert!(
        fs::read_dir(destination.join(".git/objects/pack"))
            .expect("pack directory")
            .filter_map(Result::ok)
            .all(|entry| entry.path().extension().is_none_or(|ext| ext != "keep"))
    );
    assert!(
        !destination
            .join(".git/objects/pack")
            .join("pack.promisor")
            .exists()
    );
}

#[test]
fn bundle_list_any_stops_after_downloaded_unmet_prerequisite() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle list any fixture directory");
    let (source, partial_bundle) = prepare_partial_repository(dir.path(), None);
    let full_bundle = dir.path().join("full.bundle");
    git(
        &source,
        [
            "bundle",
            "create",
            full_bundle.to_str().expect("full bundle path"),
            "--all",
        ],
    );
    let list = dir.path().join("bundle-list");
    fs::write(
        &list,
        format!(
            "[bundle]\nmode=any\nheuristic=creationToken\n\
             [bundle \"partial\"]\nuri={}\ncreationToken=20\n\
             [bundle \"full\"]\nuri={}\ncreationToken=10\n",
            partial_bundle.file_name().unwrap().to_string_lossy(),
            full_bundle.file_name().unwrap().to_string_lossy(),
        ),
    )
    .expect("write bundle list");
    let server = BundleHttpServer::new_with_options(
        dir.path().to_path_buf(),
        ResponseMode::Files,
        0,
        None,
        true,
    );
    let root_uri = server.url("/bundle-list");
    let (code, _stdout, stderr, destination) = clone_http_bundle(&dir, &server, &root_uri, &[]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    let requests = server.requests();
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("GET /bundle "))
    );
    assert!(
        !requests
            .iter()
            .any(|request| request.starts_with("GET /full.bundle "))
    );
}

#[test]
fn bundle_list_creation_token_stops_after_newest_satisfies_clone_roots() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle list stop fixture directory");
    let (source, bundle) = prepare_repository(dir.path(), None);
    fs::copy(&bundle, dir.path().join("new.bundle")).expect("copy newest bundle");
    fs::copy(&bundle, dir.path().join("old.bundle")).expect("copy old bundle");
    fs::write(
        dir.path().join("bundle-list"),
        "[bundle]\nmode=all\nheuristic=creationToken\n\
         [bundle \"new\"]\nuri=new.bundle\ncreationToken=20\n\
         [bundle \"old\"]\nuri=old.bundle\ncreationToken=10\n",
    )
    .expect("write bundle list");
    let server = BundleHttpServer::new_with_options(
        dir.path().to_path_buf(),
        ResponseMode::Files,
        0,
        None,
        true,
    );
    let root_uri = server.url("/bundle-list");
    let (code, _stdout, stderr, destination) = clone_http_bundle(
        &dir,
        &server,
        &root_uri,
        &["--single-branch", "--branch=main", "--no-tags"],
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    let requests = server.requests();
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("GET /new.bundle "))
    );
    assert!(
        !requests
            .iter()
            .any(|request| request.starts_with("GET /old.bundle "))
    );
    assert_eq!(server.origin_upload_pack_requests(), 0);
    assert_eq!(
        git(
            &destination,
            ["config", "--get", "fetch.bundleCreationToken"],
        ),
        "20"
    );
}

#[test]
fn bundle_list_creation_token_middle_failure_suppresses_progress_config() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle list progress fixture directory");
    let (source, partial_bundle) = prepare_partial_repository(dir.path(), None);
    let full_bundle = dir.path().join("full.bundle");
    git(
        &source,
        [
            "bundle",
            "create",
            full_bundle.to_str().expect("full bundle path"),
            "--all",
        ],
    );
    fs::copy(&partial_bundle, dir.path().join("new.bundle")).expect("copy newest bundle");
    fs::copy(&full_bundle, dir.path().join("old.bundle")).expect("copy old bundle");
    fs::write(
        dir.path().join("bundle-list"),
        "[bundle]\nmode=all\nheuristic=creationToken\n\
         [bundle \"new\"]\nuri=new.bundle\ncreationToken=20\n\
         [bundle \"middle\"]\nuri=missing.bundle\ncreationToken=15\n\
         [bundle \"old\"]\nuri=old.bundle\ncreationToken=10\n",
    )
    .expect("write bundle list");
    let server = BundleHttpServer::new_with_options(
        dir.path().to_path_buf(),
        ResponseMode::Files,
        0,
        None,
        true,
    );
    let root_uri = server.url("/bundle-list");
    let (code, _stdout, stderr, destination) = clone_http_bundle(
        &dir,
        &server,
        &root_uri,
        &["--single-branch", "--branch=main", "--no-tags"],
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    let requests = server.requests();
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("GET /new.bundle "))
    );
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("GET /missing.bundle "))
    );
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("GET /old.bundle "))
    );
    let config = Command::new(zmin_bin())
        .args(["config", "--get", "fetch.bundleCreationToken"])
        .current_dir(&destination)
        .output()
        .expect("read bundle progress config");
    assert!(
        !config.status.success(),
        "middle failure must suppress token"
    );
    let config = Command::new(zmin_bin())
        .args(["config", "--get", "fetch.bundleURI"])
        .current_dir(&destination)
        .output()
        .expect("read bundle progress URI config");
    assert!(!config.status.success(), "middle failure must suppress URI");
}

#[test]
fn bundle_list_any_corrupt_parse_tries_next_candidate() {
    let _lock = bundle_http_test_lock();
    let dir = TempDir::new().expect("bundle list corrupt fixture directory");
    let (source, bundle) = prepare_repository(dir.path(), None);
    fs::write(
        dir.path().join("corrupt.bundle"),
        b"not a bundle list or bundle\n",
    )
    .expect("write corrupt bundle");
    fs::copy(&bundle, dir.path().join("good.bundle")).expect("copy good bundle");
    fs::write(
        dir.path().join("bundle-list"),
        "[bundle]\nmode=any\n\
         [bundle \"corrupt\"]\nuri=corrupt.bundle\n\
         [bundle \"good\"]\nuri=good.bundle\n",
    )
    .expect("write bundle list");
    let server = BundleHttpServer::new_with_options(
        dir.path().to_path_buf(),
        ResponseMode::Files,
        0,
        None,
        true,
    );
    let root_uri = server.url("/bundle-list");
    let (code, _stdout, stderr, destination) = clone_http_bundle(&dir, &server, &root_uri, &[]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        git(&destination, ["rev-parse", "HEAD"]),
        git(&source, ["rev-parse", "HEAD"])
    );
    let requests = server.requests();
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("GET /corrupt.bundle "))
    );
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("GET /good.bundle "))
    );
    assert_eq!(server.origin_upload_pack_requests(), 0);
}
