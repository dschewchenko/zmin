mod common;

use common::{
    command_any_output, command_output_with_env, configure_identity, git, git_args, git_init,
    git_status, git_with_env, run_zmin_args, run_zmin_status, run_zmin_with_env, zmin_bin,
};

#[test]
fn reflog_show_list_and_exists_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "one"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "two"]);

    for args in [
        ["reflog"].as_slice(),
        ["reflog", "show"].as_slice(),
        ["reflog", "--date=iso"].as_slice(),
        ["reflog", "--date=unix"].as_slice(),
        ["reflog", "--date=raw"].as_slice(),
        ["reflog", "show", "main"].as_slice(),
        ["reflog", "show", "refs/heads/main", "--format=%H"].as_slice(),
        ["reflog", "list"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    assert_eq!(
        run_zmin_status(repo.path(), ["reflog", "exists", "HEAD"]),
        git_status(repo.path(), ["reflog", "exists", "HEAD"])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["reflog", "exists", "refs/heads/missing"]),
        git_status(repo.path(), ["reflog", "exists", "refs/heads/missing"])
    );
}

#[test]
fn reflog_subcommand_help_matches_stock_git_exit_shape() {
    let repo = git_init();

    let (status, stdout, stderr) = command_any_output(
        zmin_bin(),
        repo.path(),
        &["reflog", "expire", "-h"],
        "zmin",
    );
    assert_eq!(status, 129);
    assert!(stdout.contains("git reflog expire"), "{stdout}");
    assert!(stderr.is_empty(), "{stderr}");

    let (status, stdout, stderr) =
        command_any_output(zmin_bin(), repo.path(), &["reflog", "show", "-h"], "zmin");
    assert_eq!(status, 129);
    assert!(stdout.contains("git reflog [show]"), "{stdout}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn reflog_show_passes_pathspec_after_double_dash() {
    let repo = git_init();
    configure_identity(repo.path());
    common::write_file(repo.path(), "--a-file", "contents\n");
    git(repo.path(), ["add", "--", "--a-file"]);
    git_with_env(repo.path(), ["commit", "-m", "message"]);

    assert_eq!(
        run_zmin_args(repo.path(), &["reflog", "show", "--", "--does-not-exist"]),
        git_args(repo.path(), &["reflog", "show", "--", "--does-not-exist"])
    );
    assert_eq!(
        run_zmin_args(repo.path(), &["reflog", "show", "--", "--a-file"]),
        git_args(repo.path(), &["reflog", "show", "--", "--a-file"])
    );
}

#[test]
fn reset_hard_records_branch_and_head_reflog() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "one"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "two"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "three"]);

    let git_repo = tempfile::TempDir::new().expect("git repo");
    git(git_repo.path(), ["init"]);
    configure_identity(git_repo.path());
    git(git_repo.path(), ["checkout", "-b", "main"]);
    git_with_env(git_repo.path(), ["commit", "--allow-empty", "-m", "one"]);
    git_with_env(git_repo.path(), ["commit", "--allow-empty", "-m", "two"]);
    git_with_env(git_repo.path(), ["commit", "--allow-empty", "-m", "three"]);

    run_zmin_args(repo.path(), &["reset", "--hard", "HEAD~1"]);
    git(git_repo.path(), ["reset", "--hard", "HEAD~1"]);

    assert_eq!(
        run_zmin_args(repo.path(), &["reflog", "show", "main"]),
        git_args(git_repo.path(), &["reflog", "show", "main"])
    );
    assert_eq!(
        run_zmin_args(repo.path(), &["reflog", "show", "HEAD"]),
        git_args(git_repo.path(), &["reflog", "show", "HEAD"])
    );
}

#[test]
fn commit_records_branch_and_head_reflog() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    run_zmin_with_env(repo.path(), ["commit", "--allow-empty", "-m", "one"]);
    run_zmin_with_env(repo.path(), ["commit", "--allow-empty", "-m", "two"]);

    let git_repo = tempfile::TempDir::new().expect("git repo");
    git(git_repo.path(), ["init"]);
    configure_identity(git_repo.path());
    git(git_repo.path(), ["checkout", "-b", "main"]);
    git_with_env(git_repo.path(), ["commit", "--allow-empty", "-m", "one"]);
    git_with_env(git_repo.path(), ["commit", "--allow-empty", "-m", "two"]);

    assert_eq!(
        run_zmin_args(repo.path(), &["reflog", "show", "main"]),
        git_args(git_repo.path(), &["reflog", "show", "main"])
    );
    assert_eq!(
        run_zmin_args(repo.path(), &["reflog", "show", "HEAD"]),
        git_args(git_repo.path(), &["reflog", "show", "HEAD"])
    );
}

