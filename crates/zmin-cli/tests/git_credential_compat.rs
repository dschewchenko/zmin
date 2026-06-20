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
