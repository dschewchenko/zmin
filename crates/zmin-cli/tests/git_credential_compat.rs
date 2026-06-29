mod common;

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use common::{
    command_stdout_bytes, git_status_with_stdin, git_with_stdin, run_zmin_status_with_stdin,
    run_zmin_with_stdin, zmin_bin,
};
use tempfile::TempDir;

#[test]
fn credential_matches_stock_git_for_basic_protocol_flows() {
    let repo = common::git_init();
    let complete = "protocol=https\nhost=example.com\nusername=u\npassword=p\n\n";
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["credential", "fill"], complete),
        git_with_stdin(repo.path(), ["credential", "fill"], complete)
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["credential", "approve"], complete),
        git_with_stdin(repo.path(), ["credential", "approve"], complete)
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["credential", "reject"], complete),
        git_with_stdin(repo.path(), ["credential", "reject"], complete)
    );
    let missing = "protocol=https\nhost=example.com\n\n";
    assert_eq!(
        run_zmin_status_with_stdin(repo.path(), ["credential", "fill"], missing),
        git_status_with_stdin(repo.path(), ["credential", "fill"], missing)
    );
}

#[test]
fn credential_store_matches_stock_git_for_store_get_and_erase() {
    let git_home = TempDir::new().expect("git home");
    let zmin_home = TempDir::new().expect("zmin home");
    let complete = "protocol=https\nhost=example.com\nusername=u\npassword=p\n\n";
    let query = "protocol=https\nhost=example.com\n\n";
    let erase = "protocol=https\nhost=example.com\nusername=u\n\n";

    assert_eq!(
        command_with_home_stdin(
            zmin_bin(),
            zmin_home.path(),
            &["credential-store", "store"],
            complete
        ),
        command_with_home_stdin(
            "git",
            git_home.path(),
            &["credential-store", "store"],
            complete
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_home.path().join(".git-credentials"))
            .expect("read zmin credentials"),
        fs::read_to_string(git_home.path().join(".git-credentials")).expect("read git credentials")
    );
    assert_eq!(
        command_with_home_stdin(
            zmin_bin(),
            zmin_home.path(),
            &["credential-store", "get"],
            query
        ),
        command_with_home_stdin("git", git_home.path(), &["credential-store", "get"], query)
    );
    assert_eq!(
        command_with_home_stdin(
            zmin_bin(),
            zmin_home.path(),
            &["credential-store", "erase"],
            erase
        ),
        command_with_home_stdin(
            "git",
            git_home.path(),
            &["credential-store", "erase"],
            erase
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_home.path().join(".git-credentials"))
            .expect("read zmin credentials after erase"),
        fs::read_to_string(git_home.path().join(".git-credentials"))
            .expect("read git credentials after erase")
    );
}

#[test]
fn credential_fill_uses_configured_store_helpers_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_home = dir.path().join("git-home");
    let zmin_home = dir.path().join("zmin-home");
    fs::create_dir_all(&git_home).expect("create git home");
    fs::create_dir_all(&zmin_home).expect("create zmin home");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    Command::new("git")
        .args(["init", "-q", git_repo.to_str().expect("git repo path")])
        .status()
        .expect("init git repo");
    Command::new("git")
        .args(["init", "-q", zmin_repo.to_str().expect("zmin repo path")])
        .status()
        .expect("init zmin repo");
    let git_first = dir.path().join("git first credentials");
    let git_second = dir.path().join("git second credentials");
    let zmin_first = dir.path().join("zmin first credentials");
    let zmin_second = dir.path().join("zmin second credentials");
    fs::write(&git_first, "https://first:one@example.com\n").expect("write git first creds");
    fs::write(&git_second, "https://second:two@example.com\n").expect("write git second creds");
    fs::write(&zmin_first, "https://first:one@example.com\n").expect("write zmin first creds");
    fs::write(&zmin_second, "https://second:two@example.com\n").expect("write zmin second creds");
    configure_helper(
        &git_repo,
        &format!("store --file '{}'", git_first.display()),
    );
    configure_helper(
        &git_repo,
        &format!("store --file '{}'", git_second.display()),
    );
    configure_helper(
        &zmin_repo,
        &format!("store --file '{}'", zmin_first.display()),
    );
    configure_helper(
        &zmin_repo,
        &format!("store --file '{}'", zmin_second.display()),
    );
    let query = "protocol=https\nhost=example.com\n\n";
    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "fill"],
            query,
            "git credential fill",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "fill"],
            query,
            "zmin credential fill",
        )
    );
}

