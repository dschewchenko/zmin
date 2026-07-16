mod common;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::{NamedTempFile, TempDir};

#[derive(Debug, PartialEq, Eq)]
struct TimedCommandResult {
    timed_out: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn stock_git_exec_path() -> String {
    let output = Command::new(common::stock_git_bin())
        .arg("--exec-path")
        .output()
        .expect("run stock git --exec-path");
    assert!(output.status.success(), "stock git --exec-path failed");
    String::from_utf8(output.stdout)
        .expect("stock git exec path utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn stock_git_citool_available() -> bool {
    Command::new(common::stock_git_bin())
        .args(["citool", "-h"])
        .env("GIT_EXEC_PATH", stock_git_exec_path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn run_with_timeout(
    program: &std::path::Path,
    args: &[&str],
    cwd: &std::path::Path,
    timeout: Duration,
) -> TimedCommandResult {
    run_with_timeout_env(program, args, cwd, timeout, &[])
}

fn run_with_timeout_env(
    program: &std::path::Path,
    args: &[&str],
    cwd: &std::path::Path,
    timeout: Duration,
    envs: &[(&str, &str)],
) -> TimedCommandResult {
    let stdout_file = NamedTempFile::new().expect("stdout temp file");
    let stderr_file = NamedTempFile::new().expect("stderr temp file");
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::from(stdout_file.reopen().expect("reopen stdout")))
        .stderr(Stdio::from(stderr_file.reopen().expect("reopen stderr")));
    command.envs(envs.iter().copied());
    if program == common::stock_git_bin() {
        command.env("GIT_EXEC_PATH", stock_git_exec_path());
    }
    let mut child = command.spawn().expect("spawn command");
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Some(status) = child.try_wait().expect("poll command") {
            return TimedCommandResult {
                timed_out: false,
                code: status.code(),
                stdout: String::from_utf8(std::fs::read(stdout_file.path()).expect("read stdout"))
                    .expect("stdout utf8")
                    .trim_end_matches('\n')
                    .to_owned(),
                stderr: String::from_utf8(std::fs::read(stderr_file.path()).expect("read stderr"))
                    .expect("stderr utf8")
                    .trim_end_matches('\n')
                    .to_owned(),
            };
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    let _ = child.wait_with_output();
    TimedCommandResult {
        timed_out: true,
        code: None,
        stdout: String::from_utf8(std::fs::read(stdout_file.path()).expect("read stdout"))
            .expect("stdout utf8")
            .trim_end_matches('\n')
            .to_owned(),
        stderr: String::from_utf8(std::fs::read(stderr_file.path()).expect("read stderr"))
            .expect("stderr utf8")
            .trim_end_matches('\n')
            .to_owned(),
    }
}

fn setup_staged_repo() -> TempDir {
    let repo = TempDir::new().expect("temp repo");
    common::git(repo.path(), ["init"]);
    common::configure_identity(repo.path());
    common::write_file(repo.path(), "a.txt", "base\n");
    common::git(repo.path(), ["add", "a.txt"]);
    common::git_with_env(repo.path(), ["commit", "-m", "base"]);
    common::write_file(repo.path(), "a.txt", "base\nchanged\n");
    common::git(repo.path(), ["add", "a.txt"]);
    repo
}

fn run_citool_pair(args: &[&str]) -> Option<()> {
    if !stock_git_citool_available() {
        return None;
    }

    let stock_repo = setup_staged_repo();
    let zmin_repo = setup_staged_repo();
    let stock = run_with_timeout(
        common::stock_git_bin(),
        &["citool"]
            .into_iter()
            .chain(args.iter().copied())
            .collect::<Vec<_>>(),
        stock_repo.path(),
        Duration::from_secs(3),
    );
    let zmin = run_with_timeout(
        std::path::Path::new(common::zmin_bin()),
        &["citool"]
            .into_iter()
            .chain(args.iter().copied())
            .collect::<Vec<_>>(),
        zmin_repo.path(),
        Duration::from_secs(3),
    );

    assert_eq!(zmin, stock, "citool command result differs for {args:?}");
    assert_eq!(
        common::git(stock_repo.path(), ["log", "-1", "--format=%s"]),
        common::git(zmin_repo.path(), ["log", "-1", "--format=%s"]),
        "HEAD subject differs for {args:?}"
    );
    assert_eq!(
        common::git(stock_repo.path(), ["status", "--short"]),
        common::git(zmin_repo.path(), ["status", "--short"]),
        "worktree status differs for {args:?}"
    );
    Some(())
}

#[test]
fn citool_helper_option_shapes_match_stock_git() {
    let temp = TempDir::new().expect("temp dir");
    let message_file = temp.path().join("message.txt");
    std::fs::write(&message_file, "file message\n").expect("write message file");
    let message_path = message_file.to_str().expect("message path");

    for args in [
        vec!["--amend"],
        vec!["--nocommit"],
        vec!["-m", "message"],
        vec!["--file", message_path],
        vec!["-F", message_path],
    ] {
        if run_citool_pair(&args).is_none() {
            eprintln!("skipping citool stock-oracle test because git-citool is unavailable");
            return;
        }
    }
}

#[test]
fn citool_usage_error_and_timeout_do_not_depend_on_stock_git_runtime() {
    let repo = setup_staged_repo();
    let poisoned_envs = [
        ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
        ("GIT_BIN", "/definitely/missing/git"),
    ];

    let timed = run_with_timeout_env(
        std::path::Path::new(common::zmin_bin()),
        &["citool", "--amend"],
        repo.path(),
        Duration::from_secs(2),
        &poisoned_envs,
    );
    assert!(
        timed.timed_out,
        "expected citool --amend to wait for GUI: {timed:?}"
    );
    assert_eq!(timed.stdout, "");
    assert_eq!(timed.stderr, "");

    let usage = run_with_timeout_env(
        std::path::Path::new(common::zmin_bin()),
        &["citool", "-m", "message"],
        repo.path(),
        Duration::from_secs(2),
        &poisoned_envs,
    );
    assert!(!usage.timed_out, "unexpected timeout: {usage:?}");
    assert_eq!(usage.code, Some(0));
    assert_eq!(usage.stdout, "");
    assert_eq!(usage.stderr, "");

    assert_eq!(
        common::git(repo.path(), ["log", "-1", "--format=%s"]),
        "base"
    );
    assert_eq!(common::git(repo.path(), ["status", "--short"]), "M  a.txt");
}
