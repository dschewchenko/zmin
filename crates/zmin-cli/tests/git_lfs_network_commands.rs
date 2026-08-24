mod common;

use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use common::{configure_identity, git, git_init, zmin_bin};

const MEDIA: &[u8] = b"network-object\n";
const MEDIA_OID: &str = "97295c66887d83fb0b1458e412c9a8c5ee53ca64875ef58f015044d8f830c997";
const MEDIA_SIZE: u64 = 15;
const MEDIA_TWO: &[u8] = b"second-network-object\n";
const MEDIA_TWO_OID: &str = "1e61f5ccf26024da97156e82cb7ae7e8bf160e4ae03fc07fd9c9b094a1a26ec0";
const MEDIA_TWO_SIZE: u64 = 22;
const MAX_FIXTURE_HEADER_BYTES: usize = 64 * 1024;
const MAX_FIXTURE_BODY_BYTES: usize = 1024 * 1024;
const FIXTURE_TIMEOUT: Duration = Duration::from_secs(30);
const CONCURRENT_MEDIA: [ConcurrentMediaObject; 8] = [
    ConcurrentMediaObject::new(
        "525a0797023626d0effb58a725daebe11a7a3a378a52971fc839f7d21347cc49",
        b"concurrent-object-0\n",
    ),
    ConcurrentMediaObject::new(
        "5a63da5493ee53e86aaf8f294ce5043ba648f791bde8bb81ef0bdfd73f102664",
        b"concurrent-object-1\n",
    ),
    ConcurrentMediaObject::new(
        "19500a8722e95076df0826dbbc6987a777502eea28bbc78b76c801f0b4040d96",
        b"concurrent-object-2\n",
    ),
    ConcurrentMediaObject::new(
        "f29b80860096dc71f39bb502a94450aef483a2433aa0893c5c1a0ca3d9df51b9",
        b"concurrent-object-3\n",
    ),
    ConcurrentMediaObject::new(
        "4ef16eb0b332d020689a2e8b126ffca4ff6abf1ac9bb755c1b0596d05d37466c",
        b"concurrent-object-4\n",
    ),
    ConcurrentMediaObject::new(
        "f2f34d09d3a10ac59ef04c64d6f9a0c05d9bc4f6ded140b15b22ceffc7cb1942",
        b"concurrent-object-5\n",
    ),
    ConcurrentMediaObject::new(
        "864ed03c1dcc05cb0a588cc18ae120a8aa4d88ee5510bba02cf8bacab6d28f97",
        b"concurrent-object-6\n",
    ),
    ConcurrentMediaObject::new(
        "8ee1b918e9347d08b1fd1e77c31d8c07129ceb5910a2b9414ed41180e3f42a9b",
        b"concurrent-object-7\n",
    ),
];

#[derive(Clone, Copy)]
struct ConcurrentMediaObject {
    oid: &'static str,
    bytes: &'static [u8],
}

impl ConcurrentMediaObject {
    const fn new(oid: &'static str, bytes: &'static [u8]) -> Self {
        Self { oid, bytes }
    }
}

#[derive(Clone, Copy)]
enum LfsFixtureOperation {
    Download,
    DownloadError,
    Upload,
}

struct CapturedRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

struct LfsFixtureEvidence {
    batch_body: Vec<u8>,
    uploaded: Option<Vec<u8>>,
    verify_body: Option<Vec<u8>>,
}

struct LfsHttpFixture {
    remote_url: String,
    endpoint: String,
    worker: JoinHandle<LfsFixtureEvidence>,
}

struct LfsMultiRefEvidence {
    batch_bodies: Vec<Vec<u8>>,
    uploads: Vec<Vec<u8>>,
}

struct LfsMultiRefFixture {
    remote_url: String,
    endpoint: String,
    worker: JoinHandle<LfsMultiRefEvidence>,
}

struct LfsConcurrencyEvidence {
    max_active: usize,
    completed: usize,
    residual_active: usize,
}

struct LfsConcurrencyFixture {
    remote_url: String,
    endpoint: String,
    worker: JoinHandle<LfsConcurrencyEvidence>,
}

impl LfsMultiRefFixture {
    fn spawn(operation: LfsFixtureOperation) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind multi-ref fixture");
        listener
            .set_nonblocking(true)
            .expect("set multi-ref fixture nonblocking");
        let address = listener.local_addr().expect("multi-ref address");
        let remote_url = format!("http://{address}/repo.git");
        let endpoint = format!("http://{address}/repo.git/info/lfs");
        let worker = thread::spawn(move || {
            let mut batch_bodies = Vec::new();
            let mut uploads = Vec::new();
            for sequence in 0..2 {
                let (batch, mut batch_stream) = accept_request(&listener);
                assert_eq!(batch.method, "POST");
                assert_eq!(batch.path, "/repo.git/info/lfs/objects/batch");
                let batch_text = String::from_utf8_lossy(&batch.body);
                assert_ne!(
                    batch_text.contains(MEDIA_OID),
                    batch_text.contains(MEDIA_TWO_OID),
                    "multi-ref batch {sequence} must contain exactly one globally deduplicated object: {batch_text}"
                );
                let (oid, size) = if batch_text.contains(MEDIA_OID) {
                    (MEDIA_OID, MEDIA_SIZE)
                } else {
                    assert!(batch_text.contains(MEDIA_TWO_OID));
                    (MEDIA_TWO_OID, MEDIA_TWO_SIZE)
                };
                let action = match operation {
                    LfsFixtureOperation::Download => "download",
                    LfsFixtureOperation::Upload => "upload",
                    LfsFixtureOperation::DownloadError => {
                        panic!("multi-ref error fixture is unsupported")
                    }
                };
                let response = format!(
                    "{{\"transfer\":\"basic\",\"objects\":[{{\"oid\":\"{oid}\",\"size\":{size},\"actions\":{{\"{action}\":{{\"href\":\"http://{address}/media/{sequence}\"}}}}}}]}}"
                );
                write_response(&mut batch_stream, 200, response.as_bytes());
                drop(batch_stream);
                let (transfer, mut transfer_stream) = accept_request(&listener);
                assert_eq!(transfer.path, format!("/media/{sequence}"));
                match operation {
                    LfsFixtureOperation::Download => {
                        assert_eq!(transfer.method, "GET");
                        assert!(transfer.body.is_empty());
                        let media = if oid == MEDIA_OID { MEDIA } else { MEDIA_TWO };
                        write_response(&mut transfer_stream, 200, media);
                    }
                    LfsFixtureOperation::Upload => {
                        assert_eq!(transfer.method, "PUT");
                        write_response(&mut transfer_stream, 200, b"");
                        uploads.push(transfer.body);
                    }
                    LfsFixtureOperation::DownloadError => unreachable!(),
                }
                batch_bodies.push(batch.body);
            }
            LfsMultiRefEvidence {
                batch_bodies,
                uploads,
            }
        });
        Self {
            remote_url,
            endpoint,
            worker,
        }
    }

    fn finish(self) -> LfsMultiRefEvidence {
        self.worker.join().expect("multi-ref fixture worker")
    }
}

