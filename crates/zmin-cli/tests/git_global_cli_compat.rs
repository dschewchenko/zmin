mod common;

use std::fs;
use std::io::Read;
use std::process::{Command, Stdio};

use common::{
    command_any_output as command_output, command_failure_output, command_failure_output_with_env,
    command_output_with_env, command_stdout_bytes, command_stdout_bytes_with_stdin,
    configure_identity, git, git_init, git_with_env, git_with_stdin_args, run_zmin,
    run_zmin_with_env, run_zmin_with_stdin_args, stock_git_bin, zmin_bin,
};
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(unix)]
use std::process::Command as ProcessCommand;

fn extract_trace_lines(stderr: &str, label: &str) -> Vec<String> {
    stderr
        .lines()
        .filter_map(|line| {
            line.split_once(&format!("trace: {label}: "))
                .map(|(_, tail)| tail)
        })
        .map(str::to_owned)
        .collect()
}

#[test]
fn broken_stdout_pipe_matches_stock_git_without_panic_output() {
    let dir = TempDir::new().expect("temp dir");
    let mut zmin = Command::new(zmin_bin())
        .arg("--help")
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zmin");
    drop(zmin.stdout.take().expect("stdout pipe"));
    let mut zmin_stderr = String::new();
    zmin.stderr
        .take()
        .expect("stderr pipe")
        .read_to_string(&mut zmin_stderr)
        .expect("read stderr");
    let zmin_status = zmin.wait().expect("wait zmin");

    let mut stock = Command::new(stock_git_bin())
        .arg("--help")
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stock git");
    drop(stock.stdout.take().expect("stock stdout pipe"));
    let mut stock_stderr = String::new();
    stock
        .stderr
        .take()
        .expect("stock stderr pipe")
        .read_to_string(&mut stock_stderr)
        .expect("read stock stderr");
    let stock_status = stock.wait().expect("wait stock git");

    assert_eq!(zmin_status.code(), stock_status.code());
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        assert_eq!(zmin_status.signal(), stock_status.signal());
    }
    assert!(
        !zmin_stderr.contains("panicked"),
        "broken pipe should not print a panic: {zmin_stderr}"
    );
}

#[test]
fn t1050_negative_big_file_threshold_rejection_matches_stock_git() {
    let repo = common::git_init();
    fs::write(repo.path().join("large"), b"payload\n").expect("write fixture");

    assert_eq!(
        command_output(
            zmin_bin(),
            repo.path(),
            &["-c", "core.bigFileThreshold=-1", "add", "large"],
            "zmin negative core.bigFileThreshold",
        ),
        command_output(
            stock_git_bin().to_str().expect("stock git path utf8"),
            repo.path(),
            &["-c", "core.bigFileThreshold=-1", "add", "large"],
            "git negative core.bigFileThreshold",
        )
    );
}

#[cfg(unix)]
#[test]
fn shell_alias_sigterm_matches_stock_git_shell_convention() {
    let dir = TempDir::new().expect("temp dir");
    let repo = git_init();
    let repo_path = repo.path();

    let helper = dir.path().join("test-tool");
    fs::write(&helper, "#!/bin/sh\nkill -TERM $$\n").expect("write helper");
    let mut permissions = fs::metadata(&helper)
        .expect("helper metadata")
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    fs::set_permissions(&helper, permissions).expect("chmod helper");

    git(repo_path, ["config", "alias.sigterm", "!exec test-tool"]);
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").expect("PATH")
    );

    let zmin = command_failure_output_with_env(
        zmin_bin(),
        repo_path,
        &["sigterm"],
        &[("PATH", path.as_str())],
        "zmin",
    );
    let stock = command_failure_output_with_env(
        "git",
        repo_path,
        &["sigterm"],
        &[("PATH", path.as_str())],
        "git",
    );
    assert_eq!(zmin, stock);
}

#[test]
fn no_arguments_exits_one_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let output = Command::new(zmin_bin())
        .current_dir(dir.path())
        .output()
        .expect("run zmin without arguments");
    let stock = Command::new(stock_git_bin())
        .current_dir(dir.path())
        .output()
        .expect("run stock git without arguments");

    assert_eq!(
        output.status.code(),
        stock.status.code(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout == stock.stdout,
        "stdout mismatch\nzmin:\n{}\nstock:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&stock.stdout)
    );
    assert!(
        output.stderr == stock.stderr,
        "stderr mismatch\nzmin:\n{}\nstock:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&stock.stderr)
    );
}

#[test]
fn config_list_cyclic_global_include_fails_like_stock_git() {
    let home = TempDir::new().expect("home");
    let repo = git_init();

    fs::write(
        home.path().join(".gitconfig"),
        "[include]\n\tpath = cycle\n",
    )
    .expect("write stock global config");
    fs::write(
        home.path().join("cycle"),
        "[include]\n\tpath = .gitconfig\n",
    )
    .expect("write stock include");

    let git = command_failure_output_with_env(
        "git",
        repo.path(),
        &["config", "-l"],
        &[("HOME", home.path().to_str().expect("home"))],
        "git config -l cyclic include",
    );
    let zmin = command_failure_output_with_env(
        zmin_bin(),
        repo.path(),
        &["config", "-l"],
        &[("HOME", home.path().to_str().expect("home"))],
        "zmin config -l cyclic include",
    );

    assert_eq!(zmin, git);
}

#[test]
fn global_exec_path_query_and_override_match_git_shape() {
    let dir = TempDir::new().expect("temp dir");
    let query = Command::new(zmin_bin())
        .arg("--exec-path")
        .current_dir(dir.path())
        .output()
        .expect("run zmin --exec-path");
    assert_eq!(
        query.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&query.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&query.stdout).trim().is_empty(),
        "--exec-path should print an exec path"
    );

    let version = Command::new(zmin_bin())
        .arg("--exec-path=/tmp")
        .arg("version")
        .current_dir(dir.path())
        .output()
        .expect("run zmin --exec-path=/tmp version");
    assert_eq!(
        version.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&version.stdout),
        String::from_utf8_lossy(&version.stderr)
    );
    assert!(
        String::from_utf8_lossy(&version.stdout).starts_with("git version "),
        "version stdout mismatch: {}",
        String::from_utf8_lossy(&version.stdout)
    );
}

#[test]
fn web_browse_help_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let args = ["web--browse", "-h"];
    let stock = command_output(
        stock_git_bin().to_str().expect("stock git path"),
        dir.path(),
        &args,
        "stock git",
    );
    let zmin = command_output(zmin_bin(), dir.path(), &args, "zmin");
    assert_eq!(zmin, stock);
}

#[test]
fn web_browse_failure_surface_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let cases: &[&[&str]] = &[
        &["web--browse"],
        &["web--browse", "https://example.com"],
        &["web--browse", "--browser=foo", "https://example.com"],
        &["web--browse", "--browser=firefox", "https://example.com"],
        &["web--browse", "--config=foo.bar", "https://example.com"],
    ];

    for args in cases {
        let stock = command_failure_output(
            stock_git_bin().to_str().expect("stock git path"),
            dir.path(),
            args,
            "stock git web--browse",
        );
        let zmin = command_failure_output(zmin_bin(), dir.path(), args, "zmin web--browse");
        assert_eq!(zmin, stock, "args: {:?}", args);
    }
}

#[test]
fn web_browse_failure_surface_does_not_depend_on_stock_git_runtime() {
    let dir = TempDir::new().expect("temp dir");
    let poisoned_envs = &[
        ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
        ("GIT_BIN", "/definitely/missing/git"),
    ];
    let cases: &[(&[&str], i32, &str)] = &[
        (
            &["web--browse"],
            1,
            "usage: git web--browse [--browser=browser|--tool=browser] [--config=conf.var] url/file ...",
        ),
        (
            &["web--browse", "--browser=foo", "https://example.com"],
            1,
            "Unknown browser 'foo'.",
        ),
        (
            &["web--browse", "--browser=firefox", "https://example.com"],
            1,
            "The browser firefox is not available as 'firefox'.",
        ),
    ];

    for (args, expected_code, expected_stderr) in cases {
        let output = command_failure_output_with_env(
            zmin_bin(),
            dir.path(),
            args,
            poisoned_envs,
            "zmin poisoned web--browse",
        );
        assert_eq!(output.0, *expected_code, "args: {:?}", args);
        assert!(
            output.1.is_empty(),
            "args: {:?}, stdout: {}",
            args,
            output.1
        );
        assert_eq!(output.2, *expected_stderr, "args: {:?}", args);
    }
}

#[test]
fn branch_tag_and_for_each_ref_unknown_long_option_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    let cases: &[&[&str]] = &[
        &["branch", "--noopt"],
        &["tag", "--noopt"],
        &["for-each-ref", "--noopt"],
    ];

    for args in cases {
        let stock = command_failure_output(
            stock_git_bin().to_str().expect("stock git path"),
            repo.path(),
            args,
            "stock git unknown long option",
        );
        let zmin =
            command_failure_output(zmin_bin(), repo.path(), args, "zmin unknown long option");
        assert_eq!(zmin, stock, "args: {:?}", args);
    }
}

#[test]
fn root_version_option_reports_git_compatible_version_and_zmin_version() {
    let dir = TempDir::new().expect("temp dir");
    let output = Command::new(zmin_bin())
        .arg("--version")
        .current_dir(dir.path())
        .output()
        .expect("run zmin --version");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("git version 2.47.1.zmin "), "{stdout}");
    assert!(stdout.contains("(zmin "), "{stdout}");
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
}

#[test]
fn global_bare_option_applies_to_init_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let zmin_repo = dir.path().join("zmin-bare");
    let git_repo = dir.path().join("git-bare");
    let zmin_repo_arg = zmin_repo.to_str().expect("zmin bare path");
    let git_repo_arg = git_repo.to_str().expect("git bare path");

    command_output(
        zmin_bin(),
        dir.path(),
        &["--bare", "init", zmin_repo_arg],
        "zmin",
    );
    command_output("git", dir.path(), &["--bare", "init", git_repo_arg], "git");

    assert!(zmin_repo.join("HEAD").is_file());
    assert!(zmin_repo.join("objects").is_dir());
    assert!(!zmin_repo.join(".git").exists());
    assert_eq!(
        command_output(
            zmin_bin(),
            &zmin_repo,
            &["config", "--bool", "core.bare"],
            "zmin"
        ),
        (0, "true".to_owned(), String::new())
    );
    assert_eq!(
        command_output("git", &zmin_repo, &["config", "--bool", "core.bare"], "git"),
        command_output("git", &git_repo, &["config", "--bool", "core.bare"], "git")
    );
}

#[test]
fn init_honors_git_dir_and_work_tree_env_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let zmin_git_dir = dir.path().join("zmin.git");
    let zmin_work_tree = dir.path().join("zmin-work");
    let git_git_dir = dir.path().join("git.git");
    let git_work_tree = dir.path().join("git-work");
    fs::create_dir(&zmin_git_dir).expect("create zmin git dir");
    fs::create_dir(&zmin_work_tree).expect("create zmin worktree");
    fs::create_dir(&git_git_dir).expect("create git git dir");
    fs::create_dir(&git_work_tree).expect("create git worktree");

    let zmin_env = [
        ("GIT_DIR", zmin_git_dir.to_str().expect("zmin git dir")),
        (
            "GIT_WORK_TREE",
            zmin_work_tree.to_str().expect("zmin worktree"),
        ),
    ];
    let git_env = [
        ("GIT_DIR", git_git_dir.to_str().expect("git git dir")),
        (
            "GIT_WORK_TREE",
            git_work_tree.to_str().expect("git worktree"),
        ),
    ];
    assert_eq!(
        command_output_with_env(zmin_bin(), dir.path(), &["init"], &zmin_env, "zmin").0,
        command_output_with_env("git", dir.path(), &["init"], &git_env, "git").0
    );

    assert_eq!(
        command_output(
            zmin_bin(),
            &zmin_git_dir,
            &["config", "--bool", "core.bare"],
            "zmin"
        ),
        (0, "false".to_owned(), String::new())
    );
    assert_eq!(
        command_output(
            zmin_bin(),
            &zmin_git_dir,
            &["config", "core.worktree"],
            "zmin"
        ),
        (
            0,
            zmin_work_tree.to_str().expect("zmin worktree").to_owned(),
            String::new()
        )
    );
}