#[test]
fn reflog_delete_selectors_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    reflog_delete_fixture(git_repo.path());
    reflog_delete_fixture(zmin_repo.path());

    git(git_repo.path(), ["reflog", "delete", "main@{1}"]);
    run_zmin_args(zmin_repo.path(), &["reflog", "delete", "main@{1}"]);
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["reflog", "show", "main"]),
        git_args(git_repo.path(), &["reflog", "show", "main"])
    );
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["reflog", "show", "HEAD"]),
        git_args(git_repo.path(), &["reflog", "show", "HEAD"])
    );

    git(git_repo.path(), ["reflog", "delete", "HEAD@{1}"]);
    run_zmin_args(zmin_repo.path(), &["reflog", "delete", "HEAD@{1}"]);
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["reflog", "show", "main"]),
        git_args(git_repo.path(), &["reflog", "show", "main"])
    );
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["reflog", "show", "HEAD"]),
        git_args(git_repo.path(), &["reflog", "show", "HEAD"])
    );

    git(
        git_repo.path(),
        ["reflog", "delete", "main@{07.04.2005.15:15:00.-0700}"],
    );
    run_zmin_args(
        zmin_repo.path(),
        &["reflog", "delete", "main@{07.04.2005.15:15:00.-0700}"],
    );
    let zmin_main = run_zmin_args(zmin_repo.path(), &["reflog", "show", "main"]);
    assert_eq!(
        zmin_main,
        git_args(git_repo.path(), &["reflog", "show", "main"])
    );
}

#[test]
fn branch_create_reflog_handles_nested_branch_names() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        git(repo, ["checkout", "-b", "main"]);
        git_with_env(repo, ["commit", "--allow-empty", "-m", "base"]);
    }

    git(git_repo.path(), ["branch", "one/two", "main"]);
    run_zmin_args(zmin_repo.path(), &["branch", "one/two", "main"]);
    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &["log", "-g", "--format=%gd %gs", "one/two"]
        ),
        git_args(
            git_repo.path(),
            &["log", "-g", "--format=%gd %gs", "one/two"]
        )
    );

    git(git_repo.path(), ["branch", "-d", "one/two"]);
    run_zmin_args(zmin_repo.path(), &["branch", "-d", "one/two"]);
    git(git_repo.path(), ["branch", "one", "main"]);
    run_zmin_args(zmin_repo.path(), &["branch", "one", "main"]);
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["log", "-g", "--format=%gd %gs", "one"]),
        git_args(git_repo.path(), &["log", "-g", "--format=%gd %gs", "one"])
    );
}

#[test]
fn log_reflog_keeps_ordinals_for_hidden_zero_oid_entries() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
    }

    git(git_repo.path(), ["checkout", "-b", "foo"]);
    git_with_env(git_repo.path(), ["commit", "--allow-empty", "-m", "base"]);
    git(git_repo.path(), ["checkout", "-b", "baz"]);
    git(git_repo.path(), ["branch", "bam"]);
    git(git_repo.path(), ["branch", "-M", "baz", "bam"]);

    run_zmin_args(zmin_repo.path(), &["checkout", "-b", "foo"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "--allow-empty", "-m", "base"]);
    run_zmin_args(zmin_repo.path(), &["checkout", "-b", "baz"]);
    run_zmin_args(zmin_repo.path(), &["branch", "bam"]);
    run_zmin_args(zmin_repo.path(), &["branch", "-M", "baz", "bam"]);

    let args = ["log", "-g", "--format=%gd %gs", "-2", "HEAD"];
    let zmin = run_zmin_args(zmin_repo.path(), &args);
    assert_eq!(zmin, git_args(git_repo.path(), &args));
    assert!(zmin.contains("HEAD@{2} checkout: moving from foo to baz"));
}

#[test]
fn log_reflog_orphan_checkout_uses_contiguous_commit_ordinals() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
    }

    git_with_env(
        git_repo.path(),
        ["commit", "--allow-empty", "-m", "initial"],
    );
    git(git_repo.path(), ["checkout", "--orphan", "orphan1"]);
    git_with_env(
        git_repo.path(),
        ["commit", "--allow-empty", "-m", "orphan1-1"],
    );
    git_with_env(
        git_repo.path(),
        ["commit", "--allow-empty", "-m", "orphan1-2"],
    );
    git(git_repo.path(), ["checkout", "--orphan", "orphan2"]);
    git_with_env(
        git_repo.path(),
        ["commit", "--allow-empty", "-m", "orphan2-1"],
    );

    run_zmin_with_env(
        zmin_repo.path(),
        ["commit", "--allow-empty", "-m", "initial"],
    );
    run_zmin_args(zmin_repo.path(), &["checkout", "--orphan", "orphan1"]);
    run_zmin_with_env(
        zmin_repo.path(),
        ["commit", "--allow-empty", "-m", "orphan1-1"],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["commit", "--allow-empty", "-m", "orphan1-2"],
    );
    run_zmin_args(zmin_repo.path(), &["checkout", "--orphan", "orphan2"]);
    run_zmin_with_env(
        zmin_repo.path(),
        ["commit", "--allow-empty", "-m", "orphan2-1"],
    );

    let args = ["log", "-g", "--format=%gd %gs", "HEAD"];
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &args),
        git_args(git_repo.path(), &args)
    );
}