impl LfsConcurrencyFixture {
    fn spawn(expected_concurrency: usize) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind concurrency fixture");
        listener
            .set_nonblocking(true)
            .expect("set concurrency fixture nonblocking");
        let address = listener.local_addr().expect("concurrency address");
        let remote_url = format!("http://{address}/repo.git");
        let endpoint = format!("http://{address}/repo.git/info/lfs");
        let worker = thread::spawn(move || {
            let (batch, mut batch_stream) = accept_request(&listener);
            assert_eq!(batch.method, "POST");
            assert_eq!(batch.path, "/repo.git/info/lfs/objects/batch");
            let batch_text = String::from_utf8_lossy(&batch.body);
            for object in CONCURRENT_MEDIA {
                assert!(batch_text.contains(object.oid));
            }
            let objects = CONCURRENT_MEDIA
                .iter()
                .enumerate()
                .map(|(index, object)| {
                    format!(
                        "{{\"oid\":\"{}\",\"size\":{},\"actions\":{{\"download\":{{\"href\":\"http://{address}/media/{index}\"}}}}}}",
                        object.oid,
                        object.bytes.len()
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let response = format!("{{\"transfer\":\"basic\",\"objects\":[{objects}]}}");
            write_response(&mut batch_stream, 200, response.as_bytes());
            drop(batch_stream);

            let active = Arc::new(AtomicUsize::new(0));
            let maximum = Arc::new(AtomicUsize::new(0));
            let completed = Arc::new(AtomicUsize::new(0));
            let barrier = Arc::new(Barrier::new(expected_concurrency));
            let mut handlers = Vec::with_capacity(CONCURRENT_MEDIA.len());
            for _ in 0..CONCURRENT_MEDIA.len() {
                let (request, mut stream) = accept_request(&listener);
                assert_eq!(request.method, "GET");
                let index = request
                    .path
                    .strip_prefix("/media/")
                    .and_then(|value| value.parse::<usize>().ok())
                    .filter(|index| *index < CONCURRENT_MEDIA.len())
                    .expect("bounded media index");
                let active = Arc::clone(&active);
                let maximum = Arc::clone(&maximum);
                let completed = Arc::clone(&completed);
                let barrier = Arc::clone(&barrier);
                handlers.push(thread::spawn(move || {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    maximum.fetch_max(current, Ordering::SeqCst);
                    barrier.wait();
                    write_response(&mut stream, 200, CONCURRENT_MEDIA[index].bytes);
                    active.fetch_sub(1, Ordering::SeqCst);
                    completed.fetch_add(1, Ordering::SeqCst);
                }));
            }
            for handler in handlers {
                handler.join().expect("concurrency media handler");
            }
            LfsConcurrencyEvidence {
                max_active: maximum.load(Ordering::SeqCst),
                completed: completed.load(Ordering::SeqCst),
                residual_active: active.load(Ordering::SeqCst),
            }
        });
        Self {
            remote_url,
            endpoint,
            worker,
        }
    }

    fn finish(self) -> LfsConcurrencyEvidence {
        self.worker.join().expect("concurrency fixture worker")
    }
}

impl LfsHttpFixture {
    fn spawn(operation: LfsFixtureOperation) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind LFS fixture");
        listener
            .set_nonblocking(true)
            .expect("set LFS fixture nonblocking");
        let address = listener.local_addr().expect("LFS fixture address");
        let remote_url = format!("http://{address}/repo.git");
        let endpoint = format!("http://{address}/repo.git/info/lfs");
        let worker = thread::spawn(move || {
            let (batch, mut batch_stream) = accept_request(&listener);
            assert_eq!(batch.method, "POST");
            assert_eq!(batch.path, "/repo.git/info/lfs/objects/batch");
            let batch_text = String::from_utf8_lossy(&batch.body);
            assert!(batch_text.contains(MEDIA_OID));
            assert!(batch_text.contains(&format!("\"size\":{MEDIA_SIZE}")));
            let response = match operation {
                LfsFixtureOperation::Download => {
                    assert!(batch_text.contains("\"operation\":\"download\""));
                    let action = format!("\"download\":{{\"href\":\"http://{address}/media\"}}");
                    format!(
                        "{{\"transfer\":\"basic\",\"objects\":[{{\"oid\":\"{MEDIA_OID}\",\"size\":{MEDIA_SIZE},\"actions\":{{{action}}}}}]}}"
                    )
                }
                LfsFixtureOperation::DownloadError => {
                    assert!(batch_text.contains("\"operation\":\"download\""));
                    format!(
                        "{{\"transfer\":\"basic\",\"objects\":[{{\"oid\":\"{MEDIA_OID}\",\"size\":{MEDIA_SIZE},\"error\":{{\"code\":404,\"message\":\"missing\"}}}}]}}"
                    )
                }
                LfsFixtureOperation::Upload => {
                    assert!(batch_text.contains("\"operation\":\"upload\""));
                    let action = format!(
                        "\"upload\":{{\"href\":\"http://{address}/media\"}},\"verify\":{{\"href\":\"http://{address}/verify\"}}"
                    );
                    format!(
                        "{{\"transfer\":\"basic\",\"objects\":[{{\"oid\":\"{MEDIA_OID}\",\"size\":{MEDIA_SIZE},\"actions\":{{{action}}}}}]}}"
                    )
                }
            };
            write_response(&mut batch_stream, 200, response.as_bytes());

            match operation {
                LfsFixtureOperation::Download => {
                    let (media, mut stream) = accept_request(&listener);
                    assert_eq!(media.method, "GET");
                    assert_eq!(media.path, "/media");
                    assert!(media.body.is_empty());
                    write_response(&mut stream, 200, MEDIA);
                    LfsFixtureEvidence {
                        batch_body: batch.body,
                        uploaded: None,
                        verify_body: None,
                    }
                }
                LfsFixtureOperation::DownloadError => LfsFixtureEvidence {
                    batch_body: batch.body,
                    uploaded: None,
                    verify_body: None,
                },
                LfsFixtureOperation::Upload => {
                    let (upload, mut stream) = accept_request(&listener);
                    assert_eq!(upload.method, "PUT");
                    assert_eq!(upload.path, "/media");
                    assert_eq!(upload.body, MEDIA);
                    write_response(&mut stream, 200, b"");

                    let (verify, mut stream) = accept_request(&listener);
                    assert_eq!(verify.method, "POST");
                    assert_eq!(verify.path, "/verify");
                    let verify_text = String::from_utf8_lossy(&verify.body);
                    assert!(verify_text.contains(MEDIA_OID));
                    assert!(verify_text.contains(&format!("\"size\":{MEDIA_SIZE}")));
                    write_response(&mut stream, 200, b"");
                    LfsFixtureEvidence {
                        batch_body: batch.body,
                        uploaded: Some(upload.body),
                        verify_body: Some(verify.body),
                    }
                }
            }
        });
        Self {
            remote_url,
            endpoint,
            worker,
        }
    }

    fn finish(self) -> LfsFixtureEvidence {
        self.worker.join().expect("LFS fixture worker")
    }
}

fn accept_request(listener: &TcpListener) -> (CapturedRequest, TcpStream) {
    let deadline = Instant::now() + FIXTURE_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .expect("set fixture connection blocking");
                let request = read_request(&mut stream);
                return (request, stream);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for LFS request"
                );
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("accept LFS request: {error}"),
        }
    }
}