#[test]
fn command_aliases_expand_like_stock_git_for_builtin_and_shell_aliases() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    fs::write(
        repo.join(".git/config"),
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n[alias]\n\taliasedinit = init\n\tscript = !printf alias:%s\\\\n\n",
    )
    .expect("write alias config");

    let nested = repo.join("nested");
    fs::create_dir(&nested).expect("create nested");
    command_output(zmin_bin(), &nested, &["aliasedinit"], "zmin");
    assert!(nested.join(".git").is_dir());

    let bare = dir.path().join("bare.git");
    git(
        dir.path(),
        ["init", "--bare", bare.to_str().expect("bare path")],
    );
    fs::write(
        bare.join("config"),
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = true\n[alias]\n\taliasedinit = init\n",
    )
    .expect("write bare alias config");
    let bare_nested = bare.join("nested");
    fs::create_dir(&bare_nested).expect("create bare nested");
    command_output(zmin_bin(), &bare_nested, &["aliasedinit"], "zmin");
    assert!(bare_nested.join(".git").is_dir());

    let zmin = command_output(zmin_bin(), &repo, &["script", "one", "two"], "zmin");
    let git = command_output("git", &repo, &["script", "one", "two"], "git");
    assert_eq!(zmin, git);

    fs::write(
        repo.join(".git/config"),
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n[alias]\n\taliasedinit = init\n\tscript = !printf alias:%s\\\\n\n\tstatus = init\n",
    )
    .expect("rewrite alias config with builtin override");

    let zmin_rev_parse = command_output(zmin_bin(), &repo, &["rev-parse", "--git-dir"], "zmin");
    let git_rev_parse = command_output("git", &repo, &["rev-parse", "--git-dir"], "git");
    assert_eq!(zmin_rev_parse, git_rev_parse);
}

#[test]
fn command_aliases_parse_inline_section_entry_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let home = dir.path().join("alias-config");
    let zmin_work = dir.path().join("zmin-work");
    let git_work = dir.path().join("git-work");
    fs::create_dir(&home).expect("create home");
    fs::create_dir(&zmin_work).expect("create zmin work");
    fs::create_dir(&git_work).expect("create git work");
    fs::write(home.join(".gitconfig"), "[alias] aliasedinit = init\n").expect("write alias config");

    let zmin = command_output_with_env(
        zmin_bin(),
        &zmin_work,
        &["aliasedinit"],
        &[("HOME", home.to_str().expect("home path"))],
        "zmin",
    );
    let git = command_output_with_env(
        "git",
        &git_work,
        &["aliasedinit"],
        &[("HOME", home.to_str().expect("home path"))],
        "git",
    );

    assert_eq!(zmin.0, git.0);
    assert!(zmin.1.starts_with("Initialized empty Git repository in "));
    assert_eq!(zmin.2, git.2);
    assert!(zmin_work.join(".git").is_dir());
    assert!(git_work.join(".git").is_dir());
}

#[test]
fn init_reftable_materializes_head_storage_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let zmin_repo = dir.path().join("zmin-repo");
    let git_repo = dir.path().join("git-repo");
    let stock_git = stock_git_bin().to_str().expect("stock git path");

    let zmin = command_output(
        zmin_bin(),
        dir.path(),
        &[
            "init",
            "--ref-format=reftable",
            "-b",
            "main",
            zmin_repo.to_str().expect("zmin repo path"),
        ],
        "zmin",
    );
    let git = command_output(
        stock_git,
        dir.path(),
        &[
            "init",
            "--ref-format=reftable",
            "-b",
            "main",
            git_repo.to_str().expect("git repo path"),
        ],
        "git",
    );
    assert_eq!(zmin.0, git.0);
    assert_eq!(zmin.2, git.2);

    for args in [
        ["symbolic-ref", "HEAD"].as_slice(),
        ["rev-parse", "--show-ref-format"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), &zmin_repo, args, "zmin"),
            command_output(stock_git, &git_repo, args, "git"),
            "reftable init mismatch for {args:?}"
        );
    }

    assert_eq!(
        fs::read(zmin_repo.join(".git/HEAD")).expect("read zmin HEAD"),
        fs::read(git_repo.join(".git/HEAD")).expect("read git HEAD")
    );
    assert_eq!(
        fs::read(zmin_repo.join(".git/refs/heads")).expect("read zmin refs heads"),
        fs::read(git_repo.join(".git/refs/heads")).expect("read git refs heads")
    );
    assert!(!zmin_repo.join(".git/refs/tags").exists());
    assert!(zmin_repo.join(".git/reftable/tables.list").is_file());
}

#[test]
fn init_preserves_configured_global_log_all_ref_updates() {
    let dir = TempDir::new().expect("temp dir");
    let home = dir.path().join("home");
    fs::create_dir_all(&home).expect("create home");
    fs::write(
        home.join(".gitconfig"),
        "[core]\n\tlogAllRefUpdates = false\n",
    )
    .expect("write global config");
    let repo = dir.path().join("repo");
    let home_value = home.to_str().expect("home path");
    command_output_with_env(
        zmin_bin(),
        dir.path(),
        &["init", repo.to_str().expect("repo path")],
        &[("HOME", home_value), ("GIT_CONFIG_NOSYSTEM", "1")],
        "zmin init with global reflog config",
    );

    let local_config = fs::read_to_string(repo.join(".git/config")).expect("read local config");
    assert!(
        !local_config
            .to_ascii_lowercase()
            .contains("logallrefupdates")
    );
    let (_, value, _) = command_output_with_env(
        zmin_bin(),
        &repo,
        &["config", "--get", "core.logAllRefUpdates"],
        &[("HOME", home_value), ("GIT_CONFIG_NOSYSTEM", "1")],
        "zmin read global reflog config",
    );
    assert_eq!(value, "false");
}

#[test]
fn init_reftable_repo_supports_first_commit_and_update_ref_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let zmin_repo = dir.path().join("zmin-repo");
    let git_repo = dir.path().join("git-repo");
    let stock_git = stock_git_bin().to_str().expect("stock git path");

    run_zmin(
        dir.path(),
        [
            "init",
            "--ref-format=reftable",
            "-b",
            "main",
            zmin_repo.to_str().expect("zmin repo path"),
        ],
    );
    git(
        dir.path(),
        [
            "init",
            "--ref-format=reftable",
            "-b",
            "main",
            git_repo.to_str().expect("git repo path"),
        ],
    );

    configure_identity(&zmin_repo);
    configure_identity(&git_repo);
    fs::write(zmin_repo.join("tracked.txt"), b"base\n").expect("write zmin file");
    fs::write(git_repo.join("tracked.txt"), b"base\n").expect("write git file");
    run_zmin(&zmin_repo, ["add", "tracked.txt"]);
    git(&git_repo, ["add", "tracked.txt"]);

    let commit_env = [
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
    ];
    let zmin_commit = command_output_with_env(
        zmin_bin(),
        &zmin_repo,
        &["commit", "-m", "base"],
        &commit_env,
        "zmin",
    );
    let git_commit = command_output_with_env(
        stock_git,
        &git_repo,
        &["commit", "-m", "base"],
        &commit_env,
        "git",
    );
    assert_eq!(zmin_commit.0, git_commit.0);
    assert_eq!(zmin_commit.2, git_commit.2);

    let zmin_update = command_output(
        zmin_bin(),
        &zmin_repo,
        &["update-ref", "refs/heads/foo", "@"],
        "zmin",
    );
    let git_update = command_output(
        stock_git,
        &git_repo,
        &["update-ref", "refs/heads/foo", "@"],
        "git",
    );
    assert_eq!(zmin_update, git_update);
    assert_eq!(
        command_output(
            zmin_bin(),
            &zmin_repo,
            &["rev-parse", "refs/heads/foo"],
            "zmin"
        ),
        command_output(
            stock_git,
            &git_repo,
            &["rev-parse", "refs/heads/foo"],
            "git"
        )
    );
}

#[test]
fn reftable_lock_timeout_from_command_config_retries_update_ref() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    run_zmin(
        dir.path(),
        [
            "init",
            "--ref-format=reftable",
            "-b",
            "main",
            repo.to_str().expect("repo path"),
        ],
    );
    configure_identity(&repo);
    fs::write(repo.join("tracked.txt"), b"base\n").expect("write tracked file");
    run_zmin(&repo, ["add", "tracked.txt"]);
    run_zmin(&repo, ["commit", "-m", "base"]);
    let lock = repo.join(".git/reftable/tables.list.lock");
    fs::write(&lock, b"locked").expect("write stack lock");
    let locked_args = [
        "-c",
        "reftable.lockTimeout=0",
        "update-ref",
        "refs/heads/locked",
        "@",
    ];
    assert_eq!(
        command_output(zmin_bin(), &repo, &locked_args, "zmin"),
        command_output(
            stock_git_bin().to_str().expect("stock git path"),
            &repo,
            &locked_args,
            "git",
        )
    );
    let lock_to_release = lock.clone();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        fs::remove_file(lock_to_release).expect("release stack lock");
    });

    let update = command_output(
        zmin_bin(),
        &repo,
        &[
            "-c",
            "reftable.lockTimeout=500",
            "update-ref",
            "refs/heads/locked",
            "@",
        ],
        "zmin",
    );
    releaser.join().expect("lock releaser panicked");

    assert_eq!(update, (0, String::new(), String::new()));
    assert_eq!(
        run_zmin(&repo, ["rev-parse", "refs/heads/locked"]),
        run_zmin(&repo, ["rev-parse", "HEAD"])
    );
}

#[test]
fn invalid_reftable_lock_timeout_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    run_zmin(
        dir.path(),
        [
            "init",
            "--ref-format=reftable",
            repo.to_str().expect("repo path"),
        ],
    );
    let stock_git = stock_git_bin().to_str().expect("stock git path");
    for value in ["abc", "-2"] {
        let config = format!("reftable.lockTimeout={value}");
        let args = [
            "-c",
            config.as_str(),
            "update-ref",
            "refs/heads/locked",
            "1111111111111111111111111111111111111111",
        ];
        assert_eq!(
            command_output(zmin_bin(), &repo, &args, "zmin"),
            command_output(stock_git, &repo, &args, "git"),
            "lock timeout mismatch for {value}"
        );
    }
}

#[test]
fn init_reftable_default_branch_rename_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let zmin_repo = dir.path().join("zmin-repo");
    let git_repo = dir.path().join("git-repo");
    let stock_git = stock_git_bin().to_str().expect("stock git path");
    let init_env = [("GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME", "master")];

    let zmin_init = command_output_with_env(
        zmin_bin(),
        dir.path(),
        &[
            "init",
            "--ref-format=reftable",
            zmin_repo.to_str().expect("zmin repo path"),
        ],
        &init_env,
        "zmin init reftable master",
    );
    let git_init = command_output_with_env(
        stock_git,
        dir.path(),
        &[
            "init",
            "--ref-format=reftable",
            git_repo.to_str().expect("git repo path"),
        ],
        &init_env,
        "git init reftable master",
    );
    assert_eq!(zmin_init.0, git_init.0);

    configure_identity(&zmin_repo);
    configure_identity(&git_repo);
    fs::write(zmin_repo.join("tracked.txt"), b"base\n").expect("write zmin file");
    fs::write(git_repo.join("tracked.txt"), b"base\n").expect("write git file");
    run_zmin(&zmin_repo, ["add", "tracked.txt"]);
    git(&git_repo, ["add", "tracked.txt"]);

    let commit_env = [
        ("GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME", "master"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
    ];
    let zmin_commit = command_output_with_env(
        zmin_bin(),
        &zmin_repo,
        &["commit", "-m", "base"],
        &commit_env,
        "zmin commit reftable master",
    );
    let git_commit = command_output_with_env(
        stock_git,
        &git_repo,
        &["commit", "-m", "base"],
        &commit_env,
        "git commit reftable master",
    );
    assert_eq!(zmin_commit.0, git_commit.0);
    assert_eq!(zmin_commit.1, git_commit.1);

    let zmin_branch = command_output(zmin_bin(), &zmin_repo, &["branch", "-M", "main"], "zmin");
    let git_branch = command_output(stock_git, &git_repo, &["branch", "-M", "main"], "git");
    assert_eq!(zmin_branch, git_branch);
    assert_eq!(
        command_output(zmin_bin(), &zmin_repo, &["symbolic-ref", "HEAD"], "zmin"),
        command_output(stock_git, &git_repo, &["symbolic-ref", "HEAD"], "git")
    );
    assert_eq!(
        command_output(zmin_bin(), &zmin_repo, &["show-ref"], "zmin"),
        command_output(stock_git, &git_repo, &["show-ref"], "git")
    );
}