fn reflog_delete_fixture(repo: &std::path::Path) {
    configure_identity(repo);
    git(repo, ["checkout", "-b", "main"]);
    for (content, message, timestamp) in [
        ("1\n", "rabbit", "1112912053 -0700"),
        ("2\n", "dragon", "1112912113 -0700"),
        ("3\n", "ox", "1112912413 -0700"),
        ("4\n", "tiger", "1112912473 -0700"),
    ] {
        common::write_file(repo, "C", content);
        git(repo, ["add", "C"]);
        command_output_with_env(
            "git",
            repo,
            &["commit", "-m", message],
            &[
                ("GIT_AUTHOR_NAME", "Bench"),
                ("GIT_AUTHOR_EMAIL", "bench@example.test"),
                ("GIT_AUTHOR_DATE", timestamp),
                ("GIT_COMMITTER_NAME", "Bench"),
                ("GIT_COMMITTER_EMAIL", "bench@example.test"),
                ("GIT_COMMITTER_DATE", timestamp),
            ],
            "git",
        );
    }
}

#[test]
fn reflog_expire_dry_run_does_not_touch_reflog() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "one"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "two"]);
    let before = run_zmin_args(repo.path(), &["reflog", "show", "main"]);

    assert_eq!(
        run_zmin_status(repo.path(), ["reflog", "expire", "--dry-run", "main"]),
        0
    );
    assert_eq!(
        run_zmin_args(repo.path(), &["reflog", "show", "main"]),
        before
    );
}

#[test]
fn reflog_expire_default_current_entries_match_stock_git() {
    for args in [
        ["reflog", "expire"].as_slice(),
        ["reflog", "expire", "main"].as_slice(),
        ["reflog", "expire", "HEAD"].as_slice(),
        ["reflog", "expire", "--updateref", "main"].as_slice(),
        ["reflog", "expire", "--rewrite", "main"].as_slice(),
        ["reflog", "expire", "--verbose", "main"].as_slice(),
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        reflog_expire_default_fixture(git_repo.path());
        reflog_expire_default_fixture(zmin_repo.path());

        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin"),
            command_any_output("git", git_repo.path(), args, "git"),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin_args(zmin_repo.path(), &["reflog", "show", "main"]),
            git_args(git_repo.path(), &["reflog", "show", "main"]),
            "main reflog after args: {args:?}"
        );
        assert_eq!(
            run_zmin_args(zmin_repo.path(), &["reflog", "show", "HEAD"]),
            git_args(git_repo.path(), &["reflog", "show", "HEAD"]),
            "HEAD reflog after args: {args:?}"
        );
    }
}

#[test]
fn reflog_expire_pattern_config_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    reflog_expire_pattern_config_fixture(git_repo.path());
    reflog_expire_pattern_config_fixture(zmin_repo.path());

    git(
        git_repo.path(),
        [
            "reflog",
            "expire",
            "root1/branch1",
            "root1/branch2",
            "root2/branch1",
            "root2/branch2",
        ],
    );
    run_zmin_args(
        zmin_repo.path(),
        &[
            "reflog",
            "expire",
            "root1/branch1",
            "root1/branch2",
            "root2/branch1",
            "root2/branch2",
        ],
    );

    let mut expected: Vec<_> = git_args(
        git_repo.path(),
        &["log", "-g", "--format=%gD", "--branches=root*"],
    )
    .lines()
    .map(str::to_owned)
    .collect();
    let mut actual: Vec<_> = run_zmin_args(
        zmin_repo.path(),
        &["log", "-g", "--format=%gD", "--branches=root*"],
    )
    .lines()
    .map(str::to_owned)
    .collect();
    expected.sort();
    actual.sort();

    assert_eq!(actual, expected);
    assert_eq!(actual, ["root1/branch1@{0}", "root1/branch2@{0}"]);
}

fn reflog_expire_default_fixture(repo: &std::path::Path) {
    configure_identity(repo);
    git(repo, ["checkout", "-b", "main"]);
    git_with_env(repo, ["commit", "--allow-empty", "-m", "one"]);
    git_with_env(repo, ["commit", "--allow-empty", "-m", "two"]);
}

fn reflog_expire_pattern_config_fixture(repo: &std::path::Path) {
    configure_identity(repo);
    git(repo, ["checkout", "-b", "main"]);
    git_with_env(repo, ["commit", "--allow-empty", "-m", "one"]);
    git(repo, ["branch", "root1/branch1"]);
    git(repo, ["branch", "root1/branch2"]);
    git(repo, ["branch", "root2/branch1"]);
    git(repo, ["branch", "root2/branch2"]);
    git(repo, ["config", "gc.reflogexpire", "never"]);
    git(
        repo,
        ["config", "gc.refs/heads/root2/*.reflogExpire", "now"],
    );
}