fn read_request(stream: &mut TcpStream) -> CapturedRequest {
    stream
        .set_read_timeout(Some(FIXTURE_TIMEOUT))
        .expect("fixture read timeout");
    let mut header = Vec::new();
    let mut byte = [0_u8; 1];
    while !header.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).expect("read request header");
        header.push(byte[0]);
        assert!(
            header.len() <= MAX_FIXTURE_HEADER_BYTES,
            "fixture request header is too large"
        );
    }
    let header = String::from_utf8(header).expect("request header UTF-8");
    let mut lines = header.split("\r\n");
    let mut request_line = lines.next().expect("request line").split_whitespace();
    let method = request_line.next().expect("request method").to_owned();
    let path = request_line.next().expect("request path").to_owned();
    let mut content_length = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').expect("request header shape");
        if name.eq_ignore_ascii_case("content-length") {
            assert!(content_length.is_none(), "duplicate Content-Length");
            content_length = Some(value.trim().parse::<usize>().expect("Content-Length"));
        }
    }
    let content_length = content_length.unwrap_or(0);
    assert!(
        content_length <= MAX_FIXTURE_BODY_BYTES,
        "fixture request body is too large"
    );
    let mut body = vec![0_u8; content_length];
    stream.read_exact(&mut body).expect("read request body");
    CapturedRequest { method, path, body }
}

fn write_response(stream: &mut TcpStream, status: u16, body: &[u8]) {
    let reason = match status {
        200 => "OK",
        500 => "Internal Server Error",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/vnd.git-lfs+json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .expect("write response header");
    stream.write_all(body).expect("write response body");
}

fn configure_network_repo(repo: &Path, fixture: &LfsHttpFixture) {
    configure_identity(repo);
    git(repo, ["remote", "add", "origin", &fixture.remote_url]);
    git(repo, ["config", "lfs.url", &fixture.endpoint]);
    configure_lfs_filters(repo);
    configure_endpoint_access(repo, &fixture.endpoint);
}

fn configure_lfs_filters(repo: &Path) {
    configure_identity(repo);
    git(repo, ["config", "filter.lfs.required", "false"]);
    git(repo, ["config", "filter.lfs.clean", "cat"]);
    git(repo, ["config", "filter.lfs.smudge", "cat"]);
    git(repo, ["config", "filter.lfs.process", ""]);
}

fn configure_endpoint_access(repo: &Path, endpoint: &str) {
    let access_key = format!("lfs.{endpoint}.access");
    git(repo, ["config", &access_key, "none"]);
}

fn pointer_bytes() -> Vec<u8> {
    format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{MEDIA_OID}\nsize {MEDIA_SIZE}\n"
    )
    .into_bytes()
}

fn pointer_two_bytes() -> Vec<u8> {
    format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{MEDIA_TWO_OID}\nsize {MEDIA_TWO_SIZE}\n"
    )
    .into_bytes()
}

fn pkt_line(payload: &[u8]) -> Vec<u8> {
    let mut packet = format!("{:04x}", payload.len() + 4).into_bytes();
    packet.extend_from_slice(payload);
    packet
}

fn filter_process_smudge_request(path: &str, content: &[u8]) -> Vec<u8> {
    let mut input = Vec::new();
    for payload in [b"git-filter-client\n".as_slice(), b"version=2\n"] {
        input.extend_from_slice(&pkt_line(payload));
    }
    input.extend_from_slice(b"0000");
    for payload in [b"capability=clean\n".as_slice(), b"capability=smudge\n"] {
        input.extend_from_slice(&pkt_line(payload));
    }
    input.extend_from_slice(b"0000");
    input.extend_from_slice(&pkt_line(b"command=smudge\n"));
    input.extend_from_slice(&pkt_line(format!("pathname={path}\n").as_bytes()));
    input.extend_from_slice(b"0000");
    input.extend_from_slice(&pkt_line(content));
    input.extend_from_slice(b"0000");
    input
}