#[test]
fn command_alias_invalid_family_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let missing_repo = dir.path().join("missing");
    let invalid_repo = dir.path().join("invalid");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            missing_repo.to_str().expect("missing repo"),
        ],
    );
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            invalid_repo.to_str().expect("invalid repo"),
        ],
    );

    fs::write(
        missing_repo.join(".git/config"),
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n[alias]\n\tnoval\n",
    )
    .expect("write missing alias config");
    git(
        &invalid_repo,
        ["config", "alias.invalid.notcommand", "value"],
    );

    for (repo, args) in [
        (&missing_repo, ["noval"].as_slice()),
        (&invalid_repo, ["invalid"].as_slice()),
    ] {
        let stock = command_output("/usr/bin/git", repo, args, "stock git");
        let zmin = command_output(zmin_bin(), repo, args, "zmin");
        assert_eq!(zmin, stock, "alias args: {args:?}");
    }
}

#[test]
fn command_alias_subsection_shell_family_matches_expected_output() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    fs::write(
        repo.join(".git/config"),
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n[alias \"testnew\"]\n\tcommand = !printf '%s\\n' ran-subsection\n[alias \"förgrena\"]\n\tcommand = !printf '%s\\n' ran-swedish\n[alias \"test name\"]\n\tcommand = !printf '%s\\n' ran-spaces\n",
    )
    .expect("write subsection alias config");

    for (args, expected_stdout) in [
        (["testnew"].as_slice(), "ran-subsection"),
        (["förgrena"].as_slice(), "ran-swedish"),
        (["test name"].as_slice(), "ran-spaces"),
    ] {
        let zmin = command_output(zmin_bin(), &repo, args, "zmin");
        assert_eq!(zmin.0, 0, "alias args: {args:?}");
        assert_eq!(zmin.1, expected_stdout, "alias args: {args:?}");
        assert_eq!(zmin.2, "", "alias args: {args:?}");
    }
}

#[test]
fn command_alias_subsection_config_write_and_help_family_matches_expected_output() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );

    command_output(
        zmin_bin(),
        &repo,
        ["config", "alias..something.command", "!echo foobar"].as_slice(),
        "zmin",
    );
    command_output(
        zmin_bin(),
        &repo,
        ["config", "alias.förgrena.command", "!echo test"].as_slice(),
        "zmin",
    );

    let dot_alias = command_output(zmin_bin(), &repo, [".something"].as_slice(), "zmin");
    assert_eq!(dot_alias.0, 0, "stderr: {}", dot_alias.2);
    assert_eq!(dot_alias.1.trim_end(), "foobar");
    assert!(dot_alias.2.is_empty(), "stderr: {}", dot_alias.2);

    let help = command_output(zmin_bin(), &repo, ["help", "-a"].as_slice(), "zmin");
    assert_eq!(help.0, 0, "stderr: {}", help.2);
    assert!(help.1.contains("förgrena"), "stdout: {}", help.1);
}

#[test]
fn command_scope_alias_chain_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );

    let args = [
        "-c",
        "alias.one=two",
        "-c",
        "alias.two=status",
        "one",
        "--short",
    ];
    let stock = command_output("/usr/bin/git", &repo, &args, "stock git");
    let zmin = command_output(zmin_bin(), &repo, &args, "zmin");
    assert_eq!(zmin, stock);
}

#[test]
fn unknown_command_and_shell_alias_trace_family_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );

    let stock_unknown = command_failure_output_with_env(
        "/usr/bin/git",
        &repo,
        &["frotz", "a", "", "b", " ", "c"],
        &[("GIT_TRACE", "1")],
        "stock git unknown trace",
    );
    let zmin_unknown = command_failure_output_with_env(
        zmin_bin(),
        &repo,
        &["frotz", "a", "", "b", " ", "c"],
        &[("GIT_TRACE", "1")],
        "zmin unknown trace",
    );
    assert_eq!(zmin_unknown.0, stock_unknown.0);
    assert_eq!(
        extract_trace_lines(&zmin_unknown.2, "run_command"),
        extract_trace_lines(&stock_unknown.2, "run_command")
    );
    assert!(
        zmin_unknown
            .2
            .contains("git: 'frotz' is not a git command.")
    );

    git(&repo, ["config", "alias.echo", "!echo $*"]);
    let stock_alias = command_output_with_env(
        "/usr/bin/git",
        &repo,
        &["echo", "arg"],
        &[("GIT_TRACE", "1")],
        "stock git shell alias trace",
    );
    let zmin_alias = command_output_with_env(
        zmin_bin(),
        &repo,
        &["echo", "arg"],
        &[("GIT_TRACE", "1")],
        "zmin shell alias trace",
    );
    assert_eq!(zmin_alias.0, stock_alias.0);
    assert_eq!(zmin_alias.1, stock_alias.1);
    assert_eq!(
        extract_trace_lines(&zmin_alias.2, "run_command"),
        extract_trace_lines(&stock_alias.2, "run_command")
    );
    assert_eq!(
        extract_trace_lines(&zmin_alias.2, "start_command"),
        extract_trace_lines(&stock_alias.2, "start_command")
    );
}

#[test]
fn grep_short_h_without_pattern_keeps_nonempty_stdout_when_alias_is_present() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );

    let base = command_failure_output(zmin_bin(), &repo, &["grep", "-h"], "zmin grep -h");
    let aliased = command_failure_output(
        zmin_bin(),
        &repo,
        &["-c", "alias.grep=status", "grep", "-h"],
        "zmin grep -h with alias",
    );

    assert_eq!(base.0, 129);
    assert!(!base.1.is_empty(), "stdout should contain usage/help text");
    assert_eq!(aliased, base);
}

#[test]
fn config_remove_section_family_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let zmin_repo = dir.path().join("zmin-repo");
    let git_repo = dir.path().join("git-repo");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            zmin_repo.to_str().expect("zmin repo path"),
        ],
    );
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            git_repo.to_str().expect("git repo path"),
        ],
    );

    let config_body = "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n[branch \"keep\"]\n\tremote = origin\n\tmerge = refs/heads/keep\n[branch \"drop\"]\n\tremote = origin\n\tmerge = refs/heads/drop\n";
    fs::write(zmin_repo.join(".git/config"), config_body).expect("write zmin config");
    fs::write(git_repo.join(".git/config"), config_body).expect("write git config");

    for args in [
        ["config", "--remove-section", "branch.drop"].as_slice(),
        ["config", "remove-section", "branch.keep"].as_slice(),
    ] {
        let stock = command_output("/usr/bin/git", &git_repo, args, "stock git");
        let zmin = command_output(zmin_bin(), &zmin_repo, args, "zmin");
        assert_eq!(zmin, stock, "remove-section args: {args:?}");
    }

    assert_eq!(
        fs::read_to_string(zmin_repo.join(".git/config")).expect("read zmin config"),
        fs::read_to_string(git_repo.join(".git/config")).expect("read git config")
    );
}

#[test]
fn config_rename_section_family_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let zmin_repo = dir.path().join("zmin-repo");
    let git_repo = dir.path().join("git-repo");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            zmin_repo.to_str().expect("zmin repo path"),
        ],
    );
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            git_repo.to_str().expect("git repo path"),
        ],
    );

    let config_body = "# Hallo\n\t#Bello\n[branch \"eins\"]\n\tx = 1\n[branch.eins]\n\ty = 1\n\t[branch \"1 234 blabl/a\"]\nweird\n[branch \"vier\"] z = 1\n";
    fs::write(zmin_repo.join(".git/config"), config_body).expect("write zmin config");
    fs::write(git_repo.join(".git/config"), config_body).expect("write git config");

    for args in [
        ["config", "--rename-section", "branch.eins", "branch.zwei"].as_slice(),
        [
            "config",
            "rename-section",
            "branch.1 234 blabl/a",
            "branch.drei",
        ]
        .as_slice(),
        ["config", "--rename-section", "branch.vier", "branch.zwei"].as_slice(),
    ] {
        let stock = command_output("/usr/bin/git", &git_repo, args, "stock git");
        let zmin = command_output(zmin_bin(), &zmin_repo, args, "zmin");
        assert_eq!(zmin, stock, "rename-section args: {args:?}");
    }

    assert_eq!(
        fs::read_to_string(zmin_repo.join(".git/config")).expect("read zmin config"),
        fs::read_to_string(git_repo.join(".git/config")).expect("read git config")
    );
}

#[test]
fn config_git_config_scope_overrides_command_config_entries_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let zmin_repo = dir.path().join("zmin-repo");
    let git_repo = dir.path().join("git-repo");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            zmin_repo.to_str().expect("zmin repo path"),
        ],
    );
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            git_repo.to_str().expect("git repo path"),
        ],
    );

    let zmin_other = zmin_repo.join("other-config");
    let git_other = git_repo.join("other-config");
    fs::write(&zmin_other, "[ein]\n\tbahn = strasse\n").expect("write zmin other config");
    fs::write(&git_other, "[ein]\n\tbahn = strasse\n").expect("write git other config");

    for args in [
        ["config", "--list"].as_slice(),
        ["config", "list"].as_slice(),
    ] {
        let zmin = command_output_with_env(
            zmin_bin(),
            &zmin_repo,
            args,
            &[
                (
                    "GIT_CONFIG",
                    zmin_other.to_str().expect("zmin other config path"),
                ),
                ("GIT_CONFIG_PARAMETERS", "'a.b.c=d'"),
            ],
            "zmin",
        );
        let git = command_output_with_env(
            "git",
            &git_repo,
            args,
            &[
                (
                    "GIT_CONFIG",
                    git_other.to_str().expect("git other config path"),
                ),
                ("GIT_CONFIG_PARAMETERS", "'a.b.c=d'"),
            ],
            "git",
        );
        assert_eq!(zmin, git, "config args: {args:?}");
        assert_eq!(zmin.1, "ein.bahn=strasse", "config args: {args:?}");
    }
}

#[test]
fn config_invalid_negated_mode_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );

    let zmin = command_failure_output(zmin_bin(), &repo, &["config", "--no-get"], "zmin");
    let git = command_failure_output("/usr/bin/git", &repo, &["config", "--no-get"], "git");
    assert_eq!(zmin, git);
}

#[test]
fn config_conflicting_get_modes_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );

    let zmin = command_failure_output(zmin_bin(), &repo, &["config", "--get", "--get-all"], "zmin");
    let git = command_failure_output(
        "/usr/bin/git",
        &repo,
        &["config", "--get", "--get-all"],
        "git",
    );
    assert_eq!(zmin, git);
}

