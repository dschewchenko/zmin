mod common;

use std::fs;
use std::io::Read;
use std::process::{Command, Stdio};

use common::{
    command_any_output as command_output, command_output_with_env, configure_identity, git,
    git_with_env, run_skron, run_skron_with_env, skron_bin,
};
use tempfile::TempDir;

#[test]
fn broken_stdout_pipe_exits_successfully_without_panic() {
    let dir = TempDir::new().expect("temp dir");
    let mut child = Command::new(skron_bin())
        .arg("--help")
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn skron");
    drop(child.stdout.take().expect("stdout pipe"));
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr pipe")
        .read_to_string(&mut stderr)
        .expect("read stderr");
    let status = child.wait().expect("wait skron");
    assert_eq!(status.code(), Some(0), "stderr: {stderr}");
    assert!(
        !stderr.contains("panicked"),
        "broken pipe should not print a panic: {stderr}"
    );
}

#[test]
fn init_creates_stock_git_readable_repository() {
    let dir = TempDir::new().expect("temp dir");
    run_skron(dir.path(), ["init", "-b", "trunk", "repo"]);

    let repo = dir.path().join("repo");
    assert_eq!(git(&repo, ["rev-parse", "--git-dir"]), ".git");
    assert_eq!(
        run_skron(&repo, ["rev-parse", "--git-dir"]),
        git(&repo, ["rev-parse", "--git-dir"])
    );
    assert_eq!(
        run_skron(&repo, ["rev-parse", "--show-toplevel"]),
        git(&repo, ["rev-parse", "--show-toplevel"])
    );
    assert_eq!(
        run_skron(&repo, ["rev-parse", "--is-inside-work-tree"]),
        git(&repo, ["rev-parse", "--is-inside-work-tree"])
    );
    assert_eq!(
        run_skron(&repo, ["rev-parse", "--is-bare-repository"]),
        git(&repo, ["rev-parse", "--is-bare-repository"])
    );
    fs::create_dir_all(repo.join("nested/dir")).expect("create nested dir");
    assert_eq!(
        run_skron(&repo.join("nested/dir"), ["rev-parse", "--git-dir"]),
        git(&repo.join("nested/dir"), ["rev-parse", "--git-dir"])
    );
    assert_eq!(
        run_skron(&repo.join("nested/dir"), ["rev-parse", "--show-prefix"]),
        git(&repo.join("nested/dir"), ["rev-parse", "--show-prefix"])
    );
    assert_eq!(
        run_skron(&repo.join("nested/dir"), ["rev-parse", "--show-cdup"]),
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
            command_output(skron_bin(), &repo.join("nested/dir"), args, "skron"),
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
            command_output(skron_bin(), &repo.join(".git"), args, "skron"),
            command_output("git", &repo.join(".git"), args, "git"),
            "rev-parse git-dir cwd mismatch for {args:?}"
        );
    }
    assert_eq!(git(&repo, ["symbolic-ref", "HEAD"]), "refs/heads/trunk");
    assert_eq!(git(&repo, ["config", "--get", "core.bare"]), "false");
}

#[test]
fn checkout_dotfile_path_does_not_fail_branch_ref_validation() {
    let dir = TempDir::new().expect("temp dir");
    let git_repo = dir.path().join("git-repo");
    let skron_repo = dir.path().join("skron-repo");
    git(
        dir.path(),
        ["init", "-b", "main", git_repo.to_str().expect("git path")],
    );
    run_skron(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            skron_repo.to_str().expect("skron path"),
        ],
    );
    configure_identity(&git_repo);
    configure_identity(&skron_repo);
    fs::write(git_repo.join(".changelog.yml"), b"base\n").expect("write git dotfile");
    fs::write(skron_repo.join(".changelog.yml"), b"base\n").expect("write skron dotfile");
    git(&git_repo, ["add", "-A"]);
    run_skron(&skron_repo, ["add", "-A"]);
    git_with_env(&git_repo, ["commit", "-m", "base"]);
    run_skron_with_env(&skron_repo, ["commit", "-m", "base"]);

    fs::write(git_repo.join(".changelog.yml"), b"dirty\n").expect("dirty git dotfile");
    fs::write(skron_repo.join(".changelog.yml"), b"dirty\n").expect("dirty skron dotfile");
    assert_eq!(
        command_output(
            skron_bin(),
            &skron_repo,
            &["checkout", ".changelog.yml"],
            "skron"
        ),
        command_output("git", &git_repo, &["checkout", ".changelog.yml"], "git")
    );
    assert_eq!(
        fs::read_to_string(skron_repo.join(".changelog.yml")).expect("read skron dotfile"),
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
            command_output(skron_bin(), dir.path(), args, "skron"),
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
            command_output(skron_bin(), &repo, args, "skron"),
            command_output("git", &repo, args, "git"),
            "rev-parse mixed output mismatch for {args:?}"
        );
    }
}

