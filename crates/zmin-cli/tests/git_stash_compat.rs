mod common;

use std::{fs, process::Command};

use tempfile::TempDir;

use common::{
    clone_repo_fixture, command_any_output, configure_identity, git, git_args, git_failure_output,
    git_init, git_status, git_status_with_stdin, git_with_env, git_with_stdin, run_zmin,
    run_zmin_args, run_zmin_failure_output, run_zmin_status, run_zmin_status_with_stdin,
    run_zmin_with_env, run_zmin_with_stdin, zmin_bin, write_file,
};

fn stash_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    repo
}

#[test]
fn stash_show_pickaxe_matches_stock_git() {
    let repo = stash_fixture_repo();
    write_file(repo.path(), "a.txt", "one\nneedle\nneedle\n");
    write_file(repo.path(), "b.txt", "one\nchanged\n");
    git_with_env(repo.path(), ["stash", "push", "-m", "pickaxe"]);

    for args in [
        ["stash", "show", "-Sneedle", "--name-only"].as_slice(),
        ["stash", "show", "-Gchanged", "--name-only"].as_slice(),
        ["stash", "show", "--pickaxe-all", "-Sneedle", "--name-only"].as_slice(),
        [
            "stash",
            "show",
            "--pickaxe-regex",
            "-Sneed.e",
            "--name-only",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn stash_show_order_file_matches_stock_git() {
    let repo = stash_fixture_repo();
    write_file(repo.path(), "a.txt", "one\na\n");
    write_file(repo.path(), "b.txt", "b\n");
    write_file(repo.path(), "c.txt", "c\n");
    write_file(repo.path(), "order.txt", "c.txt\na.txt\n");
    git_with_env(repo.path(), ["stash", "push", "-m", "ordered"]);

    let args = ["stash", "show", "-Oorder.txt", "--name-only"];
    assert_eq!(
        run_zmin_args(repo.path(), &args),
        git_args(repo.path(), &args)
    );
}

#[test]
fn stash_show_skip_and_rotate_match_stock_git() {
    let repo = stash_fixture_repo();
    write_file(repo.path(), "a.txt", "one\na\n");
    write_file(repo.path(), "b.txt", "b\n");
    write_file(repo.path(), "c.txt", "c\n");
    git_with_env(repo.path(), ["stash", "push", "-m", "ordered"]);

    for args in [
        ["stash", "show", "--skip-to=b.txt", "--name-only"].as_slice(),
        ["stash", "show", "--rotate-to=b.txt", "--name-only"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn stash_push_apply_pop_matches_stock_git_state() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\ntwo\n");
    write_file(zmin_repo.path(), "a.txt", "one\ntwo\n");

    git_with_env(git_repo.path(), ["stash", "push", "-m", "work"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "work"]);
    assert_eq!(git(git_repo.path(), ["status", "--short"]), "");
    assert_eq!(git(zmin_repo.path(), ["status", "--short"]), "");
    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["diff-files", "--quiet"]),
        0
    );
    assert_eq!(
        run_zmin_status(
            zmin_repo.path(),
            ["diff-index", "--cached", "--quiet", "HEAD"]
        ),
        0
    );
    assert!(run_zmin(zmin_repo.path(), ["stash", "list"]).contains("stash@{0}: On main: work"));
    assert_eq!(
        run_zmin(zmin_repo.path(), ["rev-parse", "stash^"]),
        run_zmin(zmin_repo.path(), ["rev-parse", "HEAD"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["rev-parse", "stash@{0}"]),
        run_zmin(zmin_repo.path(), ["rev-parse", "stash"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["show", "stash^2:a.txt"]),
        "one"
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["show", "stash:a.txt"]),
        "one\ntwo"
    );
    let stash_worktree_diff = run_zmin(zmin_repo.path(), ["diff", "stash^2..stash"]);
    assert!(
        stash_worktree_diff.contains("+two"),
        "stash worktree diff should compare index parent to stash tree: {stash_worktree_diff}"
    );

    git(git_repo.path(), ["stash", "clear"]);
    run_zmin(zmin_repo.path(), ["stash", "clear"]);
    assert_eq!(run_zmin(zmin_repo.path(), ["stash", "list"]), "");

    write_file(git_repo.path(), "a.txt", "one\ntwo\n");
    write_file(zmin_repo.path(), "a.txt", "one\ntwo\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "work"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "work"]);

    git_with_env(git_repo.path(), ["stash", "apply"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "apply"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin file"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git file")
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(zmin_repo.path(), ["reset", "--hard"]);
    git_with_env(git_repo.path(), ["stash", "pop"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "pop"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin file"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git file")
    );
    assert_eq!(run_zmin(zmin_repo.path(), ["stash", "list"]), "");
}

#[test]
fn stash_preserves_missing_skip_worktree_entry_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "changed\n");
    write_file(zmin_repo.path(), "a.txt", "changed\n");
    git(git_repo.path(), ["add", "a.txt"]);
    run_zmin(zmin_repo.path(), ["add", "a.txt"]);
    git(
        git_repo.path(),
        ["update-index", "--skip-worktree", "a.txt"],
    );
    run_zmin(
        zmin_repo.path(),
        ["update-index", "--skip-worktree", "a.txt"],
    );
    fs::remove_file(git_repo.path().join("a.txt")).expect("remove git a");
    fs::remove_file(zmin_repo.path().join("a.txt")).expect("remove zmin a");

    git_with_env(git_repo.path(), ["stash"]);
    run_zmin_with_env(zmin_repo.path(), ["stash"]);

    assert_eq!(
        git(
            zmin_repo.path(),
            ["rev-parse", "--verify", "refs/stash:a.txt"]
        ),
        git(
            git_repo.path(),
            ["rev-parse", "--verify", "refs/stash:a.txt"]
        )
    );
}

#[test]
fn stash_include_untracked_matches_stock_git_state() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\ntwo\n");
    write_file(zmin_repo.path(), "a.txt", "one\ntwo\n");
    write_file(git_repo.path(), "new.txt", "new\n");
    write_file(zmin_repo.path(), "new.txt", "new\n");
    write_file(git_repo.path(), ".gitignore", "ignored.txt\n");
    write_file(zmin_repo.path(), ".gitignore", "ignored.txt\n");
    write_file(git_repo.path(), "ignored.txt", "ignored\n");
    write_file(zmin_repo.path(), "ignored.txt", "ignored\n");

    git_with_env(git_repo.path(), ["stash", "push", "-u", "-m", "save"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-u", "-m", "save"]);
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-tree", "-r", "--name-only", "stash"]),
        git(git_repo.path(), ["ls-tree", "-r", "--name-only", "stash"])
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["ls-tree", "-r", "--name-only", "stash^3"]
        ),
        git(git_repo.path(), ["ls-tree", "-r", "--name-only", "stash^3"])
    );

    git_with_env(git_repo.path(), ["stash", "pop"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "pop"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("new.txt")).expect("read zmin untracked"),
        fs::read_to_string(git_repo.path().join("new.txt")).expect("read git untracked")
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(run_zmin(zmin_repo.path(), ["stash", "list"]), "");
}

#[test]
fn stash_include_untracked_file_to_directory_switch_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "filler", "this\nfile\nhas\nsome\nwords\n");
        git(repo, ["add", "filler"]);
        git_with_env(repo, ["commit", "-m", "filler"]);
        git(repo, ["rm", "filler"]);
        fs::create_dir(repo.join("filler")).expect("create filler dir");
        write_file(repo, "filler/file", "contents\n");
    }

    git_with_env(git_repo.path(), ["stash", "push", "--include-untracked"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "--include-untracked"]);
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );

    git_with_env(git_repo.path(), ["stash", "apply", "--index"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "apply", "--index"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("filler/file")).expect("read zmin filler"),
        fs::read_to_string(git_repo.path().join("filler/file")).expect("read git filler")
    );
}

#[test]
fn stash_pop_restores_untracked_files_after_conflict_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "a.txt", "hello\n");
        write_file(repo, "c", "something\n");
    }

    git_with_env(git_repo.path(), ["stash", "push", "--include-untracked"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "--include-untracked"]);
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "a.txt", "conflict\n");
        git(repo, ["add", "a.txt"]);
        git_with_env(repo, ["commit", "-m", "second"]);
    }

    assert_ne!(git_status(git_repo.path(), ["stash", "pop"]), 0);
    assert_ne!(run_zmin_status(zmin_repo.path(), ["stash", "pop"]), 0);
    assert!(zmin_repo.path().join("c").is_file());
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("c")).expect("read zmin c"),
        fs::read_to_string(git_repo.path().join("c")).expect("read git c")
    );
}