#[test]
fn config_trailing_cr_value_round_trips_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );

    command_output(
        zmin_bin(),
        &repo,
        &["config", "set", "core.foo", "bar\r"],
        "zmin",
    );
    let actual = Command::new(zmin_bin())
        .args([
            "-C",
            repo.to_str().expect("repo path"),
            "config",
            "get",
            "core.foo",
        ])
        .output()
        .expect("run zmin config get");
    assert_eq!(actual.status.code(), Some(0));
    assert_eq!(actual.stdout, b"bar\r\n");

    let stock = Command::new("/usr/bin/git")
        .args([
            "-C",
            repo.to_str().expect("repo path"),
            "config",
            "get",
            "core.foo",
        ])
        .output()
        .expect("run stock git config get");
    assert_eq!(actual.stdout, stock.stdout);
    assert_eq!(actual.stderr, stock.stderr);
}

#[test]
fn config_legacy_positional_value_pattern_family_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let zmin_repo = dir.path().join("zmin-repo");
    let git_repo = dir.path().join("git-repo");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            zmin_repo.to_str().expect("zmin repo path"),
        ],
    );
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            git_repo.to_str().expect("git repo path"),
        ],
    );

    let meta = "a+b*c?d[e]f.g";
    for repo in [&zmin_repo, &git_repo] {
        fs::write(repo.join("config"), "").expect("seed empty config file");
        command_output(
            "/usr/bin/git",
            repo,
            &["config", "--file=config", "fixed.test", "bogus"],
            "seed bogus",
        );
        command_output(
            "/usr/bin/git",
            repo,
            &["config", "--file=config", "--add", "fixed.test", meta],
            "seed meta",
        );
    }

    for args in [
        [
            "config",
            "--file=config",
            "--get",
            "--fixed-value",
            "fixed.test",
            meta,
        ]
        .as_slice(),
        [
            "config",
            "--file=config",
            "--get-all",
            "--fixed-value",
            "fixed.test",
            meta,
        ]
        .as_slice(),
        [
            "config",
            "--file=config",
            "--get-regexp",
            "--fixed-value",
            "fixed+",
            meta,
        ]
        .as_slice(),
    ] {
        let zmin = command_output(zmin_bin(), &zmin_repo, args, "zmin");
        let git = command_output("/usr/bin/git", &git_repo, args, "git");
        assert_eq!(zmin, git, "legacy positional fixed-value args: {args:?}");
    }

    let zmin_unset = command_output(
        zmin_bin(),
        &zmin_repo,
        &[
            "config",
            "--file=config",
            "--fixed-value",
            "--unset",
            "fixed.test",
            meta,
        ],
        "zmin",
    );
    let git_unset = command_output(
        "/usr/bin/git",
        &git_repo,
        &[
            "config",
            "--file=config",
            "--fixed-value",
            "--unset",
            "fixed.test",
            meta,
        ],
        "git",
    );
    assert_eq!(zmin_unset, git_unset);
    assert_eq!(
        fs::read_to_string(zmin_repo.join("config")).expect("read zmin file"),
        fs::read_to_string(git_repo.join("config")).expect("read git file")
    );
}

#[test]
fn init_creates_stock_git_readable_repository() {
    let dir = TempDir::new().expect("temp dir");
    run_zmin(dir.path(), ["init", "-b", "trunk", "repo"]);

    let repo = dir.path().join("repo");
    assert_eq!(git(&repo, ["rev-parse", "--git-dir"]), ".git");
    assert_eq!(
        run_zmin(&repo, ["rev-parse", "--git-dir"]),
        git(&repo, ["rev-parse", "--git-dir"])
    );
    assert_eq!(
        run_zmin(&repo, ["rev-parse", "--show-toplevel"]),
        git(&repo, ["rev-parse", "--show-toplevel"])
    );
    assert_eq!(
        run_zmin(&repo, ["rev-parse", "--is-inside-work-tree"]),
        git(&repo, ["rev-parse", "--is-inside-work-tree"])
    );
    assert_eq!(
        run_zmin(&repo, ["rev-parse", "--is-bare-repository"]),
        git(&repo, ["rev-parse", "--is-bare-repository"])
    );
    fs::create_dir_all(repo.join("nested/dir")).expect("create nested dir");
    assert_eq!(
        run_zmin(&repo.join("nested/dir"), ["rev-parse", "--git-dir"]),
        git(&repo.join("nested/dir"), ["rev-parse", "--git-dir"])
    );
    assert_eq!(
        run_zmin(&repo.join("nested/dir"), ["rev-parse", "--show-prefix"]),
        git(&repo.join("nested/dir"), ["rev-parse", "--show-prefix"])
    );
    assert_eq!(
        run_zmin(&repo.join("nested/dir"), ["rev-parse", "--show-cdup"]),
        git(&repo.join("nested/dir"), ["rev-parse", "--show-cdup"])
    );
    for args in [
        ["rev-parse", "--absolute-git-dir"].as_slice(),
        ["rev-parse", "--git-common-dir"].as_slice(),
        ["rev-parse", "--git-path", "objects"].as_slice(),
        ["rev-parse", "--is-inside-git-dir"].as_slice(),
        ["rev-parse", "--is-shallow-repository"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), &repo.join("nested/dir"), args, "zmin"),
            command_output("git", &repo.join("nested/dir"), args, "git"),
            "rev-parse discovery mismatch for {args:?}"
        );
    }
    for args in [
        ["rev-parse", "--git-dir"].as_slice(),
        ["rev-parse", "--git-common-dir"].as_slice(),
        ["rev-parse", "--git-path", "objects"].as_slice(),
        ["rev-parse", "--is-inside-git-dir"].as_slice(),
        ["rev-parse", "--is-inside-work-tree"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), &repo.join(".git"), args, "zmin"),
            command_output("git", &repo.join(".git"), args, "git"),
            "rev-parse git-dir cwd mismatch for {args:?}"
        );
    }
    assert_eq!(git(&repo, ["symbolic-ref", "HEAD"]), "refs/heads/trunk");
    assert_eq!(git(&repo, ["config", "--get", "core.bare"]), "false");
}

#[test]
fn init_quiet_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    for (git_name, zmin_name, args) in [
        (
            "git-short",
            "zmin-short",
            ["init", "-q", "-b", "main"].as_slice(),
        ),
        (
            "git-long",
            "zmin-long",
            ["init", "--quiet", "-b", "main"].as_slice(),
        ),
    ] {
        let mut git_args = args.to_vec();
        git_args.push(git_name);
        let mut zmin_args = args.to_vec();
        zmin_args.push(zmin_name);

        assert_eq!(
            command_output("git", dir.path(), &git_args, "git"),
            command_output(zmin_bin(), dir.path(), &zmin_args, "zmin")
        );
        assert_eq!(
            git(&dir.path().join(git_name), ["branch", "--show-current"]),
            run_zmin(&dir.path().join(zmin_name), ["branch", "--show-current"])
        );
    }
}

#[test]
fn rev_parse_discovers_repository_through_gitfile_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    run_zmin(dir.path(), ["init", "repo"]);
    let real_git = repo.join(".realgit");
    fs::rename(repo.join(".git"), &real_git).expect("move git dir");
    fs::write(
        repo.join(".git"),
        format!("gitdir: {}\n", real_git.display()),
    )
    .expect("gitfile");

    assert_eq!(
        command_output(zmin_bin(), &repo, &["rev-parse"], "zmin"),
        command_output("git", &repo, &["rev-parse"], "git")
    );
    assert_eq!(
        command_output(zmin_bin(), &repo, &["rev-parse", "--git-dir"], "zmin"),
        command_output("git", &repo, &["rev-parse", "--git-dir"], "git")
    );
    fs::write(repo.join("blob.txt"), "gitfile blob\n").expect("write blob");
    let sha = run_zmin(&repo, ["hash-object", "-w", "blob.txt"]);
    assert_eq!(
        command_output(zmin_bin(), &repo, &["cat-file", "blob", &sha], "zmin"),
        command_output("git", &repo, &["cat-file", "blob", &sha], "git")
    );

    fs::write(
        repo.join(".git"),
        format!("gitdir {}\n", real_git.display()),
    )
    .expect("invalid gitfile");
    let zmin = command_output(zmin_bin(), &repo, &["rev-parse"], "zmin");
    let git = command_output("git", &repo, &["rev-parse"], "git");
    assert_eq!(zmin.0, git.0);
    assert!(
        zmin.2.contains("invalid gitfile format"),
        "stderr: {}",
        zmin.2
    );
}

#[cfg(unix)]
#[test]
fn rev_parse_rejects_fifo_gitfile_family_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let trap = dir.path().join("fifo-trap");
    fs::create_dir_all(&trap).expect("create fifo trap");
    let status = ProcessCommand::new("mkfifo")
        .arg(trap.join(".git"))
        .status()
        .expect("mkfifo");
    assert!(status.success(), "mkfifo failed: {status:?}");

    let zmin = command_output(zmin_bin(), &trap, &["rev-parse", "--git-dir"], "zmin");
    let git = command_output("git", &trap, &["rev-parse", "--git-dir"], "git");
    assert_eq!(zmin.0, git.0);
    assert!(zmin.2.contains("not a regular file"), "stderr: {}", zmin.2);
}

#[cfg(unix)]
#[test]
fn rev_parse_rejects_symlink_to_fifo_gitfile_family_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let trap = dir.path().join("symlink-fifo-trap");
    fs::create_dir_all(&trap).expect("create symlink trap");
    let status = ProcessCommand::new("mkfifo")
        .arg(trap.join("target-fifo"))
        .status()
        .expect("mkfifo");
    assert!(status.success(), "mkfifo failed: {status:?}");
    symlink("target-fifo", trap.join(".git")).expect("symlink fifo");

    let zmin = command_output(zmin_bin(), &trap, &["rev-parse", "--git-dir"], "zmin");
    let git = command_output("git", &trap, &["rev-parse", "--git-dir"], "git");
    assert_eq!(zmin.0, git.0);
    assert!(zmin.2.contains("not a regular file"), "stderr: {}", zmin.2);
}

#[test]
fn empty_dot_git_directory_is_ignored_during_repo_discovery_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let parent = dir.path().join("parent");
    let nested = parent.join("empty-dir");
    run_zmin(dir.path(), ["init", "parent"]);
    fs::create_dir_all(&nested).expect("create nested");

    let before = command_output(
        zmin_bin(),
        &nested,
        &["rev-parse", "--git-dir"],
        "zmin before",
    );
    fs::create_dir(nested.join(".git")).expect("create empty dot git");
    let after = command_output(
        zmin_bin(),
        &nested,
        &["rev-parse", "--git-dir"],
        "zmin after",
    );
    let stock_before = command_output("git", &nested, &["rev-parse", "--git-dir"], "git before");
    let stock_after = command_output("git", &nested, &["rev-parse", "--git-dir"], "git after");

    assert_eq!(before, stock_before);
    assert_eq!(after, stock_after);
    assert_eq!(before, after);
}

#[test]
fn checkout_dotfile_path_does_not_fail_branch_ref_validation() {
    let dir = TempDir::new().expect("temp dir");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    git(
        dir.path(),
        ["init", "-b", "main", git_repo.to_str().expect("git path")],
    );
    run_zmin(
        dir.path(),
        ["init", "-b", "main", zmin_repo.to_str().expect("zmin path")],
    );
    configure_identity(&git_repo);
    configure_identity(&zmin_repo);
    fs::write(git_repo.join(".changelog.yml"), b"base\n").expect("write git dotfile");
    fs::write(zmin_repo.join(".changelog.yml"), b"base\n").expect("write zmin dotfile");
    git(&git_repo, ["add", "-A"]);
    run_zmin(&zmin_repo, ["add", "-A"]);
    git_with_env(&git_repo, ["commit", "-m", "base"]);
    run_zmin_with_env(&zmin_repo, ["commit", "-m", "base"]);

    fs::write(git_repo.join(".changelog.yml"), b"dirty\n").expect("dirty git dotfile");
    fs::write(zmin_repo.join(".changelog.yml"), b"dirty\n").expect("dirty zmin dotfile");
    assert_eq!(
        command_output(
            zmin_bin(),
            &zmin_repo,
            &["checkout", ".changelog.yml"],
            "zmin"
        ),
        command_output("git", &git_repo, &["checkout", ".changelog.yml"], "git")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.join(".changelog.yml")).expect("read zmin dotfile"),
        fs::read_to_string(git_repo.join(".changelog.yml")).expect("read git dotfile")
    );
}