#[test]
fn credential_approve_and_reject_use_configured_store_helper_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_home = dir.path().join("git-home");
    let zmin_home = dir.path().join("zmin-home");
    fs::create_dir_all(&git_home).expect("create git home");
    fs::create_dir_all(&zmin_home).expect("create zmin home");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    Command::new("git")
        .args(["init", "-q", git_repo.to_str().expect("git repo path")])
        .status()
        .expect("init git repo");
    Command::new("git")
        .args(["init", "-q", zmin_repo.to_str().expect("zmin repo path")])
        .status()
        .expect("init zmin repo");
    let git_store = dir.path().join("git helper credentials");
    let zmin_store = dir.path().join("zmin helper credentials");
    configure_helper(
        &git_repo,
        &format!("store --file '{}'", git_store.display()),
    );
    configure_helper(
        &zmin_repo,
        &format!("store --file '{}'", zmin_store.display()),
    );
    let complete = "protocol=https\nhost=example.com\nusername=u\npassword=p\n\n";
    let query = "protocol=https\nhost=example.com\n\n";
    let erase = "protocol=https\nhost=example.com\nusername=u\n\n";

    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "approve"],
            complete,
            "git credential approve",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "approve"],
            complete,
            "zmin credential approve",
        )
    );
    assert_eq!(
        fs::read_to_string(&git_store).expect("read git helper store"),
        fs::read_to_string(&zmin_store).expect("read zmin helper store")
    );
    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "fill"],
            query,
            "git credential fill",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "fill"],
            query,
            "zmin credential fill",
        )
    );
    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "reject"],
            erase,
            "git credential reject",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "reject"],
            erase,
            "zmin credential reject",
        )
    );
    let git_contents = fs::read_to_string(&git_store).unwrap_or_default();
    let zmin_contents = fs::read_to_string(&zmin_store).unwrap_or_default();
    assert_eq!(git_contents, zmin_contents);
}

#[cfg(unix)]
#[test]
fn credential_cache_matches_stock_git_for_store_get_and_erase() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::set_permissions(
        dir.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .expect("tighten socket dir permissions");
    let git_socket = dir.path().join("git.sock");
    let zmin_socket = dir.path().join("zmin.sock");
    let git_socket_arg = format!("--socket={}", git_socket.display());
    let zmin_socket_arg = format!("--socket={}", zmin_socket.display());
    let complete = "protocol=https\nhost=example.com\nusername=u\npassword=p\n\n";
    let query = "protocol=https\nhost=example.com\n\n";
    let erase = "protocol=https\nhost=example.com\nusername=u\n\n";

    assert_eq!(
        command_with_stdin(
            zmin_bin(),
            dir.path(),
            &["credential-cache", zmin_socket_arg.as_str(), "store"],
            complete,
            "zmin credential-cache store",
        ),
        command_with_stdin(
            "git",
            dir.path(),
            &["credential-cache", git_socket_arg.as_str(), "store"],
            complete,
            "git credential-cache store",
        )
    );
    assert_eq!(
        command_with_stdin(
            zmin_bin(),
            dir.path(),
            &["credential-cache", zmin_socket_arg.as_str(), "get"],
            query,
            "zmin credential-cache get",
        ),
        command_with_stdin(
            "git",
            dir.path(),
            &["credential-cache", git_socket_arg.as_str(), "get"],
            query,
            "git credential-cache get",
        )
    );
    assert_eq!(
        command_with_stdin(
            zmin_bin(),
            dir.path(),
            &["credential-cache", zmin_socket_arg.as_str(), "erase"],
            erase,
            "zmin credential-cache erase",
        ),
        command_with_stdin(
            "git",
            dir.path(),
            &["credential-cache", git_socket_arg.as_str(), "erase"],
            erase,
            "git credential-cache erase",
        )
    );
    assert_eq!(
        command_with_stdin(
            zmin_bin(),
            dir.path(),
            &["credential-cache", zmin_socket_arg.as_str(), "get"],
            query,
            "zmin credential-cache get after erase",
        ),
        command_with_stdin(
            "git",
            dir.path(),
            &["credential-cache", git_socket_arg.as_str(), "get"],
            query,
            "git credential-cache get after erase",
        )
    );
    command_stdout_bytes(
        zmin_bin(),
        dir.path(),
        &["credential-cache", zmin_socket_arg.as_str(), "exit"],
    );
    command_stdout_bytes(
        "git",
        dir.path(),
        &["credential-cache", git_socket_arg.as_str(), "exit"],
    );
}