fn commit_pointer(repo: &Path) {
    fs::write(
        repo.join(".gitattributes"),
        b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("write attributes");
    fs::write(repo.join("asset.bin"), pointer_bytes()).expect("write pointer");
    git(repo, ["add", ".gitattributes", "asset.bin"]);
    git(repo, ["commit", "-m", "pointer"]);
}

fn commit_concurrency_pointers(repo: &Path) {
    fs::write(
        repo.join(".gitattributes"),
        b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("write attributes");
    for (index, object) in CONCURRENT_MEDIA.iter().enumerate() {
        let pointer = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
            object.oid,
            object.bytes.len()
        );
        fs::write(repo.join(format!("concurrent-{index}.bin")), pointer)
            .expect("write concurrency pointer");
    }
    git(repo, ["add", "."]);
    git(repo, ["commit", "-m", "concurrency pointers"]);
}

fn commit_clean_media(repo: &Path) {
    fs::write(
        repo.join(".gitattributes"),
        b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("write attributes");
    let clean = run_zmin_input(repo, &["lfs", "clean", "--", "asset.bin"], MEDIA);
    assert_success(&clean);
    fs::write(repo.join("asset.bin"), &clean.stdout).expect("write clean pointer");
    git(repo, ["add", ".gitattributes", "asset.bin"]);
    git(repo, ["commit", "-m", "pointer"]);
}

fn current_pre_push_update(repo: &Path) -> Vec<u8> {
    let head = String::from_utf8(
        Command::new(common::stock_git_bin())
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .expect("resolve HEAD")
            .stdout,
    )
    .expect("HEAD UTF-8");
    let branch = String::from_utf8(
        Command::new(common::stock_git_bin())
            .args(["symbolic-ref", "HEAD"])
            .current_dir(repo)
            .output()
            .expect("resolve branch")
            .stdout,
    )
    .expect("branch UTF-8");
    format!(
        "{} {} {} 0000000000000000000000000000000000000000\n",
        branch.trim(),
        head.trim(),
        branch.trim()
    )
    .into_bytes()
}

fn run_zmin_input(repo: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(zmin_bin())
        .args(args)
        .current_dir(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zmin");
    child
        .stdin
        .as_mut()
        .expect("zmin stdin")
        .write_all(input)
        .expect("write zmin stdin");
    child.wait_with_output().expect("wait zmin")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "zmin failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn lfs_fetch_downloads_without_checkout_over_http_batch() {
    let fixture = LfsHttpFixture::spawn(LfsFixtureOperation::Download);
    let repo = git_init();
    configure_network_repo(repo.path(), &fixture);
    commit_pointer(repo.path());

    let output = run_zmin_input(repo.path(), &["lfs", "fetch", "origin"], b"");
    assert_success(&output);
    assert_eq!(
        fs::read(repo.path().join("asset.bin")).unwrap(),
        pointer_bytes()
    );
    assert_eq!(
        fs::read(repo.path().join(format!(
            ".git/lfs/objects/{}/{}/{}",
            &MEDIA_OID[..2],
            &MEDIA_OID[2..4],
            MEDIA_OID
        )))
        .unwrap(),
        MEDIA
    );
    let evidence = fixture.finish();
    let batch = String::from_utf8_lossy(&evidence.batch_body);
    let branch = git(repo.path(), ["branch", "--show-current"]);
    assert!(batch.contains("download"));
    assert!(batch.contains(&format!("\"name\":\"refs/heads/{}\"", branch.trim())));
}

#[test]
fn lfs_fetch_honors_bounded_concurrency_one_two_and_eight() {
    for concurrency in [1_usize, 2, 8] {
        let fixture = LfsConcurrencyFixture::spawn(concurrency);
        let repo = git_init();
        configure_identity(repo.path());
        configure_lfs_filters(repo.path());
        git(
            repo.path(),
            ["remote", "add", "origin", &fixture.remote_url],
        );
        git(repo.path(), ["config", "lfs.url", &fixture.endpoint]);
        configure_endpoint_access(repo.path(), &fixture.endpoint);
        git(
            repo.path(),
            [
                "config",
                "lfs.concurrentTransfers",
                &concurrency.to_string(),
            ],
        );
        commit_concurrency_pointers(repo.path());

        let output = run_zmin_input(repo.path(), &["lfs", "fetch", "origin"], b"");
        assert_success(&output);
        let evidence = fixture.finish();
        assert_eq!(evidence.max_active, concurrency);
        assert_eq!(evidence.completed, CONCURRENT_MEDIA.len());
        assert_eq!(evidence.residual_active, 0);
        for object in CONCURRENT_MEDIA {
            let stored = repo.path().join(format!(
                ".git/lfs/objects/{}/{}/{}",
                &object.oid[..2],
                &object.oid[2..4],
                object.oid
            ));
            assert_eq!(fs::read(stored).expect("stored object"), object.bytes);
        }
    }
}

#[test]
fn invalid_concurrency_fails_before_any_network_request() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind rejection fixture");
    listener
        .set_nonblocking(true)
        .expect("set rejection fixture nonblocking");
    let address = listener.local_addr().expect("rejection fixture address");
    let remote_url = format!("http://{address}/repo.git");
    let endpoint = format!("http://{address}/repo.git/info/lfs");
    let repo = git_init();
    configure_identity(repo.path());
    configure_lfs_filters(repo.path());
    git(repo.path(), ["remote", "add", "origin", &remote_url]);
    git(repo.path(), ["config", "lfs.url", &endpoint]);
    configure_endpoint_access(repo.path(), &endpoint);
    commit_pointer(repo.path());

    for value in [
        "0",
        "-1",
        "9",
        "private-malformed-value",
        "184467440737095516160",
    ] {
        git(repo.path(), ["config", "lfs.concurrentTransfers", value]);
        let output = run_zmin_input(repo.path(), &["lfs", "fetch", "origin"], b"");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).expect("stderr UTF-8");
        assert_eq!(
            stderr,
            "error: lfs.concurrenttransfers must be an integer in 1..=8\n"
        );
        assert!(!stderr.contains(value));
    }
    match listener.accept() {
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
        Ok(_) => panic!("invalid concurrency reached the network"),
        Err(error) => panic!("network rejection probe: {error}"),
    }
}

#[test]
fn lfs_fetch_groups_explicit_refs_and_omits_ref_for_detached_oid() {
    let fixture = LfsMultiRefFixture::spawn(LfsFixtureOperation::Download);
    let repo = git_init();
    configure_lfs_filters(repo.path());
    git(
        repo.path(),
        ["remote", "add", "origin", &fixture.remote_url],
    );
    git(repo.path(), ["config", "lfs.url", &fixture.endpoint]);
    configure_endpoint_access(repo.path(), &fixture.endpoint);
    commit_pointer(repo.path());
    let main_branch = git(repo.path(), ["branch", "--show-current"]);
    git(repo.path(), ["checkout", "-b", "topic"]);
    fs::write(repo.path().join("second.bin"), pointer_two_bytes()).expect("second pointer");
    git(repo.path(), ["add", "second.bin"]);
    git(repo.path(), ["commit", "-m", "second pointer"]);

    let output = run_zmin_input(
        repo.path(),
        &["lfs", "fetch", "origin", main_branch.trim(), "topic"],
        b"",
    );
    assert_success(&output);
    let evidence = fixture.finish();
    let batches = evidence
        .batch_bodies
        .iter()
        .map(|body| String::from_utf8_lossy(body))
        .collect::<Vec<_>>();
    assert!(
        batches
            .iter()
            .any(|body| body.contains(&format!("\"name\":\"refs/heads/{}\"", main_branch.trim())))
    );
    assert!(
        batches
            .iter()
            .any(|body| body.contains("\"name\":\"refs/heads/topic\""))
    );

    let detached_fixture = LfsHttpFixture::spawn(LfsFixtureOperation::Download);
    let detached_repo = git_init();
    configure_network_repo(detached_repo.path(), &detached_fixture);
    commit_pointer(detached_repo.path());
    let head = git(detached_repo.path(), ["rev-parse", "HEAD"]);
    let output = run_zmin_input(
        detached_repo.path(),
        &["lfs", "fetch", "origin", head.trim()],
        b"",
    );
    assert_success(&output);
    let evidence = detached_fixture.finish();
    assert!(!String::from_utf8_lossy(&evidence.batch_body).contains("\"ref\""));
}

#[test]
fn lfs_pull_checks_out_only_paths_in_the_filtered_fetch_plan() {
    let fixture = LfsHttpFixture::spawn(LfsFixtureOperation::Download);
    let repo = git_init();
    configure_network_repo(repo.path(), &fixture);
    commit_pointer(repo.path());
    fs::write(repo.path().join("copy.bin"), pointer_bytes()).expect("included duplicate pointer");
    fs::write(repo.path().join("excluded.bin"), pointer_bytes()).expect("excluded pointer");
    git(repo.path(), ["add", "copy.bin", "excluded.bin"]);
    git(repo.path(), ["commit", "-m", "duplicate pointers"]);
    git(
        repo.path(),
        ["config", "lfs.fetchinclude", "asset.bin,copy.bin"],
    );

    let output = run_zmin_input(repo.path(), &["lfs", "pull", "origin"], b"");
    assert_success(&output);
    assert_eq!(fs::read(repo.path().join("asset.bin")).unwrap(), MEDIA);
    assert_eq!(fs::read(repo.path().join("copy.bin")).unwrap(), MEDIA);
    assert_eq!(
        fs::read(repo.path().join("excluded.bin")).unwrap(),
        pointer_bytes()
    );
    fixture.finish();
}

#[test]
fn lfs_explicit_remote_is_validated_before_empty_plan_shortcuts() {
    let repo = git_init();
    configure_lfs_filters(repo.path());
    git(repo.path(), ["commit", "--allow-empty", "-m", "empty"]);

    for (args, input) in [
        (["lfs", "fetch", "missing", ""], b"".as_slice()),
        (["lfs", "push", "missing", "HEAD"], b"".as_slice()),
        (["lfs", "pre-push", "missing", ""], b"".as_slice()),
    ] {
        let args = args
            .iter()
            .filter(|arg| !arg.is_empty())
            .copied()
            .collect::<Vec<_>>();
        let output = run_zmin_input(repo.path(), &args, input);
        assert_eq!(output.status.code(), Some(2), "args={args:?}");
    }

    git(
        repo.path(),
        [
            "config",
            "remote.broken.fetch",
            "+refs/heads/*:refs/remotes/broken/*",
        ],
    );
    let broken = run_zmin_input(repo.path(), &["lfs", "fetch", "broken"], b"");
    assert_eq!(broken.status.code(), Some(2));

    git(
        repo.path(),
        [
            "config",
            "lfs.url",
            "https://example.invalid/repository/info/lfs",
        ],
    );
    for args in [
        ["lfs", "fetch", "missing", ""],
        ["lfs", "push", "missing", "HEAD"],
        ["lfs", "pre-push", "missing", ""],
    ] {
        let args = args
            .iter()
            .filter(|arg| !arg.is_empty())
            .copied()
            .collect::<Vec<_>>();
        let output = run_zmin_input(repo.path(), &args, b"");
        assert_success(&output);
    }
}

#[test]
fn lfs_smudge_fetches_a_missing_object_over_http_batch() {
    let fixture = LfsHttpFixture::spawn(LfsFixtureOperation::Download);
    let repo = git_init();
    configure_network_repo(repo.path(), &fixture);
    commit_pointer(repo.path());
    let branch = git(repo.path(), ["branch", "--show-current"]);
    git(
        repo.path(),
        [
            "config",
            &format!("branch.{}.remote", branch.trim()),
            "origin",
        ],
    );
    git(
        repo.path(),
        [
            "config",
            &format!("branch.{}.merge", branch.trim()),
            "refs/heads/release",
        ],
    );
    git(repo.path(), ["config", "push.default", "upstream"]);

    let output = run_zmin_input(
        repo.path(),
        &["lfs", "smudge", "--", "asset.bin"],
        &pointer_bytes(),
    );
    assert_success(&output);
    assert_eq!(output.stdout, MEDIA);
    let evidence = fixture.finish();
    assert!(
        String::from_utf8_lossy(&evidence.batch_body).contains("\"name\":\"refs/heads/release\"")
    );
}

#[test]
fn lfs_smudge_global_endpoint_uses_sole_push_remote_for_simple_ref() {
    let fixture = LfsHttpFixture::spawn(LfsFixtureOperation::Download);
    let repo = git_init();
    configure_network_repo(repo.path(), &fixture);
    commit_pointer(repo.path());
    let branch = git(repo.path(), ["branch", "--show-current"]);
    git(
        repo.path(),
        [
            "config",
            &format!("branch.{}.merge", branch.trim()),
            "refs/heads/release",
        ],
    );
    git(repo.path(), ["config", "push.default", "simple"]);

    let output = run_zmin_input(
        repo.path(),
        &["lfs", "smudge", "--", "asset.bin"],
        &pointer_bytes(),
    );
    assert_success(&output);
    assert_eq!(output.stdout, MEDIA);
    let evidence = fixture.finish();
    let batch = String::from_utf8_lossy(&evidence.batch_body);
    assert!(batch.contains(&format!("\"name\":\"refs/heads/{}\"", branch.trim())));
    assert!(!batch.contains("\"name\":\"refs/heads/release\""));
}

#[test]
fn lfs_smudge_ref_uses_the_actual_fallback_remote() {
    let fixture = LfsHttpFixture::spawn(LfsFixtureOperation::Download);
    let repo = git_init();
    configure_network_repo(repo.path(), &fixture);
    commit_pointer(repo.path());
    git(repo.path(), ["config", "--unset", "lfs.url"]);
    git(
        repo.path(),
        ["config", "remote.origin.lfsurl", &fixture.endpoint],
    );
    let branch = git(repo.path(), ["branch", "--show-current"]);
    git(
        repo.path(),
        [
            "config",
            &format!("branch.{}.remote", branch.trim()),
            "missing",
        ],
    );
    git(
        repo.path(),
        [
            "config",
            &format!("branch.{}.merge", branch.trim()),
            "refs/heads/release",
        ],
    );
    git(repo.path(), ["config", "push.default", "upstream"]);

    let output = run_zmin_input(
        repo.path(),
        &["lfs", "smudge", "--", "asset.bin"],
        &pointer_bytes(),
    );
    assert_success(&output);
    assert_eq!(output.stdout, MEDIA);
    let evidence = fixture.finish();
    let batch = String::from_utf8_lossy(&evidence.batch_body);
    assert!(batch.contains(&format!("\"name\":\"refs/heads/{}\"", branch.trim())));
    assert!(!batch.contains("\"name\":\"refs/heads/release\""));
}

#[test]
fn lfs_detached_smudge_uses_head_oid_as_batch_ref() {
    let fixture = LfsHttpFixture::spawn(LfsFixtureOperation::Download);
    let repo = git_init();
    configure_network_repo(repo.path(), &fixture);
    commit_pointer(repo.path());
    git(repo.path(), ["checkout", "--detach"]);
    let head = git(repo.path(), ["rev-parse", "HEAD"]);

    let output = run_zmin_input(
        repo.path(),
        &["lfs", "smudge", "--", "asset.bin"],
        &pointer_bytes(),
    );
    assert_success(&output);
    assert_eq!(output.stdout, MEDIA);
    let evidence = fixture.finish();
    assert!(
        String::from_utf8_lossy(&evidence.batch_body)
            .contains(&format!("\"name\":\"{}\"", head.trim()))
    );
}

#[test]
fn lfs_skip_download_errors_leaves_the_pointer_after_a_batch_failure() {
    let fixture = LfsHttpFixture::spawn(LfsFixtureOperation::DownloadError);
    let repo = git_init();
    configure_network_repo(repo.path(), &fixture);
    git(repo.path(), ["config", "lfs.skipdownloaderrors", "true"]);

    let pointer = pointer_bytes();
    let output = run_zmin_input(repo.path(), &["lfs", "smudge", "--", "asset.bin"], &pointer);
    assert_success(&output);
    assert_eq!(output.stdout, pointer);
    let evidence = fixture.finish();
    assert!(!String::from_utf8_lossy(&evidence.batch_body).contains("\"ref\""));
}

#[test]
fn lfs_filter_remote_policy_is_lazy_until_a_missing_download() {
    let repo = git_init();
    configure_lfs_filters(repo.path());
    git(repo.path(), ["config", "lfs.remote.autodetect", "true"]);
    git(repo.path(), ["config", "lfs.remote.searchall", "true"]);
    let clean = run_zmin_input(repo.path(), &["lfs", "clean", "--", "asset.bin"], MEDIA);
    assert_success(&clean);

    let cached = run_zmin_input(
        repo.path(),
        &["lfs", "smudge", "--", "asset.bin"],
        &clean.stdout,
    );
    assert_success(&cached);
    assert_eq!(cached.stdout, MEDIA);

    fs::remove_file(repo.path().join(format!(
        ".git/lfs/objects/{}/{}/{}",
        &MEDIA_OID[..2],
        &MEDIA_OID[2..4],
        MEDIA_OID
    )))
    .expect("remove cached object");
    let missing = run_zmin_input(
        repo.path(),
        &["lfs", "smudge", "--", "asset.bin"],
        &clean.stdout,
    );
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&missing.stderr)
            .contains("unsupported LFS multi-remote selection policy")
    );
}