#[test]
fn global_c_option_changes_directory_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    fs::write(repo.join("a.txt"), b"base\n").expect("write base");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "base"]);
    fs::write(repo.join("a.txt"), b"changed\n").expect("write changed");

    for args in [
        ["-C", repo.to_str().expect("repo path"), "status", "--short"].as_slice(),
        [
            "-C",
            dir.path().to_str().expect("dir path"),
            "-C",
            "repo",
            "status",
            "--short",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), dir.path(), args, "zmin"),
            command_output("git", dir.path(), args, "git"),
            "global -C mismatch for {args:?}"
        );
    }
}

#[test]
fn rev_parse_mixed_discovery_and_revisions_preserves_stock_order() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    fs::write(repo.join("a.txt"), b"base\n").expect("write base");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "base"]);

    for args in [
        ["rev-parse", "--git-dir", "HEAD"].as_slice(),
        ["rev-parse", "HEAD", "--git-dir"].as_slice(),
        ["rev-parse", "--git-path", "objects", "HEAD"].as_slice(),
        ["rev-parse", "HEAD", "--show-object-format"].as_slice(),
        ["rev-parse", "--show-object-format=storage", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), &repo, args, "zmin"),
            command_output("git", &repo, args, "git"),
            "rev-parse mixed output mismatch for {args:?}"
        );
    }
}

#[test]
fn rev_parse_show_prefix_inside_git_dir_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    let objects = repo.join(".git/objects");
    assert_eq!(
        command_output(
            zmin_bin(),
            &objects,
            &["rev-parse", "--show-prefix"],
            "zmin"
        ),
        command_output("git", &objects, &["rev-parse", "--show-prefix"], "git")
    );
}

#[test]
fn rev_parse_path_format_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    fs::create_dir_all(repo.join("sub/dir")).expect("create subdir");

    for (cwd, args) in [
        (
            repo.as_path(),
            ["rev-parse", "--path-format=absolute", "--git-dir"].as_slice(),
        ),
        (
            repo.as_path(),
            ["rev-parse", "--path-format=relative", "--git-common-dir"].as_slice(),
        ),
        (
            repo.join("sub/dir").as_path(),
            ["rev-parse", "--path-format=absolute", "--git-dir"].as_slice(),
        ),
        (
            repo.join("sub/dir").as_path(),
            ["rev-parse", "--path-format=relative", "--git-common-dir"].as_slice(),
        ),
        (
            repo.as_path(),
            ["rev-parse", "--path-format=relative", "--absolute-git-dir"].as_slice(),
        ),
        (
            repo.as_path(),
            [
                "rev-parse",
                "--path-format=absolute",
                "--git-dir",
                "--path-format=relative",
                "--git-path",
                "objects/foo/bar",
            ]
            .as_slice(),
        ),
        (
            repo.as_path(),
            ["rev-parse", "--path-format=relative", "--show-toplevel"].as_slice(),
        ),
    ] {
        assert_eq!(
            command_output(zmin_bin(), cwd, args, "zmin"),
            command_output("git", cwd, args, "git"),
            "rev-parse path-format mismatch for {args:?}"
        );
    }
}

#[test]
fn rev_parse_since_until_filters_match_stock_git_order() {
    let repo = git_init();
    let args = [
        "rev-parse",
        "--since=1970-01-01T00:00:01Z",
        "--since=1970-01-01T00:00:01Z",
        "--after=1970-01-01T00:00:03Z",
        "--until=1970-01-01T00:00:02Z",
        "--before=1970-01-01T00:00:04Z",
    ];
    assert_eq!(
        command_output(zmin_bin(), repo.path(), &args, "zmin"),
        command_output("git", repo.path(), &args, "git")
    );
}

#[test]
fn rev_parse_show_ref_format_invalid_storage_reports_stock_error() {
    let repo = git_init();
    git(repo.path(), ["config", "extensions.refStorage", "broken"]);
    let zmin = command_output(
        zmin_bin(),
        repo.path(),
        &["rev-parse", "--show-ref-format"],
        "zmin",
    );
    let git = command_output(
        "git",
        repo.path(),
        &["rev-parse", "--show-ref-format"],
        "git",
    );
    assert_eq!(zmin.0, git.0);
    assert!(
        zmin.2
            .contains("error: invalid value for 'extensions.refstorage': 'broken'"),
        "stderr: {}",
        zmin.2
    );
}

#[test]
fn rev_parse_core_bare_config_affects_discovery_flags() {
    let repo = git_init();
    git(repo.path(), ["config", "core.bare", "true"]);
    for args in [
        ["rev-parse", "--is-bare-repository"].as_slice(),
        ["rev-parse", "--is-inside-work-tree"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), repo.path(), args, "zmin"),
            command_output("git", repo.path(), args, "git"),
            "rev-parse core.bare discovery mismatch for {args:?}"
        );
    }
}

#[test]
fn rev_parse_show_superproject_working_tree_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let super_repo = dir.path().join("super");
    let sub_repo = dir.path().join("sub");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            super_repo.to_str().expect("super path"),
        ],
    );
    git(
        dir.path(),
        ["init", "-b", "main", sub_repo.to_str().expect("sub path")],
    );
    configure_identity(&super_repo);
    configure_identity(&sub_repo);
    fs::write(super_repo.join("root.txt"), b"super\n").expect("write super");
    git(&super_repo, ["add", "-A"]);
    git_with_env(&super_repo, ["commit", "-m", "super"]);
    fs::write(sub_repo.join("sub.txt"), b"sub\n").expect("write sub");
    git(&sub_repo, ["add", "-A"]);
    git_with_env(&sub_repo, ["commit", "-m", "sub"]);
    command_output(
        "git",
        &super_repo,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "../sub",
            "dir/sub",
        ],
        "git",
    );

    let args = ["rev-parse", "--show-superproject-working-tree"];
    assert_eq!(
        command_output(zmin_bin(), &super_repo, &args, "zmin"),
        command_output("git", &super_repo, &args, "git")
    );
    assert_eq!(
        command_output(zmin_bin(), &super_repo.join("dir/sub"), &args, "zmin"),
        command_output("git", &super_repo.join("dir/sub"), &args, "git")
    );
}

#[test]
fn rev_parse_message_search_favors_most_recent_matching_commit() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    fs::write(repo.path().join("common.txt"), b"old\n").expect("write old");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "common-old"]);
    fs::write(repo.path().join("common.txt"), b"new\n").expect("write new");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "common-new"]);

    for rev in [":/common", "HEAD^{/common}"] {
        assert_eq!(
            command_output(zmin_bin(), repo.path(), &["rev-parse", rev], "zmin"),
            command_output("git", repo.path(), &["rev-parse", rev], "git"),
            "message search mismatch for {rev}"
        );
    }
}

#[test]
fn rev_parse_upstream_shorthand_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let repo = dir.path().join("repo");

    command_output(
        "git",
        dir.path(),
        &["init", "--bare", remote.to_str().expect("remote path")],
        "git",
    );
    command_output(
        "git",
        dir.path(),
        &[
            "clone",
            remote.to_str().expect("remote path"),
            repo.to_str().expect("repo path"),
        ],
        "git",
    );
    configure_identity(&repo);
    command_output("git", &repo, &["checkout", "-b", "main"], "git");
    fs::write(repo.join("a.txt"), b"one\n").expect("write one");
    command_output("git", &repo, &["add", "-A"], "git");
    git_with_env(&repo, ["commit", "-m", "one"]);
    command_output("git", &repo, &["push", "-u", "origin", "main"], "git");
    command_output("git", &repo, &["checkout", "-b", "feature"], "git");
    fs::write(repo.join("a.txt"), b"two\n").expect("write two");
    command_output("git", &repo, &["add", "-A"], "git");
    git_with_env(&repo, ["commit", "-m", "two"]);
    command_output("git", &repo, &["push", "-u", "origin", "feature"], "git");

    for rev in [
        "@{u}",
        "HEAD@{u}",
        "feature@{u}",
        "@{upstream}",
        "feature@{upstream}",
    ] {
        assert_eq!(
            command_output(zmin_bin(), &repo, &["rev-parse", rev], "zmin"),
            command_output("git", &repo, &["rev-parse", rev], "git"),
            "upstream shorthand mismatch for {rev}"
        );
    }
}

#[test]
fn rev_parse_push_shorthand_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let parent = dir.path().join("parent.git");
    let other = dir.path().join("other.git");
    let repo = dir.path().join("repo");
    for remote in [&parent, &other] {
        command_output(
            "git",
            dir.path(),
            &["init", "--bare", remote.to_str().expect("remote path")],
            "git",
        );
    }
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    command_output(
        "git",
        &repo,
        &[
            "remote",
            "add",
            "origin",
            parent.to_str().expect("parent path"),
        ],
        "git",
    );
    command_output(
        "git",
        &repo,
        &[
            "remote",
            "add",
            "other",
            other.to_str().expect("other path"),
        ],
        "git",
    );
    fs::write(repo.join("base.txt"), b"base\n").expect("write base");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "base"]);
    command_output("git", &repo, &["push", "-u", "origin", "main"], "git");
    command_output(
        "git",
        &repo,
        &["branch", "--track", "topic", "origin/main"],
        "git",
    );
    command_output("git", &repo, &["push", "origin", "topic"], "git");
    command_output("git", &repo, &["push", "other", "topic"], "git");

    for (config, branch) in [
        ("simple", "main"),
        ("current", "topic"),
        ("matching", "topic"),
    ] {
        command_output("git", &repo, &["config", "push.default", config], "git");
        let rev = format!("{branch}@{{push}}");
        assert_eq!(
            command_output(
                zmin_bin(),
                &repo,
                &["rev-parse", "--symbolic-full-name", &rev],
                "zmin"
            ),
            command_output(
                "git",
                &repo,
                &["rev-parse", "--symbolic-full-name", &rev],
                "git"
            ),
            "push shorthand mismatch for {rev}"
        );
    }
    command_output("git", &repo, &["config", "push.default", "current"], "git");
    command_output(
        "git",
        &repo,
        &["config", "branch.topic.pushRemote", "other"],
        "git",
    );
    assert_eq!(
        command_output(
            zmin_bin(),
            &repo,
            &["rev-parse", "--symbolic-full-name", "topic@{push}"],
            "zmin"
        ),
        command_output(
            "git",
            &repo,
            &["rev-parse", "--symbolic-full-name", "topic@{push}"],
            "git"
        )
    );
    command_output("git", &repo, &["config", "push.default", "nothing"], "git");
    command_output(
        "git",
        &repo,
        &["config", "--unset", "branch.topic.pushRemote"],
        "git",
    );
    command_output(
        "git",
        &repo,
        &[
            "config",
            "remote.origin.push",
            "refs/heads/*:refs/heads/magic/*",
        ],
        "git",
    );
    command_output("git", &repo, &["push"], "git");
    assert_eq!(
        command_output(
            zmin_bin(),
            &repo,
            &["rev-parse", "--symbolic-full-name", "topic@{PUSH}"],
            "zmin"
        ),
        command_output(
            "git",
            &repo,
            &["rev-parse", "--symbolic-full-name", "topic@{PUSH}"],
            "git"
        )
    );
}