#[cfg(unix)]
#[test]
fn credential_fill_approve_and_reject_use_configured_cache_helper_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::set_permissions(
        dir.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .expect("tighten socket dir permissions");
    let git_home = dir.path().join("git-home");
    let zmin_home = dir.path().join("zmin-home");
    fs::create_dir_all(&git_home).expect("create git home");
    fs::create_dir_all(&zmin_home).expect("create zmin home");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    Command::new("git")
        .args(["init", "-q", git_repo.to_str().expect("git repo path")])
        .status()
        .expect("init git repo");
    Command::new("git")
        .args(["init", "-q", zmin_repo.to_str().expect("zmin repo path")])
        .status()
        .expect("init zmin repo");
    let git_socket = dir.path().join("git helper.sock");
    let zmin_socket = dir.path().join("zmin helper.sock");
    configure_helper(
        &git_repo,
        &format!("cache --socket='{}' --timeout=60", git_socket.display()),
    );
    configure_helper(
        &zmin_repo,
        &format!("cache --socket='{}' --timeout=60", zmin_socket.display()),
    );
    let complete = "protocol=https\nhost=example.com\nusername=u\npassword=p\n\n";
    let query = "protocol=https\nhost=example.com\n\n";
    let erase = "protocol=https\nhost=example.com\nusername=u\n\n";

    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "approve"],
            complete,
            "git credential approve cache",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "approve"],
            complete,
            "zmin credential approve cache",
        )
    );
    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "fill"],
            query,
            "git credential fill cache",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "fill"],
            query,
            "zmin credential fill cache",
        )
    );
    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "reject"],
            erase,
            "git credential reject cache",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "reject"],
            erase,
            "zmin credential reject cache",
        )
    );
    assert_eq!(
        command_status_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "fill"],
            query,
            "git credential fill cache after reject",
        ),
        command_status_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "fill"],
            query,
            "zmin credential fill cache after reject",
        )
    );
    command_stdout_bytes(
        zmin_bin(),
        &zmin_repo,
        &[
            "credential-cache",
            &format!("--socket={}", zmin_socket.display()),
            "exit",
        ],
    );
    command_stdout_bytes(
        "git",
        &git_repo,
        &[
            "credential-cache",
            &format!("--socket={}", git_socket.display()),
            "exit",
        ],
    );
}

fn configure_helper(repo: &std::path::Path, helper: &str) {
    Command::new("git")
        .current_dir(repo)
        .args(["config", "--add", "credential.helper", helper])
        .status()
        .expect("configure helper");
}

fn command_with_home_stdin(
    command: &str,
    home: &std::path::Path,
    args: &[&str],
    stdin: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {command} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let (code, stdout, stderr) =
        command_status_with_home_cwd_stdin(command, home, cwd, args, stdin, label);
    assert_eq!(code, 0, "{label} failed: {stderr}");
    stdout
}

fn command_status_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> (i32, String, String) {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout)
            .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .unwrap_or_else(|err| panic!("{label} stderr utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
    )
}
