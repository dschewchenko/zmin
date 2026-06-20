mod common;

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use common::{
    clone_repo_fixture, configure_identity, git, git_args, git_failure_output, git_init,
    git_with_env, run_zmin, run_zmin_args, run_zmin_failure_output, run_zmin_with_env, write_file,
    zmin_bin,
};
use tempfile::TempDir;

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

fn http_get_local(port: u16, path: &str) -> String {
    let mut last_error = None;
    for _ in 0..100 {
        match try_http_get_local(port, path) {
            Ok(response) => return response,
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    }
    panic!(
        "read local http response: {}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "no response".to_owned())
    );
}

fn try_http_get_local(port: u16, path: &str) -> std::io::Result<String> {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

fn git_config_path_pattern(path: &Path) -> String {
    let value = path.display().to_string().replace('\\', "/");
    if let Some(rest) = value.strip_prefix("//?/UNC/") {
        return format!("//{rest}");
    }
    if let Some(rest) = value.strip_prefix("//?/") {
        return rest.to_owned();
    }
    value
}

fn first_stderr_line(output: (i32, String, String)) -> (i32, String, String) {
    let stderr = output.2.lines().next().unwrap_or_default().to_owned();
    (output.0, output.1, stderr)
}

#[test]
fn optional_gitk_and_gitweb_match_stock_unavailable_shape() {
    let repo = git_init();

    for command in ["gitk", "gitweb"] {
        let stock = git_failure_output(repo.path(), &[command, "-h"]);
        if stock.0 != 1
            || !stock.1.is_empty()
            || !stock
                .2
                .starts_with(&format!("git: '{command}' is not a git command."))
        {
            continue;
        }
        assert_eq!(
            first_stderr_line(run_zmin_failure_output(repo.path(), &[command, "-h"])),
            first_stderr_line(stock)
        );
    }
}

fn command_with_home(command: &str, home: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .output()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
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

fn command_failure_with_home(
    command: &str,
    home: &std::path::Path,
    args: &[&str],
) -> (i32, String, String) {
    let output = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .output()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    assert!(
        !output.status.success(),
        "{command} unexpectedly succeeded: {}",
        String::from_utf8_lossy(&output.stdout)
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

fn command_with_isolated_config(
    command: &str,
    cwd: &std::path::Path,
    home: &std::path::Path,
    args: &[&str],
) -> String {
    let xdg = home.join(".config");
    let output = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", &xdg)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
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

fn command_failure_with_isolated_config(
    command: &str,
    cwd: &std::path::Path,
    home: &std::path::Path,
    args: &[&str],
) -> (i32, String, String) {
    let xdg = home.join(".config");
    let output = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", &xdg)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    assert!(
        !output.status.success(),
        "{command} unexpectedly succeeded: {}",
        String::from_utf8_lossy(&output.stdout)
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

fn command_any_with_isolated_config_and_env(
    command: &str,
    cwd: &std::path::Path,
    home: &std::path::Path,
    envs: &[(&str, &str)],
    args: &[&str],
) -> (i32, String, String) {
    let xdg = home.join(".config");
    let output = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", &xdg)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .envs(test_envs(envs))
        .output()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
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

fn test_envs<'a>(envs: &'a [(&'a str, &str)]) -> Vec<(&'a str, String)> {
    envs.iter()
        .map(|(key, value)| (*key, test_env_value(key, value)))
        .collect()
}

#[cfg(windows)]
fn test_env_value(key: &str, value: &str) -> String {
    if matches!(
        key,
        "GIT_EDITOR" | "GIT_SEQUENCE_EDITOR" | "VISUAL" | "EDITOR"
    ) {
        return value.replace('\\', "/");
    }

    value.to_owned()
}

#[cfg(not(windows))]
fn test_env_value(_key: &str, value: &str) -> String {
    value.to_owned()
}

fn command_any_with_git_editor(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
) -> (i32, String, String) {
    let output = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .env("GIT_EDITOR", "true")
        .output()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
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

fn command_any_with_git_editor_and_env(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> (i32, String, String) {
    let output = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .env("GIT_EDITOR", "true")
        .envs(test_envs(envs))
        .output()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
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

fn command_any_with_git_editor_env_and_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
    stdin: &str,
) -> (i32, String, String) {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .env("GIT_EDITOR", "true")
        .envs(test_envs(envs))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
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

#[test]
fn for_each_repo_matches_stock_git_for_configured_repositories() {
    let home = TempDir::new().expect("home dir");
    let first = git_init();
    let second = git_init();
    for repo in [first.path(), second.path()] {
        let repo = repo.to_str().expect("repo path");
        Command::new(common::stock_git_bin())
            .args(["config", "--global", "--add", "test.repos", repo])
            .env("HOME", home.path())
            .output()
            .expect("git config");
    }

    let args = [
        "for-each-repo",
        "--config=test.repos",
        "rev-parse",
        "--is-inside-work-tree",
    ];
    assert_eq!(
        command_with_home(zmin_bin(), home.path(), &args),
        command_with_home("git", home.path(), &args)
    );
}

#[test]
fn for_each_repo_missing_repo_failures_match_stock_git() {
    let home = TempDir::new().expect("home dir");
    let existing = git_init();
    let missing = home.path().join("missing-repo");
    for repo in [existing.path(), missing.as_path()] {
        let repo = repo.to_str().expect("repo path");
        Command::new(common::stock_git_bin())
            .args(["config", "--global", "--add", "test.repos", repo])
            .env("HOME", home.path())
            .output()
            .expect("git config");
    }

    let args = [
        "for-each-repo",
        "--config=test.repos",
        "rev-parse",
        "--is-inside-work-tree",
    ];
    assert_eq!(
        command_failure_with_home(zmin_bin(), home.path(), &args),
        command_failure_with_home("git", home.path(), &args)
    );

    let keep_going_args = [
        "for-each-repo",
        "--config=test.repos",
        "--keep-going",
        "rev-parse",
        "--is-inside-work-tree",
    ];
    assert_eq!(
        command_failure_with_home(zmin_bin(), home.path(), &keep_going_args),
        command_failure_with_home("git", home.path(), &keep_going_args)
    );
    assert_eq!(
        command_with_home(
            zmin_bin(),
            home.path(),
            &[
                "for-each-repo",
                "--config=missing.repos",
                "rev-parse",
                "--is-inside-work-tree",
            ],
        ),
        command_with_home(
            "git",
            home.path(),
            &[
                "for-each-repo",
                "--config=missing.repos",
                "rev-parse",
                "--is-inside-work-tree",
            ],
        )
    );
}

#[test]
fn bugreport_creates_report_file_in_output_directory() {
    let repo = git_init();
    let output = TempDir::new().expect("bugreport output");
    let zmin_output = command_any_with_git_editor(
        zmin_bin(),
        repo.path(),
        &[
            "bugreport",
            "--no-suffix",
            "-o",
            output.path().to_str().expect("output path"),
        ],
    );
    assert_eq!(zmin_output.0, 0);
    assert_eq!(zmin_output.1, "");
    assert!(zmin_output.2.contains("Created new report at '"));
    let report = output.path().join("git-bugreport.txt");
    let content = fs::read_to_string(report).expect("read bugreport");
    assert!(content.contains("[System Info]"));
    assert!(content.contains("[Repository]"));
}

#[test]
fn bugreport_suffix_modes_match_stock_git_files() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output = TempDir::new().expect("git bugreport output");
    let zmin_output = TempDir::new().expect("zmin bugreport output");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after unix epoch")
        .as_secs();
    let current_year = chrono::DateTime::from_timestamp(now as i64, 0)
        .expect("timestamp in chrono range")
        .format("%Y")
        .to_string();
    let strftime_file = format!("git-bugreport-{current_year}.txt");

    for (label, args, expected_file) in [
        (
            "custom suffix",
            vec!["bugreport", "-o", "__OUT__", "--suffix", "custom"],
            "git-bugreport-custom.txt".to_owned(),
        ),
        (
            "strftime suffix",
            vec!["bugreport", "-o", "__OUT__", "--suffix", "%Y"],
            strftime_file,
        ),
        (
            "path suffix",
            vec!["bugreport", "-o", "__OUT__", "--suffix", "nested/name"],
            "git-bugreport-nested/name.txt".to_owned(),
        ),
    ] {
        let git_args = args
            .iter()
            .map(|arg| {
                if *arg == "__OUT__" {
                    git_output.path().to_str().expect("git output path")
                } else {
                    arg
                }
            })
            .collect::<Vec<_>>();
        let zmin_args = args
            .iter()
            .map(|arg| {
                if *arg == "__OUT__" {
                    zmin_output.path().to_str().expect("zmin output path")
                } else {
                    arg
                }
            })
            .collect::<Vec<_>>();

        let git_result = command_any_with_git_editor("git", git_repo.path(), &git_args);
        let zmin_result = command_any_with_git_editor(zmin_bin(), zmin_repo.path(), &zmin_args);
        assert_eq!(zmin_result.0, git_result.0, "{label} exit mismatch");
        assert_eq!(zmin_result.1, git_result.1, "{label} stdout mismatch");
        assert!(
            git_output.path().join(&expected_file).is_file(),
            "stock Git did not create expected file for {label}",
        );
        assert!(
            zmin_output.path().join(&expected_file).is_file(),
            "zmin did not create expected file for {label}",
        );
    }
}

#[test]
fn diagnose_creates_zip_archive_with_stats_files() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "content\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let output = TempDir::new().expect("diagnose output");

    let result = command_any_with_git_editor(
        zmin_bin(),
        repo.path(),
        &[
            "diagnose",
            "-o",
            output.path().to_str().expect("output path"),
            "--suffix",
            "stats",
        ],
    );
    assert_eq!(result.0, 0);
    assert!(result.1.contains("Collecting diagnostic info"));
    assert!(result.1.contains("Repository root:"));
    assert!(result.2.contains("Diagnostics complete."));

    let archive = output.path().join("git-diagnostics-stats.zip");
    let bytes = fs::read(archive).expect("read diagnose archive");
    assert!(bytes.starts_with(b"PK\x03\x04"));
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("diagnostics.log"));
    assert!(text.contains("packs-local.txt"));
    assert!(text.contains("objects-local.txt"));
}

#[test]
fn bugreport_diagnose_creates_report_and_diagnostics_archive() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "content\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let output = TempDir::new().expect("bugreport diagnose output");

    let result = command_any_with_git_editor(
        zmin_bin(),
        repo.path(),
        &[
            "bugreport",
            "-o",
            output.path().to_str().expect("output path"),
            "--suffix",
            "diag",
            "--diagnose=stats",
        ],
    );
    assert_eq!(result.0, 0);
    assert!(result.1.contains("Collecting diagnostic info"));
    assert!(result.2.contains("Diagnostics complete."));
    assert!(result.2.contains("Created new report at '"));

    let report = output.path().join("git-bugreport-diag.txt");
    let archive = output.path().join("git-diagnostics-diag.zip");
    assert!(report.is_file());
    let bytes = fs::read(archive).expect("read diagnostics archive");
    assert!(bytes.starts_with(b"PK\x03\x04"));
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("diagnostics.log"));
    assert!(text.contains("packs-local.txt"));
    assert!(text.contains("objects-local.txt"));
}

#[test]
fn repo_info_reports_repository_metadata() {
    let repo = git_init();

    let info = command_any_with_git_editor(
        zmin_bin(),
        repo.path(),
        &[
            "repo",
            "info",
            "layout.bare",
            "layout.shallow",
            "object.format",
            "references.format",
        ],
    );
    assert_eq!(info.0, 0);
    assert_eq!(
        info.1,
        "layout.bare=false\nlayout.shallow=false\nobject.format=sha1\nreferences.format=files"
    );
    assert_eq!(info.2, "");

    let keys = command_any_with_git_editor(zmin_bin(), repo.path(), &["repo", "info", "--keys"]);
    assert_eq!(keys.0, 0);
    assert!(keys.1.contains("layout.bare"));
    assert!(keys.1.contains("references.format"));

    let nul = command_any_with_git_editor(
        zmin_bin(),
        repo.path(),
        &["repo", "info", "-z", "object.format"],
    );
    assert_eq!(nul.0, 0);
    assert_eq!(nul.1, "object.format\nsha1\0");
}

#[test]
fn backfill_matches_stock_git_for_complete_repository_noop() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "content\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    for args in [
        ["backfill"].as_slice(),
        ["backfill", "--min-batch-size=0"].as_slice(),
        ["backfill", "--sparse"].as_slice(),
        ["backfill", "--no-sparse"].as_slice(),
        ["backfill", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_any_with_git_editor(zmin_bin(), repo.path(), args),
            command_any_with_git_editor("git", repo.path(), args),
            "args: {args:?}"
        );
    }
}

fn loose_object_path(repo: &Path, hex: &str) -> std::path::PathBuf {
    repo.join(".git/objects").join(&hex[..2]).join(&hex[2..])
}

fn init_promisor_work_repo(remote: &Path, branch: &str) -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(
        repo.path(),
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path utf8"),
        ],
    );
    git(repo.path(), ["fetch", "origin"]);
    git(
        repo.path(),
        ["checkout", "-B", branch, &format!("origin/{branch}")],
    );
    git(repo.path(), ["config", "remote.origin.promisor", "true"]);
    repo
}

#[test]
fn backfill_promisor_remote_recovers_missing_local_objects() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    write_file(source.path(), "dir/b.txt", "two\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "initial"]);
    let source_head = git(source.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    let remote = TempDir::new().expect("remote dir");
    git_args(
        source.path(),
        &[
            "clone",
            "--bare",
            ".",
            remote.path().to_str().expect("remote path utf8"),
        ],
    );

    let git_repo = init_promisor_work_repo(remote.path(), &source_head);
    let zmin_repo = init_promisor_work_repo(remote.path(), &source_head);

    let blob = git(git_repo.path(), ["rev-list", "--objects", "HEAD"])
        .lines()
        .find_map(|line| line.strip_suffix(" a.txt").map(str::to_owned))
        .expect("blob id");
    let git_object_path = loose_object_path(git_repo.path(), &blob);
    let zmin_object_path = loose_object_path(zmin_repo.path(), &blob);
    assert!(
        git_object_path.is_file(),
        "expected loose object at {}",
        git_object_path.display()
    );
    assert!(
        zmin_object_path.is_file(),
        "expected loose object at {}",
        zmin_object_path.display()
    );
    fs::remove_file(&git_object_path).expect("remove git blob");
    fs::remove_file(&zmin_object_path).expect("remove zmin blob");
    assert!(!git_object_path.exists());
    assert!(!zmin_object_path.exists());

    let zmin_backfill =
        command_any_with_git_editor(zmin_bin(), zmin_repo.path(), &["backfill", "HEAD"]);
    let git_backfill = command_any_with_git_editor("git", git_repo.path(), &["backfill", "HEAD"]);
    assert_eq!(zmin_backfill.0, 0);
    assert_eq!(git_backfill.0, 0);
    assert_eq!(zmin_backfill.1, git_backfill.1);

    assert_eq!(git(git_repo.path(), ["cat-file", "-t", &blob]), "blob");
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-t", &blob]),
        "blob"
    );
}

#[test]
fn replay_matches_stock_git_for_linear_range() {
    let source = git_init();
    configure_identity(source.path());
    git(source.path(), ["config", "commit.gpgsign", "false"]);
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    let base = git(source.path(), ["rev-parse", "HEAD"]);
    write_file(source.path(), "a.txt", "two\n");
    git(source.path(), ["commit", "-am", "two"]);
    let tip = git(source.path(), ["rev-parse", "HEAD"]);
    let range = format!("{base}..{tip}");

    for args in [
        vec!["replay", range.as_str()],
        vec![
            "replay",
            "--contained",
            "--advance",
            "topic",
            range.as_str(),
        ],
    ] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["branch", "topic", &base]);
        git(zmin_repo.path(), ["branch", "topic", &base]);

        let zmin_output = command_any_with_git_editor(zmin_bin(), zmin_repo.path(), &args);
        let git_output = command_any_with_git_editor("git", git_repo.path(), &args);
        if args.len() == 2 {
            assert_eq!(zmin_output.0, git_output.0, "args: {args:?}");
            assert_eq!(zmin_output.1, git_output.1, "args: {args:?}");
            assert!(
                zmin_output.2.starts_with("error:")
                    && git_output.2.starts_with("error:")
                    && zmin_output.2.contains("usage:")
                    && git_output.2.contains("usage:"),
                "args: {args:?}\nzmin: {}\ngit: {}",
                zmin_output.2,
                git_output.2
            );
        } else {
            assert_eq!(zmin_output, git_output, "args: {args:?}");
        }
    }

    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    git(git_repo.path(), ["branch", "topic", &base]);
    git(zmin_repo.path(), ["branch", "topic", &base]);

    let zmin_output = command_any_with_git_editor_and_env(
        zmin_bin(),
        zmin_repo.path(),
        &["replay", "--advance", "topic", &range],
        &[("GIT_COMMITTER_DATE", "1700000100 +0000")],
    );
    let git_output = command_any_with_git_editor_and_env(
        "git",
        git_repo.path(),
        &["replay", "--advance", "topic", &range],
        &[("GIT_COMMITTER_DATE", "1700000100 +0000")],
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.2, git_output.2);
    if git_output.1.starts_with("update refs/heads/topic ") {
        assert_eq!(git(git_repo.path(), ["rev-parse", "topic"]), base);
    } else {
        assert_eq!(
            git(zmin_repo.path(), ["rev-parse", "topic"]),
            git(git_repo.path(), ["rev-parse", "topic"])
        );
    }

    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "commit.gpgsign", "false"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    let base = git(repo.path(), ["rev-parse", "HEAD"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["commit", "-am", "two"]);
    let tip = git(repo.path(), ["rev-parse", "HEAD"]);
    let range = format!("{base}..{tip}");
    assert_eq!(
        command_any_with_git_editor_and_env(
            zmin_bin(),
            repo.path(),
            &["replay", "--onto", &base, &range],
            &[("GIT_COMMITTER_DATE", "1700000100 +0000")]
        ),
        command_any_with_git_editor_and_env(
            "git",
            repo.path(),
            &["replay", "--onto", &base, &range],
            &[("GIT_COMMITTER_DATE", "1700000100 +0000")]
        )
    );
}

#[test]
fn history_reword_dry_run_prints_ref_updates_without_moving_branch() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "old"]);
    let old_tip = git(repo.path(), ["rev-parse", "HEAD"]);
    let editor = repo.path().join("editor.sh");
    fs::write(&editor, "#!/bin/sh\nprintf 'new message\\n' > \"$1\"\n").expect("write editor");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&editor)
            .expect("editor metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&editor, permissions).expect("chmod editor");
    }

    let output = command_any_with_git_editor_and_env(
        zmin_bin(),
        repo.path(),
        &["history", "reword", "HEAD", "--dry-run"],
        &[("GIT_EDITOR", editor.to_str().expect("editor path utf8"))],
    );
    assert_eq!(output.0, 0, "stdout: {}\nstderr: {}", output.1, output.2);
    assert_eq!(output.2, "");
    let head_ref = git(repo.path(), ["symbolic-ref", "HEAD"]);
    assert!(output.1.starts_with(&format!("update {head_ref} ")));
    assert!(output.1.ends_with(&format!(" {old_tip}")));
    assert_eq!(git(repo.path(), ["rev-parse", "HEAD"]), old_tip);
    let new_tip = output.1.split_whitespace().nth(2).expect("new tip");
    assert!(git(repo.path(), ["cat-file", "-p", new_tip]).contains("new message"));
}