#[test]
fn add_index_version_and_skip_hash_controls_match_stock_git() {
    for (args, envs) in [
        (vec!["add", "a.txt"], vec![("GIT_INDEX_VERSION", "2bogus")]),
        (vec!["-c", "index.version=4", "add", "a.txt"], vec![]),
        (vec!["-c", "index.skipHash=true", "add", "a.txt"], vec![]),
        (vec!["-c", "feature.manyFiles=true", "add", "a.txt"], vec![]),
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        fs::write(git_repo.path().join("a.txt"), b"one\n").expect("write git file");
        fs::write(zmin_repo.path().join("a.txt"), b"one\n").expect("write zmin file");
        let args = args.to_vec();
        assert_eq!(
            command_output_with_env(zmin_bin(), zmin_repo.path(), &args, &envs, "zmin"),
            command_output_with_env("git", git_repo.path(), &args, &envs, "git"),
            "add index controls mismatch for {args:?}"
        );
        let git_index = fs::read(git_repo.path().join(".git/index")).expect("read git index");
        let zmin_index = fs::read(zmin_repo.path().join(".git/index")).expect("read zmin index");
        assert_eq!(
            &zmin_index[..8],
            &git_index[..8],
            "index header for {args:?}"
        );
        let zmin_skip_hash = zmin_index[zmin_index.len() - 20..]
            .iter()
            .all(|byte| *byte == 0);
        let git_skip_hash = git_index[git_index.len() - 20..]
            .iter()
            .all(|byte| *byte == 0);
        assert_eq!(
            zmin_skip_hash, git_skip_hash,
            "index checksum policy for {args:?}"
        );
    }
}

#[test]
fn rev_parse_symbolic_full_name_bisect_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    for index in 0..5 {
        fs::write(repo.join("a.txt"), format!("{index}\n")).expect("write file");
        git(&repo, ["add", "-A"]);
        git_with_env(&repo, ["commit", "-m", &format!("commit {index}")]);
    }

    for (name, rev) in [
        ("refs/bisect/bad-1", "HEAD~1"),
        ("refs/bisect/b", "HEAD~2"),
        ("refs/bisect/bad-3", "HEAD~3"),
        ("refs/bisect/good-3", "HEAD~3"),
        ("refs/bisect/bad-4", "HEAD~4"),
        ("refs/bisect/go", "HEAD~4"),
    ] {
        git(&repo, ["update-ref", name, rev]);
    }

    let args = ["rev-parse", "--symbolic-full-name", "--bisect"];
    assert_eq!(
        command_output(zmin_bin(), &repo, &args, "zmin"),
        command_output("git", &repo, &args, "git")
    );

    git(
        &repo,
        ["bisect", "start", "--term-old=known", "--term-new=curious"],
    );
    for (name, rev) in [
        ("refs/bisect/curious-1", "HEAD~1"),
        ("refs/bisect/curious-3", "HEAD~3"),
        ("refs/bisect/known-3", "HEAD~3"),
        ("refs/bisect/curious-4", "HEAD~4"),
    ] {
        git(&repo, ["update-ref", name, rev]);
    }
    assert_eq!(
        command_output(zmin_bin(), &repo, &args, "zmin"),
        (
            0,
            "refs/bisect/curious-1\nrefs/bisect/curious-3\nrefs/bisect/curious-4\n^refs/bisect/known-3".to_owned(),
            String::new(),
        )
    );
}

#[test]
fn rev_parse_ref_selection_family_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    fs::write(repo.join("a.txt"), b"base\n").expect("write base");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "base"]);
    git(&repo, ["branch", "feature"]);
    git(&repo, ["tag", "v1"]);
    git(&repo, ["update-ref", "refs/remotes/origin/main", "HEAD"]);

    for args in [
        ["rev-parse", "--all"].as_slice(),
        ["rev-parse", "--exclude-hidden=fetch", "--all"].as_slice(),
        ["rev-parse", "--branches"].as_slice(),
        ["rev-parse", "--branches=main*"].as_slice(),
        ["rev-parse", "--tags"].as_slice(),
        ["rev-parse", "--remotes"].as_slice(),
        ["rev-parse", "--glob=refs/heads/main*"].as_slice(),
        ["rev-parse", "--exclude=feature", "--branches"].as_slice(),
        ["rev-parse", "--exclude=refs/heads/feature", "--all"].as_slice(),
        ["rev-parse", "--local-env-vars"].as_slice(),
        ["rev-parse", "--resolve-git-dir", ".git"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), &repo, args, "zmin"),
            command_output("git", &repo, args, "git"),
            "rev-parse ref selection mismatch for {args:?}"
        );
    }
}

#[test]
fn rev_parse_local_env_vars_and_resolve_git_dir_outside_repo_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    for args in [
        ["rev-parse", "--local-env-vars"].as_slice(),
        ["rev-parse", "--resolve-git-dir", "."].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), dir.path(), args, "zmin"),
            command_output("git", dir.path(), args, "git"),
            "rev-parse outside-repo mismatch for {args:?}"
        );
    }
}

#[test]
fn rev_parse_resolve_git_dir_resolves_separate_git_dir_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let worktree = dir.path().join("worktree");
    let git_dir = dir.path().join("git-dir");
    git(
        dir.path(),
        [
            "init",
            "--separate-git-dir",
            git_dir.to_str().expect("git dir"),
            worktree.to_str().expect("worktree"),
        ],
    );

    let args = ["rev-parse", "--resolve-git-dir", ".git"];
    assert_eq!(
        command_output(zmin_bin(), &worktree, &args, "zmin"),
        command_output("git", &worktree, &args, "git")
    );
}

#[test]
fn rev_parse_filter_prefix_and_symbolic_family_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    fs::write(repo.join("a.txt"), b"base\n").expect("write base");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "base"]);
    fs::create_dir_all(repo.join("sub")).expect("create subdir");
    fs::write(repo.join("sub/file.txt"), b"nested\n").expect("write nested");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "nested"]);

    for args in [
        ["rev-parse", "--flags", "HEAD", "--", "x", "-n"].as_slice(),
        ["rev-parse", "--no-flags", "HEAD", "--", "x", "-n"].as_slice(),
        ["rev-parse", "--revs-only", "HEAD", "--", "x", "-n"].as_slice(),
        ["rev-parse", "--no-revs", "HEAD", "--", "x", "-n"].as_slice(),
        ["rev-parse", "--default", "HEAD"].as_slice(),
        ["rev-parse", "--default", "HEAD", "--verify"].as_slice(),
        ["rev-parse", "--not", "HEAD", "^HEAD", "refs/heads/main"].as_slice(),
        ["rev-parse", "--symbolic", "HEAD", "refs/heads/main", "main"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), &repo, args, "zmin"),
            command_output("git", &repo, args, "git"),
            "rev-parse filter/symbolic mismatch for {args:?}"
        );
    }

    for args in [
        ["rev-parse", "--prefix", "sub/", "--", "a.txt", "../b.txt"].as_slice(),
        [
            "rev-parse",
            "--sq",
            "--prefix",
            "sub/",
            "--",
            "a.txt",
            "../b.txt",
        ]
        .as_slice(),
        ["rev-parse", "--prefix", "sub/", "HEAD:./file.txt"].as_slice(),
        ["rev-parse", "--prefix", "sub/", "HEAD:../a.txt"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), &repo.join("sub"), args, "zmin"),
            command_output("git", &repo.join("sub"), args, "git"),
            "rev-parse prefix/sq mismatch for {args:?}"
        );
    }
}

#[test]
fn rev_parse_sq_quote_mode_matches_stock_git_outside_repo() {
    let dir = TempDir::new().expect("temp dir");
    let args = ["rev-parse", "--sq-quote", "a b", "c'd"];
    assert_eq!(
        command_output(zmin_bin(), dir.path(), &args, "zmin"),
        command_output("git", dir.path(), &args, "git")
    );
}

#[test]
fn rev_parse_parseopt_and_output_modes_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    fs::write(repo.join("a.txt"), b"base\n").expect("write base");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "base"]);

    for args in [
        ["rev-parse", "--output-object-format=storage", "HEAD"].as_slice(),
        ["rev-parse", "--output-object-format=sha1", "HEAD"].as_slice(),
        ["rev-parse", "--shared-index-path"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), &repo, args, "zmin"),
            command_output("git", &repo, args, "git"),
            "rev-parse output mode mismatch for {args:?}"
        );
    }

    let head = git(&repo, ["rev-parse", "HEAD"]);
    let disambiguate = format!("--disambiguate={}", &head[..7]);
    let args = ["rev-parse", disambiguate.as_str()];
    assert_eq!(
        command_output(zmin_bin(), &repo, &args, "zmin"),
        command_output("git", &repo, &args, "git")
    );

    let spec = "cmd\n--\na,alpha= arg help\nb,beta help\n\n";
    let args = [
        "rev-parse",
        "--parseopt",
        "--keep-dashdash",
        "--stop-at-non-option",
        "--stuck-long",
        "--",
        "--alpha=1",
        "--",
        "--beta",
        "foo",
        "bar",
    ];
    assert_eq!(
        run_zmin_with_stdin_args(dir.path(), &args, spec),
        git_with_stdin_args(dir.path(), &args, spec)
    );

    let args = [
        "rev-parse",
        "--parseopt",
        "--",
        "--alpha=1",
        "--beta",
        "foo",
    ];
    assert_eq!(
        run_zmin_with_stdin_args(dir.path(), &args, spec),
        git_with_stdin_args(dir.path(), &args, spec)
    );
}

#[test]
fn rev_parse_output_object_format_sha256_matches_stock_git_error() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    fs::write(repo.join("a.txt"), b"base\n").expect("write base");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "base"]);

    let args = ["rev-parse", "--output-object-format=sha256", "HEAD"];
    assert_eq!(
        command_output(zmin_bin(), &repo, &args, "zmin"),
        command_output("git", &repo, &args, "git")
    );
}

#[test]
fn compatible_object_format_translation_matches_stock_git_for_core_object_kinds() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"base\n").expect("write base");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git_with_env(repo.path(), ["tag", "-a", "v1", "-m", "tag"]);
    git(repo.path(), ["config", "core.repositoryformatversion", "1"]);
    git(repo.path(), ["config", "extensions.objectformat", "sha1"]);
    git(
        repo.path(),
        ["config", "extensions.compatobjectformat", "sha256"],
    );

    let revisions = ["HEAD:a.txt", "HEAD^{tree}", "HEAD", "v1"];
    let mut compatible_ids = Vec::new();
    for revision in revisions {
        let native_id = git(repo.path(), ["rev-parse", revision]);
        let args = [
            "rev-parse",
            "--output-object-format=sha256",
            native_id.as_str(),
        ];
        let compatible_id = run_zmin(repo.path(), args);
        assert_eq!(
            compatible_id,
            git(repo.path(), args),
            "revision: {revision}"
        );
        for mode in ["-t", "-s", "-p"] {
            let cat_args = ["cat-file", mode, compatible_id.as_str()];
            if mode == "-p" {
                assert_eq!(
                    command_stdout_bytes(zmin_bin(), repo.path(), &cat_args),
                    command_stdout_bytes("git", repo.path(), &cat_args),
                    "revision: {revision}, mode: {mode}"
                );
            } else {
                assert_eq!(
                    command_output(zmin_bin(), repo.path(), &cat_args, "zmin"),
                    command_output("git", repo.path(), &cat_args, "git"),
                    "revision: {revision}, mode: {mode}"
                );
            }
        }
        compatible_ids.push(compatible_id);
    }

    let stdin = format!("{}\n", compatible_ids.join("\n"));
    for mode in ["--batch", "--batch-check=%(objecttype) %(objectsize)"] {
        assert_eq!(
            command_stdout_bytes_with_stdin(
                zmin_bin(),
                repo.path(),
                &["cat-file", mode],
                stdin.as_bytes(),
            ),
            command_stdout_bytes_with_stdin(
                "git",
                repo.path(),
                &["cat-file", mode],
                stdin.as_bytes(),
            ),
            "batch mode: {mode}"
        );
    }

    let mapping = fs::read_to_string(repo.path().join(".git/objects/loose-object-idx"))
        .expect("read object-format mapping");
    assert_eq!(mapping.lines().next(), Some("# loose-object-idx"));
    assert!(mapping.lines().skip(1).count() >= compatible_ids.len());
}