#[test]
fn stash_all_includes_ignored_files_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), ".gitignore", "ignored.txt\n");
    write_file(zmin_repo.path(), ".gitignore", "ignored.txt\n");
    write_file(git_repo.path(), "new.txt", "new\n");
    write_file(zmin_repo.path(), "new.txt", "new\n");
    write_file(git_repo.path(), "ignored.txt", "ignored\n");
    write_file(zmin_repo.path(), "ignored.txt", "ignored\n");

    git_with_env(git_repo.path(), ["stash", "push", "-a", "-m", "all"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-a", "-m", "all"]);
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short", "--ignored"]),
        git(git_repo.path(), ["status", "--short", "--ignored"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git_with_env(git_repo.path(), ["stash", "pop"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "pop"]);
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short", "--ignored"]),
        git(git_repo.path(), ["status", "--short", "--ignored"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("ignored.txt")).expect("read zmin ignored"),
        fs::read_to_string(git_repo.path().join("ignored.txt")).expect("read git ignored")
    );
}

#[test]
fn stash_pathspec_limits_saved_worktree_changes_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "b.txt", "one\n");
    write_file(zmin_repo.path(), "b.txt", "one\n");
    git(git_repo.path(), ["add", "b.txt"]);
    git(zmin_repo.path(), ["add", "b.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "add b"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "add b"]);

    write_file(git_repo.path(), "a.txt", "one\ntwo\n");
    write_file(zmin_repo.path(), "a.txt", "one\ntwo\n");
    write_file(git_repo.path(), "b.txt", "one\ntwo\n");
    write_file(zmin_repo.path(), "b.txt", "one\ntwo\n");

    git_with_env(
        git_repo.path(),
        ["stash", "push", "-m", "only-a", "--", "a.txt"],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["stash", "push", "-m", "only-a", "--", "a.txt"],
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["diff-files", "--quiet"]),
        git_status(zmin_repo.path(), ["diff-files", "--quiet"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("b.txt")).expect("read zmin b"),
        fs::read_to_string(git_repo.path().join("b.txt")).expect("read git b")
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git_with_env(git_repo.path(), ["stash", "pop"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "pop"]);
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
}

#[test]
fn stash_pathspec_keeps_staged_non_pathspec_file_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "foo", "");
    write_file(zmin_repo.path(), "foo", "");
    write_file(git_repo.path(), "bar", "");
    write_file(zmin_repo.path(), "bar", "");
    git(git_repo.path(), ["add", "foo", "bar"]);
    run_zmin(zmin_repo.path(), ["add", "foo", "bar"]);

    git_with_env(git_repo.path(), ["stash", "push", "--", "foo"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "--", "foo"]);
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "show", "-p"]),
        git_args(git_repo.path(), &["stash", "show", "-p"])
    );

    git_with_env(git_repo.path(), ["stash", "pop"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "pop"]);
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
}

#[test]
fn stash_pathspec_from_subdirectory_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    fs::create_dir(git_repo.path().join("sub")).expect("create git sub");
    fs::create_dir(zmin_repo.path().join("sub")).expect("create zmin sub");
    write_file(git_repo.path(), "foo", "");
    write_file(zmin_repo.path(), "foo", "");
    write_file(git_repo.path(), "bar", "");
    write_file(zmin_repo.path(), "bar", "");
    git(git_repo.path(), ["add", "foo", "bar"]);
    run_zmin(zmin_repo.path(), ["add", "foo", "bar"]);

    git_with_env(
        &git_repo.path().join("sub"),
        ["stash", "push", "--", "../foo"],
    );
    run_zmin_with_env(
        &zmin_repo.path().join("sub"),
        ["stash", "push", "--", "../foo"],
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );

    git_with_env(git_repo.path(), ["stash", "pop"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "pop"]);
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
}

#[test]
fn stash_pathspec_untracked_without_include_untracked_errors_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "untracked", "");
    write_file(zmin_repo.path(), "untracked", "");

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["stash", "push", "untracked"]),
        git_failure_output(git_repo.path(), &["stash", "push", "untracked"])
    );
    assert!(zmin_repo.path().join("untracked").is_file());
}

#[test]
fn stash_without_verb_accepts_pathspec_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "foo bar", "");
    write_file(zmin_repo.path(), "foo bar", "");
    write_file(git_repo.path(), "foo", "");
    write_file(zmin_repo.path(), "foo", "");
    write_file(git_repo.path(), "bar", "");
    write_file(zmin_repo.path(), "bar", "");
    git(git_repo.path(), ["add", "foo bar", "foo"]);
    run_zmin(zmin_repo.path(), ["add", "foo bar", "foo"]);

    git_with_env(git_repo.path(), ["stash", "--", "foo b*"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "--", "foo b*"]);
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );

    git_with_env(git_repo.path(), ["stash", "pop"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "pop"]);
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
}

#[test]
fn stash_keep_index_pathspec_leaves_non_pathspec_dirty_file_like_stock_git() {
    let git_repo = git_init();
    configure_identity(git_repo.path());
    git(git_repo.path(), ["checkout", "-b", "main"]);
    write_file(git_repo.path(), "foo", "");
    write_file(git_repo.path(), "bar", "");
    git(git_repo.path(), ["add", "foo", "bar"]);
    git_with_env(git_repo.path(), ["commit", "-m", "test"]);
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "foo", "foo\n");
    write_file(zmin_repo.path(), "foo", "foo\n");
    write_file(git_repo.path(), "bar", "bar\n");
    write_file(zmin_repo.path(), "bar", "bar\n");

    git_with_env(git_repo.path(), ["stash", "-k", "--", "foo"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "-k", "--", "foo"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("foo")).expect("read zmin foo"),
        fs::read_to_string(git_repo.path().join("foo")).expect("read git foo")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("bar")).expect("read zmin bar"),
        fs::read_to_string(git_repo.path().join("bar")).expect("read git bar")
    );

    git_with_env(git_repo.path(), ["stash", "pop"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "pop"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("foo")).expect("read zmin popped foo"),
        fs::read_to_string(git_repo.path().join("foo")).expect("read git popped foo")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("bar")).expect("read zmin popped bar"),
        fs::read_to_string(git_repo.path().join("bar")).expect("read git popped bar")
    );
}

#[test]
fn stash_push_unborn_head_errors_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    write_file(git_repo.path(), "bar", "");
    write_file(zmin_repo.path(), "bar", "");

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["stash", "push"]),
        git_failure_output(git_repo.path(), &["stash", "push"])
    );
    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["stash", "push", "-q"]),
        git_failure_output(git_repo.path(), &["stash", "push", "-q"])
    );
}

#[test]
fn stash_uses_git_stash_identity_when_user_identity_is_missing_like_stock_git() {
    fn run_without_identity(
        command: &str,
        cwd: &std::path::Path,
        args: &[&str],
    ) -> (i32, String, String) {
        let output = Command::new(command)
            .args(args)
            .current_dir(cwd)
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_AUTHOR_NAME")
            .env_remove("GIT_AUTHOR_EMAIL")
            .env_remove("GIT_COMMITTER_NAME")
            .env_remove("GIT_COMMITTER_EMAIL")
            .env_remove("EMAIL")
            .output()
            .unwrap_or_else(|error| panic!("run {command}: {error}"));
        (
            output.status.code().expect("exit code"),
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

    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "user.useconfigonly", "true"]);
        git(repo, ["config", "--unset", "user.name"]);
        git(repo, ["config", "--unset", "user.email"]);
        write_file(repo, "missing-identity", "");
        git(repo, ["add", "missing-identity"]);
    }

    assert_eq!(
        run_without_identity("git", git_repo.path(), &["stash"]).0,
        0
    );
    assert_eq!(
        run_without_identity(zmin_bin(), zmin_repo.path(), &["stash"]).0,
        0
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["show", "-s", "--format=%an <%ae>", "refs/stash"]
        ),
        git(
            git_repo.path(),
            ["show", "-s", "--format=%an <%ae>", "refs/stash"]
        )
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["show", "-s", "--format=%an <%ae>", "refs/stash"]
        ),
        "git stash <git@stash>"
    );
}

#[test]
fn stash_push_captures_same_size_rewrite_after_reset_like_stock_git() {
    let git_repo = git_init();
    configure_identity(git_repo.path());
    git(git_repo.path(), ["checkout", "-b", "main"]);
    write_file(git_repo.path(), "file", "1\n");
    write_file(git_repo.path(), "other-file", "unrelated\n");
    git(git_repo.path(), ["add", "file", "other-file"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "other-file", "changed\n");
    write_file(zmin_repo.path(), "other-file", "changed\n");
    git(git_repo.path(), ["add", "other-file"]);
    git(zmin_repo.path(), ["add", "other-file"]);
    git_with_env(git_repo.path(), ["commit", "-m", "other-file"]);
    git_with_env(zmin_repo.path(), ["commit", "-m", "other-file"]);

    for value in ["8\n", "9\n"] {
        write_file(git_repo.path(), "file", value);
        write_file(zmin_repo.path(), "file", value);
        git_with_env(git_repo.path(), ["stash"]);
        run_zmin_with_env(zmin_repo.path(), ["stash"]);
    }

    assert_eq!(
        run_zmin(zmin_repo.path(), ["diff", "stash@{0}^..stash@{0}"]),
        git(git_repo.path(), ["diff", "stash@{0}^..stash@{0}"])
    );

    git_with_env(git_repo.path(), ["stash", "drop", "stash@{1}"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "drop", "stash@{1}"]);
    git_with_env(git_repo.path(), ["stash", "apply"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "apply"]);

    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("file")).expect("read zmin file"),
        fs::read_to_string(git_repo.path().join("file")).expect("read git file")
    );
}