#[test]
fn history_split_dry_run_splits_selected_file_hunks() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["commit", "--allow-empty", "-m", "base"]);
    let base = git(repo.path(), ["rev-parse", "HEAD"]);
    write_file(repo.path(), "bar", "bar\n");
    write_file(repo.path(), "foo", "foo\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "original"]);
    let old_tip = git(repo.path(), ["rev-parse", "HEAD"]);
    let editor = repo.path().join("split-editor.sh");
    let counter = repo.path().join("split-editor-count");
    fs::write(
        &editor,
        format!(
            "#!/bin/sh\nif [ ! -f {:?} ]; then echo 1 > {:?}; printf 'split-out commit\\n' > \"$1\"; else printf 'original rewritten\\n' > \"$1\"; fi\n",
            counter, counter
        ),
    )
    .expect("write editor");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&editor)
            .expect("editor metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&editor, permissions).expect("chmod editor");
    }

    let output = command_any_with_git_editor_env_and_stdin(
        zmin_bin(),
        repo.path(),
        &["history", "split", "HEAD", "--dry-run"],
        &[("GIT_EDITOR", editor.to_str().expect("editor path utf8"))],
        "y\nn\n",
    );
    assert_eq!(output.0, 0);
    assert_eq!(output.2, "");
    let head_ref = git(repo.path(), ["symbolic-ref", "HEAD"]);
    assert!(output.1.starts_with(&format!("update {head_ref} ")));
    assert!(output.1.ends_with(&format!(" {old_tip}")));
    assert_eq!(git(repo.path(), ["rev-parse", "HEAD"]), old_tip);

    let new_tip = output.1.split_whitespace().nth(2).expect("new tip");
    let split_id = git(repo.path(), ["log", "--format=%P", "-n1", new_tip]);
    assert_ne!(split_id, base);
    assert_eq!(
        git(repo.path(), ["ls-tree", "--name-only", "-r", &split_id]),
        "bar"
    );
    assert_eq!(
        git(repo.path(), ["ls-tree", "--name-only", "-r", new_tip]),
        "bar\nfoo"
    );
    assert!(git(repo.path(), ["cat-file", "-p", &split_id]).contains("split-out commit"));
    assert!(git(repo.path(), ["cat-file", "-p", new_tip]).contains("original rewritten"));
}