#[test]
fn global_config_option_overrides_runtime_config_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let zmin_repo = dir.path().join("zmin-repo");
    let git_repo = dir.path().join("git-repo");
    run_zmin(dir.path(), ["init", "-b", "main", "zmin-repo"]);
    git(
        dir.path(),
        ["init", "-b", "main", git_repo.to_str().expect("git repo")],
    );

    for args in [
        [
            "-c",
            "user.name=Inline Name",
            "config",
            "--get",
            "user.name",
        ]
        .as_slice(),
        ["-c", "demo.flag", "config", "--bool", "--get", "demo.flag"].as_slice(),
        [
            "-c",
            "demo.empty=",
            "config",
            "--bool",
            "--get",
            "demo.empty",
        ]
        .as_slice(),
        ["-c", "demo.empty=", "config", "--get", "demo.empty"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), &zmin_repo, args, "zmin"),
            command_output("git", &git_repo, args, "git"),
            "global -c mismatch for {args:?}"
        );
    }

    let config_env_args = [
        "--config-env=demo.value=ZMIN_TEST_CONFIG_ENV",
        "config",
        "--get",
        "demo.value",
    ];
    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            &zmin_repo,
            &config_env_args,
            &[("ZMIN_TEST_CONFIG_ENV", "from-env")],
            "zmin"
        ),
        command_output_with_env(
            "git",
            &git_repo,
            &config_env_args,
            &[("ZMIN_TEST_CONFIG_ENV", "from-env")],
            "git"
        )
    );
    let missing_config_env_args = [
        "--config-env=demo.value=ZMIN_TEST_CONFIG_ENV_MISSING",
        "config",
        "--get",
        "demo.value",
    ];
    assert_eq!(
        command_output(zmin_bin(), &zmin_repo, &missing_config_env_args, "zmin"),
        command_output("git", &git_repo, &missing_config_env_args, "git")
    );

    assert_eq!(
        command_output(
            zmin_bin(),
            &zmin_repo,
            &["-c", "bad", "config", "--get", "bad"],
            "zmin"
        ),
        command_output(
            "git",
            &git_repo,
            &["-c", "bad", "config", "--get", "bad"],
            "git"
        )
    );

    fs::write(zmin_repo.join("a.txt"), b"zmin\n").expect("write zmin file");
    fs::write(git_repo.join("a.txt"), b"git\n").expect("write git file");
    run_zmin(&zmin_repo, ["add", "-A"]);
    git(&git_repo, ["add", "-A"]);

    let commit_args = [
        "-c",
        "user.name=Inline Name",
        "-c",
        "user.email=inline@example.test",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-m",
        "inline identity",
    ];
    assert_eq!(
        command_output(zmin_bin(), &zmin_repo, &commit_args, "zmin").0,
        0
    );
    assert_eq!(command_output("git", &git_repo, &commit_args, "git").0, 0);
    assert_eq!(
        git(&zmin_repo, ["log", "-1", "--format=%an <%ae>"]),
        git(&git_repo, ["log", "-1", "--format=%an <%ae>"])
    );
    let local_config = fs::read_to_string(zmin_repo.join(".git/config")).expect("read config");
    assert!(!local_config.contains("Inline Name"));
    assert!(!local_config.contains("inline@example.test"));
}

#[test]
fn global_git_dir_and_work_tree_options_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    fs::write(repo.join("a.txt"), b"base\n").expect("write base");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "base"]);
    fs::write(repo.join("a.txt"), b"changed\n").expect("write changed");

    let git_dir = repo.join(".git");
    let git_dir_arg = format!("--git-dir={}", git_dir.display());
    let work_tree_arg = format!("--work-tree={}", repo.display());
    for args in [
        [
            git_dir_arg.as_str(),
            work_tree_arg.as_str(),
            "status",
            "--short",
        ]
        .as_slice(),
        [
            "--git-dir",
            git_dir.to_str().expect("git dir"),
            "--work-tree",
            repo.to_str().expect("work tree"),
            "rev-parse",
            "--show-toplevel",
        ]
        .as_slice(),
        [
            "--git-dir",
            git_dir.to_str().expect("git dir"),
            "rev-parse",
            "--git-dir",
        ]
        .as_slice(),
        [
            "-C",
            dir.path().to_str().expect("dir path"),
            "--git-dir",
            "repo/.git",
            "--work-tree",
            "repo",
            "status",
            "--short",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), dir.path(), args, "zmin"),
            command_output("git", dir.path(), args, "git"),
            "global git-dir/work-tree mismatch for {args:?}"
        );
    }
}

#[test]
fn global_c_order_independent_path_options_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("c").join("a");
    fs::create_dir_all(repo.parent().expect("repo parent")).expect("create repo parent");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    fs::write(repo.join("a.txt"), b"base\n").expect("write base");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "base"]);
    fs::write(repo.join("a.txt"), b"changed\n").expect("write changed");

    for args in [
        ["-C", "c", "--git-dir=a/.git", "rev-parse", "--git-dir"].as_slice(),
        ["--git-dir=a/.git", "-C", "c", "rev-parse", "--git-dir"].as_slice(),
        [
            "-C",
            "c",
            "--git-dir=a/.git",
            "--work-tree=a",
            "status",
            "--short",
        ]
        .as_slice(),
        [
            "--git-dir=a/.git",
            "-C",
            "c",
            "--work-tree=a",
            "status",
            "--short",
        ]
        .as_slice(),
        [
            "--git-dir=a/.git",
            "--work-tree=a",
            "-C",
            "c",
            "status",
            "--short",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), dir.path(), args, "zmin"),
            command_output("git", dir.path(), args, "git"),
            "global path option ordering mismatch for {args:?}"
        );
    }
}

#[test]
fn global_work_tree_option_from_git_dir_cwd_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let work_tree = dir.path().join("c").join("a");
    let git_dir = dir.path().join("c").join("a.git");
    fs::create_dir_all(&work_tree).expect("create work tree");
    fs::create_dir_all(git_dir.parent().expect("git dir parent")).expect("create git parent");
    git(
        dir.path(),
        ["init", "--bare", git_dir.to_str().expect("git dir path")],
    );
    fs::write(work_tree.join("a.txt"), b"base\n").expect("write base");
    for args in [
        [
            "--git-dir",
            git_dir.to_str().expect("git dir path"),
            "--work-tree",
            work_tree.to_str().expect("work tree path"),
            "config",
            "user.name",
            "Test User",
        ]
        .as_slice(),
        [
            "--git-dir",
            git_dir.to_str().expect("git dir path"),
            "--work-tree",
            work_tree.to_str().expect("work tree path"),
            "config",
            "user.email",
            "test@example.com",
        ]
        .as_slice(),
        [
            "--git-dir",
            git_dir.to_str().expect("git dir path"),
            "--work-tree",
            work_tree.to_str().expect("work tree path"),
            "add",
            "a.txt",
        ]
        .as_slice(),
        [
            "--git-dir",
            git_dir.to_str().expect("git dir path"),
            "--work-tree",
            work_tree.to_str().expect("work tree path"),
            "commit",
            "-m",
            "base",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_output("git", dir.path(), args, "git").0,
            0,
            "{args:?}"
        );
    }
    fs::remove_file(work_tree.join("a.txt")).expect("remove tracked file");

    for args in [
        ["-C", "c/a.git", "--work-tree=../a", "status"].as_slice(),
        ["--work-tree=../a", "-C", "c/a.git", "status"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), dir.path(), args, "zmin"),
            command_output("git", dir.path(), args, "git"),
            "global work-tree from git-dir cwd mismatch for {args:?}"
        );
    }
}

#[test]
fn global_add_path_resolution_from_explicit_git_dir_and_work_tree_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let work_tree = dir.path().join("c").join("a");
    let git_dir = dir.path().join("c").join("a.git");
    fs::create_dir_all(&work_tree).expect("create work tree");
    fs::create_dir_all(git_dir.parent().expect("git dir parent")).expect("create git parent");
    git(
        dir.path(),
        ["init", "--bare", git_dir.to_str().expect("git dir path")],
    );

    for args in [
        [
            "--git-dir",
            git_dir.to_str().expect("git dir path"),
            "--work-tree",
            work_tree.to_str().expect("work tree path"),
            "config",
            "user.name",
            "Test User",
        ]
        .as_slice(),
        [
            "--git-dir",
            git_dir.to_str().expect("git dir path"),
            "--work-tree",
            work_tree.to_str().expect("work tree path"),
            "config",
            "user.email",
            "test@example.com",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_output("git", dir.path(), args, "git").0,
            0,
            "{args:?}"
        );
    }

    fs::write(work_tree.join("a.txt"), b"base\n").expect("write base");
    for args in [
        ["--git-dir", "c/a.git", "--work-tree=c/a", "add", "a.txt"].as_slice(),
        [
            "-C",
            "c",
            "--git-dir=a.git",
            "--work-tree=a",
            "add",
            "a.txt",
        ]
        .as_slice(),
        [
            "--git-dir=a.git",
            "-C",
            "c",
            "--work-tree=a",
            "add",
            "a.txt",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), dir.path(), args, "zmin"),
            command_output("git", dir.path(), args, "git"),
            "global add path resolution mismatch for {args:?}"
        );
    }
}

#[test]
fn rev_parse_git_common_dir_env_affects_git_path_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    git(&repo, ["--git-dir=bar", "init", "--bare"]);
    fs::create_dir(repo.join("foo")).expect("create object override dir");

    for args in [
        ["rev-parse", "--git-path", "info/grafts"].as_slice(),
        ["rev-parse", "--git-path", "info/////grafts"].as_slice(),
        ["rev-parse", "--git-path", "index"].as_slice(),
        ["rev-parse", "--git-path", "objects"].as_slice(),
        ["rev-parse", "--git-path", "objects/foo"].as_slice(),
        ["rev-parse", "--git-path", "HEAD"].as_slice(),
        ["rev-parse", "--git-path", "logs/HEAD"].as_slice(),
        ["rev-parse", "--git-path", "logs/HEAD.lock"].as_slice(),
        ["rev-parse", "--git-path", "logs/refs/bisect/foo"].as_slice(),
        ["rev-parse", "--git-path", "logs/refs/"].as_slice(),
        ["rev-parse", "--git-path", "info/sparse-checkout"].as_slice(),
        ["rev-parse", "--git-path", "info//sparse-checkout"].as_slice(),
        ["rev-parse", "--git-path", "refs/bisect/foo"].as_slice(),
        ["rev-parse", "--git-path", "hooks/me"].as_slice(),
        ["rev-parse", "--git-path", "config"].as_slice(),
        ["rev-parse", "--git-path", "packed-refs"].as_slice(),
        ["rev-parse", "--git-path", "shallow"].as_slice(),
        ["rev-parse", "--git-path", "common"].as_slice(),
        ["rev-parse", "--git-path", "common/file"].as_slice(),
    ] {
        let env = [
            ("GIT_DIR", ".git"),
            ("GIT_COMMON_DIR", "bar"),
            ("GIT_GRAFT_FILE", "foo"),
            ("GIT_INDEX_FILE", "foo"),
            ("GIT_OBJECT_DIRECTORY", "foo"),
        ];
        assert_eq!(
            command_output_with_env(zmin_bin(), &repo, args, &env, "zmin"),
            command_output_with_env("git", &repo, args, &env, "git"),
            "GIT_COMMON_DIR git-path mismatch for {args:?}"
        );
    }
}