#[test]
fn lfs_push_and_pre_push_upload_over_http_batch() {
    let push_fixture = LfsHttpFixture::spawn(LfsFixtureOperation::Upload);
    let repo = git_init();
    configure_network_repo(repo.path(), &push_fixture);
    commit_clean_media(repo.path());

    let output = run_zmin_input(repo.path(), &["lfs", "push", "origin", "HEAD"], b"");
    assert_success(&output);
    let pushed = push_fixture.finish();
    assert_eq!(pushed.uploaded.as_deref(), Some(MEDIA));
    assert!(pushed.verify_body.is_some());

    let pre_push_fixture = LfsHttpFixture::spawn(LfsFixtureOperation::Upload);
    git(
        repo.path(),
        ["config", "lfs.url", &pre_push_fixture.endpoint],
    );
    let access_key = format!("lfs.{}.access", pre_push_fixture.endpoint);
    git(repo.path(), ["config", &access_key, "none"]);
    let update = current_pre_push_update(repo.path());
    let output = run_zmin_input(
        repo.path(),
        &["lfs", "pre-push", "origin", &pre_push_fixture.remote_url],
        &update,
    );
    assert_success(&output);
    let pushed = pre_push_fixture.finish();
    assert_eq!(pushed.uploaded.as_deref(), Some(MEDIA));
    assert!(pushed.verify_body.is_some());
}