#[test]
fn history_split_pathspec_can_select_all_matching_hunks() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["commit", "--allow-empty", "-m", "base"]);
    write_file(repo.path(), "bar", "bar\n");
    write_file(repo.path(), "foo", "foo\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "original"]);
    let old_tip = git(repo.path(), ["rev-parse", "HEAD"]);
    let editor = repo.path().join("split-pathspec-editor.sh");
    fs::write(&editor, "#!/bin/sh\nprintf 'edited\\n' > \"$1\"\n").expect("write editor");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&editor)
            .expect("editor metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&editor, permissions).expect("chmod editor");
    }

    let output = command_any_with_git_editor_env_and_stdin(
        zmin_bin(),
        repo.path(),
        &["history", "split", "HEAD", "--dry-run", "--", "bar"],
        &[("GIT_EDITOR", editor.to_str().expect("editor path utf8"))],
        "y\n",
    );
    assert_eq!(output.0, 0);
    assert_eq!(output.2, "");
    assert!(output.1.ends_with(&format!(" {old_tip}")));
    let new_tip = output.1.split_whitespace().nth(2).expect("new tip");
    let split_id = git(repo.path(), ["log", "--format=%P", "-n1", new_tip]);
    assert_eq!(
        git(repo.path(), ["ls-tree", "--name-only", "-r", &split_id]),
        "bar"
    );
    assert_eq!(
        git(repo.path(), ["ls-tree", "--name-only", "-r", new_tip]),
        "bar\nfoo"
    );
}

#[test]
#[cfg(unix)]
fn hook_run_matches_stock_git_for_missing_and_executable_hooks() {
    let repo = git_init();

    assert_eq!(
        command_any_with_git_editor(zmin_bin(), repo.path(), &["hook", "run", "pre-commit"]),
        command_any_with_git_editor("git", repo.path(), &["hook", "run", "pre-commit"])
    );
    assert_eq!(
        command_any_with_git_editor(
            zmin_bin(),
            repo.path(),
            &["hook", "run", "--ignore-missing", "pre-commit"]
        ),
        command_any_with_git_editor(
            "git",
            repo.path(),
            &["hook", "run", "--ignore-missing", "pre-commit"]
        )
    );

    let hook = repo.path().join(".git/hooks/pre-commit");
    fs::write(
        &hook,
        "#!/bin/sh\nprintf 'out:%s:' \"$1\"\ncat\nprintf 'err:%s\\n' \"$1\" >&2\nexit 3\n",
    )
    .expect("write hook");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("chmod hook");
    write_file(repo.path(), "hook-input.txt", "stdin");

    let args = [
        "hook",
        "run",
        "--to-stdin=hook-input.txt",
        "pre-commit",
        "--",
        "arg",
    ];
    assert_eq!(
        command_any_with_git_editor(zmin_bin(), repo.path(), &args),
        command_any_with_git_editor("git", repo.path(), &args)
    );
}