#[test]
fn stash_pop_rejects_overlapping_dirty_paths_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nstashed\n");
    write_file(zmin_repo.path(), "a.txt", "one\nstashed\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "overlap"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "overlap"]);

    write_file(git_repo.path(), "a.txt", "one\nlocal\n");
    write_file(zmin_repo.path(), "a.txt", "one\nlocal\n");

    let git_result = git_failure_output(git_repo.path(), &["stash", "pop"]);
    let zmin_result = run_zmin_failure_output(zmin_repo.path(), &["stash", "pop"]);
    assert_eq!(zmin_result.0, git_result.0);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_pop_drops_selected_stack_entry_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nfirst\n");
    write_file(zmin_repo.path(), "a.txt", "one\nfirst\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "first"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "first"]);

    write_file(git_repo.path(), "a.txt", "one\nsecond\n");
    write_file(zmin_repo.path(), "a.txt", "one\nsecond\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "second"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "second"]);

    git_with_env(git_repo.path(), ["stash", "pop", "stash@{1}"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "pop", "stash@{1}"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    write_file(git_repo.path(), "a.txt", "one\nno-message\n");
    write_file(zmin_repo.path(), "a.txt", "one\nno-message\n");
    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &["stash", "push", "--message=custom", "--no-message"],
        ),
        git_args(
            git_repo.path(),
            &["stash", "push", "--message=custom", "--no-message"],
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    write_file(git_repo.path(), "a.txt", "one\nmessage-after-no\n");
    write_file(zmin_repo.path(), "a.txt", "one\nmessage-after-no\n");
    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &["stash", "push", "--no-message", "--message=after-no"],
        ),
        git_args(
            git_repo.path(),
            &["stash", "push", "--no-message", "--message=after-no"],
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(zmin_repo.path(), ["reset", "--hard"]);
    write_file(git_repo.path(), "b.txt", "one\n");
    write_file(zmin_repo.path(), "b.txt", "one\n");
    git(git_repo.path(), ["add", "b.txt"]);
    git(zmin_repo.path(), ["add", "b.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "track b"]);
    git_with_env(zmin_repo.path(), ["commit", "-m", "track b"]);
    write_file(git_repo.path(), "a.txt", "one\nno-file-a\n");
    write_file(zmin_repo.path(), "a.txt", "one\nno-file-a\n");
    write_file(git_repo.path(), "b.txt", "one\nno-file-b\n");
    write_file(zmin_repo.path(), "b.txt", "one\nno-file-b\n");
    write_file(git_repo.path(), "paths.txt", "b.txt\n");
    write_file(zmin_repo.path(), "paths.txt", "b.txt\n");
    git_with_env(
        git_repo.path(),
        [
            "stash",
            "push",
            "-m",
            "no-pathspec-file",
            "--pathspec-from-file=paths.txt",
            "--no-pathspec-from-file",
        ],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        [
            "stash",
            "push",
            "-m",
            "no-pathspec-file",
            "--pathspec-from-file=paths.txt",
            "--no-pathspec-from-file",
        ],
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "show", "--patch", "--stat"]),
        git_args(git_repo.path(), &["stash", "show", "--patch", "--stat"])
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(zmin_repo.path(), ["reset", "--hard"]);
    write_file(git_repo.path(), "a.txt", "one\nno-nul\n");
    write_file(zmin_repo.path(), "a.txt", "one\nno-nul\n");
    write_file(git_repo.path(), "b.txt", "one\nno-nul-kept\n");
    write_file(zmin_repo.path(), "b.txt", "one\nno-nul-kept\n");
    write_file(git_repo.path(), "paths.txt", "a.txt\n");
    write_file(zmin_repo.path(), "paths.txt", "a.txt\n");
    git_with_env(
        git_repo.path(),
        [
            "stash",
            "push",
            "-m",
            "no-pathspec-nul",
            "--pathspec-file-nul",
            "--no-pathspec-file-nul",
            "--pathspec-from-file=paths.txt",
        ],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        [
            "stash",
            "push",
            "-m",
            "no-pathspec-nul",
            "--pathspec-file-nul",
            "--no-pathspec-file-nul",
            "--pathspec-from-file=paths.txt",
        ],
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "show", "--patch", "--stat"]),
        git_args(git_repo.path(), &["stash", "show", "--patch", "--stat"])
    );
}

#[test]
fn stash_apply_and_drop_selected_stack_entry_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nfirst\n");
    write_file(zmin_repo.path(), "a.txt", "one\nfirst\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "first"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "first"]);

    write_file(git_repo.path(), "a.txt", "one\nsecond\n");
    write_file(zmin_repo.path(), "a.txt", "one\nsecond\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "second"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "second"]);

    git_with_env(git_repo.path(), ["stash", "apply", "stash@{1}"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "apply", "stash@{1}"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(zmin_repo.path(), ["reset", "--hard"]);
    git(git_repo.path(), ["stash", "drop", "1"]);
    run_zmin(zmin_repo.path(), ["stash", "drop", "1"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_index_config_implies_apply_index_like_stock_git() {
    let zmin_repo = stash_fixture_repo();
    write_file(zmin_repo.path(), "a.txt", "index\n");
    git(zmin_repo.path(), ["add", "a.txt"]);
    write_file(zmin_repo.path(), "a.txt", "working\n");

    run_zmin_with_env(zmin_repo.path(), ["stash"]);
    run_zmin(
        zmin_repo.path(),
        ["-c", "stash.index=true", "stash", "apply"],
    );

    assert_eq!(git(zmin_repo.path(), ["show", ":0:a.txt"]), "index");
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        "working\n"
    );
}

#[test]
fn stash_quiet_options_match_stock_git_output_and_state() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "push", "-q"]),
        git_args(git_repo.path(), &["stash", "push", "-q"])
    );

    write_file(git_repo.path(), "a.txt", "one\nquiet\n");
    write_file(zmin_repo.path(), "a.txt", "one\nquiet\n");
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "push", "-q", "-m", "quiet"]),
        git_args(git_repo.path(), &["stash", "push", "-q", "-m", "quiet"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );

    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "apply", "-q"]),
        git_args(git_repo.path(), &["stash", "apply", "-q"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    git(git_repo.path(), ["reset", "--hard"]);
    git(zmin_repo.path(), ["reset", "--hard"]);
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "pop", "-q"]),
        git_args(git_repo.path(), &["stash", "pop", "-q"])
    );
    assert_eq!(run_zmin(zmin_repo.path(), ["stash", "list"]), "");

    git(git_repo.path(), ["reset", "--hard"]);
    git(zmin_repo.path(), ["reset", "--hard"]);
    write_file(git_repo.path(), "a.txt", "one\nfirst\n");
    write_file(zmin_repo.path(), "a.txt", "one\nfirst\n");
    git_with_env(git_repo.path(), ["stash", "push", "-q", "-m", "first"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-q", "-m", "first"]);
    write_file(git_repo.path(), "a.txt", "one\nsecond\n");
    write_file(zmin_repo.path(), "a.txt", "one\nsecond\n");
    git_with_env(git_repo.path(), ["stash", "push", "-q", "-m", "second"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-q", "-m", "second"]);
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "drop", "-q", "stash@{1}"]),
        git_args(git_repo.path(), &["stash", "drop", "-q", "stash@{1}"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_push_message_equals_option_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nequals\n");
    write_file(zmin_repo.path(), "a.txt", "one\nequals\n");
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "push", "--message=equals"]),
        git_args(git_repo.path(), &["stash", "push", "--message=equals"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_push_short_message_without_space_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nunspaced\n");
    write_file(zmin_repo.path(), "a.txt", "one\nunspaced\n");
    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &["stash", "push", "-munspaced test message"],
        ),
        git_args(
            git_repo.path(),
            &["stash", "push", "-munspaced test message"],
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_save_quiet_matches_stock_git_output_and_state() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nlegacy quiet\n");
    write_file(zmin_repo.path(), "a.txt", "one\nlegacy quiet\n");
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "save", "--quiet"]),
        git_args(git_repo.path(), &["stash", "save", "--quiet"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
}

#[test]
fn stash_save_rm_then_recreate_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    git(git_repo.path(), ["rm", "a.txt"]);
    run_zmin(zmin_repo.path(), ["rm", "a.txt"]);
    write_file(git_repo.path(), "a.txt", "recreated\n");
    write_file(zmin_repo.path(), "a.txt", "recreated\n");

    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "save", "rm then recreate"]),
        git_args(git_repo.path(), &["stash", "save", "rm then recreate"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin restored file"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git restored file")
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["stash", "apply"]),
        git_status(git_repo.path(), ["stash", "apply"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin applied file"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git applied file")
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
}

#[test]
fn stash_save_file_to_directory_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    fs::remove_file(git_repo.path().join("a.txt")).expect("remove git file");
    fs::remove_file(zmin_repo.path().join("a.txt")).expect("remove zmin file");
    fs::create_dir(git_repo.path().join("a.txt")).expect("create git dir");
    fs::create_dir(zmin_repo.path().join("a.txt")).expect("create zmin dir");
    write_file(git_repo.path(), "a.txt/file", "directory content\n");
    write_file(zmin_repo.path(), "a.txt/file", "directory content\n");

    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "save", "file to directory"]),
        git_args(git_repo.path(), &["stash", "save", "file to directory"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin restored file"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git restored file")
    );
    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["stash", "apply"]),
        git_status(git_repo.path(), ["stash", "apply"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin file after apply"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git file after apply")
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
}

#[test]
fn stash_apply_pop_branch_reject_too_many_refs_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    for content in ["first\n", "second\n"] {
        write_file(git_repo.path(), "a.txt", content);
        write_file(zmin_repo.path(), "a.txt", content);
        git_with_env(git_repo.path(), ["stash", "push", "-q"]);
        run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-q"]);
    }
    write_file(git_repo.path(), "a.txt", "worktree\n");
    write_file(zmin_repo.path(), "a.txt", "worktree\n");

    for args in [
        ["stash", "apply", "stash@{0}", "stash@{1}"].as_slice(),
        ["stash", "pop", "stash@{0}", "stash@{1}"].as_slice(),
        ["stash", "show", "stash@{0}", "stash@{1}"].as_slice(),
        ["stash", "branch", "stash-branch", "stash@{0}", "stash@{1}"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), args),
            git_failure_output(git_repo.path(), args),
            "args: {args:?}"
        );
        assert_eq!(
            fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin file"),
            fs::read_to_string(git_repo.path().join("a.txt")).expect("read git file"),
            "args: {args:?}"
        );
    }
}

#[test]
fn stash_rejects_intent_to_add_entries_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "file4", "new\n");
    write_file(zmin_repo.path(), "file4", "new\n");
    git(git_repo.path(), ["add", "--intent-to-add", "file4"]);
    run_zmin(zmin_repo.path(), ["add", "--intent-to-add", "file4"]);

    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["stash"]),
        git_status(git_repo.path(), ["stash"])
    );
}