#[test]
fn lfs_pre_push_local_transport_url_does_not_override_remote_lfspushurl() {
    let fixture = LfsHttpFixture::spawn(LfsFixtureOperation::Upload);
    let repo = git_init();
    let local_remote = git_init();
    configure_identity(repo.path());
    let local_remote_url = local_remote.path().to_string_lossy().into_owned();
    git(repo.path(), ["remote", "add", "origin", &local_remote_url]);
    git(
        repo.path(),
        ["config", "remote.origin.lfspushurl", &fixture.endpoint],
    );
    let access_key = format!("lfs.{}.access", fixture.endpoint);
    git(repo.path(), ["config", &access_key, "none"]);
    git(repo.path(), ["config", "filter.lfs.required", "false"]);
    git(repo.path(), ["config", "filter.lfs.clean", "cat"]);
    git(repo.path(), ["config", "filter.lfs.smudge", "cat"]);
    git(repo.path(), ["config", "filter.lfs.process", ""]);
    commit_clean_media(repo.path());

    let update = current_pre_push_update(repo.path());
    let output = run_zmin_input(
        repo.path(),
        &["lfs", "pre-push", "origin", &local_remote_url],
        &update,
    );
    assert_success(&output);
    let evidence = fixture.finish();
    assert_eq!(evidence.uploaded.as_deref(), Some(MEDIA));
    assert!(evidence.verify_body.is_some());
    assert!(!local_remote.path().join(".git/lfs/objects").exists());
}