#[test]
fn managed_hooks_add_list_remove_and_protect_manual_hooks() {
    let repo = git_init();
    configure_identity(repo.path());

    run_zmin(repo.path(), ["hooks", "init"]);
    assert!(repo.path().join(".git/hooks").is_dir());
    assert!(repo.path().join(".git/zmin").is_dir());

    run_zmin(
        repo.path(),
        [
            "hooks",
            "add",
            "pre-commit",
            "printf managed > hook-ran.txt",
        ],
    );
    run_zmin(
        repo.path(),
        [
            "hooks",
            "add",
            "pre-commit",
            "printf second > hook-second.txt",
        ],
    );
    assert_eq!(
        run_zmin(repo.path(), ["hooks", "list"]),
        "pre-commit\tprintf managed > hook-ran.txt\npre-commit\tprintf second > hook-second.txt"
    );
    assert_eq!(
        git(
            repo.path(),
            ["config", "--get-all", "zmin.hooks.pre-commit"]
        ),
        "printf managed > hook-ran.txt\nprintf second > hook-second.txt"
    );

    let hook = repo.path().join(".git/hooks/pre-commit");
    let hook_contents = fs::read_to_string(&hook).expect("read managed hook");
    assert!(hook_contents.contains("# zmin-managed-hook"));
    assert!(
        hook_contents.contains("sh -c 'printf managed > hook-ran.txt' zmin-managed-hook \"$@\"")
    );
    assert!(
        hook_contents.contains("sh -c 'printf second > hook-second.txt' zmin-managed-hook \"$@\"")
    );

    write_file(repo.path(), "file.txt", "content\n");
    run_zmin(repo.path(), ["add", "file.txt"]);
    run_zmin(repo.path(), ["commit", "-m", "initial"]);
    assert_eq!(
        fs::read_to_string(repo.path().join("hook-ran.txt")).expect("read hook output"),
        "managed"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("hook-second.txt")).expect("read second hook output"),
        "second"
    );

    run_zmin(
        repo.path(),
        [
            "hooks",
            "add",
            "pre-push",
            "printf before > hook-before-fail.txt",
        ],
    );
    run_zmin(repo.path(), ["hooks", "add", "pre-push", "exit 17"]);
    run_zmin(
        repo.path(),
        [
            "hooks",
            "add",
            "pre-push",
            "printf after > hook-after-fail.txt",
        ],
    );
    let failing_hook = run_zmin_failure_output(repo.path(), &["hook", "run", "pre-push"]);
    assert_eq!(failing_hook.0, 17);
    assert_eq!(
        fs::read_to_string(repo.path().join("hook-before-fail.txt"))
            .expect("read before fail hook"),
        "before"
    );
    assert!(!repo.path().join("hook-after-fail.txt").exists());
    run_zmin(repo.path(), ["hooks", "remove", "pre-push"]);

    run_zmin(repo.path(), ["hooks", "remove", "pre-commit"]);
    assert_eq!(run_zmin(repo.path(), ["hooks", "list"]), "");
    assert_eq!(
        run_zmin_failure_output(
            repo.path(),
            &["config", "--get-all", "zmin.hooks.pre-commit"]
        )
        .0,
        1
    );
    assert!(!hook.exists());

    run_zmin(
        repo.path(),
        [
            "hooks",
            "add",
            "commit-msg",
            "printf \"$1\" > hook-message-path.txt",
        ],
    );
    write_file(repo.path(), "second.txt", "content\n");
    run_zmin(repo.path(), ["add", "second.txt"]);
    run_zmin(repo.path(), ["commit", "-m", "second"]);
    let message_path =
        fs::read_to_string(repo.path().join("hook-message-path.txt")).expect("read hook arg");
    assert!(message_path.ends_with("COMMIT_EDITMSG"));
    run_zmin(repo.path(), ["hooks", "remove", "commit-msg"]);

    let manual_hook = repo.path().join(".git/hooks/post-merge");
    fs::write(&manual_hook, "#!/bin/sh\nexit 0\n").expect("write manual hook");
    let failure = run_zmin_failure_output(
        repo.path(),
        &["hooks", "add", "post-merge", "printf managed"],
    );
    assert_eq!(failure.0, 1);
    assert!(failure.2.contains("refusing to overwrite existing hook"));
    assert_eq!(
        fs::read_to_string(manual_hook).expect("read manual hook"),
        "#!/bin/sh\nexit 0\n"
    );

    run_zmin(
        repo.path(),
        [
            "hooks",
            "add",
            "--force",
            "post-merge",
            "printf forced > forced-hook.txt",
        ],
    );
    let forced_hook_contents =
        fs::read_to_string(repo.path().join(".git/hooks/post-merge")).expect("read forced hook");
    assert!(forced_hook_contents.contains("# zmin-managed-hook"));
    assert_eq!(
        git(
            repo.path(),
            ["config", "--get-all", "zmin.hooks.post-merge"]
        ),
        "printf forced > forced-hook.txt"
    );
    run_zmin(repo.path(), ["hook", "run", "post-merge"]);
    assert_eq!(
        fs::read_to_string(repo.path().join("forced-hook.txt")).expect("read forced hook output"),
        "forced"
    );
}

#[test]
fn version_command_reports_git_compatible_version_shape() {
    let repo = git_init();

    let version = command_any_with_git_editor(zmin_bin(), repo.path(), &["version"]);
    assert_eq!(version.0, 0);
    assert!(version.1.starts_with("git version "));
    assert!(version.1.contains("(zmin "));
    assert_eq!(version.2, "");

    let build_options =
        command_any_with_git_editor(zmin_bin(), repo.path(), &["version", "--build-options"]);
    assert_eq!(build_options.0, 0);
    assert!(build_options.1.starts_with("git version "));
    for expected in [
        "cpu:",
        "sizeof-long:",
        "sizeof-size_t:",
        "shell-path:",
        "default-ref-format:",
        "zmin-version:",
        "SHA-1:",
        "SHA-256:",
    ] {
        assert!(
            build_options.1.contains(expected),
            "missing {expected} in {}",
            build_options.1
        );
    }
    assert_eq!(build_options.2, "");
}

#[test]
fn config_get_set_and_list_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    git(git_repo.path(), ["config", "user.name", "Bench"]);
    run_zmin(zmin_repo.path(), ["config", "user.name", "Bench"]);
    git(
        git_repo.path(),
        ["config", "user.email", "bench@example.test"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "user.email", "bench@example.test"],
    );
    git(git_repo.path(), ["config", "branch.main.remote", "origin"]);
    run_zmin(zmin_repo.path(), ["config", "branch.main.remote", "origin"]);
    git(
        git_repo.path(),
        ["config", "branch.main.merge", "refs/heads/main"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "branch.main.merge", "refs/heads/main"],
    );

    assert_eq!(
        run_zmin(zmin_repo.path(), ["config", "--get", "user.name"]),
        git(git_repo.path(), ["config", "--get", "user.name"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["config", "branch.main.remote"]),
        git(git_repo.path(), ["config", "branch.main.remote"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["config", "--list"]),
        git(git_repo.path(), ["config", "--list"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["config", "--null", "--list"]),
        git(git_repo.path(), ["config", "--null", "--list"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["config", "--get-regexp", "^user\\."]),
        git(git_repo.path(), ["config", "--get-regexp", "^user\\."])
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["config", "--null", "--get-regexp", "^user\\."]
        ),
        git(
            git_repo.path(),
            ["config", "--null", "--get-regexp", "^user\\."]
        )
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["config", "--get-regexp", "^user\\.", "^Bench$"]
        ),
        git(
            git_repo.path(),
            ["config", "--get-regexp", "^user\\.", "^Bench$"]
        )
    );
    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["config", "--get-regexp", "^missing\\."]),
        git_failure_output(git_repo.path(), &["config", "--get-regexp", "^missing\\."])
    );
}

#[test]
fn config_local_scope_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    git(
        git_repo.path(),
        ["config", "--local", "core.ignoreCase", "false"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "--local", "core.ignoreCase", "false"],
    );

    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["config", "--local", "--get", "core.ignoreCase"]
        ),
        git(
            git_repo.path(),
            ["config", "--local", "--get", "core.ignoreCase"]
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["config", "--local", "--list"]),
        git(git_repo.path(), ["config", "--local", "--list"])
    );
}

#[test]
fn config_set_append_preserves_multivalue_entries_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["symbolic-ref", "HEAD", "refs/heads/main"]);
    }

    git(
        git_repo.path(),
        [
            "config",
            "set",
            "remote.two.fetch",
            "+refs/heads/main:refs/remotes/two/main",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "config",
            "set",
            "remote.two.fetch",
            "+refs/heads/main:refs/remotes/two/main",
        ],
    );
    git(
        git_repo.path(),
        [
            "config",
            "set",
            "--append",
            "remote.two.fetch",
            "+refs/heads/one:refs/remotes/two/one",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "config",
            "set",
            "--append",
            "remote.two.fetch",
            "+refs/heads/one:refs/remotes/two/one",
        ],
    );

    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["config", "--get-all", "remote.two.fetch"]
        ),
        git(git_repo.path(), ["config", "--get-all", "remote.two.fetch"])
    );
}