#[test]
fn stash_push_negated_options_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, ".gitignore", "*.log\n");
        write_file(repo, "ignored.log", "ignored\n");
        write_file(repo, "untracked.txt", "untracked\n");
    }
    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &["stash", "push", "--all", "--no-all", "--include-untracked"],
        ),
        git_args(
            git_repo.path(),
            &["stash", "push", "--all", "--no-all", "--include-untracked"],
        )
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short", "--ignored"]),
        git(git_repo.path(), ["status", "--short", "--ignored"])
    );

    git(git_repo.path(), ["stash", "clear"]);
    run_zmin(zmin_repo.path(), ["stash", "clear"]);
    git(git_repo.path(), ["reset", "--hard"]);
    git(zmin_repo.path(), ["reset", "--hard"]);
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "untracked.txt", "untracked\n");
    }
    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &[
                "stash",
                "push",
                "--include-untracked",
                "--no-include-untracked"
            ],
        ),
        git_args(
            git_repo.path(),
            &[
                "stash",
                "push",
                "--include-untracked",
                "--no-include-untracked"
            ],
        )
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short", "--ignored"]),
        git(git_repo.path(), ["status", "--short", "--ignored"])
    );

    write_file(git_repo.path(), "a.txt", "one\nquiet\n");
    write_file(zmin_repo.path(), "a.txt", "one\nquiet\n");
    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &["stash", "push", "--quiet", "--no-quiet", "--no-patch"],
        ),
        git_args(
            git_repo.path(),
            &["stash", "push", "--quiet", "--no-quiet", "--no-patch"],
        )
    );
}

#[test]
fn stash_list_formats_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nfirst\n");
    write_file(zmin_repo.path(), "a.txt", "one\nfirst\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "first"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "first"]);
    write_file(git_repo.path(), "a.txt", "one\nsecond\n");
    write_file(zmin_repo.path(), "a.txt", "one\nsecond\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "second"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "second"]);

    let zmin_oneline = run_zmin_args(zmin_repo.path(), &["stash", "list", "--oneline"]);
    let git_oneline = git_args(git_repo.path(), &["stash", "list", "--oneline"]);
    assert_eq!(
        normalize_stash_oneline_hashes(&zmin_oneline),
        normalize_stash_oneline_hashes(&git_oneline)
    );
    for args in [
        ["stash", "list", "--pretty=%H"].as_slice(),
        ["stash", "list", "--format=%H"].as_slice(),
    ] {
        let zmin_hashes = run_zmin_args(zmin_repo.path(), args);
        let git_hashes = git_args(git_repo.path(), args);
        assert_eq!(
            zmin_hashes.lines().count(),
            git_hashes.lines().count(),
            "stash list hash count should match for {args:?}",
        );
        assert!(
            zmin_hashes
                .lines()
                .all(|line| line.len() == 40 && line.chars().all(|ch| ch.is_ascii_hexdigit())),
            "zmin stash list should print full hashes for {args:?}: {zmin_hashes:?}",
        );
    }
    for args in [
        ["stash", "list", "--format=%gd|%gD|%s|%gs"].as_slice(),
        ["stash", "list", "--pretty=format:%gd:%h:%gs"].as_slice(),
        ["stash", "list", "--format=format:%gd%x00%H%x00%gs"].as_slice(),
        ["stash", "list", "--format=tformat:%%:%gd:%s"].as_slice(),
        [
            "stash",
            "list",
            "--format=%an|%ae|%cn|%ce|%at|%ct|%P|%p|%T|%t",
        ]
        .as_slice(),
        ["stash", "list", "--format=%ad|%ai|%aI|%cd|%ci|%cI"].as_slice(),
        ["stash", "list", "--format=%B|%b|%f|%D|%d|%e|%N|%m|%S"].as_slice(),
        ["stash", "list", "--format=%aD|%cD|%as|%cs|%al|%aL|%cl|%cL"].as_slice(),
        ["stash", "list", "--format=%G?|%GK|%GF|%GP|%GT|%gK"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(zmin_repo.path(), args),
            "stash list custom format should match for {args:?}",
        );
    }
    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["stash", "list", "--bad"]).0,
        git_failure_output(git_repo.path(), &["stash", "list", "--bad"]).0
    );
}

#[test]
fn stash_invalid_top_level_and_push_usage_match_stock_git_shape() {
    let repo = stash_fixture_repo();

    let top_level = run_zmin_failure_output(repo.path(), &["stash", "--invalid-option"]);
    let push = run_zmin_failure_output(repo.path(), &["stash", "push", "--invalid-option"]);
    let help = run_zmin_failure_output(repo.path(), &["stash", "-h"]);
    let ambiguous = run_zmin_failure_output(repo.path(), &["stash", "-q", "drop"]);
    let pathspec_before_patch = run_zmin_failure_output(repo.path(), &["stash", "file", "-p"]);

    assert_eq!(top_level.0, 129);
    assert_eq!(top_level.1, "");
    assert!(
        top_level
            .2
            .contains("error: unknown option `invalid-option"),
        "unexpected top-level stderr: {}",
        top_level.2
    );
    assert!(
        top_level.2.contains("or: git stash"),
        "top-level usage should include alternate stash forms: {}",
        top_level.2
    );

    assert_eq!(push.0, 129);
    assert_eq!(push.1, "");
    assert!(
        push.2.contains("error: unknown option `invalid-option"),
        "unexpected push stderr: {}",
        push.2
    );
    assert!(
        !push.2.contains("or: git stash"),
        "push usage should not include top-level alternate forms: {}",
        push.2
    );

    assert_eq!(help.0, 129);
    assert_eq!(help.2, "");
    assert!(
        help.1.contains("usage: git stash list"),
        "top-level help should print stash usage to stdout: {}",
        help.1
    );
    assert!(
        help.1.contains("or: git stash show"),
        "top-level help should include alternate stash forms: {}",
        help.1
    );

    assert_eq!(ambiguous.0, 128);
    assert_eq!(ambiguous.1, "");
    assert!(
        ambiguous
            .2
            .contains("subcommand wasn't specified; 'push' can't be assumed"),
        "unexpected ambiguous top-level stderr: {}",
        ambiguous.2
    );

    assert_eq!(pathspec_before_patch.0, 128);
    assert_eq!(pathspec_before_patch.1, "");
    assert!(
        pathspec_before_patch.2.contains(
            "subcommand wasn't specified; 'push' can't be assumed due to unexpected token 'file'"
        ),
        "unexpected pathspec-before-patch stderr: {}",
        pathspec_before_patch.2
    );
}

#[test]
fn stash_list_max_count_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    for message in ["first", "second", "third"] {
        write_file(git_repo.path(), "a.txt", &format!("one\n{message}\n"));
        write_file(zmin_repo.path(), "a.txt", &format!("one\n{message}\n"));
        git_with_env(git_repo.path(), ["stash", "push", "-m", message]);
        run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", message]);
    }

    for args in [
        ["stash", "list", "--max-count=1"].as_slice(),
        ["stash", "list", "-1"].as_slice(),
        ["stash", "list", "-n", "2"].as_slice(),
        ["stash", "list", "--max-count=0"].as_slice(),
        ["stash", "list", "--skip=1"].as_slice(),
        ["stash", "list", "--skip", "2"].as_slice(),
        ["stash", "list", "--skip=1", "--max-count=1"].as_slice(),
        ["stash", "list", "--grep=ir"].as_slice(),
        ["stash", "list", "--grep", "second"].as_slice(),
        ["stash", "list", "--grep=ir", "--max-count=1"].as_slice(),
        ["stash", "list", "--grep=missing"].as_slice(),
        ["stash", "list", "--walk-reflogs"].as_slice(),
        ["stash", "list", "--no-walk"].as_slice(),
        ["stash", "list", "--grep=ir", "--invert-grep"].as_slice(),
        ["stash", "list", "--grep=IR", "--regexp-ignore-case"].as_slice(),
        ["stash", "list", "--grep=IR", "-i", "--invert-grep"].as_slice(),
        ["stash", "list", "--grep=first", "--grep=second"].as_slice(),
        [
            "stash",
            "list",
            "--grep=first",
            "--grep=second",
            "--all-match",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash list output should match for {args:?}",
        );
    }
    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["stash", "list", "--max-count=bad"]).0,
        git_failure_output(git_repo.path(), &["stash", "list", "--max-count=bad"]).0
    );
    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["stash", "list", "--skip=bad"]).0,
        git_failure_output(git_repo.path(), &["stash", "list", "--skip=bad"]).0
    );

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["stash", "clear"]);
        write_file(repo, "a.txt", "one\na.b\n");
        git_with_env(repo, ["stash", "push", "-m", "a.b"]);
        write_file(repo, "a.txt", "one\naxb\n");
        git_with_env(repo, ["stash", "push", "-m", "axb"]);
    }
    for args in [
        ["stash", "list", "--grep=a.b"].as_slice(),
        ["stash", "list", "--extended-regexp", "--grep=a.b|missing"].as_slice(),
        ["stash", "list", "-E", "--grep=a.b|missing"].as_slice(),
        ["stash", "list", "--fixed-strings", "--grep=a.b"].as_slice(),
        ["stash", "list", "-F", "--grep=a.b"].as_slice(),
        ["stash", "list", "-F", "-E", "--grep=a.b|missing"].as_slice(),
        ["stash", "list", "-E", "-F", "--grep=a.b|missing"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash list grep should match for {args:?}",
        );
    }
}

