mod common;

use std::fs;

use common::{
    command_any_output, command_stdout_bytes_with_stdin, configure_identity, git,
    git_failure_output, git_status, git_status_with_stdin, git_with_env, git_with_stdin, run_zmin,
    run_zmin_failure_output, run_zmin_status, run_zmin_status_with_stdin, run_zmin_with_stdin,
    zmin_bin,
};

#[test]
fn check_ignore_matches_stock_git_for_common_modes() {
    let repo = common::git_init();
    configure_identity(repo.path());
    fs::write(
        repo.path().join(".gitignore"),
        b"target/\n*.log\n/build/*.tmp\n/dist\n",
    )
    .expect("write gitignore");
    fs::create_dir_all(repo.path().join("target")).expect("create target");
    fs::create_dir_all(repo.path().join("build")).expect("create build");
    fs::create_dir_all(repo.path().join("src")).expect("create src");
    fs::write(repo.path().join("target/a"), b"ignored\n").expect("write target");
    fs::write(repo.path().join("src/debug.log"), b"ignored\n").expect("write log");
    fs::write(repo.path().join("build/cache.tmp"), b"ignored\n").expect("write tmp");
    fs::write(repo.path().join("keep.txt"), b"keep\n").expect("write keep");

    assert_eq!(
        run_zmin(
            repo.path(),
            ["check-ignore", "target/a", "src/debug.log", "keep.txt"]
        ),
        git(
            repo.path(),
            ["check-ignore", "target/a", "src/debug.log", "keep.txt"]
        )
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            [
                "check-ignore",
                "-v",
                "target/a",
                "src/debug.log",
                "keep.txt"
            ]
        ),
        git(
            repo.path(),
            [
                "check-ignore",
                "-v",
                "target/a",
                "src/debug.log",
                "keep.txt"
            ]
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["check-ignore", "--stdin"],
            "target/a\nkeep.txt\nsrc/debug.log\n"
        ),
        git_with_stdin(
            repo.path(),
            ["check-ignore", "--stdin"],
            "target/a\nkeep.txt\nsrc/debug.log\n"
        )
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["check-ignore", "-q", "target/a"]),
        git_status(repo.path(), ["check-ignore", "-q", "target/a"])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["check-ignore", "keep.txt"]),
        git_status(repo.path(), ["check-ignore", "keep.txt"])
    );

    fs::write(repo.path().join("tracked.log"), b"tracked\n").expect("write tracked");
    git(repo.path(), ["add", "-f", "tracked.log"]);
    git_with_env(repo.path(), ["commit", "-m", "tracked"]);
    assert_eq!(
        run_zmin(repo.path(), ["check-ignore", "--no-index", "tracked.log"]),
        git(repo.path(), ["check-ignore", "--no-index", "tracked.log"])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["check-ignore", "tracked.log"]),
        git_status(repo.path(), ["check-ignore", "tracked.log"])
    );
}

#[test]
fn check_ignore_matches_stock_git_for_upstream_t0008_cli_contract() {
    let repo = common::git_init();
    fs::write(repo.path().join(".gitignore"), b"ignored-*\n").expect("write gitignore");
    fs::write(repo.path().join("ignored-file"), b"ignored\n").expect("write ignored");
    fs::write(repo.path().join("kept"), b"kept\n").expect("write kept");

    assert_eq!(
        run_zmin_status_with_stdin(repo.path(), ["check-ignore", "--stdin"], ""),
        git_status_with_stdin(repo.path(), ["check-ignore", "--stdin"], "")
    );
    assert_eq!(
        run_zmin_status_with_stdin(
            repo.path(),
            ["check-ignore", "-q", "--stdin"],
            "ignored-file\nkept\n"
        ),
        git_status_with_stdin(
            repo.path(),
            ["check-ignore", "-q", "--stdin"],
            "ignored-file\nkept\n"
        )
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["check-ignore", "-v", "-n", "kept"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["check-ignore", "-v", "-n", "kept"],
            "git"
        )
    );
    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["check-ignore", "."], "zmin"),
        command_any_output("git", repo.path(), &["check-ignore", "."], "git")
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["check-ignore", "-v", "-n", "."],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["check-ignore", "-v", "-n", "."],
            "git"
        )
    );
    assert_eq!(
        command_stdout_bytes_with_stdin(
            zmin_bin(),
            repo.path(),
            &["check-ignore", "-z", "--stdin"],
            b"ignored-file\0",
        ),
        command_stdout_bytes_with_stdin(
            "git",
            repo.path(),
            &["check-ignore", "-z", "--stdin"],
            b"ignored-file\0",
        )
    );

    for args in [
        &["check-ignore"][..],
        &["check-ignore", "-q"][..],
        &["check-ignore", "-q", "-v", "kept"][..],
        &["check-ignore", "-q", "-v", "-q", "-v", "kept"][..],
        &["check-ignore", "--quiet", "one", "two"][..],
        &[
            "check-ignore",
            "--quiet",
            "--verbose",
            "--quiet",
            "one",
            "two",
        ][..],
        &["check-ignore", "--"][..],
        &["check-ignore", "--stdin", "kept"][..],
        &["check-ignore", "-z", "kept"][..],
    ] {
        let zmin = run_zmin_failure_output(repo.path(), args);
        let git = git_failure_output(repo.path(), args);
        assert_eq!(zmin.0, git.0, "status mismatch for {args:?}");
        assert_eq!(zmin.1, git.1, "stdout mismatch for {args:?}");
        assert_eq!(zmin.2, git.2, "stderr mismatch for {args:?}");
    }

    assert_eq!(
        run_zmin_failure_output(&repo.path().join(".git"), &["check-ignore", "kept"]),
        git_failure_output(&repo.path().join(".git"), &["check-ignore", "kept"])
    );
}
