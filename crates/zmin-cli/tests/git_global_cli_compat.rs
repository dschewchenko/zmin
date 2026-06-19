mod common;

use std::fs;
use std::io::Read;
use std::process::{Command, Stdio};

use common::{
    command_any_output as command_output, command_output_with_env, configure_identity, git,
    git_init, git_with_env, run_zmin, run_zmin_with_env, zmin_bin,
};
use tempfile::TempDir;

#[test]
fn broken_stdout_pipe_exits_successfully_without_panic() {
    let dir = TempDir::new().expect("temp dir");
    let mut child = Command::new(zmin_bin())
        .arg("--help")
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zmin");
    drop(child.stdout.take().expect("stdout pipe"));
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr pipe")
        .read_to_string(&mut stderr)
        .expect("read stderr");
    let status = child.wait().expect("wait zmin");
    assert_eq!(status.code(), Some(0), "stderr: {stderr}");
    assert!(
        !stderr.contains("panicked"),
        "broken pipe should not print a panic: {stderr}"
    );
}

#[test]
fn no_arguments_exits_one_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let output = Command::new(zmin_bin())
        .current_dir(dir.path())
        .output()
        .expect("run zmin without arguments");

    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Usage:"),
        "stdout should contain usage help: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty like stock Git: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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
    assert!(stdout.starts_with("git version 2.36.0 "), "{stdout}");
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
        command_output(
            "git",
            &zmin_repo,
            &["config", "--bool", "core.bare"],
            "git"
        ),
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
        [
            "init",
            "-b",
            "main",
            zmin_repo.to_str().expect("zmin path"),
        ],
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
        zmin
            .2
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
        command_output(
            zmin_bin(),
            &zmin_repo,
            &["rm", "--cached", "dir/*"],
            "zmin"
        ),
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