#[test]
fn config_include_origin_and_scope_match_stock_git() {
    let home = TempDir::new().expect("home dir");
    let git_repo = git_init();
    let zmin_repo = git_init();

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["symbolic-ref", "HEAD", "refs/heads/main"]);
        fs::write(repo.join("inc.cfg"), "[demo]\n\tincluded = from-include\n")
            .expect("write include config");
        let local_config = repo.join(".git/config");
        let mut config = fs::read_to_string(&local_config).expect("read local config");
        config.push_str("[include]\n\tpath = ../inc.cfg\n[demo]\n\tlocal = from-local\n");
        fs::write(local_config, config).expect("write local config");
    }

    for args in [
        &["config", "--get", "demo.included"][..],
        &["config", "--includes", "--get", "demo.included"],
        &["config", "--show-origin", "--get", "demo.included"],
        &[
            "config",
            "--null",
            "--show-origin",
            "--get",
            "demo.included",
        ],
        &[
            "config",
            "--show-scope",
            "--show-origin",
            "--includes",
            "--get",
            "demo.included",
        ],
        &["config", "--show-origin", "--list"],
        &["config", "--null", "--show-origin", "--list"],
        &["config", "--show-scope", "--show-origin", "--list"],
        &[
            "config",
            "--null",
            "--show-scope",
            "--show-origin",
            "--list",
        ],
    ] {
        assert_eq!(
            command_with_isolated_config(zmin_bin(), zmin_repo.path(), home.path(), args),
            command_with_isolated_config("git", git_repo.path(), home.path(), args),
            "config include/origin/scope mismatch for {args:?}"
        );
    }

    let global_args = [
        "-c",
        "demo.flag",
        "config",
        "--show-scope",
        "--show-origin",
        "--bool",
        "--get",
        "demo.flag",
    ];
    assert_eq!(
        command_with_isolated_config(zmin_bin(), zmin_repo.path(), home.path(), &global_args),
        command_with_isolated_config("git", git_repo.path(), home.path(), &global_args)
    );

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join("conditional.cfg"),
            "[demo]\n\tconditional = from-include-if\n",
        )
        .expect("write conditional include config");
        fs::write(
            repo.join("conditional-miss.cfg"),
            "[demo]\n\tconditionalMiss = should-not-load\n",
        )
        .expect("write nonmatching conditional include config");
        fs::write(
            repo.join("onbranch-main.cfg"),
            "[demo]\n\tonbranch = main\n",
        )
        .expect("write onbranch main config");
        fs::write(
            repo.join("onbranch-feature.cfg"),
            "[demo]\n\tonbranch = feature\n",
        )
        .expect("write onbranch feature config");
        let git_dir = fs::canonicalize(repo.join(".git")).expect("canonical git dir");
        let git_dir_pattern = git_config_path_pattern(&git_dir);
        let local_config = git_dir.join("config");
        let mut config = fs::read_to_string(&local_config).expect("read local config");
        config.push_str(&format!(
            "[includeIf \"gitdir:{}\"]\n\tpath = ../conditional.cfg\n[includeIf \"gitdir:{}/missing\"]\n\tpath = ../conditional-miss.cfg\n[includeIf \"onbranch:main\"]\n\tpath = ../onbranch-main.cfg\n[includeIf \"onbranch:feature/*\"]\n\tpath = ../onbranch-feature.cfg\n",
            git_dir_pattern, git_dir_pattern
        ));
        fs::write(local_config, config).expect("write local config");
    }

    for args in [
        &["config", "--get", "demo.conditional"][..],
        &[
            "config",
            "--show-scope",
            "--show-origin",
            "--get",
            "demo.conditional",
        ],
    ] {
        assert_eq!(
            command_with_isolated_config(zmin_bin(), zmin_repo.path(), home.path(), args),
            command_with_isolated_config("git", git_repo.path(), home.path(), args),
            "config includeIf gitdir mismatch for {args:?}"
        );
    }
    assert_eq!(
        command_failure_with_isolated_config(
            zmin_bin(),
            zmin_repo.path(),
            home.path(),
            &["config", "--get", "demo.conditionalmiss"],
        ),
        command_failure_with_isolated_config(
            "git",
            git_repo.path(),
            home.path(),
            &["config", "--get", "demo.conditionalmiss"],
        )
    );

    assert_eq!(
        command_with_isolated_config(
            zmin_bin(),
            zmin_repo.path(),
            home.path(),
            &["config", "--get", "demo.onbranch"],
        ),
        command_with_isolated_config(
            "git",
            git_repo.path(),
            home.path(),
            &["config", "--get", "demo.onbranch"],
        )
    );
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["checkout", "-b", "feature/test"]);
    }
    assert_eq!(
        command_with_isolated_config(
            zmin_bin(),
            zmin_repo.path(),
            home.path(),
            &["config", "--get", "demo.onbranch"],
        ),
        command_with_isolated_config(
            "git",
            git_repo.path(),
            home.path(),
            &["config", "--get", "demo.onbranch"],
        )
    );
}

#[test]
fn config_include_hasconfig_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_home = TempDir::new().expect("git home");
    let zmin_home = TempDir::new().expect("zmin home");
    for home in [git_home.path(), zmin_home.path()] {
        fs::write(
            home.join(".gitconfig"),
            "[includeIf \"hasconfig:remote.*.url:https://example.com/**\"]\n\tpath = inc.cfg\n",
        )
        .expect("write global config");
        fs::write(home.join("inc.cfg"), "[demo]\n\tflag = yes\n").expect("write include config");
    }

    git(
        git_repo.path(),
        [
            "config",
            "remote.origin.url",
            "https://example.com/org/repo.git",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "config",
            "remote.origin.url",
            "https://example.com/org/repo.git",
        ],
    );
    assert_eq!(
        command_any_with_isolated_config_and_env(
            zmin_bin(),
            zmin_repo.path(),
            zmin_home.path(),
            &[],
            &["config", "--get", "demo.flag"],
        ),
        command_any_with_isolated_config_and_env(
            "git",
            git_repo.path(),
            git_home.path(),
            &[],
            &["config", "--get", "demo.flag"],
        ),
        "hasconfig include should match remote URL"
    );

    git(
        git_repo.path(),
        [
            "config",
            "remote.origin.url",
            "https://other.test/org/repo.git",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "config",
            "remote.origin.url",
            "https://other.test/org/repo.git",
        ],
    );
    assert_eq!(
        command_any_with_isolated_config_and_env(
            zmin_bin(),
            zmin_repo.path(),
            zmin_home.path(),
            &[],
            &["config", "--get", "demo.flag"],
        ),
        command_any_with_isolated_config_and_env(
            "git",
            git_repo.path(),
            git_home.path(),
            &[],
            &["config", "--get", "demo.flag"],
        ),
        "hasconfig include should skip non-matching remote URL"
    );

    for home in [git_home.path(), zmin_home.path()] {
        fs::write(
            home.join("inc.cfg"),
            "[remote \"nested\"]\n\turl = https://nested.test/repo.git\n[demo]\n\tflag = bad\n",
        )
        .expect("write forbidden include config");
    }
    git(
        git_repo.path(),
        [
            "config",
            "remote.origin.url",
            "https://example.com/org/repo.git",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "config",
            "remote.origin.url",
            "https://example.com/org/repo.git",
        ],
    );
    let zmin_forbidden = command_any_with_isolated_config_and_env(
        zmin_bin(),
        zmin_repo.path(),
        zmin_home.path(),
        &[],
        &["config", "--get", "demo.flag"],
    );
    let git_forbidden = command_any_with_isolated_config_and_env(
        "git",
        git_repo.path(),
        git_home.path(),
        &[],
        &["config", "--get", "demo.flag"],
    );
    assert_eq!(zmin_forbidden.0, git_forbidden.0);
    assert_eq!(zmin_forbidden.1, git_forbidden.1);
    assert_eq!(
        zmin_forbidden.2, git_forbidden.2,
        "hasconfig include must reject nested remote URLs"
    );
}