#[test]
fn init_honors_git_object_directory_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let zmin_root = dir.path().join("zmin");
    let git_root = dir.path().join("git");
    let zmin_objects = dir.path().join("zmin-objects");
    let git_objects = dir.path().join("git-objects");
    fs::create_dir(&zmin_root).expect("create zmin root");
    fs::create_dir(&git_root).expect("create git root");

    let zmin_output = command_output_with_env(
        zmin_bin(),
        &zmin_root,
        &["init", "-q", "-b", "main", "repo"],
        &[(
            "GIT_OBJECT_DIRECTORY",
            zmin_objects.to_str().expect("zmin objects path"),
        )],
        "zmin",
    );
    let git_output = command_output_with_env(
        "git",
        &git_root,
        &["init", "-q", "-b", "main", "repo"],
        &[(
            "GIT_OBJECT_DIRECTORY",
            git_objects.to_str().expect("git objects path"),
        )],
        "git",
    );

    assert_eq!(zmin_output, git_output);
    for objects in [&zmin_objects, &git_objects] {
        assert!(objects.join("info").is_dir());
        assert!(objects.join("pack").is_dir());
    }
    for root in [&zmin_root, &git_root] {
        assert!(!root.join("repo/.git/objects/info").exists());
        assert!(!root.join("repo/.git/objects/pack").exists());
    }

    for (command, root) in [(zmin_bin(), &zmin_root), ("git", &git_root)] {
        command_output_with_env(
            command,
            root,
            &["init", "-q", "-b", "main", "relative"],
            &[("GIT_OBJECT_DIRECTORY", "custom-odb")],
            command,
        );
        assert!(root.join("relative/custom-odb/info").is_dir());
        assert!(root.join("relative/custom-odb/pack").is_dir());
        assert!(!root.join("custom-odb").exists());
    }
}

#[test]
fn global_bare_option_matches_stock_git_ordering_and_discovery() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let bare = dir.path().join("repo.git");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    git(
        dir.path(),
        [
            "clone",
            "--bare",
            source.to_str().expect("source path"),
            bare.to_str().expect("bare path"),
        ],
    );

    for args in [
        ["--bare", "rev-parse", "--git-dir"].as_slice(),
        ["--bare", "rev-parse", "--is-bare-repository"].as_slice(),
        ["--bare", "rev-parse", "--is-inside-work-tree"].as_slice(),
        ["--bare", "rev-parse", "--show-prefix"].as_slice(),
        ["--bare", "rev-parse", "--show-cdup"].as_slice(),
        ["--bare", "rev-parse", "--absolute-git-dir"].as_slice(),
        ["--bare", "rev-parse", "--git-common-dir"].as_slice(),
        ["--bare", "rev-parse", "--git-path", "objects"].as_slice(),
        ["--bare", "rev-parse", "--is-inside-git-dir"].as_slice(),
        ["--bare", "rev-parse", "--is-shallow-repository"].as_slice(),
        ["--bare", "cat-file", "-t", "HEAD"].as_slice(),
        ["--bare", "show-ref", "--heads"].as_slice(),
        ["--bare", "status", "--short"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), &bare, args, "zmin"),
            command_output("git", &bare, args, "git"),
            "global --bare mismatch for {args:?}"
        );
    }

    for args in [
        ["-C", "repo.git", "--bare", "rev-parse", "--git-dir"].as_slice(),
        ["--bare", "-C", "repo.git", "rev-parse", "--git-dir"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), dir.path(), args, "zmin"),
            command_output("git", dir.path(), args, "git"),
            "global --bare ordering mismatch for {args:?}"
        );
    }
}

#[test]
fn global_noop_options_match_stock_git_in_noninteractive_mode() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    fs::write(repo.join("a.txt"), b"base\n").expect("write base");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "base"]);
    fs::write(repo.join("a.txt"), b"changed\n").expect("write changed");

    for option in [
        "-P",
        "--no-pager",
        "-p",
        "--paginate",
        "--no-replace-objects",
        "--no-lazy-fetch",
        "--no-optional-locks",
        "--no-advice",
    ] {
        let args = [option, "status", "--short"];
        assert_eq!(
            command_output(zmin_bin(), &repo, &args, "zmin"),
            command_output("git", &repo, &args, "git"),
            "global no-op mismatch for {option}"
        );
    }
}

#[test]
fn global_pathspec_options_match_stock_git_for_ls_files() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    let literal_glob_path = literal_glob_fixture_path();
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    fs::write(repo.join(literal_glob_path), b"literal\n").expect("write literal glob file");
    fs::write(repo.join("ab.txt"), b"glob\n").expect("write glob file");
    fs::write(repo.join("abc.txt"), b"icase\n").expect("write icase file");
    fs::create_dir_all(repo.join("dir")).expect("create dir");
    fs::write(repo.join("dir/aXb.txt"), b"nested\n").expect("write nested file");
    fs::create_dir_all(repo.join("dir/sub")).expect("create nested dir");
    fs::write(repo.join("dir/sub/a.txt"), b"deep\n").expect("write deep file");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "pathspec base"]);
    let literal_magic_path = format!(":(literal){literal_glob_path}");

    for args in [
        ["ls-files", "a*b.txt"].as_slice(),
        ["--literal-pathspecs", "ls-files", literal_glob_path].as_slice(),
        ["--glob-pathspecs", "ls-files", "a*b.txt"].as_slice(),
        ["--noglob-pathspecs", "ls-files", literal_glob_path].as_slice(),
        ["ls-files", "*.txt"].as_slice(),
        ["ls-files", literal_magic_path.as_str()].as_slice(),
        ["ls-files", ":(glob)a*b.txt"].as_slice(),
        ["ls-files", ":(icase)ABC.TXT"].as_slice(),
        ["ls-files", "a[bc]*.txt"].as_slice(),
        ["ls-files", "dir/*.txt"].as_slice(),
        ["ls-files", ":(glob)dir/*.txt"].as_slice(),
        ["ls-files", "*.txt", ":(exclude)ab.txt"].as_slice(),
        ["ls-files", "*.txt", ":!ab.txt"].as_slice(),
        [
            "--literal-pathspecs",
            "--noglob-pathspecs",
            "ls-files",
            literal_glob_path,
        ]
        .as_slice(),
        [
            "--noglob-pathspecs",
            "--icase-pathspecs",
            "ls-files",
            "ABC.TXT",
        ]
        .as_slice(),
        ["--icase-pathspecs", "ls-files", "ABC.TXT"].as_slice(),
        [
            "--icase-pathspecs",
            "--literal-pathspecs",
            "ls-files",
            "ABC.TXT",
        ]
        .as_slice(),
        [
            "--literal-pathspecs",
            "--icase-pathspecs",
            "ls-files",
            "ABC.TXT",
        ]
        .as_slice(),
        [
            "--literal-pathspecs",
            "--glob-pathspecs",
            "ls-files",
            literal_glob_path,
        ]
        .as_slice(),
        [
            "--glob-pathspecs",
            "--literal-pathspecs",
            "ls-files",
            literal_glob_path,
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), &repo, args, "zmin"),
            command_output("git", &repo, args, "git"),
            "global pathspec mismatch for {args:?}"
        );
    }
}

#[test]
fn global_pathspec_options_match_stock_git_for_mutating_commands() {
    let dir = TempDir::new().expect("temp dir");
    let zmin_repo = dir.path().join("zmin-repo");
    let git_repo = dir.path().join("git-repo");
    let literal_glob_path = literal_glob_fixture_path();
    for repo in [&zmin_repo, &git_repo] {
        git(
            dir.path(),
            ["init", "-b", "main", repo.to_str().expect("repo path")],
        );
        configure_identity(repo);
        fs::write(repo.join(literal_glob_path), b"literal\n").expect("write literal");
        fs::write(repo.join("ab.txt"), b"glob\n").expect("write glob");
        fs::create_dir_all(repo.join("dir")).expect("create dir");
        fs::write(repo.join("dir/a"), b"a\n").expect("write dir a");
        fs::write(repo.join("dir/b"), b"b\n").expect("write dir b");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "pathspec base"]);
    }

    for repo in [&zmin_repo, &git_repo] {
        fs::write(repo.join(literal_glob_path), b"literal changed\n").expect("change literal");
        fs::write(repo.join("ab.txt"), b"glob changed\n").expect("change glob");
    }
    assert_eq!(
        command_output(zmin_bin(), &zmin_repo, &["add", "-u", "a*.txt"], "zmin"),
        command_output("git", &git_repo, &["add", "-u", "a*.txt"], "git")
    );
    assert_eq!(
        run_zmin(&zmin_repo, ["status", "--porcelain=v1"]),
        git(&git_repo, ["status", "--porcelain=v1"])
    );

    let literal_zmin_repo = dir.path().join("literal-zmin-repo");
    let literal_git_repo = dir.path().join("literal-git-repo");
    for repo in [&literal_zmin_repo, &literal_git_repo] {
        git(
            dir.path(),
            ["init", "-b", "main", repo.to_str().expect("repo path")],
        );
        configure_identity(repo);
        fs::write(repo.join(literal_glob_path), b"literal\n").expect("write literal");
        fs::write(repo.join("ab.txt"), b"glob\n").expect("write glob");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "literal pathspec base"]);
        fs::write(repo.join(literal_glob_path), b"literal changed\n").expect("change literal");
        fs::write(repo.join("ab.txt"), b"glob changed\n").expect("change glob");
    }
    assert_eq!(
        command_output(
            zmin_bin(),
            &literal_zmin_repo,
            &["--literal-pathspecs", "add", "-u", literal_glob_path],
            "zmin"
        ),
        command_output(
            "git",
            &literal_git_repo,
            &["--literal-pathspecs", "add", "-u", literal_glob_path],
            "git"
        )
    );
    assert_eq!(
        run_zmin(&literal_zmin_repo, ["status", "--porcelain=v1"]),
        git(&literal_git_repo, ["status", "--porcelain=v1"])
    );
    assert_eq!(
        run_zmin(&literal_zmin_repo, ["diff", "--cached", "--name-only"]),
        literal_glob_path
    );

    let magic_zmin_repo = dir.path().join("magic-zmin-repo");
    let magic_git_repo = dir.path().join("magic-git-repo");
    for repo in [&magic_zmin_repo, &magic_git_repo] {
        git(
            dir.path(),
            ["init", "-b", "main", repo.to_str().expect("repo path")],
        );
        configure_identity(repo);
        fs::write(repo.join(literal_glob_path), b"literal\n").expect("write literal");
        fs::write(repo.join("ab.txt"), b"glob\n").expect("write glob");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "magic pathspec base"]);
        fs::write(repo.join(literal_glob_path), b"literal changed\n").expect("change literal");
        fs::write(repo.join("ab.txt"), b"glob changed\n").expect("change glob");
    }
    assert_eq!(
        command_output(
            zmin_bin(),
            &magic_zmin_repo,
            &["add", "-u", ":(glob)a*b.txt"],
            "zmin"
        ),
        command_output(
            "git",
            &magic_git_repo,
            &["add", "-u", ":(glob)a*b.txt"],
            "git"
        )
    );
    assert_eq!(
        run_zmin(&magic_zmin_repo, ["status", "--porcelain=v1"]),
        git(&magic_git_repo, ["status", "--porcelain=v1"])
    );

    assert_eq!(
        command_output(zmin_bin(), &zmin_repo, &["rm", "--cached", "dir/*"], "zmin"),
        command_output("git", &git_repo, &["rm", "--cached", "dir/*"], "git")
    );
    assert_eq!(
        run_zmin(&zmin_repo, ["status", "--porcelain=v1"]),
        git(&git_repo, ["status", "--porcelain=v1"])
    );
}

fn literal_glob_fixture_path() -> &'static str {
    if cfg!(windows) { "a[b].txt" } else { "a*b.txt" }
}

#[test]
fn config_rejects_unsupported_repository_format_like_stock_git() {
    let repo = git_init();
    git(
        repo.path(),
        [
            "config",
            "--file=.git/config",
            "core.repositoryformatversion",
            "99",
        ],
    );
    assert_eq!(
        command_output(
            zmin_bin(),
            repo.path(),
            &["config", "core.repositoryformatversion"],
            "zmin config",
        ),
        command_output(
            "git",
            repo.path(),
            &["config", "core.repositoryformatversion"],
            "git config",
        )
    );
}