#[test]
fn global_config_option_overrides_runtime_config_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let skron_repo = dir.path().join("skron-repo");
    let git_repo = dir.path().join("git-repo");
    run_skron(dir.path(), ["init", "-b", "main", "skron-repo"]);
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
            command_output(skron_bin(), &skron_repo, args, "skron"),
            command_output("git", &git_repo, args, "git"),
            "global -c mismatch for {args:?}"
        );
    }

    let config_env_args = [
        "--config-env=demo.value=SKRON_TEST_CONFIG_ENV",
        "config",
        "--get",
        "demo.value",
    ];
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            &skron_repo,
            &config_env_args,
            &[("SKRON_TEST_CONFIG_ENV", "from-env")],
            "skron"
        ),
        command_output_with_env(
            "git",
            &git_repo,
            &config_env_args,
            &[("SKRON_TEST_CONFIG_ENV", "from-env")],
            "git"
        )
    );
    let missing_config_env_args = [
        "--config-env=demo.value=SKRON_TEST_CONFIG_ENV_MISSING",
        "config",
        "--get",
        "demo.value",
    ];
    assert_eq!(
        command_output(skron_bin(), &skron_repo, &missing_config_env_args, "skron"),
        command_output("git", &git_repo, &missing_config_env_args, "git")
    );

    assert_eq!(
        command_output(
            skron_bin(),
            &skron_repo,
            &["-c", "bad", "config", "--get", "bad"],
            "skron"
        ),
        command_output(
            "git",
            &git_repo,
            &["-c", "bad", "config", "--get", "bad"],
            "git"
        )
    );

    fs::write(skron_repo.join("a.txt"), b"skron\n").expect("write skron file");
    fs::write(git_repo.join("a.txt"), b"git\n").expect("write git file");
    run_skron(&skron_repo, ["add", "-A"]);
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
        command_output(skron_bin(), &skron_repo, &commit_args, "skron").0,
        0
    );
    assert_eq!(command_output("git", &git_repo, &commit_args, "git").0, 0);
    assert_eq!(
        git(&skron_repo, ["log", "-1", "--format=%an <%ae>"]),
        git(&git_repo, ["log", "-1", "--format=%an <%ae>"])
    );
    let local_config = fs::read_to_string(skron_repo.join(".git/config")).expect("read config");
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
            command_output(skron_bin(), dir.path(), args, "skron"),
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
            command_output(skron_bin(), &bare, args, "skron"),
            command_output("git", &bare, args, "git"),
            "global --bare mismatch for {args:?}"
        );
    }

    for args in [
        ["-C", "repo.git", "--bare", "rev-parse", "--git-dir"].as_slice(),
        ["--bare", "-C", "repo.git", "rev-parse", "--git-dir"].as_slice(),
    ] {
        assert_eq!(
            command_output(skron_bin(), dir.path(), args, "skron"),
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
            command_output(skron_bin(), &repo, &args, "skron"),
            command_output("git", &repo, &args, "git"),
            "global no-op mismatch for {option}"
        );
    }
}

#[test]
fn global_pathspec_options_match_stock_git_for_ls_files() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    fs::write(repo.join("a*b.txt"), b"literal\n").expect("write literal glob file");
    fs::write(repo.join("ab.txt"), b"glob\n").expect("write glob file");
    fs::write(repo.join("abc.txt"), b"icase\n").expect("write icase file");
    fs::create_dir_all(repo.join("dir")).expect("create dir");
    fs::write(repo.join("dir/aXb.txt"), b"nested\n").expect("write nested file");
    fs::create_dir_all(repo.join("dir/sub")).expect("create nested dir");
    fs::write(repo.join("dir/sub/a.txt"), b"deep\n").expect("write deep file");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "pathspec base"]);

    for args in [
        ["ls-files", "a*b.txt"].as_slice(),
        ["--literal-pathspecs", "ls-files", "a*b.txt"].as_slice(),
        ["--glob-pathspecs", "ls-files", "a*b.txt"].as_slice(),
        ["--noglob-pathspecs", "ls-files", "a*b.txt"].as_slice(),
        ["ls-files", "*.txt"].as_slice(),
        ["ls-files", ":(literal)a*b.txt"].as_slice(),
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
            "a*b.txt",
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
            "a*b.txt",
        ]
        .as_slice(),
        [
            "--glob-pathspecs",
            "--literal-pathspecs",
            "ls-files",
            "a*b.txt",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_output(skron_bin(), &repo, args, "skron"),
            command_output("git", &repo, args, "git"),
            "global pathspec mismatch for {args:?}"
        );
    }
}