#[test]
fn stash_push_staged_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "b.txt", "base\n");
        git(repo, ["add", "b.txt"]);
        git_with_env(repo, ["commit", "-m", "add b"]);
        write_file(repo, "a.txt", "one\nstaged\n");
        git(repo, ["add", "a.txt"]);
        write_file(repo, "b.txt", "base\nunstaged\n");
    }

    assert_eq!(
        run_zmin_with_env(
            zmin_repo.path(),
            ["stash", "push", "--staged", "-m", "staged"]
        ),
        git_with_env(
            git_repo.path(),
            ["stash", "push", "--staged", "-m", "staged"]
        )
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("b.txt")).expect("read zmin b"),
        fs::read_to_string(git_repo.path().join("b.txt")).expect("read git b")
    );
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "show", "--patch", "--stat"]),
        git_args(git_repo.path(), &["stash", "show", "--patch", "--stat"])
    );

    let git_clean = stash_fixture_repo();
    let zmin_clean = clone_repo_fixture(git_clean.path());
    configure_identity(zmin_clean.path());
    assert_eq!(
        run_zmin_with_env(
            zmin_clean.path(),
            ["stash", "push", "--no-staged", "--staged", "--no-staged"]
        ),
        git_with_env(
            git_clean.path(),
            ["stash", "push", "--no-staged", "--staged", "--no-staged"]
        )
    );

    write_file(git_clean.path(), "a.txt", "one\nstaged\n");
    write_file(zmin_clean.path(), "a.txt", "one\nstaged\n");
    git(git_clean.path(), ["add", "a.txt"]);
    git(zmin_clean.path(), ["add", "a.txt"]);
    assert_eq!(
        run_zmin_failure_output(zmin_clean.path(), &["stash", "push", "--staged", "-u"]).0,
        git_failure_output(git_clean.path(), &["stash", "push", "--staged", "-u"]).0
    );
    assert_eq!(
        run_zmin_failure_output(zmin_clean.path(), &["stash", "push", "--staged", "-a"]).0,
        git_failure_output(git_clean.path(), &["stash", "push", "--staged", "-a"]).0
    );
}

#[test]
fn stash_push_keep_index_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "b.txt", "base\n");
        git(repo, ["add", "b.txt"]);
        git_with_env(repo, ["commit", "-m", "add b"]);
        write_file(repo, "a.txt", "one\nstaged\n");
        git(repo, ["add", "a.txt"]);
        write_file(repo, "b.txt", "base\nunstaged\n");
    }

    assert_eq!(
        run_zmin_with_env(
            zmin_repo.path(),
            ["stash", "push", "--keep-index", "-m", "keep"]
        ),
        git_with_env(
            git_repo.path(),
            ["stash", "push", "--keep-index", "-m", "keep"]
        )
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("b.txt")).expect("read zmin b"),
        fs::read_to_string(git_repo.path().join("b.txt")).expect("read git b")
    );
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "show", "--patch", "--stat"]),
        git_args(git_repo.path(), &["stash", "show", "--patch", "--stat"])
    );

    let git_reset = stash_fixture_repo();
    let zmin_reset = clone_repo_fixture(git_reset.path());
    configure_identity(zmin_reset.path());
    for repo in [git_reset.path(), zmin_reset.path()] {
        write_file(repo, "a.txt", "one\nchange\n");
        git(repo, ["add", "a.txt"]);
    }
    assert_eq!(
        run_zmin_with_env(
            zmin_reset.path(),
            [
                "stash",
                "push",
                "--keep-index",
                "--no-keep-index",
                "-m",
                "reset"
            ]
        ),
        git_with_env(
            git_reset.path(),
            [
                "stash",
                "push",
                "--keep-index",
                "--no-keep-index",
                "-m",
                "reset"
            ]
        )
    );
    assert_eq!(
        git(zmin_reset.path(), ["status", "--short"]),
        git(git_reset.path(), ["status", "--short"])
    );
}

#[test]
fn stash_show_diff_options_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\ntwo\n");
    write_file(zmin_repo.path(), "a.txt", "one\ntwo\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "diff"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "diff"]);

    for args in [
        ["stash", "show", "--stat"].as_slice(),
        ["stash", "show", "--patch-with-stat"].as_slice(),
        ["stash", "show", "--patch-with-raw"].as_slice(),
        ["stash", "show", "--no-patch"].as_slice(),
        ["stash", "show", "-s"].as_slice(),
        ["stash", "show", "--patch"].as_slice(),
        ["stash", "show", "--patch", "--stat"].as_slice(),
        ["stash", "show", "--numstat"].as_slice(),
        ["stash", "show", "--shortstat"].as_slice(),
        ["stash", "show", "--summary"].as_slice(),
        ["stash", "show", "--raw"].as_slice(),
        ["stash", "show", "--raw", "-z"].as_slice(),
        ["stash", "show", "--raw", "--abbrev=12"].as_slice(),
        ["stash", "show", "--raw", "--abbrev"].as_slice(),
        ["stash", "show", "--raw", "--abbrev=12", "--no-abbrev"].as_slice(),
        ["stash", "show", "--raw", "--patch"].as_slice(),
        ["stash", "show", "--raw", "--patch", "--abbrev=12"].as_slice(),
        ["stash", "show", "--patch", "--full-index"].as_slice(),
        ["stash", "show", "--patch", "--full-index", "--abbrev=12"].as_slice(),
        [
            "stash",
            "show",
            "--patch",
            "--full-index",
            "--no-full-index",
        ]
        .as_slice(),
        ["stash", "show", "--no-ext-diff", "--patch"].as_slice(),
        ["stash", "show", "--no-textconv", "--patch"].as_slice(),
        ["stash", "show", "--no-renames", "--patch"].as_slice(),
        ["stash", "show", "--no-color", "--patch"].as_slice(),
        ["stash", "show", "--no-color-moved", "--patch"].as_slice(),
        ["stash", "show", "--no-color-moved-ws", "--patch"].as_slice(),
        ["stash", "show", "--default-prefix", "--patch"].as_slice(),
        ["stash", "show", "--no-prefix", "--patch"].as_slice(),
        ["stash", "show", "--src-prefix=old/", "--patch"].as_slice(),
        ["stash", "show", "--dst-prefix=new/", "--patch"].as_slice(),
        [
            "stash",
            "show",
            "--src-prefix=old/",
            "--dst-prefix=new/",
            "--patch",
        ]
        .as_slice(),
        [
            "stash",
            "show",
            "--no-prefix",
            "--src-prefix=old/",
            "--patch",
        ]
        .as_slice(),
        ["stash", "show", "--raw", "--numstat"].as_slice(),
        ["stash", "show", "--name-only"].as_slice(),
        ["stash", "show", "--name-only", "-z"].as_slice(),
        ["stash", "show", "--name-status"].as_slice(),
        ["stash", "show", "--name-status", "-z"].as_slice(),
        ["stash", "show", "--numstat", "-z"].as_slice(),
        ["stash", "show", "--stat", "--name-only"].as_slice(),
        ["stash", "show", "--patch", "--name-status"].as_slice(),
        ["stash", "show", "--include-untracked"].as_slice(),
        ["stash", "show", "--only-untracked"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show output should match for {args:?}",
        );
    }
    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["stash", "show", "--bad"]).0,
        git_failure_output(git_repo.path(), &["stash", "show", "--bad"]).0
    );
}