#[test]
fn lfsconfig_remote_lfsurl_precedes_local_direct_transport() {
    let fixture = LfsHttpFixture::spawn(LfsFixtureOperation::Upload);
    let repo = git_init();
    let local_remote = git_init();
    configure_lfs_filters(repo.path());
    let local_remote_url = local_remote.path().to_string_lossy().into_owned();
    git(repo.path(), ["remote", "add", "origin", &local_remote_url]);
    fs::write(
        repo.path().join(".lfsconfig"),
        format!("[remote \"origin\"]\nlfsurl = {}\n", fixture.endpoint),
    )
    .expect("write .lfsconfig");
    configure_endpoint_access(repo.path(), &fixture.endpoint);
    commit_clean_media(repo.path());

    let update = current_pre_push_update(repo.path());
    let output = run_zmin_input(
        repo.path(),
        &["lfs", "pre-push", "origin", &local_remote_url],
        &update,
    );
    assert_success(&output);
    assert_eq!(fixture.finish().uploaded.as_deref(), Some(MEDIA));
    assert!(!local_remote.path().join(".git/lfs/objects").exists());
}

#[test]
fn lfs_fetch_uses_global_endpoint_without_any_named_remote() {
    let fixture = LfsHttpFixture::spawn(LfsFixtureOperation::Download);
    let repo = git_init();
    configure_lfs_filters(repo.path());
    git(repo.path(), ["config", "lfs.url", &fixture.endpoint]);
    configure_endpoint_access(repo.path(), &fixture.endpoint);
    commit_pointer(repo.path());

    let output = run_zmin_input(repo.path(), &["lfs", "fetch"], b"");
    assert_success(&output);
    assert_eq!(
        fs::read(repo.path().join(format!(
            ".git/lfs/objects/{}/{}/{}",
            &MEDIA_OID[..2],
            &MEDIA_OID[2..4],
            MEDIA_OID
        )))
        .expect("downloaded object"),
        MEDIA
    );
    fixture.finish();
}

#[test]
fn lfs_tracking_remote_precedes_lfs_default_remote() {
    let fixture = LfsHttpFixture::spawn(LfsFixtureOperation::Download);
    let repo = git_init();
    configure_lfs_filters(repo.path());
    git(
        repo.path(),
        [
            "remote",
            "add",
            "preferred",
            "http://127.0.0.1:1/unused.git",
        ],
    );
    git(
        repo.path(),
        ["remote", "add", "tracking", &fixture.remote_url],
    );
    git(repo.path(), ["config", "remote.lfsdefault", "preferred"]);
    git(repo.path(), ["config", "branch.main.remote", "tracking"]);
    configure_endpoint_access(repo.path(), &fixture.endpoint);
    commit_pointer(repo.path());

    let output = run_zmin_input(repo.path(), &["lfs", "fetch"], b"");
    assert_success(&output);
    fixture.finish();
}

#[test]
fn lfs_pre_push_accepts_anonymous_transport_url() {
    let fixture = LfsHttpFixture::spawn(LfsFixtureOperation::Upload);
    let repo = git_init();
    configure_lfs_filters(repo.path());
    configure_endpoint_access(repo.path(), &fixture.endpoint);
    commit_clean_media(repo.path());
    git(repo.path(), ["config", "lfs.remote.autodetect", "true"]);
    git(repo.path(), ["config", "lfs.remote.searchall", "true"]);

    let update = current_pre_push_update(repo.path());
    let output = run_zmin_input(
        repo.path(),
        &["lfs", "pre-push", &fixture.remote_url],
        &update,
    );
    assert_success(&output);
    let evidence = fixture.finish();
    assert_eq!(evidence.uploaded.as_deref(), Some(MEDIA));
}

#[test]
fn lfs_url_scoped_locksverify_fails_before_network_and_redacts_endpoint() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind unused endpoint");
    listener
        .set_nonblocking(true)
        .expect("nonblocking endpoint");
    let endpoint = format!(
        "http://{}/repo.git/info/lfs",
        listener.local_addr().expect("endpoint address")
    );
    let repo = git_init();
    configure_lfs_filters(repo.path());
    git(repo.path(), ["config", "lfs.url", &endpoint]);
    configure_endpoint_access(repo.path(), &endpoint);
    let locks_key = format!("lfs.{endpoint}.locksverify");
    git(repo.path(), ["config", &locks_key, "true"]);
    commit_clean_media(repo.path());

    let output = run_zmin_input(repo.path(), &["lfs", "push", "anonymous", "HEAD"], b"");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unsupported locking API"));
    assert!(!stderr.contains(&endpoint));
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock
    ));
}

#[test]
fn lfs_smudge_and_filter_process_share_lfs_exclude_or_semantics() {
    let repo = git_init();
    configure_lfs_filters(repo.path());
    git(repo.path(), ["config", "lfs.fetchexclude", "private.bin"]);
    let clean = run_zmin_input(repo.path(), &["lfs", "clean", "--", "asset.bin"], MEDIA);
    assert_success(&clean);

    let direct_allowed = run_zmin_input(
        repo.path(),
        &["lfs", "smudge", "--", "asset.bin"],
        &clean.stdout,
    );
    assert_success(&direct_allowed);
    assert_eq!(direct_allowed.stdout, MEDIA);

    let direct_excluded = run_zmin_input(
        repo.path(),
        &["lfs", "smudge", "--", "private.bin"],
        &clean.stdout,
    );
    assert_success(&direct_excluded);
    assert_eq!(direct_excluded.stdout, clean.stdout);

    let process_allowed = run_zmin_input(
        repo.path(),
        &["lfs", "filter-process"],
        &filter_process_smudge_request("asset.bin", &clean.stdout),
    );
    assert_success(&process_allowed);
    assert!(
        process_allowed
            .stdout
            .windows(MEDIA.len())
            .any(|window| window == MEDIA)
    );

    let process_excluded = run_zmin_input(
        repo.path(),
        &["lfs", "filter-process"],
        &filter_process_smudge_request("private.bin", &clean.stdout),
    );
    assert_success(&process_excluded);
    assert!(
        process_excluded
            .stdout
            .windows(clean.stdout.len())
            .any(|window| window == clean.stdout)
    );
    assert!(
        !process_excluded
            .stdout
            .windows(MEDIA.len())
            .any(|window| window == MEDIA)
    );
}

