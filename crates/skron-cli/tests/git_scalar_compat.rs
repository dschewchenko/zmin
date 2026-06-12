mod common;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

fn run_scalar(home: &TempDir, cwd: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    run_scalar_command(common::skron_bin(), home, cwd, args)
}

fn run_stock_scalar(
    home: &TempDir,
    cwd: &std::path::Path,
    args: &[&str],
) -> Option<(i32, String, String)> {
    run_scalar_command_with_timeout("scalar", home, cwd, args, Duration::from_secs(10))
}

fn assert_scalar_fsmonitor_shape(repo: &std::path::Path) {
    #[cfg(target_os = "linux")]
    assert_eq!(
        common::git_status_args(repo, &["config", "--get", "core.fsmonitor"]),
        1
    );
    #[cfg(not(target_os = "linux"))]
    assert_eq!(
        common::git(repo, ["config", "--get", "core.fsmonitor"]),
        "true"
    );
}

fn run_scalar_command(
    command: &str,
    home: &TempDir,
    cwd: &std::path::Path,
    args: &[&str],
) -> (i32, String, String) {
    run_scalar_command_with_timeout(command, home, cwd, args, Duration::from_secs(30))
        .unwrap_or_else(|| panic!("run {command}: timed out"))
}

fn run_scalar_command_with_timeout(
    command: &str,
    home: &TempDir,
    cwd: &std::path::Path,
    args: &[&str],
    timeout: Duration,
) -> Option<(i32, String, String)> {
    let xdg_config_home = home.path().join(".config");
    let stdout_file = tempfile::NamedTempFile::new().expect("stdout temp file");
    let stderr_file = tempfile::NamedTempFile::new().expect("stderr temp file");
    let mut child = match Command::new(command)
        .args(args)
        .current_dir(cwd)
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdout(Stdio::from(stdout_file.reopen().expect("reopen stdout")))
        .stderr(Stdio::from(stderr_file.reopen().expect("reopen stderr")))
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return None,
    };
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Some(status) = child.try_wait().expect("poll scalar command") {
            return Some((
                status.code().expect("process exit code"),
                String::from_utf8(std::fs::read(stdout_file.path()).expect("read stdout"))
                    .expect("stdout utf8")
                    .trim_end_matches('\n')
                    .to_owned(),
                String::from_utf8(std::fs::read(stderr_file.path()).expect("read stderr"))
                    .expect("stderr utf8")
                    .trim_end_matches('\n')
                    .to_owned(),
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    let _ = child.wait_with_output();
    None
}

fn normalize_scalar_platform_stderr(result: (i32, String, String)) -> (i32, String, String) {
    let (code, stdout, stderr) = result;
    let stderr = stderr
        .lines()
        .filter(|line| {
            !(line.starts_with("fatal: failed to bootstrap service ")
                || *line == "warning: could not toggle maintenance")
        })
        .collect::<Vec<_>>()
        .join("\n");
    (code, stdout, stderr)
}

fn canonical_path(path: &std::path::Path) -> String {
    std::fs::canonicalize(path)
        .expect("canonical path")
        .display()
        .to_string()
}

fn global_gitconfig(home: &TempDir) -> String {
    std::fs::read_to_string(home.path().join(".gitconfig")).expect("global config")
}

fn assert_global_config_contains_repo(home: &TempDir, section: &str, repo: &str) {
    let config = global_gitconfig(home);
    assert!(
        config.contains(&format!("[{section}]")) && config.contains(&format!("\trepo = {repo}")),
        "{config}"
    );
}

fn git_config_get(repo: &std::path::Path, key: &str) -> (i32, String, String) {
    common::command_any_output("git", repo, &["config", "--get", key], "git")
}

fn bare_remote_with_main_branch() -> (TempDir, std::path::PathBuf) {
    let workspace = TempDir::new().expect("remote workspace");
    let source = workspace.path().join("source");
    let remote = workspace.path().join("remote.git");
    std::fs::create_dir(&source).expect("source dir");
    common::git(&source, ["init"]);
    common::configure_identity(&source);
    common::write_file(&source, "a.txt", "hello\n");
    common::git(&source, ["add", "-A"]);
    common::git_with_env(&source, ["commit", "-m", "initial"]);
    common::git(&source, ["branch", "-m", "main"]);
    common::git(&source, ["tag", "v1"]);
    let output = Command::new("git")
        .args(["clone", "--bare"])
        .arg(&source)
        .arg(&remote)
        .output()
        .expect("git clone --bare");
    assert!(
        output.status.success(),
        "git clone --bare failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (workspace, remote)
}

#[test]
fn scalar_register_list_and_unregister_track_global_scalar_repo() {
    let home = TempDir::new().expect("home");
    let repo = common::git_init();

    let (code, stdout, stderr) = run_scalar(
        &home,
        repo.path(),
        &["scalar", "register", "--no-maintenance"],
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "");

    let registered = canonical_path(repo.path());
    let (code, stdout, stderr) = run_scalar(&home, repo.path(), &["scalar", "list"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, registered);

    let config = std::fs::read_to_string(home.path().join(".gitconfig")).expect("global config");
    assert!(config.contains("[scalar]"));
    assert!(config.contains(&format!("repo = {registered}")));

    let (code, stdout, stderr) = run_scalar(&home, repo.path(), &["scalar", "unregister"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, registered);

    let (code, stdout, stderr) = run_scalar(&home, repo.path(), &["scalar", "list"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "");
}

#[test]
fn scalar_register_default_maintenance_writes_git_maintenance_repo() {
    let home = TempDir::new().expect("home");
    let repo = common::git_init();
    let registered = canonical_path(repo.path());

    let (code, stdout, stderr) = run_scalar(&home, repo.path(), &["scalar", "register"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "");

    assert_global_config_contains_repo(&home, "scalar", &registered);
    assert_global_config_contains_repo(&home, "maintenance", &registered);
    assert!(
        global_gitconfig(&home).matches("\trepo = ").count() >= 2,
        "scalar register should write scalar.repo and maintenance.repo"
    );
    assert_eq!(
        common::git(repo.path(), ["config", "--get", "maintenance.auto"]),
        "false"
    );
    assert_eq!(
        common::git(repo.path(), ["config", "--get", "maintenance.strategy"]),
        "incremental"
    );
}

#[test]
fn scalar_root_help_flags_and_version_match_stock_exit_shape() {
    let home = TempDir::new().expect("home");
    let cwd = TempDir::new().expect("cwd");

    for flag in ["-h", "--help"] {
        let (code, stdout, stderr) = run_scalar(&home, cwd.path(), &["scalar", flag]);
        assert_eq!(code, 129);
        assert_eq!(stdout, "");
        assert!(stderr.starts_with("usage: scalar "));
        assert!(stderr.contains("Commands:"));
    }

    let (code, stdout, stderr) = run_scalar(&home, cwd.path(), &["scalar", "version"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "");
    assert!(stderr.starts_with("git version "));
}

#[test]
fn scalar_version_options_match_stock_exit_shape() {
    let home = TempDir::new().expect("home");
    let cwd = TempDir::new().expect("cwd");

    for args in [
        &["scalar", "version", "-v"][..],
        &["scalar", "version", "--no-verbose"][..],
        &["scalar", "version", "--no-build-options"][..],
    ] {
        let (code, stdout, stderr) = run_scalar(&home, cwd.path(), args);
        assert_eq!(code, 0, "{args:?}: {stderr}");
        assert_eq!(stdout, "");
        assert!(stderr.starts_with("git version "), "{args:?}: {stderr}");
        assert!(!stderr.contains("sizeof-size_t"), "{args:?}: {stderr}");
    }

    for args in [
        &["scalar", "version", "--build-options"][..],
        &["scalar", "version", "-v", "--build-options"][..],
        &["scalar", "version", "--no-build-options", "--build-options"][..],
    ] {
        let (code, stdout, stderr) = run_scalar(&home, cwd.path(), args);
        assert_eq!(code, 0, "{args:?}: {stderr}");
        assert_eq!(stdout, "");
        assert!(stderr.starts_with("git version "), "{args:?}: {stderr}");
        assert!(stderr.contains("sizeof-size_t"), "{args:?}: {stderr}");
    }

    let (code, stdout, stderr) = run_scalar(
        &home,
        cwd.path(),
        &["scalar", "version", "--build-options", "--no-build-options"],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "");
    assert!(stderr.starts_with("git version "));
    assert!(!stderr.contains("sizeof-size_t"));

    for flag in ["-h", "--help"] {
        let (code, stdout, stderr) = run_scalar(&home, cwd.path(), &["scalar", "version", flag]);
        assert_eq!(code, 129);
        assert_eq!(stderr, "");
        assert!(stdout.starts_with("usage: scalar verbose "));
    }
}

#[test]
fn scalar_help_prints_manual_style_command_reference() {
    let home = TempDir::new().expect("home");
    let cwd = TempDir::new().expect("cwd");

    let (code, stdout, stderr) = run_scalar(&home, cwd.path(), &["scalar", "help"]);
    assert_eq!(code, 0);
    assert_eq!(stderr, "");
    assert!(stdout.starts_with("SCALAR(1)"));
    assert!(stdout.contains("NAME"));
    assert!(stdout.contains("SYNOPSIS"));
    assert!(stdout.contains("DESCRIPTION"));
    assert!(stdout.contains("COMMANDS"));
    assert!(stdout.contains("scalar - A tool for managing large Git repositories"));
    assert!(stdout.contains("scalar clone [--single-branch]"));
    assert!(stdout.contains("scalar diagnose [<enlistment>]"));
    assert!(stdout.contains("delete <enlistment>"));
    assert!(stdout.contains("--[no-]maintenance"));
    assert!(
        stdout.len() > 4_000,
        "help should be manual-style, not short usage"
    );
}

#[test]
fn scalar_subcommand_help_flags_match_stock_exit_shape() {
    let home = TempDir::new().expect("home");
    let cwd = TempDir::new().expect("cwd");

    for (subcommand, expected) in [
        ("clone", "usage: scalar clone"),
        ("register", "usage: scalar register"),
        ("unregister", "usage: scalar unregister"),
        ("reconfigure", "usage: scalar reconfigure"),
        ("diagnose", "usage: scalar diagnose"),
        ("delete", "usage: scalar delete"),
    ] {
        let (code, stdout, stderr) = run_scalar(&home, cwd.path(), &["scalar", subcommand, "-h"]);
        assert_eq!(code, 129, "{subcommand}: {stderr}");
        assert!(stderr.is_empty(), "{subcommand}: {stderr}");
        assert!(
            stdout.starts_with(expected),
            "{subcommand}: expected {expected:?}, got {stdout:?}"
        );
    }

    let (code, stdout, stderr) = run_scalar(&home, cwd.path(), &["scalar", "list", "-h"]);
    assert_eq!(code, 128);
    assert_eq!(stdout, "");
    assert!(stderr.contains("fatal: `scalar list` does not take arguments"));
}

#[test]
fn scalar_run_help_lists_stock_task_block() {
    let home = TempDir::new().expect("home");
    let cwd = TempDir::new().expect("cwd");

    let (code, stdout, stderr) = run_scalar(&home, cwd.path(), &["scalar", "run", "-h"]);
    assert_eq!(code, 129);
    assert_eq!(stderr, "");
    assert_eq!(
        stdout,
        concat!(
            "usage: scalar run <task> [<enlistment>]\n",
            "       Tasks:\n",
            "       \tconfig\n",
            "       \tcommit-graph\n",
            "       \tfetch\n",
            "       \tloose-objects\n",
            "       \tpack-files\n",
            "       ",
        )
    );
}

#[test]
fn scalar_unknown_command_and_reconfigure_all_conflict_match_stock_shape() {
    let home = TempDir::new().expect("home");
    let repo = common::git_init();

    let (code, stdout, stderr) = run_scalar(&home, repo.path(), &["scalar", "unknown"]);
    assert_eq!(code, 129);
    assert_eq!(stdout, "");
    assert!(stderr.starts_with("usage: scalar "));

    let path = canonical_path(repo.path());
    let (code, stdout, stderr) = run_scalar(
        &home,
        repo.path(),
        &["scalar", "reconfigure", "--all", &path],
    );
    assert_eq!(code, 129);
    assert_eq!(stdout, "");
    assert!(stderr.contains("fatal: --all or <enlistment>, but not both"));
    assert!(stderr.contains("usage: scalar reconfigure"));
}

#[test]
fn scalar_leading_c_and_config_errors_match_stock_shape() {
    let home = TempDir::new().expect("home");
    let cwd = TempDir::new().expect("cwd");

    let (code, stdout, stderr) = run_scalar(&home, cwd.path(), &["scalar", "-C"]);
    assert_eq!(code, 128);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "fatal: -C requires a <directory>");

    let missing = cwd.path().join("missing");
    let missing_arg = missing.to_str().expect("missing path");
    let (code, stdout, stderr) =
        run_scalar(&home, cwd.path(), &["scalar", "-C", missing_arg, "list"]);
    assert_eq!(code, 128);
    assert_eq!(stdout, "");
    assert!(
        stderr.starts_with(&format!("fatal: could not change to '{missing_arg}'")),
        "{stderr}"
    );

    let (code, stdout, stderr) = run_scalar(&home, cwd.path(), &["scalar", "-c", "foo", "list"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "");
    assert!(stderr.contains("error: key does not contain a section: foo"));
    assert!(stderr.contains("fatal: unable to parse command-line config"));
}

#[test]
fn scalar_non_repo_and_missing_enlistment_errors_match_stock_shape() {
    let home = TempDir::new().expect("home");
    let plain = TempDir::new().expect("plain");

    for args in [
        &["scalar", "register"][..],
        &["scalar", "unregister"][..],
        &["scalar", "run", "config"][..],
        &["scalar", "diagnose"][..],
        &["scalar", "reconfigure"][..],
        &[
            "scalar",
            "register",
            plain.path().to_str().expect("plain path"),
        ][..],
    ] {
        let (code, stdout, stderr) = run_scalar(&home, plain.path(), args);
        assert_eq!(code, 128, "{args:?}: {stderr}");
        assert_eq!(stdout, "", "{args:?}");
        assert_eq!(
            stderr, "fatal: not a git repository (or any of the parent directories): .git",
            "{args:?}"
        );
    }

    let missing = plain.path().join("missing");
    let missing_arg = missing.to_str().expect("missing path");
    let (code, stdout, stderr) =
        run_scalar(&home, plain.path(), &["scalar", "delete", missing_arg]);
    assert_eq!(code, 128);
    assert_eq!(stdout, "");
    assert!(stderr.starts_with("fatal: '"), "{stderr}");
    assert!(stderr.ends_with("' does not exist"), "{stderr}");
}

#[test]
fn scalar_missing_and_extra_arguments_match_stock_usage_shape() {
    let home = TempDir::new().expect("home");
    let repo = common::git_init();

    for args in [
        &["scalar", "delete"][..],
        &["scalar", "run"][..],
        &["scalar", "run", "config", "extra1", "extra2"][..],
        &["scalar", "diagnose", "extra1", "extra2"][..],
        &["scalar", "unregister", "extra1", "extra2"][..],
        &["scalar", "help", "extra"][..],
        &["scalar", "version", "extra"][..],
    ] {
        let (code, stdout, stderr) = run_scalar(&home, repo.path(), args);
        assert_eq!(code, 129, "{args:?}: {stdout}{stderr}");
        assert_eq!(stdout, "", "{args:?}");
        assert!(stderr.starts_with("usage: scalar "), "{args:?}: {stderr}");
    }
}

#[test]
fn scalar_conflicting_and_incomplete_options_match_stock_usage_shape() {
    let repo = common::git_init();
    let repo_path = canonical_path(repo.path());

    for args in [
        vec!["list", "extra"],
        vec!["list", "-h", "extra"],
        vec!["register", "extra1", "extra2"],
        vec!["register", "--maintenance", "--no-maintenance", &repo_path],
        vec!["register", "--no-maintenance", "--maintenance", &repo_path],
        vec!["register", "--maintenance=bad", &repo_path],
        vec!["register", "--maintenance=false", &repo_path],
        vec!["unregister", "--maintenance", &repo_path],
        vec!["run", "--foo"],
        vec!["run", "config", "--foo"],
        vec!["clone", "--full-clone=bad", &repo_path, "dst"],
        vec!["clone", "--no-full-clone=bad", &repo_path, "dst"],
        vec!["clone", "--single-branch=bad", &repo_path, "dst"],
        vec!["clone", "--no-single-branch=bad", &repo_path, "dst"],
        vec!["clone", "--no-branch=bad", &repo_path, "dst"],
        vec!["clone", "--src=bad", &repo_path, "dst"],
        vec!["clone", "--no-src=bad", &repo_path, "dst"],
        vec!["clone", "--tags=bad", &repo_path, "dst"],
        vec!["clone", "--no-tags=bad", &repo_path, "dst"],
        vec!["clone", "--maintenance=bad", &repo_path, "dst"],
        vec!["clone", "--no-maintenance=bad", &repo_path, "dst"],
        vec!["clone", "--branch", &repo_path],
        vec!["clone", "--branch=", &repo_path, "dst"],
        vec!["clone", "--branch="],
        vec!["reconfigure", "--no-all"],
        vec!["reconfigure", "--all", "--no-all", &repo_path],
        vec!["reconfigure", "--all=false"],
        vec!["reconfigure", "--no-all=false"],
        vec!["reconfigure", "--maintenance"],
        vec!["reconfigure", "--maintenance=bad", &repo_path],
        vec!["reconfigure", "--all", "--maintenance=bad"],
        vec![
            "reconfigure",
            "--maintenance=enable",
            "--maintenance=disable",
            &repo_path,
        ],
        vec!["diagnose", "--mode", "all", &repo_path],
        vec!["diagnose", "--mode=all", &repo_path],
        vec!["delete", "--force", &repo_path],
        vec!["version", "-v", "--unknown"],
    ] {
        let stock_home = TempDir::new().expect("stock home");
        let skron_home = TempDir::new().expect("skron home");
        let stock_parent = TempDir::new().expect("stock parent");
        let skron_parent = TempDir::new().expect("skron parent");
        let stock =
            run_stock_scalar(&stock_home, stock_parent.path(), &args).expect("stock scalar");
        let mut skron_args = vec!["scalar"];
        skron_args.extend(args.iter().copied());
        let skron = run_scalar(&skron_home, skron_parent.path(), &skron_args);
        assert_eq!(skron, stock, "{args:?}");
    }
}

#[test]
fn scalar_register_uses_parent_enlistment_for_src_worktree() {
    let home = TempDir::new().expect("home");
    let enlistment = TempDir::new().expect("enlistment");
    let src = enlistment.path().join("src");
    std::fs::create_dir(&src).expect("src dir");
    common::git(&src, ["init"]);

    let (code, stdout, stderr) =
        run_scalar(&home, &src, &["scalar", "register", "--no-maintenance"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "");

    let registered = canonical_path(enlistment.path());
    let (code, stdout, stderr) = run_scalar(&home, &src, &["scalar", "list"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, registered);
}

#[test]
fn scalar_clone_default_src_registers_src_and_configures_partial_clone() {
    let home = TempDir::new().expect("home");
    let (_remote_workspace, remote) = bare_remote_with_main_branch();
    let parent = TempDir::new().expect("parent");
    let enlistment = parent.path().join("enlistment");
    let remote_arg = remote.to_str().expect("remote path");
    let enlistment_arg = enlistment.to_str().expect("enlistment path");

    let (code, _stdout, stderr) = run_scalar(
        &home,
        parent.path(),
        &[
            "scalar",
            "clone",
            "--no-maintenance",
            remote_arg,
            enlistment_arg,
        ],
    );
    assert_eq!(code, 0, "{stderr}");

    let repo = enlistment.join("src");
    assert_eq!(
        std::fs::read_to_string(repo.join("a.txt")).expect("cloned file"),
        "hello\n"
    );
    let registered = canonical_path(&repo);
    let (code, stdout, stderr) = run_scalar(&home, parent.path(), &["scalar", "list"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, registered);
    assert_eq!(
        common::git(&repo, ["config", "--get", "remote.origin.promisor"]),
        "true"
    );
    assert_eq!(
        common::git(
            &repo,
            ["config", "--get", "remote.origin.partialCloneFilter"]
        ),
        "blob:none"
    );
    assert_scalar_fsmonitor_shape(&repo);
}

#[test]
fn scalar_clone_default_maintenance_registers_git_maintenance_repo() {
    let home = TempDir::new().expect("home");
    let (_remote_workspace, remote) = bare_remote_with_main_branch();
    let parent = TempDir::new().expect("parent");
    let enlistment = parent.path().join("enlistment");
    let remote_arg = remote.to_str().expect("remote path");
    let enlistment_arg = enlistment.to_str().expect("enlistment path");

    let (code, _stdout, stderr) = run_scalar(
        &home,
        parent.path(),
        &["scalar", "clone", remote_arg, enlistment_arg],
    );
    assert_eq!(code, 0, "{stderr}");

    let repo = enlistment.join("src");
    let registered = canonical_path(&repo);
    assert_global_config_contains_repo(&home, "scalar", &registered);
    assert_global_config_contains_repo(&home, "maintenance", &registered);
    assert!(
        global_gitconfig(&home).matches("\trepo = ").count() >= 2,
        "scalar clone should write scalar.repo and maintenance.repo"
    );
    assert_eq!(
        common::git(&repo, ["config", "--get", "maintenance.auto"]),
        "false"
    );
    assert_eq!(
        common::git(&repo, ["config", "--get", "maintenance.strategy"]),
        "incremental"
    );
}

#[test]
fn scalar_clone_no_src_registers_enlistment_root() {
    let home = TempDir::new().expect("home");
    let (_remote_workspace, remote) = bare_remote_with_main_branch();
    let parent = TempDir::new().expect("parent");
    let enlistment = parent.path().join("root");
    let remote_arg = remote.to_str().expect("remote path");
    let enlistment_arg = enlistment.to_str().expect("enlistment path");

    let (code, _stdout, stderr) = run_scalar(
        &home,
        parent.path(),
        &[
            "scalar",
            "clone",
            "--no-maintenance",
            "--no-src",
            remote_arg,
            enlistment_arg,
        ],
    );
    assert_eq!(code, 0, "{stderr}");

    assert_eq!(
        std::fs::read_to_string(enlistment.join("a.txt")).expect("cloned file"),
        "hello\n"
    );
    let registered = canonical_path(&enlistment);
    let (code, stdout, stderr) = run_scalar(&home, parent.path(), &["scalar", "list"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, registered);
}

#[test]
fn scalar_clone_honors_full_clone_tags_and_default_enlistment_name() {
    let home = TempDir::new().expect("home");
    let (_remote_workspace, remote) = bare_remote_with_main_branch();
    let parent = TempDir::new().expect("parent");
    let remote_arg = remote.to_str().expect("remote path");

    let (code, _stdout, stderr) = run_scalar(
        &home,
        parent.path(),
        &[
            "scalar",
            "clone",
            "--no-maintenance",
            "--full-clone",
            remote_arg,
        ],
    );
    assert_eq!(code, 0, "{stderr}");
    let default_repo = parent.path().join("remote/src");
    assert_eq!(
        std::fs::read_to_string(default_repo.join("a.txt")).expect("default cloned file"),
        "hello\n"
    );
    assert_eq!(
        common::git_status_args(
            &default_repo,
            &["config", "--get", "remote.origin.partialCloneFilter"]
        ),
        1,
        "--full-clone should not configure a partial clone filter"
    );
    assert_eq!(
        common::git_status_args(&default_repo, &["show-ref", "--verify", "refs/tags/v1"]),
        0,
        "default scalar clone should copy tags"
    );

    let no_tags = parent.path().join("no-tags");
    let (code, _stdout, stderr) = run_scalar(
        &home,
        parent.path(),
        &[
            "scalar",
            "clone",
            "--no-maintenance",
            "--no-tags",
            remote_arg,
            no_tags.to_str().expect("no-tags path"),
        ],
    );
    assert_eq!(code, 0, "{stderr}");
    let no_tags_repo = no_tags.join("src");
    assert_eq!(
        common::git(&no_tags_repo, ["tag", "--list", "v1"]),
        "",
        "--no-tags should not copy tags"
    );
    assert_eq!(
        common::git(&no_tags_repo, ["config", "--get", "remote.origin.tagOpt"]),
        "--no-tags"
    );

    let tags = parent.path().join("tags");
    let (code, _stdout, stderr) = run_scalar(
        &home,
        parent.path(),
        &[
            "scalar",
            "clone",
            "--no-maintenance",
            "--tags",
            remote_arg,
            tags.to_str().expect("tags path"),
        ],
    );
    assert_eq!(code, 0, "{stderr}");
    let tags_repo = tags.join("src");
    assert_eq!(
        common::git_status_args(&tags_repo, &["show-ref", "--verify", "refs/tags/v1"]),
        0,
        "--tags should copy tags"
    );
    assert_eq!(
        common::git_status_args(&tags_repo, &["config", "--get", "remote.origin.tagOpt"]),
        1,
        "--tags should not leave --no-tags tagOpt"
    );

    let no_full_override = parent.path().join("no-full-override");
    let (code, _stdout, stderr) = run_scalar(
        &home,
        parent.path(),
        &[
            "scalar",
            "clone",
            "--no-maintenance",
            "--no-full-clone",
            "--full-clone",
            remote_arg,
            no_full_override.to_str().expect("no-full-override path"),
        ],
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        common::git_status_args(
            &no_full_override.join("src"),
            &["config", "--get", "remote.origin.partialCloneFilter"]
        ),
        1,
        "--full-clone should win when it appears after --no-full-clone"
    );

    let no_branch_override = parent.path().join("no-branch-override");
    let (code, _stdout, stderr) = run_scalar(
        &home,
        parent.path(),
        &[
            "scalar",
            "clone",
            "--no-maintenance",
            "--branch",
            "missing",
            "--no-branch",
            remote_arg,
            no_branch_override
                .to_str()
                .expect("no-branch-override path"),
        ],
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        std::fs::read_to_string(no_branch_override.join("src/a.txt"))
            .expect("no-branch cloned file"),
        "hello\n",
        "--no-branch should clear an earlier --branch selection"
    );
}

#[test]
fn scalar_clone_missing_requested_branch_keeps_partial_checkout_state() {
    let home = TempDir::new().expect("home");
    let (_remote_workspace, remote) = bare_remote_with_main_branch();
    let parent = TempDir::new().expect("parent");
    let enlistment = parent.path().join("missing-branch");
    let remote_arg = remote.to_str().expect("remote path");
    let enlistment_arg = enlistment.to_str().expect("enlistment path");

    let (code, stdout, stderr) = run_scalar(
        &home,
        parent.path(),
        &[
            "scalar",
            "clone",
            "--no-maintenance",
            "--branch",
            "missing",
            remote_arg,
            enlistment_arg,
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("fatal: 'origin/missing' is not a commit and a branch 'missing' cannot be created from it"),
        "{stderr}"
    );

    let repo = enlistment.join("src");
    assert!(
        repo.join(".git").exists(),
        "failed scalar clone should leave initialized src repo"
    );
    assert_eq!(common::git(&repo, ["branch", "--show-current"]), "missing");
    assert_eq!(
        common::git_status_args(&repo, &["show-ref", "--verify", "refs/remotes/origin/main"]),
        0
    );
    assert_eq!(
        common::git(&repo, ["config", "--get", "branch.missing.remote"]),
        "origin"
    );
    assert_eq!(
        common::git(&repo, ["config", "--get", "branch.missing.merge"]),
        "refs/heads/missing"
    );
    assert_eq!(
        common::git(
            &repo,
            ["config", "--get", "remote.origin.partialCloneFilter"]
        ),
        "blob:none"
    );
    assert_eq!(
        run_scalar(&home, parent.path(), &["scalar", "list"]).1,
        "",
        "failed scalar clone should not register scalar.repo"
    );
}

#[test]
fn scalar_clone_existing_enlistment_destination_matches_stock_failure_shape() {
    let home = TempDir::new().expect("home");
    let (_remote_workspace, remote) = bare_remote_with_main_branch();
    let parent = TempDir::new().expect("parent");
    let existing = parent.path().join("existing");
    std::fs::create_dir(&existing).expect("existing dir");
    let remote_arg = remote.to_str().expect("remote path");

    for args in [
        vec![
            "scalar",
            "clone",
            "--no-maintenance",
            remote_arg,
            "existing",
        ],
        vec![
            "scalar",
            "clone",
            "--no-maintenance",
            "--no-src",
            remote_arg,
            "existing",
        ],
    ] {
        let (code, stdout, stderr) = run_scalar(&home, parent.path(), &args);
        assert_eq!(code, 128, "{args:?}: {stderr}");
        assert_eq!(stdout, "");
        assert_eq!(stderr, "fatal: directory 'existing' exists already");
    }

    let file = parent.path().join("file.txt");
    std::fs::write(&file, "data").expect("file destination");
    let (code, stdout, stderr) = run_scalar(
        &home,
        parent.path(),
        &[
            "scalar",
            "clone",
            "--no-maintenance",
            remote_arg,
            "file.txt",
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "fatal: cannot mkdir file.txt/src: File exists");

    let (code, stdout, stderr) = run_scalar(
        &home,
        parent.path(),
        &[
            "scalar",
            "clone",
            "--no-maintenance",
            "--no-src",
            remote_arg,
            "file.txt",
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "fatal: cannot mkdir file.txt: File exists");
}

#[test]
fn scalar_clone_ssh_transport_failure_does_not_register_empty_repo() {
    let home = TempDir::new().expect("home");
    let parent = TempDir::new().expect("parent");
    let fake_ssh = parent.path().join("fake-ssh.sh");
    std::fs::write(
        &fake_ssh,
        "#!/bin/sh\nprintf 'fatal: fake ssh failed\\n' >&2\nexit 255\n",
    )
    .expect("fake ssh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&fake_ssh)
            .expect("fake ssh metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake_ssh, perms).expect("fake ssh chmod");
    }

    let xdg_config_home = home.path().join(".config");
    let output = Command::new(common::skron_bin())
        .args([
            "scalar",
            "clone",
            "--no-maintenance",
            "ssh://example.invalid/org/repo.git",
            "repo",
        ])
        .current_dir(parent.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_SSH_COMMAND", fake_ssh.to_str().expect("fake ssh path"))
        .output()
        .expect("run scalar clone ssh failure");
    assert_eq!(output.status.code(), Some(255));
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert_eq!(stdout.trim_end_matches('\n'), "");
    assert!(stderr.contains("fatal: fake ssh failed"), "{stderr}");
    assert!(
        !stderr.contains("warning: You appear to have cloned an empty repository."),
        "{stderr}"
    );
    assert!(
        !home.path().join(".gitconfig").exists(),
        "failed ssh clone should not register scalar.repo"
    );
}

#[test]
fn scalar_run_config_writes_scalar_repository_defaults() {
    let home = TempDir::new().expect("home");
    let repo = common::git_init();

    let (code, stdout, stderr) = run_scalar(&home, repo.path(), &["scalar", "run", "config"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "");

    assert_scalar_fsmonitor_shape(repo.path());
    assert_eq!(
        common::git(repo.path(), ["config", "--get", "fetch.unpackLimit"]),
        "1"
    );
    assert_eq!(
        common::git(repo.path(), ["config", "--get", "index.version"]),
        "4"
    );
    assert_eq!(
        common::git(repo.path(), ["config", "--get", "maintenance.strategy"]),
        "incremental"
    );
    assert_eq!(
        common::git(repo.path(), ["config", "--get", "maintenance.auto"]),
        "false"
    );
}

#[test]
fn scalar_run_unknown_task_matches_scalar_usage_failure_shape() {
    let home = TempDir::new().expect("home");
    let repo = common::git_init();

    let (code, stdout, stderr) = run_scalar(&home, repo.path(), &["scalar", "run", "unknown"]);
    assert_eq!(code, 129);
    assert_eq!(stdout, "");
    assert!(stderr.contains("error: no such task: 'unknown'"));
    assert!(stderr.contains("usage: scalar run <task> [<enlistment>]"));
}

#[test]
fn scalar_run_maintenance_tasks_match_stock_empty_repo_shape() {
    for task in [
        "commit-graph",
        "fetch",
        "loose-objects",
        "pack-files",
        "all",
    ] {
        let stock_home = TempDir::new().expect("stock home");
        let skron_home = TempDir::new().expect("skron home");
        let stock_repo = common::git_init();
        let skron_repo = common::git_init();
        let Some(stock) = run_stock_scalar(&stock_home, stock_repo.path(), &["run", task]) else {
            continue;
        };
        assert_eq!(
            run_scalar(&skron_home, skron_repo.path(), &["scalar", "run", task]),
            stock,
            "scalar run {task}"
        );
        if task == "all" {
            assert_eq!(
                common::git(
                    skron_repo.path(),
                    ["config", "--get", "maintenance.strategy"]
                ),
                "incremental",
                "scalar run all should apply Scalar maintenance config before running tasks"
            );
        }
    }
}

#[test]
fn scalar_diagnose_writes_zip_inside_scalar_diagnostics_directory() {
    let home = TempDir::new().expect("home");
    let repo = common::git_init();

    let (code, stdout, stderr) = run_scalar(&home, repo.path(), &["scalar", "diagnose"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("Collecting diagnostic info"));
    assert!(stderr.contains("Diagnostics complete."));

    let diagnostics_dir = repo.path().join(".scalarDiagnostics");
    let archives = std::fs::read_dir(&diagnostics_dir)
        .expect("diagnostics dir")
        .map(|entry| entry.expect("diagnostics entry").path())
        .collect::<Vec<_>>();
    assert_eq!(archives.len(), 1);
    assert_eq!(
        archives[0].extension().and_then(|value| value.to_str()),
        Some("zip")
    );
}

#[test]
fn scalar_diagnose_matches_stock_scalar_output_location() {
    let stock_home = TempDir::new().expect("stock home");
    let skron_home = TempDir::new().expect("skron home");
    let stock_repo = common::git_init();
    let skron_repo = common::git_init();

    let Some(stock_diagnose) = run_stock_scalar(&stock_home, stock_repo.path(), &["diagnose"])
    else {
        return;
    };
    let skron_diagnose = run_scalar(&skron_home, skron_repo.path(), &["scalar", "diagnose"]);
    assert_eq!(skron_diagnose.0, stock_diagnose.0);
    assert!(stock_diagnose.1.contains("Collecting diagnostic info"));
    assert!(skron_diagnose.1.contains("Collecting diagnostic info"));
    assert!(stock_diagnose.2.contains("Diagnostics complete."));
    assert!(skron_diagnose.2.contains("Diagnostics complete."));

    let stock_archives = scalar_diagnostic_archives(stock_repo.path());
    let skron_archives = scalar_diagnostic_archives(skron_repo.path());
    assert_eq!(stock_archives.len(), 1);
    assert_eq!(skron_archives.len(), 1);
    assert!(
        stock_archives[0]
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("git-diagnostics-") && name.ends_with(".zip"))
    );
    assert!(
        skron_archives[0]
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("git-diagnostics-") && name.ends_with(".zip"))
    );
    let bytes = std::fs::read(&skron_archives[0]).expect("read skron diagnostics archive");
    assert!(bytes.starts_with(b"PK\x03\x04"));
}

fn scalar_diagnostic_archives(repo: &std::path::Path) -> Vec<std::path::PathBuf> {
    let diagnostics_dir = repo.join(".scalarDiagnostics");
    let mut archives = std::fs::read_dir(&diagnostics_dir)
        .expect("diagnostics dir")
        .map(|entry| entry.expect("diagnostics entry").path())
        .collect::<Vec<_>>();
    archives.sort();
    archives
}

#[test]
fn scalar_delete_unregisters_and_removes_enlistment() {
    let home = TempDir::new().expect("home");
    let parent = TempDir::new().expect("parent");
    let repo_path = parent.path().join("repo");
    std::fs::create_dir(&repo_path).expect("repo dir");
    common::git(&repo_path, ["init"]);
    let registered = canonical_path(&repo_path);

    let (code, stdout, stderr) = run_scalar(
        &home,
        parent.path(),
        &["scalar", "register", "--no-maintenance", &registered],
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "");

    let (code, stdout, stderr) =
        run_scalar(&home, parent.path(), &["scalar", "delete", &registered]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, registered);
    assert!(
        !repo_path.exists(),
        "scalar delete should remove enlistment"
    );

    let (code, stdout, stderr) = run_scalar(&home, parent.path(), &["scalar", "list"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "");
}

#[test]
fn scalar_delete_matches_stock_scalar_shape() {
    let stock_home = TempDir::new().expect("stock home");
    let skron_home = TempDir::new().expect("skron home");
    let stock_parent = TempDir::new().expect("stock parent");
    let skron_parent = TempDir::new().expect("skron parent");
    let stock_repo = stock_parent.path().join("repo");
    let skron_repo = skron_parent.path().join("repo");
    std::fs::create_dir(&stock_repo).expect("stock repo dir");
    std::fs::create_dir(&skron_repo).expect("skron repo dir");
    common::git(&stock_repo, ["init"]);
    common::git(&skron_repo, ["init"]);
    let stock_path = canonical_path(&stock_repo);
    let skron_path = canonical_path(&skron_repo);

    let Some(stock_register) = run_stock_scalar(
        &stock_home,
        stock_parent.path(),
        &["register", "--no-maintenance", &stock_path],
    ) else {
        return;
    };
    let skron_register = run_scalar(
        &skron_home,
        skron_parent.path(),
        &["scalar", "register", "--no-maintenance", &skron_path],
    );
    assert_eq!(
        (
            skron_register.0,
            skron_register.1.as_str(),
            skron_register.2.as_str()
        ),
        (
            stock_register.0,
            stock_register.1.as_str(),
            stock_register.2.as_str()
        )
    );

    let stock_delete = run_stock_scalar(&stock_home, stock_parent.path(), &["delete", &stock_path])
        .expect("stock scalar");
    let skron_delete = run_scalar(
        &skron_home,
        skron_parent.path(),
        &["scalar", "delete", &skron_path],
    );
    assert_eq!(skron_delete.0, stock_delete.0);
    assert_eq!(skron_delete.1, skron_path);
    assert_eq!(stock_delete.1, stock_path);
    assert_eq!(skron_delete.2, stock_delete.2);
    assert!(!stock_repo.exists());
    assert!(!skron_repo.exists());
}

#[test]
fn scalar_register_list_unregister_match_stock_scalar_shape() {
    let stock_home = TempDir::new().expect("stock home");
    let skron_home = TempDir::new().expect("skron home");
    let repo = common::git_init();
    let registered = canonical_path(repo.path());

    let Some(stock_register) = run_stock_scalar(
        &stock_home,
        repo.path(),
        &["register", "--no-maintenance", &registered],
    ) else {
        return;
    };
    let skron_register = run_scalar(
        &skron_home,
        repo.path(),
        &["scalar", "register", "--no-maintenance", &registered],
    );
    assert_eq!(skron_register, stock_register);

    let stock_list = run_stock_scalar(&stock_home, repo.path(), &["list"]).expect("stock scalar");
    let skron_list = run_scalar(&skron_home, repo.path(), &["scalar", "list"]);
    assert_eq!(skron_list, stock_list);
    assert_eq!(skron_list.1, registered);

    let stock_unregister = run_stock_scalar(&stock_home, repo.path(), &["unregister", &registered])
        .expect("stock scalar");
    let skron_unregister = run_scalar(
        &skron_home,
        repo.path(),
        &["scalar", "unregister", &registered],
    );
    assert_eq!(skron_unregister, stock_unregister);
}

#[test]
fn scalar_repeated_register_and_unregister_match_stock_shape() {
    let stock_home = TempDir::new().expect("stock home");
    let skron_home = TempDir::new().expect("skron home");
    let stock_repo = common::git_init();
    let skron_repo = common::git_init();
    let stock_path = canonical_path(stock_repo.path());
    let skron_path = canonical_path(skron_repo.path());

    let Some(stock_first_register) = run_stock_scalar(
        &stock_home,
        stock_repo.path(),
        &["register", "--no-maintenance", &stock_path],
    ) else {
        return;
    };
    let skron_first_register = run_scalar(
        &skron_home,
        skron_repo.path(),
        &["scalar", "register", "--no-maintenance", &skron_path],
    );
    assert_eq!(skron_first_register.0, stock_first_register.0);
    assert_eq!(skron_first_register.1, stock_first_register.1);
    assert_eq!(skron_first_register.2, stock_first_register.2);

    let stock_second_register = run_stock_scalar(
        &stock_home,
        stock_repo.path(),
        &["register", "--no-maintenance", &stock_path],
    )
    .expect("stock scalar");
    let skron_second_register = run_scalar(
        &skron_home,
        skron_repo.path(),
        &["scalar", "register", "--no-maintenance", &skron_path],
    );
    assert_eq!(skron_second_register.0, stock_second_register.0);
    assert_eq!(skron_second_register.1, skron_path);
    assert_eq!(stock_second_register.1, stock_path);
    assert_eq!(skron_second_register.2, stock_second_register.2);

    let stock_first_unregister =
        run_stock_scalar(&stock_home, stock_repo.path(), &["unregister", &stock_path])
            .expect("stock scalar");
    let skron_first_unregister = run_scalar(
        &skron_home,
        skron_repo.path(),
        &["scalar", "unregister", &skron_path],
    );
    assert_eq!(skron_first_unregister.0, stock_first_unregister.0);
    assert_eq!(skron_first_unregister.1, skron_path);
    assert_eq!(stock_first_unregister.1, stock_path);
    assert_eq!(skron_first_unregister.2, stock_first_unregister.2);

    let stock_second_unregister =
        run_stock_scalar(&stock_home, stock_repo.path(), &["unregister", &stock_path])
            .expect("stock scalar");
    let skron_second_unregister = run_scalar(
        &skron_home,
        skron_repo.path(),
        &["scalar", "unregister", &skron_path],
    );
    assert_eq!(skron_second_unregister, stock_second_unregister);
}

#[test]
fn scalar_run_config_and_reconfigure_keep_match_stock_scalar_config_effects() {
    let stock_home = TempDir::new().expect("stock home");
    let skron_home = TempDir::new().expect("skron home");
    let stock_repo = common::git_init();
    let skron_repo = common::git_init();
    let stock_path = canonical_path(stock_repo.path());
    let skron_path = canonical_path(skron_repo.path());

    let Some(stock_run) = run_stock_scalar(
        &stock_home,
        stock_repo.path(),
        &["run", "config", &stock_path],
    ) else {
        return;
    };
    let skron_run = run_scalar(
        &skron_home,
        skron_repo.path(),
        &["scalar", "run", "config", &skron_path],
    );
    assert_eq!(
        normalize_scalar_platform_stderr(skron_run),
        normalize_scalar_platform_stderr(stock_run)
    );
    assert_eq!(
        common::git_status_args(skron_repo.path(), &["config", "--get", "core.fsmonitor"]),
        common::git_status_args(stock_repo.path(), &["config", "--get", "core.fsmonitor"]),
        "config key core.fsmonitor"
    );
    for key in [
        "fetch.unpackLimit",
        "index.version",
        "maintenance.auto",
        "maintenance.strategy",
    ] {
        assert_eq!(
            git_config_get(skron_repo.path(), key),
            git_config_get(stock_repo.path(), key),
            "config key {key}"
        );
    }

    let stock_reconfigure_repo = common::git_init();
    let skron_reconfigure_repo = common::git_init();
    let stock_reconfigure_path = canonical_path(stock_reconfigure_repo.path());
    let skron_reconfigure_path = canonical_path(skron_reconfigure_repo.path());
    let stock_reconfigure = run_stock_scalar(
        &stock_home,
        stock_reconfigure_repo.path(),
        &["reconfigure", "--maintenance=keep", &stock_reconfigure_path],
    )
    .expect("stock scalar");
    let skron_reconfigure = run_scalar(
        &skron_home,
        skron_reconfigure_repo.path(),
        &[
            "scalar",
            "reconfigure",
            "--maintenance=keep",
            &skron_reconfigure_path,
        ],
    );
    assert_eq!(
        normalize_scalar_platform_stderr(skron_reconfigure),
        normalize_scalar_platform_stderr(stock_reconfigure)
    );
    assert_eq!(
        common::git_status_args(
            skron_reconfigure_repo.path(),
            &["config", "--get", "core.fsmonitor"]
        ),
        common::git_status_args(
            stock_reconfigure_repo.path(),
            &["config", "--get", "core.fsmonitor"]
        )
    );
    assert_eq!(
        common::git_status_args(
            skron_reconfigure_repo.path(),
            &["config", "--get", "maintenance.strategy"]
        ),
        common::git_status_args(
            stock_reconfigure_repo.path(),
            &["config", "--get", "maintenance.strategy"]
        )
    );
}

#[test]
fn scalar_reconfigure_single_enlistment_matches_stock_maintenance_effects() {
    for mode in ["enable", "disable", "keep"] {
        let stock_home = TempDir::new().expect("stock home");
        let skron_home = TempDir::new().expect("skron home");
        let stock_repo = common::git_init();
        let skron_repo = common::git_init();
        let stock_path = canonical_path(stock_repo.path());
        let skron_path = canonical_path(skron_repo.path());

        let Some(stock_register) = run_stock_scalar(
            &stock_home,
            stock_repo.path(),
            &["register", "--no-maintenance", &stock_path],
        ) else {
            return;
        };
        let skron_register = run_scalar(
            &skron_home,
            skron_repo.path(),
            &["scalar", "register", "--no-maintenance", &skron_path],
        );
        assert_eq!(skron_register, stock_register, "register {mode}");

        let stock = run_stock_scalar(
            &stock_home,
            stock_repo.path(),
            &["reconfigure", &format!("--maintenance={mode}"), &stock_path],
        )
        .expect("stock scalar");
        let skron = run_scalar(
            &skron_home,
            skron_repo.path(),
            &[
                "scalar",
                "reconfigure",
                &format!("--maintenance={mode}"),
                &skron_path,
            ],
        );
        assert_eq!(skron, stock, "reconfigure {mode}");

        for key in ["core.fsmonitor", "index.version", "maintenance.strategy"] {
            assert_eq!(
                common::git_status_args(skron_repo.path(), &["config", "--get", key]),
                common::git_status_args(stock_repo.path(), &["config", "--get", key]),
                "{mode}: config key {key}"
            );
        }

        let stock_global = global_gitconfig(&stock_home).replace(&stock_path, "<repo>");
        let skron_global = global_gitconfig(&skron_home).replace(&skron_path, "<repo>");
        assert_eq!(skron_global, stock_global, "{mode}: global config");
    }
}

#[test]
fn scalar_reconfigure_all_applies_maintenance_mode_to_registered_repos() {
    let home = TempDir::new().expect("home");
    let repo_a = common::git_init();
    let repo_b = common::git_init();
    let repo_a_path = canonical_path(repo_a.path());
    let repo_b_path = canonical_path(repo_b.path());

    for repo_path in [&repo_a_path, &repo_b_path] {
        let (code, stdout, stderr) = run_scalar(
            &home,
            repo_a.path(),
            &["scalar", "register", "--no-maintenance", repo_path],
        );
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, "");
        assert_global_config_contains_repo(&home, "scalar", repo_path);
    }

    let (code, stdout, stderr) =
        run_scalar(&home, repo_a.path(), &["scalar", "reconfigure", "--all"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "");
    for (repo, repo_path) in [(repo_a.path(), &repo_a_path), (repo_b.path(), &repo_b_path)] {
        assert_global_config_contains_repo(&home, "maintenance", repo_path);
        assert_scalar_fsmonitor_shape(repo);
        assert_eq!(
            common::git(repo, ["config", "--get", "maintenance.auto"]),
            "false"
        );
        assert_eq!(
            common::git(repo, ["config", "--get", "maintenance.strategy"]),
            "incremental"
        );
    }

    let (code, stdout, stderr) = run_scalar(
        &home,
        repo_a.path(),
        &["scalar", "reconfigure", "--all", "--maintenance=disable"],
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "");

    let config = global_gitconfig(&home);
    assert!(config.contains(&format!("[scalar]\n\trepo = {repo_a_path}")));
    assert!(config.contains(&format!("\trepo = {repo_b_path}")));
    assert!(
        !config.contains("[maintenance]"),
        "disabled maintenance should unregister all maintenance repos: {config}"
    );
}