#[test]
fn config_type_bool_matches_stock_git_for_get_set_and_failures() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    git(
        git_repo.path(),
        ["config", "--type=bool", "feature.enabled", "yes"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "--type=bool", "feature.enabled", "yes"],
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["config", "--get", "feature.enabled"]),
        git(git_repo.path(), ["config", "--get", "feature.enabled"])
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["config", "--type", "bool", "feature.enabled"]
        ),
        git(
            git_repo.path(),
            ["config", "--type", "bool", "feature.enabled"]
        )
    );

    git(
        git_repo.path(),
        ["config", "--type=bool", "feature.disabled", "no"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "--type=bool", "feature.disabled", "no"],
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["config", "--get", "feature.disabled"]),
        git(git_repo.path(), ["config", "--get", "feature.disabled"])
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["config", "--type=bool", "feature.disabled"]
        ),
        git(
            git_repo.path(),
            ["config", "--type=bool", "feature.disabled"]
        )
    );
    assert_eq!(
        run_zmin_failure_output(
            zmin_repo.path(),
            &["config", "--type=bool", "feature.bad", "maybe"],
        ),
        git_failure_output(
            git_repo.path(),
            &["config", "--type=bool", "feature.bad", "maybe"],
        )
    );

    git(git_repo.path(), ["config", "feature.raw", "maybe"]);
    run_zmin(zmin_repo.path(), ["config", "feature.raw", "maybe"]);
    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["config", "--type=bool", "feature.raw"]),
        git_failure_output(git_repo.path(), &["config", "--type=bool", "feature.raw"])
    );
}

#[test]
fn config_type_int_and_bool_or_int_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for (key, value) in [
        ("demo.plain", "42"),
        ("demo.kib", "1k"),
        ("demo.mib", "2m"),
        ("demo.gib", "3g"),
    ] {
        git(git_repo.path(), ["config", "--type=int", key, value]);
        run_zmin(zmin_repo.path(), ["config", "--type=int", key, value]);
        assert_eq!(
            run_zmin(zmin_repo.path(), ["config", "--get", key]),
            git(git_repo.path(), ["config", "--get", key]),
            "raw stored value for {key}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["config", "--type=int", key]),
            git(git_repo.path(), ["config", "--type=int", key]),
            "typed value for {key}"
        );
    }

    git(
        git_repo.path(),
        ["config", "--type=bool-or-int", "demo.mix", "true"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "--type=bool-or-int", "demo.mix", "true"],
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["config", "--type=bool-or-int", "demo.mix"]
        ),
        git(
            git_repo.path(),
            ["config", "--type=bool-or-int", "demo.mix"]
        )
    );
    git(
        git_repo.path(),
        ["config", "--type=bool-or-int", "demo.mix", "2k"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "--type=bool-or-int", "demo.mix", "2k"],
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["config", "--get", "demo.mix"]),
        git(git_repo.path(), ["config", "--get", "demo.mix"])
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["config", "--type=bool-or-int", "demo.mix"]
        ),
        git(
            git_repo.path(),
            ["config", "--type=bool-or-int", "demo.mix"]
        )
    );
    assert_eq!(
        run_zmin_failure_output(
            zmin_repo.path(),
            &["config", "--type=int", "demo.bad", "bad"],
        ),
        git_failure_output(
            git_repo.path(),
            &["config", "--type=int", "demo.bad", "bad"]
        )
    );
}

#[test]
fn config_type_bool_or_str_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    git(
        git_repo.path(),
        ["config", "--type=bool-or-str", "demo.flag", "yes"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "--type=bool-or-str", "demo.flag", "yes"],
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["config", "--get", "demo.flag"]),
        git(git_repo.path(), ["config", "--get", "demo.flag"])
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["config", "--type=bool-or-str", "demo.flag"]
        ),
        git(
            git_repo.path(),
            ["config", "--type=bool-or-str", "demo.flag"]
        )
    );

    git(
        git_repo.path(),
        ["config", "--type=bool-or-str", "demo.label", "plain"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "--type=bool-or-str", "demo.label", "plain"],
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["config", "--get", "demo.label"]),
        git(git_repo.path(), ["config", "--get", "demo.label"])
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["config", "--type=bool-or-str", "demo.label"]
        ),
        git(
            git_repo.path(),
            ["config", "--type=bool-or-str", "demo.label"]
        )
    );
}

#[test]
fn config_legacy_type_flags_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for (key, value, args) in [
        (
            "demo.bool",
            "yes",
            ["config", "--bool", "demo.bool"].as_slice(),
        ),
        ("demo.int", "1k", ["config", "--int", "demo.int"].as_slice()),
        (
            "demo.boolint",
            "1k",
            ["config", "--bool-or-int", "demo.boolint"].as_slice(),
        ),
        (
            "demo.boolstr",
            "yes",
            ["config", "--bool-or-str", "demo.boolstr"].as_slice(),
        ),
        (
            "demo.path",
            "~/abc",
            ["config", "--path", "demo.path"].as_slice(),
        ),
        (
            "demo.expiry",
            "never",
            ["config", "--expiry-date", "demo.expiry"].as_slice(),
        ),
    ] {
        git(git_repo.path(), ["config", key, value]);
        run_zmin(zmin_repo.path(), ["config", key, value]);
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "legacy type flag output should match for {args:?}",
        );
    }

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["config", "--int", "--bool", "demo.int"]),
        git_failure_output(git_repo.path(), &["config", "--int", "--bool", "demo.int"])
    );
}

#[test]
fn config_type_path_expiry_and_color_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for repo in [git_repo.path(), zmin_repo.path()] {
        std::fs::create_dir_all(repo.join("nested")).expect("create nested path");
    }

    git(
        git_repo.path(),
        ["config", "--type=path", "demo.home", "~/abc"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "--type=path", "demo.home", "~/abc"],
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["config", "--get", "demo.home"]),
        git(git_repo.path(), ["config", "--get", "demo.home"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["config", "--type=path", "demo.home"]),
        git(git_repo.path(), ["config", "--type=path", "demo.home"])
    );

    git(
        git_repo.path(),
        ["config", "--type=path", "demo.relative", "./nested/file"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "--type=path", "demo.relative", "./nested/file"],
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["config", "--type=path", "demo.relative"]),
        git(git_repo.path(), ["config", "--type=path", "demo.relative"])
    );

    for (key, value) in [("demo.never", "never"), ("demo.now", "now")] {
        git(
            git_repo.path(),
            ["config", "--type=expiry-date", key, value],
        );
        run_zmin(
            zmin_repo.path(),
            ["config", "--type=expiry-date", key, value],
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["config", "--type=expiry-date", key]),
            git(git_repo.path(), ["config", "--type=expiry-date", key]),
            "expiry value for {key}"
        );
    }

    git(git_repo.path(), ["config", "demo.badexpiry", "bad"]);
    run_zmin(zmin_repo.path(), ["config", "demo.badexpiry", "bad"]);
    assert_eq!(
        run_zmin_failure_output(
            zmin_repo.path(),
            &["config", "--type=expiry-date", "demo.badexpiry"],
        )
        .0,
        git_failure_output(
            git_repo.path(),
            &["config", "--type=expiry-date", "demo.badexpiry"],
        )
        .0
    );

    for (key, value) in [
        ("demo.red", "red"),
        ("demo.boldred", "red bold"),
        ("demo.rgb", "#010203 #0a0b0c"),
        ("demo.reset", "reset red"),
    ] {
        git(git_repo.path(), ["config", "--type=color", key, value]);
        run_zmin(zmin_repo.path(), ["config", "--type=color", key, value]);
        assert_eq!(
            run_zmin(zmin_repo.path(), ["config", "--type=color", key]).into_bytes(),
            git(git_repo.path(), ["config", "--type=color", key]).into_bytes(),
            "color sequence for {key}"
        );
    }
    assert_eq!(
        run_zmin_failure_output(
            zmin_repo.path(),
            &["config", "--type=color", "demo.bad", "bright blue"],
        ),
        git_failure_output(
            git_repo.path(),
            &["config", "--type=color", "demo.bad", "bright blue"],
        )
    );
    assert_eq!(
        run_zmin_failure_output(
            zmin_repo.path(),
            &["config", "--type=bogus", "demo.bad", "value"],
        ),
        git_failure_output(
            git_repo.path(),
            &["config", "--type=bogus", "demo.bad", "value"],
        )
    );
}

#[test]
fn var_identity_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());

    assert_eq!(
        run_zmin_with_env(repo.path(), ["var", "GIT_AUTHOR_IDENT"]),
        git_with_env(repo.path(), ["var", "GIT_AUTHOR_IDENT"])
    );
    assert_eq!(
        run_zmin_with_env(repo.path(), ["var", "GIT_COMMITTER_IDENT"]),
        git_with_env(repo.path(), ["var", "GIT_COMMITTER_IDENT"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["var", "GIT_DEFAULT_BRANCH"]),
        git(repo.path(), ["var", "GIT_DEFAULT_BRANCH"])
    );
}