#[test]
fn global_pathspec_options_match_stock_git_for_mutating_commands() {
    let dir = TempDir::new().expect("temp dir");
    let skron_repo = dir.path().join("skron-repo");
    let git_repo = dir.path().join("git-repo");
    for repo in [&skron_repo, &git_repo] {
        git(
            dir.path(),
            ["init", "-b", "main", repo.to_str().expect("repo path")],
        );
        configure_identity(repo);
        fs::write(repo.join("a*b.txt"), b"literal\n").expect("write literal");
        fs::write(repo.join("ab.txt"), b"glob\n").expect("write glob");
        fs::create_dir_all(repo.join("dir")).expect("create dir");
        fs::write(repo.join("dir/a"), b"a\n").expect("write dir a");
        fs::write(repo.join("dir/b"), b"b\n").expect("write dir b");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "pathspec base"]);
    }

    for repo in [&skron_repo, &git_repo] {
        fs::write(repo.join("a*b.txt"), b"literal changed\n").expect("change literal");
        fs::write(repo.join("ab.txt"), b"glob changed\n").expect("change glob");
    }
    assert_eq!(
        command_output(skron_bin(), &skron_repo, &["add", "-u", "a*.txt"], "skron"),
        command_output("git", &git_repo, &["add", "-u", "a*.txt"], "git")
    );
    assert_eq!(
        run_skron(&skron_repo, ["status", "--porcelain=v1"]),
        git(&git_repo, ["status", "--porcelain=v1"])
    );

    let literal_skron_repo = dir.path().join("literal-skron-repo");
    let literal_git_repo = dir.path().join("literal-git-repo");
    for repo in [&literal_skron_repo, &literal_git_repo] {
        git(
            dir.path(),
            ["init", "-b", "main", repo.to_str().expect("repo path")],
        );
        configure_identity(repo);
        fs::write(repo.join("a*b.txt"), b"literal\n").expect("write literal");
        fs::write(repo.join("ab.txt"), b"glob\n").expect("write glob");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "literal pathspec base"]);
        fs::write(repo.join("a*b.txt"), b"literal changed\n").expect("change literal");
        fs::write(repo.join("ab.txt"), b"glob changed\n").expect("change glob");
    }
    assert_eq!(
        command_output(
            skron_bin(),
            &literal_skron_repo,
            &["--literal-pathspecs", "add", "-u", "a*b.txt"],
            "skron"
        ),
        command_output(
            "git",
            &literal_git_repo,
            &["--literal-pathspecs", "add", "-u", "a*b.txt"],
            "git"
        )
    );
    assert_eq!(
        run_skron(&literal_skron_repo, ["status", "--porcelain=v1"]),
        git(&literal_git_repo, ["status", "--porcelain=v1"])
    );
    assert_eq!(
        run_skron(&literal_skron_repo, ["diff", "--cached", "--name-only"]),
        "a*b.txt"
    );

    let magic_skron_repo = dir.path().join("magic-skron-repo");
    let magic_git_repo = dir.path().join("magic-git-repo");
    for repo in [&magic_skron_repo, &magic_git_repo] {
        git(
            dir.path(),
            ["init", "-b", "main", repo.to_str().expect("repo path")],
        );
        configure_identity(repo);
        fs::write(repo.join("a*b.txt"), b"literal\n").expect("write literal");
        fs::write(repo.join("ab.txt"), b"glob\n").expect("write glob");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "magic pathspec base"]);
        fs::write(repo.join("a*b.txt"), b"literal changed\n").expect("change literal");
        fs::write(repo.join("ab.txt"), b"glob changed\n").expect("change glob");
    }
    assert_eq!(
        command_output(
            skron_bin(),
            &magic_skron_repo,
            &["add", "-u", ":(glob)a*b.txt"],
            "skron"
        ),
        command_output(
            "git",
            &magic_git_repo,
            &["add", "-u", ":(glob)a*b.txt"],
            "git"
        )
    );
    assert_eq!(
        run_skron(&magic_skron_repo, ["status", "--porcelain=v1"]),
        git(&magic_git_repo, ["status", "--porcelain=v1"])
    );

    assert_eq!(
        command_output(
            skron_bin(),
            &skron_repo,
            &["rm", "--cached", "dir/*"],
            "skron"
        ),
        command_output("git", &git_repo, &["rm", "--cached", "dir/*"], "git")
    );
    assert_eq!(
        run_skron(&skron_repo, ["status", "--porcelain=v1"]),
        git(&git_repo, ["status", "--porcelain=v1"])
    );
}