#[test]
fn stash_show_quiet_and_exit_code_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\ntwo\n");
    write_file(zmin_repo.path(), "a.txt", "one\ntwo\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "diff"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "diff"]);

    for args in [
        ["stash", "show", "--exit-code"].as_slice(),
        ["stash", "show", "--exit-code", "--patch"].as_slice(),
        ["stash", "show", "--exit-code", "--no-exit-code"].as_slice(),
        ["stash", "show", "--no-exit-code", "--exit-code"].as_slice(),
        ["stash", "show", "--quiet"].as_slice(),
        ["stash", "show", "--quiet", "--no-quiet"].as_slice(),
        ["stash", "show", "--no-quiet", "--quiet"].as_slice(),
        ["stash", "show", "--quiet", "--no-exit-code"].as_slice(),
        ["stash", "show", "--quiet", "--no-quiet", "--exit-code"].as_slice(),
        ["stash", "show", "--only-untracked", "--exit-code"].as_slice(),
        ["stash", "show", "--only-untracked", "--quiet"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin"),
            command_any_output("git", git_repo.path(), args, "git"),
            "stash show status/output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_unified_context_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    let base = (1..=12)
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let changed = base.replace("3\n", "three\n").replace("10\n", "ten\n");
    write_file(git_repo.path(), "a.txt", &base);
    write_file(zmin_repo.path(), "a.txt", &base);
    git(git_repo.path(), ["add", "a.txt"]);
    run_zmin(zmin_repo.path(), ["add", "a.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "multi-line"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "multi-line"]);

    write_file(git_repo.path(), "a.txt", &changed);
    write_file(zmin_repo.path(), "a.txt", &changed);
    git_with_env(git_repo.path(), ["stash", "push", "-m", "context"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "context"]);

    for args in [
        ["stash", "show", "--unified=0", "--patch"].as_slice(),
        ["stash", "show", "-U0", "--patch"].as_slice(),
        ["stash", "show", "--unified=1", "--patch"].as_slice(),
        [
            "stash",
            "show",
            "--inter-hunk-context=6",
            "--unified=0",
            "--patch",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show context output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_whitespace_diff_options_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "a  b\nkeep\n\n");
    write_file(zmin_repo.path(), "a.txt", "a  b\nkeep\n\n");
    git(git_repo.path(), ["add", "a.txt"]);
    run_zmin(zmin_repo.path(), ["add", "a.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "whitespace-base"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "whitespace-base"]);

    write_file(git_repo.path(), "a.txt", "a b  \nkeep\n\nextra\n");
    write_file(zmin_repo.path(), "a.txt", "a b  \nkeep\n\nextra\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "whitespace"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "whitespace"]);

    for args in [
        ["stash", "show", "--ignore-space-at-eol"].as_slice(),
        ["stash", "show", "--ignore-space-change"].as_slice(),
        ["stash", "show", "-b"].as_slice(),
        ["stash", "show", "--ignore-all-space"].as_slice(),
        ["stash", "show", "-w"].as_slice(),
        ["stash", "show", "--patch", "--ignore-space-change"].as_slice(),
        ["stash", "show", "--ignore-space-change", "--stat"].as_slice(),
        ["stash", "show", "--patch", "--ignore-blank-lines"].as_slice(),
        ["stash", "show", "--stat", "--ignore-blank-lines"].as_slice(),
        ["stash", "show", "--numstat", "--ignore-blank-lines"].as_slice(),
        ["stash", "show", "--shortstat", "--ignore-blank-lines"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show whitespace output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_ignore_matching_lines_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(
        git_repo.path(),
        "noise.txt",
        "keep\nDEBUG old\nTRACE old\nkeep2\n",
    );
    write_file(
        zmin_repo.path(),
        "noise.txt",
        "keep\nDEBUG old\nTRACE old\nkeep2\n",
    );
    git(git_repo.path(), ["add", "noise.txt"]);
    run_zmin(zmin_repo.path(), ["add", "noise.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "noise-base"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "noise-base"]);

    write_file(
        git_repo.path(),
        "noise.txt",
        "keep\nDEBUG new\nTRACE new\nkeep2\n",
    );
    write_file(
        zmin_repo.path(),
        "noise.txt",
        "keep\nDEBUG new\nTRACE new\nkeep2\n",
    );
    git_with_env(git_repo.path(), ["stash", "push", "-m", "noise"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "noise"]);

    for args in [
        ["stash", "show", "-IDEBUG|TRACE"].as_slice(),
        ["stash", "show", "--ignore-matching-lines=DEBUG|TRACE"].as_slice(),
        ["stash", "show", "--stat", "-IDEBUG|TRACE"].as_slice(),
        ["stash", "show", "--numstat", "-IDEBUG|TRACE"].as_slice(),
        ["stash", "show", "--shortstat", "-IDEBUG|TRACE"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show ignore-matching-lines output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_diff_algorithm_flags_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(
        git_repo.path(),
        "algorithm.txt",
        "alpha\ncommon\nbeta\ncommon\ngamma\n",
    );
    write_file(
        zmin_repo.path(),
        "algorithm.txt",
        "alpha\ncommon\nbeta\ncommon\ngamma\n",
    );
    git(git_repo.path(), ["add", "algorithm.txt"]);
    run_zmin(zmin_repo.path(), ["add", "algorithm.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "algorithm-base"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "algorithm-base"]);

    write_file(
        git_repo.path(),
        "algorithm.txt",
        "alpha\ncommon\nbeta changed\ncommon\ngamma\n",
    );
    write_file(
        zmin_repo.path(),
        "algorithm.txt",
        "alpha\ncommon\nbeta changed\ncommon\ngamma\n",
    );
    git_with_env(git_repo.path(), ["stash", "push", "-m", "algorithm"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "algorithm"]);

    for args in [
        ["stash", "show", "--minimal"].as_slice(),
        ["stash", "show", "--patience"].as_slice(),
        ["stash", "show", "--histogram"].as_slice(),
        ["stash", "show", "--diff-algorithm=myers"].as_slice(),
        ["stash", "show", "--anchored=common"].as_slice(),
        ["stash", "show", "--patch", "--diff-algorithm=histogram"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show algorithm option should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_diff_filter_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "mod.txt", "old\n");
    write_file(git_repo.path(), "del.txt", "gone\n");
    write_file(zmin_repo.path(), "mod.txt", "old\n");
    write_file(zmin_repo.path(), "del.txt", "gone\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "filter-base"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "filter-base"]);

    write_file(git_repo.path(), "mod.txt", "new\n");
    write_file(git_repo.path(), "add.txt", "add\n");
    fs::remove_file(git_repo.path().join("del.txt")).expect("remove git deleted file");
    write_file(zmin_repo.path(), "mod.txt", "new\n");
    write_file(zmin_repo.path(), "add.txt", "add\n");
    fs::remove_file(zmin_repo.path().join("del.txt")).expect("remove zmin deleted file");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["stash", "push", "-m", "filter"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "filter"]);

    for args in [
        ["stash", "show", "--name-status", "--diff-filter=A"].as_slice(),
        ["stash", "show", "--name-status", "--diff-filter=D"].as_slice(),
        ["stash", "show", "--name-status", "--diff-filter=M"].as_slice(),
        ["stash", "show", "--name-status", "--diff-filter=a"].as_slice(),
        ["stash", "show", "--stat", "--diff-filter=AD"].as_slice(),
        ["stash", "show", "--patch", "--diff-filter=A"].as_slice(),
        ["stash", "show", "--diff-filter=A"].as_slice(),
        ["stash", "show", "--numstat", "--diff-filter=DM"].as_slice(),
        ["stash", "show", "--shortstat", "--diff-filter=m"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show diff-filter output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_rename_detection_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "old.txt", "same\n");
    write_file(zmin_repo.path(), "old.txt", "same\n");
    git(git_repo.path(), ["add", "old.txt"]);
    run_zmin(zmin_repo.path(), ["add", "old.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "rename-base"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "rename-base"]);

    git(git_repo.path(), ["mv", "old.txt", "new.txt"]);
    run_zmin(zmin_repo.path(), ["mv", "old.txt", "new.txt"]);
    git_with_env(git_repo.path(), ["stash", "push", "-m", "rename"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "rename"]);

    for args in [
        ["stash", "show", "--name-status"].as_slice(),
        ["stash", "show", "--name-status", "-M"].as_slice(),
        ["stash", "show", "--name-status", "--find-renames"].as_slice(),
        ["stash", "show", "--name-status", "--no-renames"].as_slice(),
        ["stash", "show", "--stat", "-M"].as_slice(),
        ["stash", "show", "--patch", "-M"].as_slice(),
        ["stash", "show", "--name-status", "--diff-filter=R"].as_slice(),
        [
            "stash",
            "show",
            "--name-status",
            "--no-renames",
            "--diff-filter=R",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show rename output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_copy_detection_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "old.txt", "same\n");
    write_file(zmin_repo.path(), "old.txt", "same\n");
    git(git_repo.path(), ["add", "old.txt"]);
    run_zmin(zmin_repo.path(), ["add", "old.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "copy-base"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "copy-base"]);

    fs::copy(
        git_repo.path().join("old.txt"),
        git_repo.path().join("copy.txt"),
    )
    .expect("copy git file");
    fs::copy(
        zmin_repo.path().join("old.txt"),
        zmin_repo.path().join("copy.txt"),
    )
    .expect("copy zmin file");
    git(git_repo.path(), ["add", "copy.txt"]);
    run_zmin(zmin_repo.path(), ["add", "copy.txt"]);
    git_with_env(git_repo.path(), ["stash", "push", "-m", "copy"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "copy"]);

    for args in [
        ["stash", "show", "--name-status"].as_slice(),
        ["stash", "show", "--name-status", "-C"].as_slice(),
        ["stash", "show", "--name-status", "--find-copies"].as_slice(),
        ["stash", "show", "--name-status", "--find-copies-harder"].as_slice(),
        ["stash", "show", "--stat", "--find-copies-harder"].as_slice(),
        ["stash", "show", "--patch", "--find-copies-harder"].as_slice(),
        [
            "stash",
            "show",
            "--name-status",
            "--find-copies-harder",
            "--diff-filter=C",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show copy output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_break_rewrites_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    let base = (1..=100)
        .map(|value| format!("{value}\n"))
        .collect::<String>();
    write_file(git_repo.path(), "rewrite.txt", &base);
    write_file(zmin_repo.path(), "rewrite.txt", &base);
    git(git_repo.path(), ["add", "rewrite.txt"]);
    run_zmin(zmin_repo.path(), ["add", "rewrite.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "rewrite-base"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "rewrite-base"]);

    let changed = (201..=300)
        .map(|value| format!("{value}\n"))
        .collect::<String>();
    write_file(git_repo.path(), "rewrite.txt", &changed);
    write_file(zmin_repo.path(), "rewrite.txt", &changed);
    git_with_env(git_repo.path(), ["stash", "push", "-m", "rewrite"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "rewrite"]);

    for args in [
        ["stash", "show", "--patch", "-B"].as_slice(),
        ["stash", "show", "--patch", "--break-rewrites"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show break-rewrites output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_irreversible_delete_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "deleted.txt", "old\nline\n");
    write_file(zmin_repo.path(), "deleted.txt", "old\nline\n");
    git(git_repo.path(), ["add", "deleted.txt"]);
    run_zmin(zmin_repo.path(), ["add", "deleted.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "delete-base"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "delete-base"]);

    fs::remove_file(git_repo.path().join("deleted.txt")).expect("remove git file");
    fs::remove_file(zmin_repo.path().join("deleted.txt")).expect("remove zmin file");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "delete"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "delete"]);

    for args in [
        ["stash", "show", "--patch", "-D"].as_slice(),
        ["stash", "show", "--patch", "--irreversible-delete"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show irreversible-delete output should match for {args:?}",
        );
    }
}

#[test]
fn stash_reference_commands_accept_no_quiet_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nfirst\n");
    write_file(zmin_repo.path(), "a.txt", "one\nfirst\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "first"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "first"]);

    git_with_env(
        git_repo.path(),
        ["stash", "apply", "--quiet", "--no-quiet", "--no-index"],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["stash", "apply", "--quiet", "--no-quiet", "--no-index"],
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(zmin_repo.path(), ["reset", "--hard"]);
    git_with_env(
        git_repo.path(),
        ["stash", "pop", "--quiet", "--no-quiet", "--no-index"],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["stash", "pop", "--quiet", "--no-quiet", "--no-index"],
    );
    assert_eq!(run_zmin(zmin_repo.path(), ["stash", "list"]), "");

    write_file(git_repo.path(), "a.txt", "one\nsecond\n");
    write_file(zmin_repo.path(), "a.txt", "one\nsecond\n");
    git_with_env(git_repo.path(), ["stash", "push", "-q", "-m", "second"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-q", "-m", "second"]);
    git(git_repo.path(), ["stash", "drop", "--quiet", "--no-quiet"]);
    run_zmin(
        zmin_repo.path(),
        ["stash", "drop", "--quiet", "--no-quiet"],
    );
    assert_eq!(run_zmin(zmin_repo.path(), ["stash", "list"]), "");
}

fn normalize_stash_oneline_hashes(output: &str) -> String {
    output
        .lines()
        .map(|line| {
            let Some((_, rest)) = line.split_once(' ') else {
                return line.to_owned();
            };
            format!("<hash> {rest}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn stash_branch_checks_out_base_applies_and_drops_selected_entry_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nfirst\n");
    write_file(zmin_repo.path(), "a.txt", "one\nfirst\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "first"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "first"]);

    write_file(git_repo.path(), "a.txt", "one\nsecond\n");
    write_file(zmin_repo.path(), "a.txt", "one\nsecond\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "second"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "push", "-m", "second"]);

    git_with_env(
        git_repo.path(),
        ["stash", "branch", "from-first", "stash@{1}"],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["stash", "branch", "from-first", "stash@{1}"],
    );
    assert_eq!(
        git(zmin_repo.path(), ["branch", "--show-current"]),
        git(git_repo.path(), ["branch", "--show-current"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_branch_accepts_raw_create_commit_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nraw\n");
    write_file(zmin_repo.path(), "a.txt", "one\nraw\n");
    let git_id = git(git_repo.path(), ["stash", "create"]);
    let zmin_id = run_zmin(zmin_repo.path(), ["stash", "create"]);

    git(git_repo.path(), ["reset", "--hard"]);
    git(zmin_repo.path(), ["reset", "--hard"]);
    git_with_env(git_repo.path(), ["stash", "branch", "raw-branch", &git_id]);
    run_zmin_with_env(
        zmin_repo.path(),
        ["stash", "branch", "raw-branch", &zmin_id],
    );

    assert_eq!(
        git(zmin_repo.path(), ["branch", "--show-current"]),
        git(git_repo.path(), ["branch", "--show-current"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_branch_without_name_errors_like_stock_git() {
    let repo = stash_fixture_repo();

    assert_eq!(
        run_zmin_failure_output(repo.path(), &["stash", "branch"]),
        git_failure_output(repo.path(), &["stash", "branch"])
    );
}

#[test]
fn stash_numeric_selector_branch_then_checkout_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "changed\n");
    write_file(zmin_repo.path(), "a.txt", "changed\n");
    write_file(git_repo.path(), "file2", "staged\n");
    write_file(zmin_repo.path(), "file2", "staged\n");
    git(git_repo.path(), ["add", "file2"]);
    run_zmin(zmin_repo.path(), ["add", "file2"]);
    git_with_env(git_repo.path(), ["stash"]);
    run_zmin_with_env(zmin_repo.path(), ["stash"]);

    git_with_env(git_repo.path(), ["stash", "branch", "tmp", "0"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "branch", "tmp", "0"]);
    git(git_repo.path(), ["checkout", "main"]);
    run_zmin(zmin_repo.path(), ["checkout", "main"]);

    assert_eq!(
        git(zmin_repo.path(), ["branch", "--show-current"]),
        git(git_repo.path(), ["branch", "--show-current"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
}

#[test]
fn stash_branch_keeps_stash_when_apply_would_overwrite_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\ninitial\n");
    write_file(zmin_repo.path(), "a.txt", "one\ninitial\n");
    git(git_repo.path(), ["add", "a.txt"]);
    run_zmin(zmin_repo.path(), ["add", "a.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "initial"]);

    write_file(git_repo.path(), "a.txt", "one\nstashed\n");
    write_file(zmin_repo.path(), "a.txt", "one\nstashed\n");
    git_with_env(git_repo.path(), ["stash"]);
    run_zmin_with_env(zmin_repo.path(), ["stash"]);
    write_file(git_repo.path(), "a.txt", "one\ndirty\n");
    write_file(zmin_repo.path(), "a.txt", "one\ndirty\n");

    assert_eq!(
        run_zmin_status(
            zmin_repo.path(),
            ["stash", "branch", "apply-fails", "stash@{0}"]
        ),
        git_status(
            git_repo.path(),
            ["stash", "branch", "apply-fails", "stash@{0}"]
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_create_and_store_match_stock_git_stack_behavior() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    assert_eq!(git(git_repo.path(), ["stash", "create"]), "");
    assert_eq!(run_zmin(zmin_repo.path(), ["stash", "create"]), "");

    write_file(git_repo.path(), "a.txt", "one\ncreated\n");
    write_file(zmin_repo.path(), "a.txt", "one\ncreated\n");
    let git_id = git(git_repo.path(), ["stash", "create", "custom"]);
    let zmin_id = run_zmin(zmin_repo.path(), ["stash", "create", "custom"]);
    assert_eq!(git_id.len(), 40);
    assert_eq!(zmin_id.len(), 40);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &["stash", "store", "-m", "stored", &zmin_id],
        ),
        git_args(
            git_repo.path(),
            &["stash", "store", "-m", "stored", &git_id],
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
    assert!(
        run_zmin_args(zmin_repo.path(), &["reflog", "--format=%H", "stash"])
            .lines()
            .any(|line| line == zmin_id)
    );
    assert!(
        git_args(git_repo.path(), &["reflog", "--format=%H", "stash"])
            .lines()
            .any(|line| line == git_id)
    );
    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["stash", "store", "HEAD"]),
        git_failure_output(git_repo.path(), &["stash", "store", "HEAD"])
    );

    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["stash", "store", &zmin_id]),
        git_args(git_repo.path(), &["stash", "store", &git_id])
    );
    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &["stash", "store", "--quiet", "--no-quiet", &zmin_id],
        ),
        git_args(
            git_repo.path(),
            &["stash", "store", "--quiet", "--no-quiet", &git_id],
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git(git_repo.path(), ["stash", "clear"]);
    run_zmin(zmin_repo.path(), ["stash", "clear"]);
    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &["stash", "store", "-munspaced store", &zmin_id],
        ),
        git_args(
            git_repo.path(),
            &["stash", "store", "-munspaced store", &git_id],
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git(git_repo.path(), ["stash", "clear"]);
    run_zmin(zmin_repo.path(), ["stash", "clear"]);
    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &[
                "stash",
                "store",
                "--message=custom",
                "--no-message",
                &zmin_id,
            ],
        ),
        git_args(
            git_repo.path(),
            &[
                "stash",
                "store",
                "--message=custom",
                "--no-message",
                &git_id,
            ],
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git(git_repo.path(), ["stash", "clear"]);
    run_zmin(zmin_repo.path(), ["stash", "clear"]);
    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &[
                "stash",
                "store",
                "--no-message",
                "--message=after-no",
                &zmin_id,
            ],
        ),
        git_args(
            git_repo.path(),
            &[
                "stash",
                "store",
                "--no-message",
                "--message=after-no",
                &git_id,
            ],
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(zmin_repo.path(), ["reset", "--hard"]);
    git_with_env(git_repo.path(), ["stash", "apply"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "apply"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
}

#[test]
fn stash_create_reports_locked_index_like_stock_git() {
    let repo = stash_fixture_repo();
    write_file(repo.path(), "a.txt", "changed\n");
    fs::write(repo.path().join(".git/index.lock"), "").expect("write index lock");

    let output = run_zmin_failure_output(repo.path(), &["stash", "create"]);
    assert_eq!(output.0, 1);
    assert!(output.1.is_empty());
    assert!(
        output.2.contains("error: could not write index"),
        "stderr should report index write failure: {output:?}"
    );
    assert!(
        output.2.contains("error: Unable to create")
            && output.2.contains("index.lock")
            && output.2.contains("File exists"),
        "stderr should report index.lock creation failure: {output:?}"
    );
}

#[test]
fn stash_push_reports_locked_index_like_stock_git() {
    let repo = stash_fixture_repo();
    write_file(repo.path(), "a.txt", "changed\n");
    fs::write(repo.path().join(".git/index.lock"), "").expect("write index lock");

    let output = run_zmin_failure_output(repo.path(), &["stash", "push"]);
    assert_eq!(output.0, 1);
    assert!(output.1.is_empty());
    assert!(
        output.2.contains("error: could not write index"),
        "stderr should report index write failure: {output:?}"
    );
    assert!(
        output.2.contains("error: Unable to create")
            && output.2.contains("index.lock")
            && output.2.contains("File exists"),
        "stderr should report index.lock creation failure: {output:?}"
    );
}

#[test]
fn stash_apply_reports_locked_index_like_stock_git() {
    let repo = stash_fixture_repo();
    write_file(repo.path(), "a.txt", "changed\n");
    run_zmin_with_env(repo.path(), ["stash", "push"]);
    fs::write(repo.path().join(".git/index.lock"), "").expect("write index lock");

    let output = run_zmin_failure_output(repo.path(), &["stash", "apply"]);
    assert_eq!(output.0, 1);
    assert!(output.1.is_empty());
    assert!(
        output.2.contains("error: could not write index"),
        "stderr should report index write failure: {output:?}"
    );
    assert!(
        output.2.contains("error: Unable to create")
            && output.2.contains("index.lock")
            && output.2.contains("File exists"),
        "stderr should report index.lock creation failure: {output:?}"
    );
}

#[test]
fn stash_show_invalid_option_reports_usage_like_stock_git() {
    let repo = stash_fixture_repo();
    write_file(repo.path(), "a.txt", "changed\n");
    run_zmin_with_env(repo.path(), ["stash", "push"]);

    let output = run_zmin_failure_output(repo.path(), &["stash", "show", "-p", "--invalid"]);
    assert_eq!(output.0, 129);
    assert!(output.1.is_empty());
    assert!(
        output.2.contains("usage: git stash show"),
        "stderr should include stash show usage: {output:?}"
    );
}

#[test]
fn stash_store_negative_arguments_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\ncreated\n");
    write_file(zmin_repo.path(), "a.txt", "one\ncreated\n");
    let git_stash_id = git(git_repo.path(), ["stash", "create", "custom"]);
    let zmin_stash_id = run_zmin(zmin_repo.path(), ["stash", "create", "custom"]);
    let git_head_id = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let zmin_head_id = run_zmin(zmin_repo.path(), ["rev-parse", "HEAD"]);

    let cases = [
        vec!["stash", "store"],
        vec!["stash", "store", "--message", "msg"],
        vec!["stash", "store", "--bad", git_stash_id.as_str()],
        vec![
            "stash",
            "store",
            git_stash_id.as_str(),
            git_stash_id.as_str(),
        ],
        vec!["stash", "store", "deadbeef"],
        vec!["stash", "store", git_head_id.as_str()],
    ];
    let zmin_cases = [
        vec!["stash", "store"],
        vec!["stash", "store", "--message", "msg"],
        vec!["stash", "store", "--bad", zmin_stash_id.as_str()],
        vec![
            "stash",
            "store",
            zmin_stash_id.as_str(),
            zmin_stash_id.as_str(),
        ],
        vec!["stash", "store", "deadbeef"],
        vec!["stash", "store", zmin_head_id.as_str()],
    ];

    for (git_args, zmin_args) in cases.iter().zip(zmin_cases.iter()) {
        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), zmin_args),
            git_failure_output(git_repo.path(), git_args),
            "stash store failure output should match for {git_args:?}",
        );
    }
}

#[test]
fn stash_pathspec_from_file_limits_saved_changes_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "b.txt", "one\n");
    write_file(zmin_repo.path(), "b.txt", "one\n");
    git(git_repo.path(), ["add", "b.txt"]);
    git(zmin_repo.path(), ["add", "b.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "add b"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "add b"]);

    write_file(git_repo.path(), "a.txt", "one\nfrom-file\n");
    write_file(zmin_repo.path(), "a.txt", "one\nfrom-file\n");
    write_file(git_repo.path(), "b.txt", "one\nkept-local\n");
    write_file(zmin_repo.path(), "b.txt", "one\nkept-local\n");

    write_file(git_repo.path(), "paths.txt", "a.txt\n");
    write_file(zmin_repo.path(), "paths.txt", "a.txt\n");
    git_with_env(
        git_repo.path(),
        [
            "stash",
            "push",
            "-m",
            "from-file",
            "--pathspec-from-file",
            "paths.txt",
        ],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        [
            "stash",
            "push",
            "-m",
            "from-file",
            "--pathspec-from-file",
            "paths.txt",
        ],
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("b.txt")).expect("read zmin b"),
        fs::read_to_string(git_repo.path().join("b.txt")).expect("read git b")
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(zmin_repo.path(), ["reset", "--hard"]);
    write_file(git_repo.path(), "a.txt", "one\nstdin\n");
    write_file(zmin_repo.path(), "a.txt", "one\nstdin\n");
    write_file(git_repo.path(), "b.txt", "one\nstdin-kept\n");
    write_file(zmin_repo.path(), "b.txt", "one\nstdin-kept\n");
    git_with_stdin(
        git_repo.path(),
        [
            "stash",
            "push",
            "-m",
            "stdin-nul",
            "--pathspec-from-file",
            "-",
            "--pathspec-file-nul",
        ],
        "a.txt\0",
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        [
            "stash",
            "push",
            "-m",
            "stdin-nul",
            "--pathspec-from-file",
            "-",
            "--pathspec-file-nul",
        ],
        "a.txt\0",
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_patch_selects_hunks_and_leaves_rejected_hunks_like_stock_git() {
    let git_repo = git_init();
    configure_identity(git_repo.path());
    git(git_repo.path(), ["checkout", "-b", "main"]);
    let base = (1..=12)
        .map(|line| format!("line {line}\n"))
        .collect::<String>();
    write_file(git_repo.path(), "a.txt", &base);
    git(git_repo.path(), ["add", "a.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "base"]);
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    let changed = base
        .replace("line 2\n", "line 2 changed\n")
        .replace("line 10\n", "line 10 changed\n");
    write_file(git_repo.path(), "a.txt", &changed);
    write_file(zmin_repo.path(), "a.txt", &changed);

    git_with_stdin(
        git_repo.path(),
        ["stash", "push", "--patch", "-m", "patchy"],
        "y\nn\n",
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        ["stash", "push", "--patch", "-m", "patchy"],
        "y\nn\n",
    );

    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "show", "--patch"]),
        git(git_repo.path(), ["stash", "show", "--patch"])
    );
}

#[test]
fn stash_patch_all_done_quit_and_pathspec_match_stock_git() {
    let git_repo = git_init();
    configure_identity(git_repo.path());
    git(git_repo.path(), ["checkout", "-b", "main"]);
    let base = (1..=12)
        .map(|line| format!("line {line}\n"))
        .collect::<String>();
    write_file(git_repo.path(), "a.txt", &base);
    write_file(git_repo.path(), "b.txt", &base);
    git(git_repo.path(), ["add", "a.txt", "b.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "base"]);
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    let changed = base
        .replace("line 2\n", "line 2 changed\n")
        .replace("line 10\n", "line 10 changed\n");
    write_file(git_repo.path(), "a.txt", &changed);
    write_file(zmin_repo.path(), "a.txt", &changed);
    write_file(git_repo.path(), "b.txt", &changed);
    write_file(zmin_repo.path(), "b.txt", &changed);
    git_with_stdin(
        git_repo.path(),
        ["stash", "push", "--patch", "-m", "all-a", "--", "a.txt"],
        "a\n",
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        ["stash", "push", "--patch", "-m", "all-a", "--", "a.txt"],
        "a\n",
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("b.txt")).expect("read zmin b"),
        fs::read_to_string(git_repo.path().join("b.txt")).expect("read git b")
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "show", "--patch"]),
        git(git_repo.path(), ["stash", "show", "--patch"])
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(zmin_repo.path(), ["reset", "--hard"]);
    write_file(git_repo.path(), "a.txt", &changed);
    write_file(zmin_repo.path(), "a.txt", &changed);
    assert_eq!(
        run_zmin_status_with_stdin(
            zmin_repo.path(),
            ["stash", "push", "--patch", "-m", "done"],
            "d\n",
        ),
        git_status_with_stdin(
            git_repo.path(),
            ["stash", "push", "--patch", "-m", "done"],
            "d\n",
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin done a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git done a")
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(zmin_repo.path(), ["reset", "--hard"]);
    write_file(git_repo.path(), "a.txt", &changed);
    write_file(zmin_repo.path(), "a.txt", &changed);
    write_file(git_repo.path(), "b.txt", &changed);
    write_file(zmin_repo.path(), "b.txt", &changed);
    assert_eq!(
        run_zmin_status_with_stdin(
            zmin_repo.path(),
            ["stash", "push", "--patch", "-m", "quit"],
            "q\ny\n",
        ),
        git_status_with_stdin(
            git_repo.path(),
            ["stash", "push", "--patch", "-m", "quit"],
            "q\ny\n",
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("a.txt")).expect("read zmin quit a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git quit a")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("b.txt")).expect("read zmin quit b"),
        fs::read_to_string(git_repo.path().join("b.txt")).expect("read git quit b")
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_patch_split_pathspec_restores_selected_hunk_like_stock_git() {
    let git_repo = git_init();
    configure_identity(git_repo.path());
    git(git_repo.path(), ["checkout", "-b", "main"]);
    write_file(git_repo.path(), "file", "b\nc\n");
    git(git_repo.path(), ["add", "file"]);
    git_with_env(git_repo.path(), ["commit", "-m", "add a few lines"]);
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "file", "a\nb\nc\nd\n");
    write_file(zmin_repo.path(), "file", "a\nb\nc\nd\n");
    write_file(git_repo.path(), "other-file", "changed-other-file\n");
    write_file(zmin_repo.path(), "other-file", "changed-other-file\n");

    git_with_stdin(
        git_repo.path(),
        ["stash", "push", "-m", "stash bar", "--patch", "file"],
        "s\ny\nn\n",
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        ["stash", "push", "-m", "stash bar", "--patch", "file"],
        "s\ny\nn\n",
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("file")).expect("read zmin file"),
        fs::read_to_string(git_repo.path().join("file")).expect("read git file")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("other-file")).expect("read zmin other"),
        fs::read_to_string(git_repo.path().join("other-file")).expect("read git other")
    );

    git(git_repo.path(), ["checkout", "HEAD", "--", "file"]);
    run_zmin(zmin_repo.path(), ["checkout", "HEAD", "--", "file"]);
    git_with_env(git_repo.path(), ["stash", "pop"]);
    run_zmin_with_env(zmin_repo.path(), ["stash", "pop"]);

    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("file")).expect("read zmin popped file"),
        fs::read_to_string(git_repo.path().join("file")).expect("read git popped file")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("other-file")).expect("read zmin popped other"),
        fs::read_to_string(git_repo.path().join("other-file")).expect("read git popped other")
    );
}