#[test]
fn linked_worktree_default_lfs_storage_uses_common_git_directory() {
    let repo = git_init();
    configure_lfs_filters(repo.path());
    fs::write(repo.path().join("seed"), b"seed\n").expect("seed file");
    git(repo.path(), ["add", "seed"]);
    git(repo.path(), ["commit", "-m", "seed"]);
    let linked = repo.path().with_file_name(format!(
        "{}-lfs-linked",
        repo.path()
            .file_name()
            .and_then(|name| name.to_str())
            .expect("temporary repository name")
    ));
    git(
        repo.path(),
        [
            "worktree",
            "add",
            "-b",
            "lfs-linked",
            linked.to_str().expect("linked worktree path"),
        ],
    );

    let clean = run_zmin_input(&linked, &["lfs", "clean", "--", "asset.bin"], MEDIA);
    assert_success(&clean);
    let common_object = repo.path().join(format!(
        ".git/lfs/objects/{}/{}/{}",
        &MEDIA_OID[..2],
        &MEDIA_OID[2..4],
        MEDIA_OID
    ));
    assert_eq!(fs::read(common_object).expect("common LFS object"), MEDIA);
    assert!(!repo.path().join(".git/worktrees/lfs-linked/lfs").exists());

    fs::write(linked.join("asset.bin"), &clean.stdout).expect("linked pointer");
    git(
        &linked,
        [
            "-c",
            "filter.lfs.required=false",
            "-c",
            "filter.lfs.process=",
            "-c",
            "filter.lfs.clean=cat",
            "add",
            "asset.bin",
        ],
    );
    let listed = run_zmin_input(&linked, &["lfs", "ls-files"], b"");
    assert_success(&listed);
    assert_eq!(
        String::from_utf8(listed.stdout).expect("LFS listing UTF-8"),
        format!("{} * asset.bin\n", &MEDIA_OID[..10])
    );

    let env = run_zmin_input(&linked, &["lfs", "env"], b"");
    assert_success(&env);
    let env = String::from_utf8(env.stdout).expect("LFS env UTF-8");
    let canonical_common = fs::canonicalize(repo.path().join(".git")).expect("common Git dir");
    assert!(env.contains(&format!(
        "LocalMediaDir={}",
        canonical_common.join("lfs/objects").display()
    )));

    git(&linked, ["config", "lfs.storage", "custom-lfs"]);
    let custom_clean = run_zmin_input(&linked, &["lfs", "clean", "--", "second.bin"], MEDIA_TWO);
    assert_success(&custom_clean);
    assert_eq!(
        fs::read(canonical_common.join(format!(
            "custom-lfs/objects/{}/{}/{}",
            &MEDIA_TWO_OID[..2],
            &MEDIA_TWO_OID[2..4],
            MEDIA_TWO_OID
        )))
        .expect("relative custom LFS object in common Git dir"),
        MEDIA_TWO
    );
    assert!(
        !repo
            .path()
            .join(".git/worktrees/lfs-linked/custom-lfs")
            .exists()
    );
    let custom_env = run_zmin_input(&linked, &["lfs", "env"], b"");
    assert_success(&custom_env);
    assert!(
        String::from_utf8(custom_env.stdout)
            .expect("custom LFS env UTF-8")
            .contains(&format!(
                "LocalMediaDir={}",
                canonical_common.join("custom-lfs/objects").display()
            ))
    );
    git(
        repo.path(),
        [
            "worktree",
            "remove",
            "--force",
            linked.to_str().expect("path"),
        ],
    );
}

#[test]
fn lfs_push_sends_one_batch_ref_per_destination_group() {
    let fixture = LfsMultiRefFixture::spawn(LfsFixtureOperation::Upload);
    let repo = git_init();
    configure_lfs_filters(repo.path());
    git(repo.path(), ["config", "lfs.url", &fixture.endpoint]);
    configure_endpoint_access(repo.path(), &fixture.endpoint);

    let first = run_zmin_input(repo.path(), &["lfs", "clean", "--", "first.bin"], MEDIA);
    assert_success(&first);
    fs::write(repo.path().join("first.bin"), &first.stdout).expect("first pointer");
    git(repo.path(), ["add", "first.bin"]);
    git(repo.path(), ["commit", "-m", "first pointer"]);
    let main_branch = git(repo.path(), ["branch", "--show-current"]);
    let main_spec = format!("{}:refs/heads/dst-main", main_branch.trim());

    git(repo.path(), ["checkout", "-b", "topic"]);
    let second = run_zmin_input(
        repo.path(),
        &["lfs", "clean", "--", "second.bin"],
        MEDIA_TWO,
    );
    assert_success(&second);
    fs::write(repo.path().join("second.bin"), &second.stdout).expect("second pointer");
    git(repo.path(), ["add", "second.bin"]);
    git(repo.path(), ["commit", "-m", "second pointer"]);

    let output = run_zmin_input(
        repo.path(),
        &[
            "lfs",
            "push",
            "anonymous",
            &main_spec,
            "topic:refs/heads/dst-topic",
        ],
        b"",
    );
    assert_success(&output);
    let evidence = fixture.finish();
    assert_eq!(evidence.batch_bodies.len(), 2);
    let batches = evidence
        .batch_bodies
        .iter()
        .map(|body| String::from_utf8_lossy(body))
        .collect::<Vec<_>>();
    assert!(
        batches
            .iter()
            .any(|body| body.contains("\"name\":\"refs/heads/dst-main\""))
    );
    assert!(
        batches
            .iter()
            .any(|body| body.contains("\"name\":\"refs/heads/dst-topic\""))
    );
    assert!(evidence.uploads.iter().any(|upload| upload == MEDIA));
    assert!(evidence.uploads.iter().any(|upload| upload == MEDIA_TWO));
}