#[test]
fn var_list_and_failures_match_stock_git() {
    let home = TempDir::new().expect("home dir");
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
    }
    let envs = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
    ];

    for args in [
        &["var", "-l"][..],
        &["var", "GIT_PAGER"],
        &["var", "GIT_SHELL_PATH"],
        &["var", "GIT_ATTR_SYSTEM"],
        &["var", "GIT_ATTR_GLOBAL"],
        &["var", "GIT_CONFIG_GLOBAL"],
        &["var"],
        &["var", "GIT_UNKNOWN"],
        &["var", "-l", "GIT_AUTHOR_IDENT"],
    ] {
        assert_eq!(
            command_any_with_isolated_config_and_env(
                zmin_bin(),
                zmin_repo.path(),
                home.path(),
                &envs,
                args,
            ),
            command_any_with_isolated_config_and_env(
                "git",
                git_repo.path(),
                home.path(),
                &envs,
                args,
            ),
            "git var mismatch for {args:?}"
        );
    }
}

#[test]
fn shell_pure_helpers_match_git_232_direct_execution() {
    let repo = git_init();

    assert_eq!(run_zmin(repo.path(), ["sh-i18n"]), "");
    assert_eq!(run_zmin(repo.path(), ["sh-i18n", "-h"]), "");
    assert_eq!(run_zmin(repo.path(), ["sh-i18n", "ignored"]), "");
    assert_eq!(run_zmin(repo.path(), ["sh-setup"]), "");
    assert_eq!(
        run_zmin(repo.path(), ["sh-setup", "-h"]),
        "usage: git sh-setup "
    );
    assert_eq!(run_zmin(repo.path(), ["sh-setup", "ignored"]), "");
}

#[test]
fn instaweb_starts_serves_repo_summary_and_stops() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "README.md", "hello\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "instaweb commit"]);
    let port = unused_local_port();

    let start = Command::new(zmin_bin())
        .args([
            "instaweb",
            "--start",
            "--local",
            "--httpd",
            "builtin",
            "--port",
            &port.to_string(),
        ])
        .current_dir(repo.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("start instaweb");
    assert!(start.success(), "start instaweb: {start}");
    wait_for_tcp_port(port);
    let response = http_get_local(port, "/");
    run_zmin(repo.path(), ["instaweb", "--stop"]);

    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("instaweb commit"));
    assert!(response.contains(&git(repo.path(), ["rev-parse", "HEAD"])));
    assert!(!repo.path().join(".git/gitweb/pid").exists());
}

#[cfg(unix)]
#[test]
fn instaweb_starts_external_httpd_command_and_stops_it() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "README.md", "hello\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "instaweb external"]);
    let temp = TempDir::new().expect("temp external httpd");
    let log = temp.path().join("httpd.log");
    let script = temp.path().join("fake-httpd");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf 'port=%s bind=%s git_dir=%s work_tree=%s\\n' \"$GITWEB_PORT\" \"$GITWEB_BIND\" \"$GIT_DIR\" \"$GIT_WORK_TREE\" > '{}'\nsleep 60\n",
            log.display()
        ),
    )
    .expect("write fake httpd");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod fake httpd");
    let port = unused_local_port();

    run_zmin(
        repo.path(),
        [
            "instaweb",
            "--start",
            "--local",
            "--httpd",
            script.to_str().expect("script path"),
            "--port",
            &port.to_string(),
        ],
    );
    for _ in 0..500 {
        if log.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let logged = fs::read_to_string(&log).unwrap_or_else(|error| {
        let pid = fs::read_to_string(repo.path().join(".git/gitweb/pid")).unwrap_or_default();
        panic!(
            "read fake httpd log after waiting for startup: {error}; pid={}",
            pid.trim()
        )
    });
    run_zmin(repo.path(), ["instaweb", "--stop"]);

    assert!(logged.contains(&format!("port={port}")));
    assert!(logged.contains("bind=127.0.0.1"));
    assert!(logged.contains("git_dir="));
    assert!(logged.contains(".git"));
    assert!(logged.contains("work_tree="));
    assert!(!repo.path().join(".git/gitweb/pid").exists());
}

#[test]
fn remote_config_commands_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    git(
        git_repo.path(),
        ["remote", "add", "origin", "https://example.com/repo.git"],
    );
    run_zmin(
        zmin_repo.path(),
        ["remote", "add", "origin", "https://example.com/repo.git"],
    );

    assert_eq!(
        run_zmin(zmin_repo.path(), ["remote"]),
        git(git_repo.path(), ["remote"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["remote", "-v"]),
        git(git_repo.path(), ["remote", "-v"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["remote", "get-url", "origin"]),
        git(git_repo.path(), ["remote", "get-url", "origin"])
    );

    git(git_repo.path(), ["config", "branch.main.remote", "origin"]);
    git(
        git_repo.path(),
        ["config", "branch.main.merge", "refs/heads/main"],
    );
    run_zmin(zmin_repo.path(), ["config", "branch.main.remote", "origin"]);
    run_zmin(
        zmin_repo.path(),
        ["config", "branch.main.merge", "refs/heads/main"],
    );

    git(
        git_repo.path(),
        [
            "remote",
            "set-url",
            "origin",
            "https://example.com/renamed.git",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "remote",
            "set-url",
            "origin",
            "https://example.com/renamed.git",
        ],
    );
    git(git_repo.path(), ["remote", "rename", "origin", "upstream"]);
    run_zmin(zmin_repo.path(), ["remote", "rename", "origin", "upstream"]);

    assert_eq!(
        run_zmin(zmin_repo.path(), ["remote", "-v"]),
        git(git_repo.path(), ["remote", "-v"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["config", "--local", "--list"]),
        git(git_repo.path(), ["config", "--local", "--list"])
    );

    git(git_repo.path(), ["remote", "remove", "upstream"]);
    run_zmin(zmin_repo.path(), ["remote", "remove", "upstream"]);

    assert_eq!(
        run_zmin(zmin_repo.path(), ["remote"]),
        git(git_repo.path(), ["remote"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["config", "--local", "--list"]),
        git(git_repo.path(), ["config", "--local", "--list"])
    );
}

#[test]
fn remote_show_and_prune_match_stock_git_for_local_remote() {
    let fixture = TempDir::new().expect("remote fixture");
    let remote = fixture.path().join("remote");
    let git_clone = fixture.path().join("git-local");
    let zmin_clone = fixture.path().join("zmin-local");

    let output = Command::new(common::stock_git_bin())
        .args(["init", remote.to_str().expect("remote path")])
        .output()
        .expect("git init remote");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    configure_identity(&remote);
    git(&remote, ["config", "commit.gpgsign", "false"]);
    write_file(&remote, "README.md", "remote\n");
    git(&remote, ["add", "-A"]);
    git_with_env(&remote, ["commit", "-m", "remote init"]);
    git(&remote, ["branch", "feature"]);

    for destination in [&git_clone, &zmin_clone] {
        let output = Command::new(common::stock_git_bin())
            .args([
                "clone",
                remote.to_str().expect("remote path"),
                destination.to_str().expect("destination path"),
            ])
            .output()
            .expect("git clone");
        assert!(
            output.status.success(),
            "git clone failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    git(&remote, ["branch", "-D", "feature"]);

    assert_eq!(
        run_zmin(&zmin_clone, ["remote", "show", "-n", "origin"]),
        git(&git_clone, ["remote", "show", "-n", "origin"])
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["remote", "prune", "-n", "origin"]),
        git(&git_clone, ["remote", "prune", "-n", "origin"])
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["remote", "prune", "origin"]),
        git(&git_clone, ["remote", "prune", "origin"])
    );
    assert_eq!(
        git(&zmin_clone, ["branch", "-r"]),
        git(&git_clone, ["branch", "-r"])
    );
    assert_eq!(
        run_zmin_failure_output(&zmin_clone, &["remote", "prune", "missing"]),
        git_failure_output(&git_clone, &["remote", "prune", "missing"])
    );
}

#[test]
fn remote_set_branches_matches_stock_git_config() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    git(
        git_repo.path(),
        ["remote", "add", "origin", "https://example.com/repo.git"],
    );
    run_zmin(
        zmin_repo.path(),
        ["remote", "add", "origin", "https://example.com/repo.git"],
    );

    git(
        git_repo.path(),
        ["remote", "set-branches", "origin", "main", "dev"],
    );
    run_zmin(
        zmin_repo.path(),
        ["remote", "set-branches", "origin", "main", "dev"],
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["config", "--local", "--get-all", "remote.origin.fetch"]
        ),
        git(
            git_repo.path(),
            ["config", "--local", "--get-all", "remote.origin.fetch"]
        )
    );

    git(
        git_repo.path(),
        ["remote", "set-branches", "--add", "origin", "release"],
    );
    run_zmin(
        zmin_repo.path(),
        ["remote", "set-branches", "--add", "origin", "release"],
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["config", "--local", "--get-all", "remote.origin.fetch"]
        ),
        git(
            git_repo.path(),
            ["config", "--local", "--get-all", "remote.origin.fetch"]
        )
    );

    git(git_repo.path(), ["remote", "set-branches", "origin"]);
    run_zmin(zmin_repo.path(), ["remote", "set-branches", "origin"]);
    assert_eq!(
        git(zmin_repo.path(), ["config", "--local", "--list"]),
        git(git_repo.path(), ["config", "--local", "--list"])
    );
    assert_eq!(
        run_zmin_failure_output(
            zmin_repo.path(),
            &["remote", "set-branches", "missing", "main"],
        ),
        git_failure_output(
            git_repo.path(),
            &["remote", "set-branches", "missing", "main"],
        )
    );
}

