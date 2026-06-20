mod common;

use std::fs;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};

use tempfile::TempDir;

use common::{
    command_failure_output, command_failure_output_with_env, command_output,
    command_output_with_env, configure_identity, ensure_remote_http_helper, git, git_args,
    git_init, git_with_env, git_with_stdin_args, git_with_stdin_bytes, run_zmin, run_zmin_args,
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

struct StaticHttpServer {
    port: u16,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl StaticHttpServer {
    fn new(root: std::path::PathBuf) -> Self {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind static server");
        let port = listener.local_addr().expect("local addr").port();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_stop = stop.clone();
        let handle = std::thread::spawn(move || {
            loop {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if thread_stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let root = root.clone();
                std::thread::spawn(move || serve_static_http_connection(&root, &mut stream));
            }
        });
        Self {
            port,
            stop,
            handle: Some(handle),
        }
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
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct BackendHttpServer {
    port: u16,
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
exec /bin/sh -c "$cmd"
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

fn set_bare_head_to_main(remote: &std::path::Path) {
    git(remote, ["symbolic-ref", "HEAD", "refs/heads/main"]);
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
    configure_identity(&work);
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
        Self::with_service_newline(project_root, true)
    }

    fn bitbucket_style(project_root: std::path::PathBuf) -> Self {
        Self::with_service_newline(project_root, false)
    }

    fn with_service_newline(project_root: std::path::PathBuf, service_newline: bool) -> Self {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind smart http");
        let port = listener.local_addr().expect("local addr").port();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let upload_pack_requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let thread_stop = stop.clone();
        let thread_upload_pack_requests = upload_pack_requests.clone();
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
                std::thread::spawn(move || {
                    serve_smart_http_connection(
                        &root,
                        service_newline,
                        &upload_pack_requests,
                        &mut stream,
                    )
                });
            }
        });
        Self {
            port,
            upload_pack_requests,
            stop,
            handle: Some(handle),
        }
    }

    fn upload_pack_requests(&self) -> usize {
        self.upload_pack_requests
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl BackendHttpServer {
    fn new(command: String, project_root: std::path::PathBuf) -> Self {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind backend http");
        let port = listener.local_addr().expect("local addr").port();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_stop = stop.clone();
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
                std::thread::spawn(move || {
                    serve_backend_http_connection(&command, &root, &mut stream)
                });
            }
        });
        Self {
            port,
            stop,
            handle: Some(handle),
        }
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

    fn spawn_with_args(root: &std::path::Path, port: u16, extra_args: &[&str]) -> Self {
        let port_arg = format!("--port={port}");
        let base_path = format!("--base-path={}", root.display());
        let mut command = Command::new(stock_git_bin());
        command.args([
            "daemon",
            "--export-all",
            "--listen=127.0.0.1",
            port_arg.as_str(),
            base_path.as_str(),
        ]);
        command.args(extra_args);
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

fn serve_static_http_connection(root: &std::path::Path, stream: &mut std::net::TcpStream) {
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
    let headers = String::from_utf8_lossy(&request[..header_end]).to_string();
    let mut lines = headers.lines();
    let line = lines.next().unwrap_or_default().to_owned();
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
    if method == "POST" && path.ends_with("/git-upload-pack") {
        upload_pack_requests.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    let mut backend =
        http_backend_response_with_body("git", project_root, path, query, method, &body);
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

fn serve_backend_http_connection(
    command: &str,
    project_root: &std::path::Path,
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
    let headers = String::from_utf8_lossy(&request[..header_end]).to_string();
    let mut lines = headers.lines();
    let line = lines.next().unwrap_or_default().to_owned();
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
    let backend =
        http_backend_response_with_body(command, project_root, path, query, method, &body);
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
    if command == "git" {
        return Command::new(stock_git_bin());
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

    let git_server = BackendHttpServer::new("git".to_owned(), dir.path().to_path_buf());
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
        "manual-keep"
    );
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
fn clone_reads_smart_http_pack_like_stock_git() {
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
    let zmin_packs = fs::read_dir(zmin_clone.join(".git/objects/pack"))
        .expect("read zmin pack dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("pack"))
        .count();
    assert!(zmin_packs > 0, "smart HTTP clone should store a pack");
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
    let zmin_packs = fs::read_dir(zmin_client.join(".git/objects/pack"))
        .expect("read zmin pack dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("pack"))
        .count();
    assert!(zmin_packs > 0, "smart HTTP fetch should store a pack");
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
    let git_output = daemon_inetd_failure(
        "git",
        &["daemon", "--inetd", "--export-all", base_path.as_str()],
        &request,
    );
    let zmin_output = daemon_inetd_failure(
        zmin_bin(),
        &["daemon", "--inetd", "--export-all", base_path.as_str()],
        &request,
    );

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(zmin_output.2, git_output.2);
}

fn daemon_inetd_failure(command: &str, args: &[&str], stdin: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
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
    assert!(
        !output.status.success(),
        "{command} unexpectedly succeeded: {}",
        String::from_utf8_lossy(&output.stdout)
    );
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