#[test]
fn remote_set_url_variants_match_stock_git_config() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(
            repo,
            ["remote", "add", "origin", "https://example.com/repo.git"],
        );
    }

    git(
        git_repo.path(),
        ["remote", "set-url", "origin", "https://example.com/new.git"],
    );
    run_zmin(
        zmin_repo.path(),
        ["remote", "set-url", "origin", "https://example.com/new.git"],
    );
    git(
        git_repo.path(),
        [
            "remote",
            "set-url",
            "origin",
            "https://example.com/newer.git",
            "https://example.com/new.git",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "remote",
            "set-url",
            "origin",
            "https://example.com/newer.git",
            "https://example.com/new.git",
        ],
    );
    git(
        git_repo.path(),
        [
            "remote",
            "set-url",
            "--add",
            "origin",
            "https://mirror.example.com/repo.git",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "remote",
            "set-url",
            "--add",
            "origin",
            "https://mirror.example.com/repo.git",
        ],
    );
    git(
        git_repo.path(),
        [
            "remote",
            "set-url",
            "--delete",
            "origin",
            "https://mirror.example.com/repo.git",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "remote",
            "set-url",
            "--delete",
            "origin",
            "https://mirror.example.com/repo.git",
        ],
    );
    git(
        git_repo.path(),
        [
            "remote",
            "set-url",
            "--push",
            "origin",
            "https://push.example.com/repo.git",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "remote",
            "set-url",
            "--push",
            "origin",
            "https://push.example.com/repo.git",
        ],
    );
    git(
        git_repo.path(),
        [
            "remote",
            "set-url",
            "--push",
            "--add",
            "origin",
            "https://push2.example.com/repo.git",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "remote",
            "set-url",
            "--push",
            "--add",
            "origin",
            "https://push2.example.com/repo.git",
        ],
    );
    git(
        git_repo.path(),
        [
            "remote",
            "set-url",
            "--push",
            "--delete",
            "origin",
            "https://push2.example.com/repo.git",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "remote",
            "set-url",
            "--push",
            "--delete",
            "origin",
            "https://push2.example.com/repo.git",
        ],
    );

    assert_eq!(
        git(zmin_repo.path(), ["config", "--local", "--list"]),
        git(git_repo.path(), ["config", "--local", "--list"])
    );
    assert_eq!(
        run_zmin_failure_output(
            zmin_repo.path(),
            &[
                "remote",
                "set-url",
                "missing",
                "https://example.com/repo.git"
            ],
        ),
        git_failure_output(
            git_repo.path(),
            &[
                "remote",
                "set-url",
                "missing",
                "https://example.com/repo.git"
            ],
        )
    );
    assert_eq!(
        run_zmin_failure_output(
            zmin_repo.path(),
            &[
                "remote",
                "set-url",
                "origin",
                "https://nomatch.example.com/repo.git",
                "https://absent.example.com/repo.git",
            ],
        ),
        git_failure_output(
            git_repo.path(),
            &[
                "remote",
                "set-url",
                "origin",
                "https://nomatch.example.com/repo.git",
                "https://absent.example.com/repo.git",
            ],
        )
    );
}

#[test]
fn remote_update_matches_stock_git_for_local_remotes() {
    let fixture = TempDir::new().expect("remote update fixture");
    let remote = fixture.path().join("remote");
    let git_clone = fixture.path().join("git-local");
    let zmin_clone = fixture.path().join("zmin-local");

    let output = Command::new(common::stock_git_bin())
        .args(["init", remote.to_str().expect("remote path")])
        .output()
        .expect("git init remote");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    configure_identity(&remote);
    git(&remote, ["config", "commit.gpgsign", "false"]);
    write_file(&remote, "README.md", "remote\n");
    git(&remote, ["add", "-A"]);
    git_with_env(&remote, ["commit", "-m", "remote init"]);
    git(&remote, ["branch", "feature"]);

    for destination in [&git_clone, &zmin_clone] {
        let output = Command::new(common::stock_git_bin())
            .args([
                "clone",
                remote.to_str().expect("remote path"),
                destination.to_str().expect("destination path"),
            ])
            .output()
            .expect("git clone");
        assert!(
            output.status.success(),
            "git clone failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    git(&remote, ["branch", "-D", "feature"]);

    assert_eq!(
        run_zmin(&zmin_clone, ["remote", "update"]),
        git(&git_clone, ["remote", "update"])
    );
    assert_eq!(
        git(&zmin_clone, ["branch", "-r"]),
        git(&git_clone, ["branch", "-r"])
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["remote", "update", "-p"]),
        git(&git_clone, ["remote", "update", "-p"])
    );
    assert_eq!(
        git(&zmin_clone, ["branch", "-r"]),
        git(&git_clone, ["branch", "-r"])
    );
    assert_eq!(
        run_zmin_failure_output(&zmin_clone, &["remote", "update", "missing"]),
        git_failure_output(&git_clone, &["remote", "update", "missing"])
    );

    for destination in [&git_clone, &zmin_clone] {
        git(
            destination,
            [
                "remote",
                "add",
                "backup",
                remote.to_str().expect("remote path"),
            ],
        );
        git(destination, ["config", "--add", "remotes.team", "origin"]);
        git(destination, ["config", "--add", "remotes.team", "backup"]);
    }
    git(&remote, ["branch", "group-feature"]);

    assert_eq!(
        run_zmin(&zmin_clone, ["remote", "update", "team"]),
        git(&git_clone, ["remote", "update", "team"])
    );
    assert_eq!(
        git(&zmin_clone, ["branch", "-r"]),
        git(&git_clone, ["branch", "-r"])
    );

    for destination in [&git_clone, &zmin_clone] {
        git(destination, ["config", "--add", "remotes.broken", "origin"]);
        git(
            destination,
            ["config", "--add", "remotes.broken", "missing"],
        );
    }
    assert_eq!(
        run_zmin_failure_output(&zmin_clone, &["remote", "update", "broken"]),
        git_failure_output(&git_clone, &["remote", "update", "broken"])
    );

    for destination in [&git_clone, &zmin_clone] {
        git(
            destination,
            ["config", "remote.backup.skipDefaultUpdate", "true"],
        );
    }
    git(&remote, ["branch", "skip-default-feature"]);

    assert_eq!(
        run_zmin(&zmin_clone, ["remote", "update"]),
        git(&git_clone, ["remote", "update"])
    );
    assert_eq!(
        git(&zmin_clone, ["branch", "-r"]),
        git(&git_clone, ["branch", "-r"])
    );

    for destination in [&git_clone, &zmin_clone] {
        git(
            destination,
            ["config", "--unset", "remote.backup.skipDefaultUpdate"],
        );
        git(destination, ["config", "remotes.default", "backup"]);
    }
    git(&remote, ["branch", "default-feature"]);

    assert_eq!(
        run_zmin(&zmin_clone, ["remote", "update"]),
        git(&git_clone, ["remote", "update"])
    );
    assert_eq!(
        git(&zmin_clone, ["branch", "-r"]),
        git(&git_clone, ["branch", "-r"])
    );
}
