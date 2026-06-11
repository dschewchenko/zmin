mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    clone_repo_fixture, command_any_output, configure_identity, git, git_args, git_failure_output,
    git_init, git_status_with_stdin, git_with_env, git_with_stdin, run_skron, run_skron_args,
    run_skron_failure_output, run_skron_status_with_stdin, run_skron_with_env,
    run_skron_with_stdin, skron_bin, write_file,
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
            run_skron_args(repo.path(), args),
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
        run_skron_args(repo.path(), &args),
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
            run_skron_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn stash_push_apply_pop_matches_stock_git_state() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "one\ntwo\n");
    write_file(skron_repo.path(), "a.txt", "one\ntwo\n");

    git_with_env(git_repo.path(), ["stash", "push", "-m", "work"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "work"]);
    assert_eq!(git(git_repo.path(), ["status", "--short"]), "");
    assert_eq!(git(skron_repo.path(), ["status", "--short"]), "");
    assert!(run_skron(skron_repo.path(), ["stash", "list"]).contains("stash@{0}: On main: work"));

    git(git_repo.path(), ["stash", "clear"]);
    run_skron(skron_repo.path(), ["stash", "clear"]);
    assert_eq!(run_skron(skron_repo.path(), ["stash", "list"]), "");

    write_file(git_repo.path(), "a.txt", "one\ntwo\n");
    write_file(skron_repo.path(), "a.txt", "one\ntwo\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "work"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "work"]);

    git_with_env(git_repo.path(), ["stash", "apply"]);
    run_skron_with_env(skron_repo.path(), ["stash", "apply"]);
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron file"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git file")
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(skron_repo.path(), ["reset", "--hard"]);
    git_with_env(git_repo.path(), ["stash", "pop"]);
    run_skron_with_env(skron_repo.path(), ["stash", "pop"]);
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron file"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git file")
    );
    assert_eq!(run_skron(skron_repo.path(), ["stash", "list"]), "");
}

#[test]
fn stash_include_untracked_matches_stock_git_state() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "one\ntwo\n");
    write_file(skron_repo.path(), "a.txt", "one\ntwo\n");
    write_file(git_repo.path(), "new.txt", "new\n");
    write_file(skron_repo.path(), "new.txt", "new\n");
    write_file(git_repo.path(), ".gitignore", "ignored.txt\n");
    write_file(skron_repo.path(), ".gitignore", "ignored.txt\n");
    write_file(git_repo.path(), "ignored.txt", "ignored\n");
    write_file(skron_repo.path(), "ignored.txt", "ignored\n");

    git_with_env(git_repo.path(), ["stash", "push", "-u", "-m", "save"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-u", "-m", "save"]);
    assert_eq!(
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git_with_env(git_repo.path(), ["stash", "pop"]);
    run_skron_with_env(skron_repo.path(), ["stash", "pop"]);
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("new.txt")).expect("read skron untracked"),
        fs::read_to_string(git_repo.path().join("new.txt")).expect("read git untracked")
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(run_skron(skron_repo.path(), ["stash", "list"]), "");
}

#[test]
fn stash_all_includes_ignored_files_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), ".gitignore", "ignored.txt\n");
    write_file(skron_repo.path(), ".gitignore", "ignored.txt\n");
    write_file(git_repo.path(), "new.txt", "new\n");
    write_file(skron_repo.path(), "new.txt", "new\n");
    write_file(git_repo.path(), "ignored.txt", "ignored\n");
    write_file(skron_repo.path(), "ignored.txt", "ignored\n");

    git_with_env(git_repo.path(), ["stash", "push", "-a", "-m", "all"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-a", "-m", "all"]);
    assert_eq!(
        git(skron_repo.path(), ["status", "--short", "--ignored"]),
        git(git_repo.path(), ["status", "--short", "--ignored"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git_with_env(git_repo.path(), ["stash", "pop"]);
    run_skron_with_env(skron_repo.path(), ["stash", "pop"]);
    assert_eq!(
        git(skron_repo.path(), ["status", "--short", "--ignored"]),
        git(git_repo.path(), ["status", "--short", "--ignored"])
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("ignored.txt")).expect("read skron ignored"),
        fs::read_to_string(git_repo.path().join("ignored.txt")).expect("read git ignored")
    );
}

#[test]
fn stash_pathspec_limits_saved_worktree_changes_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "b.txt", "one\n");
    write_file(skron_repo.path(), "b.txt", "one\n");
    git(git_repo.path(), ["add", "b.txt"]);
    git(skron_repo.path(), ["add", "b.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "add b"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "add b"]);

    write_file(git_repo.path(), "a.txt", "one\ntwo\n");
    write_file(skron_repo.path(), "a.txt", "one\ntwo\n");
    write_file(git_repo.path(), "b.txt", "one\ntwo\n");
    write_file(skron_repo.path(), "b.txt", "one\ntwo\n");

    git_with_env(
        git_repo.path(),
        ["stash", "push", "-m", "only-a", "--", "a.txt"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["stash", "push", "-m", "only-a", "--", "a.txt"],
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("b.txt")).expect("read skron b"),
        fs::read_to_string(git_repo.path().join("b.txt")).expect("read git b")
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git_with_env(git_repo.path(), ["stash", "pop"]);
    run_skron_with_env(skron_repo.path(), ["stash", "pop"]);
    assert_eq!(
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
}

#[test]
fn stash_pop_rejects_overlapping_dirty_paths_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nstashed\n");
    write_file(skron_repo.path(), "a.txt", "one\nstashed\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "overlap"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "overlap"]);

    write_file(git_repo.path(), "a.txt", "one\nlocal\n");
    write_file(skron_repo.path(), "a.txt", "one\nlocal\n");

    let git_result = git_failure_output(git_repo.path(), &["stash", "pop"]);
    let skron_result = run_skron_failure_output(skron_repo.path(), &["stash", "pop"]);
    assert_eq!(skron_result.0, git_result.0);
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_pop_drops_selected_stack_entry_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nfirst\n");
    write_file(skron_repo.path(), "a.txt", "one\nfirst\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "first"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "first"]);

    write_file(git_repo.path(), "a.txt", "one\nsecond\n");
    write_file(skron_repo.path(), "a.txt", "one\nsecond\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "second"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "second"]);

    git_with_env(git_repo.path(), ["stash", "pop", "stash@{1}"]);
    run_skron_with_env(skron_repo.path(), ["stash", "pop", "stash@{1}"]);
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    write_file(git_repo.path(), "a.txt", "one\nno-message\n");
    write_file(skron_repo.path(), "a.txt", "one\nno-message\n");
    assert_eq!(
        run_skron_args(
            skron_repo.path(),
            &["stash", "push", "--message=custom", "--no-message"],
        ),
        git_args(
            git_repo.path(),
            &["stash", "push", "--message=custom", "--no-message"],
        )
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    write_file(git_repo.path(), "a.txt", "one\nmessage-after-no\n");
    write_file(skron_repo.path(), "a.txt", "one\nmessage-after-no\n");
    assert_eq!(
        run_skron_args(
            skron_repo.path(),
            &["stash", "push", "--no-message", "--message=after-no"],
        ),
        git_args(
            git_repo.path(),
            &["stash", "push", "--no-message", "--message=after-no"],
        )
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(skron_repo.path(), ["reset", "--hard"]);
    write_file(git_repo.path(), "a.txt", "one\nno-file-a\n");
    write_file(skron_repo.path(), "a.txt", "one\nno-file-a\n");
    write_file(git_repo.path(), "b.txt", "one\nno-file-b\n");
    write_file(skron_repo.path(), "b.txt", "one\nno-file-b\n");
    write_file(git_repo.path(), "paths.txt", "b.txt\n");
    write_file(skron_repo.path(), "paths.txt", "b.txt\n");
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
    run_skron_with_env(
        skron_repo.path(),
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
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_skron_args(skron_repo.path(), &["stash", "show", "--patch", "--stat"]),
        git_args(git_repo.path(), &["stash", "show", "--patch", "--stat"])
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(skron_repo.path(), ["reset", "--hard"]);
    write_file(git_repo.path(), "a.txt", "one\nno-nul\n");
    write_file(skron_repo.path(), "a.txt", "one\nno-nul\n");
    write_file(git_repo.path(), "b.txt", "one\nno-nul-kept\n");
    write_file(skron_repo.path(), "b.txt", "one\nno-nul-kept\n");
    write_file(git_repo.path(), "paths.txt", "a.txt\n");
    write_file(skron_repo.path(), "paths.txt", "a.txt\n");
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
    run_skron_with_env(
        skron_repo.path(),
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
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_skron_args(skron_repo.path(), &["stash", "show", "--patch", "--stat"]),
        git_args(git_repo.path(), &["stash", "show", "--patch", "--stat"])
    );
}

#[test]
fn stash_apply_and_drop_selected_stack_entry_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nfirst\n");
    write_file(skron_repo.path(), "a.txt", "one\nfirst\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "first"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "first"]);

    write_file(git_repo.path(), "a.txt", "one\nsecond\n");
    write_file(skron_repo.path(), "a.txt", "one\nsecond\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "second"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "second"]);

    git_with_env(git_repo.path(), ["stash", "apply", "stash@{1}"]);
    run_skron_with_env(skron_repo.path(), ["stash", "apply", "stash@{1}"]);
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(skron_repo.path(), ["reset", "--hard"]);
    git(git_repo.path(), ["stash", "drop", "stash@{1}"]);
    run_skron(skron_repo.path(), ["stash", "drop", "stash@{1}"]);
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_quiet_options_match_stock_git_output_and_state() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    assert_eq!(
        run_skron_args(skron_repo.path(), &["stash", "push", "-q"]),
        git_args(git_repo.path(), &["stash", "push", "-q"])
    );

    write_file(git_repo.path(), "a.txt", "one\nquiet\n");
    write_file(skron_repo.path(), "a.txt", "one\nquiet\n");
    assert_eq!(
        run_skron_args(skron_repo.path(), &["stash", "push", "-q", "-m", "quiet"]),
        git_args(git_repo.path(), &["stash", "push", "-q", "-m", "quiet"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );

    assert_eq!(
        run_skron_args(skron_repo.path(), &["stash", "apply", "-q"]),
        git_args(git_repo.path(), &["stash", "apply", "-q"])
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    git(git_repo.path(), ["reset", "--hard"]);
    git(skron_repo.path(), ["reset", "--hard"]);
    assert_eq!(
        run_skron_args(skron_repo.path(), &["stash", "pop", "-q"]),
        git_args(git_repo.path(), &["stash", "pop", "-q"])
    );
    assert_eq!(run_skron(skron_repo.path(), ["stash", "list"]), "");

    git(git_repo.path(), ["reset", "--hard"]);
    git(skron_repo.path(), ["reset", "--hard"]);
    write_file(git_repo.path(), "a.txt", "one\nfirst\n");
    write_file(skron_repo.path(), "a.txt", "one\nfirst\n");
    git_with_env(git_repo.path(), ["stash", "push", "-q", "-m", "first"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-q", "-m", "first"]);
    write_file(git_repo.path(), "a.txt", "one\nsecond\n");
    write_file(skron_repo.path(), "a.txt", "one\nsecond\n");
    git_with_env(git_repo.path(), ["stash", "push", "-q", "-m", "second"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-q", "-m", "second"]);
    assert_eq!(
        run_skron_args(skron_repo.path(), &["stash", "drop", "-q", "stash@{1}"]),
        git_args(git_repo.path(), &["stash", "drop", "-q", "stash@{1}"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_push_message_equals_option_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nequals\n");
    write_file(skron_repo.path(), "a.txt", "one\nequals\n");
    assert_eq!(
        run_skron_args(skron_repo.path(), &["stash", "push", "--message=equals"]),
        git_args(git_repo.path(), &["stash", "push", "--message=equals"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_push_negated_options_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    for repo in [git_repo.path(), skron_repo.path()] {
        write_file(repo, ".gitignore", "*.log\n");
        write_file(repo, "ignored.log", "ignored\n");
        write_file(repo, "untracked.txt", "untracked\n");
    }
    assert_eq!(
        run_skron_args(
            skron_repo.path(),
            &["stash", "push", "--all", "--no-all", "--include-untracked"],
        ),
        git_args(
            git_repo.path(),
            &["stash", "push", "--all", "--no-all", "--include-untracked"],
        )
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--short", "--ignored"]),
        git(git_repo.path(), ["status", "--short", "--ignored"])
    );

    git(git_repo.path(), ["stash", "clear"]);
    run_skron(skron_repo.path(), ["stash", "clear"]);
    git(git_repo.path(), ["reset", "--hard"]);
    git(skron_repo.path(), ["reset", "--hard"]);
    for repo in [git_repo.path(), skron_repo.path()] {
        write_file(repo, "untracked.txt", "untracked\n");
    }
    assert_eq!(
        run_skron_args(
            skron_repo.path(),
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
        git(skron_repo.path(), ["status", "--short", "--ignored"]),
        git(git_repo.path(), ["status", "--short", "--ignored"])
    );

    write_file(git_repo.path(), "a.txt", "one\nquiet\n");
    write_file(skron_repo.path(), "a.txt", "one\nquiet\n");
    assert_eq!(
        run_skron_args(
            skron_repo.path(),
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
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nfirst\n");
    write_file(skron_repo.path(), "a.txt", "one\nfirst\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "first"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "first"]);
    write_file(git_repo.path(), "a.txt", "one\nsecond\n");
    write_file(skron_repo.path(), "a.txt", "one\nsecond\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "second"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "second"]);

    let skron_oneline = run_skron_args(skron_repo.path(), &["stash", "list", "--oneline"]);
    let git_oneline = git_args(git_repo.path(), &["stash", "list", "--oneline"]);
    assert_eq!(
        normalize_stash_oneline_hashes(&skron_oneline),
        normalize_stash_oneline_hashes(&git_oneline)
    );
    for args in [
        ["stash", "list", "--pretty=%H"].as_slice(),
        ["stash", "list", "--format=%H"].as_slice(),
    ] {
        let skron_hashes = run_skron_args(skron_repo.path(), args);
        let git_hashes = git_args(git_repo.path(), args);
        assert_eq!(
            skron_hashes.lines().count(),
            git_hashes.lines().count(),
            "stash list hash count should match for {args:?}",
        );
        assert!(
            skron_hashes
                .lines()
                .all(|line| line.len() == 40 && line.chars().all(|ch| ch.is_ascii_hexdigit())),
            "skron stash list should print full hashes for {args:?}: {skron_hashes:?}",
        );
    }
    assert_eq!(
        run_skron_failure_output(skron_repo.path(), &["stash", "list", "--bad"]).0,
        git_failure_output(git_repo.path(), &["stash", "list", "--bad"]).0
    );
}

#[test]
fn stash_list_max_count_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    for message in ["first", "second", "third"] {
        write_file(git_repo.path(), "a.txt", &format!("one\n{message}\n"));
        write_file(skron_repo.path(), "a.txt", &format!("one\n{message}\n"));
        git_with_env(git_repo.path(), ["stash", "push", "-m", message]);
        run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", message]);
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
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash list output should match for {args:?}",
        );
    }
    assert_eq!(
        run_skron_failure_output(skron_repo.path(), &["stash", "list", "--max-count=bad"]).0,
        git_failure_output(git_repo.path(), &["stash", "list", "--max-count=bad"]).0
    );
    assert_eq!(
        run_skron_failure_output(skron_repo.path(), &["stash", "list", "--skip=bad"]).0,
        git_failure_output(git_repo.path(), &["stash", "list", "--skip=bad"]).0
    );

    for repo in [git_repo.path(), skron_repo.path()] {
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
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash list grep should match for {args:?}",
        );
    }
}

#[test]
fn stash_push_staged_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    for repo in [git_repo.path(), skron_repo.path()] {
        write_file(repo, "b.txt", "base\n");
        git(repo, ["add", "b.txt"]);
        git_with_env(repo, ["commit", "-m", "add b"]);
        write_file(repo, "a.txt", "one\nstaged\n");
        git(repo, ["add", "a.txt"]);
        write_file(repo, "b.txt", "base\nunstaged\n");
    }

    assert_eq!(
        run_skron_with_env(
            skron_repo.path(),
            ["stash", "push", "--staged", "-m", "staged"]
        ),
        git_with_env(
            git_repo.path(),
            ["stash", "push", "--staged", "-m", "staged"]
        )
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("b.txt")).expect("read skron b"),
        fs::read_to_string(git_repo.path().join("b.txt")).expect("read git b")
    );
    assert_eq!(
        run_skron_args(skron_repo.path(), &["stash", "show", "--patch", "--stat"]),
        git_args(git_repo.path(), &["stash", "show", "--patch", "--stat"])
    );

    let git_clean = stash_fixture_repo();
    let skron_clean = clone_repo_fixture(git_clean.path());
    configure_identity(skron_clean.path());
    assert_eq!(
        run_skron_with_env(
            skron_clean.path(),
            ["stash", "push", "--no-staged", "--staged", "--no-staged"]
        ),
        git_with_env(
            git_clean.path(),
            ["stash", "push", "--no-staged", "--staged", "--no-staged"]
        )
    );

    write_file(git_clean.path(), "a.txt", "one\nstaged\n");
    write_file(skron_clean.path(), "a.txt", "one\nstaged\n");
    git(git_clean.path(), ["add", "a.txt"]);
    git(skron_clean.path(), ["add", "a.txt"]);
    assert_eq!(
        run_skron_failure_output(skron_clean.path(), &["stash", "push", "--staged", "-u"]).0,
        git_failure_output(git_clean.path(), &["stash", "push", "--staged", "-u"]).0
    );
    assert_eq!(
        run_skron_failure_output(skron_clean.path(), &["stash", "push", "--staged", "-a"]).0,
        git_failure_output(git_clean.path(), &["stash", "push", "--staged", "-a"]).0
    );
}

#[test]
fn stash_push_keep_index_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    for repo in [git_repo.path(), skron_repo.path()] {
        write_file(repo, "b.txt", "base\n");
        git(repo, ["add", "b.txt"]);
        git_with_env(repo, ["commit", "-m", "add b"]);
        write_file(repo, "a.txt", "one\nstaged\n");
        git(repo, ["add", "a.txt"]);
        write_file(repo, "b.txt", "base\nunstaged\n");
    }

    assert_eq!(
        run_skron_with_env(
            skron_repo.path(),
            ["stash", "push", "--keep-index", "-m", "keep"]
        ),
        git_with_env(
            git_repo.path(),
            ["stash", "push", "--keep-index", "-m", "keep"]
        )
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("b.txt")).expect("read skron b"),
        fs::read_to_string(git_repo.path().join("b.txt")).expect("read git b")
    );
    assert_eq!(
        run_skron_args(skron_repo.path(), &["stash", "show", "--patch", "--stat"]),
        git_args(git_repo.path(), &["stash", "show", "--patch", "--stat"])
    );

    let git_reset = stash_fixture_repo();
    let skron_reset = clone_repo_fixture(git_reset.path());
    configure_identity(skron_reset.path());
    for repo in [git_reset.path(), skron_reset.path()] {
        write_file(repo, "a.txt", "one\nchange\n");
        git(repo, ["add", "a.txt"]);
    }
    assert_eq!(
        run_skron_with_env(
            skron_reset.path(),
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
        git(skron_reset.path(), ["status", "--short"]),
        git(git_reset.path(), ["status", "--short"])
    );
}

#[test]
fn stash_show_diff_options_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "one\ntwo\n");
    write_file(skron_repo.path(), "a.txt", "one\ntwo\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "diff"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "diff"]);

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
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show output should match for {args:?}",
        );
    }
    assert_eq!(
        run_skron_failure_output(skron_repo.path(), &["stash", "show", "--bad"]).0,
        git_failure_output(git_repo.path(), &["stash", "show", "--bad"]).0
    );
}

#[test]
fn stash_show_quiet_and_exit_code_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "one\ntwo\n");
    write_file(skron_repo.path(), "a.txt", "one\ntwo\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "diff"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "diff"]);

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
            command_any_output(skron_bin(), skron_repo.path(), args, "skron"),
            command_any_output("git", git_repo.path(), args, "git"),
            "stash show status/output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_unified_context_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    let base = (1..=12)
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let changed = base.replace("3\n", "three\n").replace("10\n", "ten\n");
    write_file(git_repo.path(), "a.txt", &base);
    write_file(skron_repo.path(), "a.txt", &base);
    git(git_repo.path(), ["add", "a.txt"]);
    run_skron(skron_repo.path(), ["add", "a.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "multi-line"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "multi-line"]);

    write_file(git_repo.path(), "a.txt", &changed);
    write_file(skron_repo.path(), "a.txt", &changed);
    git_with_env(git_repo.path(), ["stash", "push", "-m", "context"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "context"]);

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
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show context output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_whitespace_diff_options_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "a  b\nkeep\n\n");
    write_file(skron_repo.path(), "a.txt", "a  b\nkeep\n\n");
    git(git_repo.path(), ["add", "a.txt"]);
    run_skron(skron_repo.path(), ["add", "a.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "whitespace-base"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "whitespace-base"]);

    write_file(git_repo.path(), "a.txt", "a b  \nkeep\n\nextra\n");
    write_file(skron_repo.path(), "a.txt", "a b  \nkeep\n\nextra\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "whitespace"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "whitespace"]);

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
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show whitespace output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_ignore_matching_lines_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(
        git_repo.path(),
        "noise.txt",
        "keep\nDEBUG old\nTRACE old\nkeep2\n",
    );
    write_file(
        skron_repo.path(),
        "noise.txt",
        "keep\nDEBUG old\nTRACE old\nkeep2\n",
    );
    git(git_repo.path(), ["add", "noise.txt"]);
    run_skron(skron_repo.path(), ["add", "noise.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "noise-base"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "noise-base"]);

    write_file(
        git_repo.path(),
        "noise.txt",
        "keep\nDEBUG new\nTRACE new\nkeep2\n",
    );
    write_file(
        skron_repo.path(),
        "noise.txt",
        "keep\nDEBUG new\nTRACE new\nkeep2\n",
    );
    git_with_env(git_repo.path(), ["stash", "push", "-m", "noise"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "noise"]);

    for args in [
        ["stash", "show", "-IDEBUG|TRACE"].as_slice(),
        ["stash", "show", "--ignore-matching-lines=DEBUG|TRACE"].as_slice(),
        ["stash", "show", "--stat", "-IDEBUG|TRACE"].as_slice(),
        ["stash", "show", "--numstat", "-IDEBUG|TRACE"].as_slice(),
        ["stash", "show", "--shortstat", "-IDEBUG|TRACE"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show ignore-matching-lines output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_diff_algorithm_flags_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(
        git_repo.path(),
        "algorithm.txt",
        "alpha\ncommon\nbeta\ncommon\ngamma\n",
    );
    write_file(
        skron_repo.path(),
        "algorithm.txt",
        "alpha\ncommon\nbeta\ncommon\ngamma\n",
    );
    git(git_repo.path(), ["add", "algorithm.txt"]);
    run_skron(skron_repo.path(), ["add", "algorithm.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "algorithm-base"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "algorithm-base"]);

    write_file(
        git_repo.path(),
        "algorithm.txt",
        "alpha\ncommon\nbeta changed\ncommon\ngamma\n",
    );
    write_file(
        skron_repo.path(),
        "algorithm.txt",
        "alpha\ncommon\nbeta changed\ncommon\ngamma\n",
    );
    git_with_env(git_repo.path(), ["stash", "push", "-m", "algorithm"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "algorithm"]);

    for args in [
        ["stash", "show", "--minimal"].as_slice(),
        ["stash", "show", "--patience"].as_slice(),
        ["stash", "show", "--histogram"].as_slice(),
        ["stash", "show", "--diff-algorithm=myers"].as_slice(),
        ["stash", "show", "--anchored=common"].as_slice(),
        ["stash", "show", "--patch", "--diff-algorithm=histogram"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show algorithm option should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_diff_filter_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "mod.txt", "old\n");
    write_file(git_repo.path(), "del.txt", "gone\n");
    write_file(skron_repo.path(), "mod.txt", "old\n");
    write_file(skron_repo.path(), "del.txt", "gone\n");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "filter-base"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "filter-base"]);

    write_file(git_repo.path(), "mod.txt", "new\n");
    write_file(git_repo.path(), "add.txt", "add\n");
    fs::remove_file(git_repo.path().join("del.txt")).expect("remove git deleted file");
    write_file(skron_repo.path(), "mod.txt", "new\n");
    write_file(skron_repo.path(), "add.txt", "add\n");
    fs::remove_file(skron_repo.path().join("del.txt")).expect("remove skron deleted file");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["stash", "push", "-m", "filter"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "filter"]);

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
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show diff-filter output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_rename_detection_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "old.txt", "same\n");
    write_file(skron_repo.path(), "old.txt", "same\n");
    git(git_repo.path(), ["add", "old.txt"]);
    run_skron(skron_repo.path(), ["add", "old.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "rename-base"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "rename-base"]);

    git(git_repo.path(), ["mv", "old.txt", "new.txt"]);
    run_skron(skron_repo.path(), ["mv", "old.txt", "new.txt"]);
    git_with_env(git_repo.path(), ["stash", "push", "-m", "rename"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "rename"]);

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
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show rename output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_copy_detection_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "old.txt", "same\n");
    write_file(skron_repo.path(), "old.txt", "same\n");
    git(git_repo.path(), ["add", "old.txt"]);
    run_skron(skron_repo.path(), ["add", "old.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "copy-base"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "copy-base"]);

    fs::copy(
        git_repo.path().join("old.txt"),
        git_repo.path().join("copy.txt"),
    )
    .expect("copy git file");
    fs::copy(
        skron_repo.path().join("old.txt"),
        skron_repo.path().join("copy.txt"),
    )
    .expect("copy skron file");
    git(git_repo.path(), ["add", "copy.txt"]);
    run_skron(skron_repo.path(), ["add", "copy.txt"]);
    git_with_env(git_repo.path(), ["stash", "push", "-m", "copy"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "copy"]);

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
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show copy output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_break_rewrites_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    let base = (1..=100)
        .map(|value| format!("{value}\n"))
        .collect::<String>();
    write_file(git_repo.path(), "rewrite.txt", &base);
    write_file(skron_repo.path(), "rewrite.txt", &base);
    git(git_repo.path(), ["add", "rewrite.txt"]);
    run_skron(skron_repo.path(), ["add", "rewrite.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "rewrite-base"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "rewrite-base"]);

    let changed = (201..=300)
        .map(|value| format!("{value}\n"))
        .collect::<String>();
    write_file(git_repo.path(), "rewrite.txt", &changed);
    write_file(skron_repo.path(), "rewrite.txt", &changed);
    git_with_env(git_repo.path(), ["stash", "push", "-m", "rewrite"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "rewrite"]);

    for args in [
        ["stash", "show", "--patch", "-B"].as_slice(),
        ["stash", "show", "--patch", "--break-rewrites"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show break-rewrites output should match for {args:?}",
        );
    }
}

#[test]
fn stash_show_irreversible_delete_matches_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "deleted.txt", "old\nline\n");
    write_file(skron_repo.path(), "deleted.txt", "old\nline\n");
    git(git_repo.path(), ["add", "deleted.txt"]);
    run_skron(skron_repo.path(), ["add", "deleted.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "delete-base"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "delete-base"]);

    fs::remove_file(git_repo.path().join("deleted.txt")).expect("remove git file");
    fs::remove_file(skron_repo.path().join("deleted.txt")).expect("remove skron file");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "delete"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "delete"]);

    for args in [
        ["stash", "show", "--patch", "-D"].as_slice(),
        ["stash", "show", "--patch", "--irreversible-delete"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "stash show irreversible-delete output should match for {args:?}",
        );
    }
}

#[test]
fn stash_reference_commands_accept_no_quiet_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nfirst\n");
    write_file(skron_repo.path(), "a.txt", "one\nfirst\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "first"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "first"]);

    git_with_env(
        git_repo.path(),
        ["stash", "apply", "--quiet", "--no-quiet", "--no-index"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["stash", "apply", "--quiet", "--no-quiet", "--no-index"],
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(skron_repo.path(), ["reset", "--hard"]);
    git_with_env(
        git_repo.path(),
        ["stash", "pop", "--quiet", "--no-quiet", "--no-index"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["stash", "pop", "--quiet", "--no-quiet", "--no-index"],
    );
    assert_eq!(run_skron(skron_repo.path(), ["stash", "list"]), "");

    write_file(git_repo.path(), "a.txt", "one\nsecond\n");
    write_file(skron_repo.path(), "a.txt", "one\nsecond\n");
    git_with_env(git_repo.path(), ["stash", "push", "-q", "-m", "second"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-q", "-m", "second"]);
    git(git_repo.path(), ["stash", "drop", "--quiet", "--no-quiet"]);
    run_skron(
        skron_repo.path(),
        ["stash", "drop", "--quiet", "--no-quiet"],
    );
    assert_eq!(run_skron(skron_repo.path(), ["stash", "list"]), "");
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
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "one\nfirst\n");
    write_file(skron_repo.path(), "a.txt", "one\nfirst\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "first"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "first"]);

    write_file(git_repo.path(), "a.txt", "one\nsecond\n");
    write_file(skron_repo.path(), "a.txt", "one\nsecond\n");
    git_with_env(git_repo.path(), ["stash", "push", "-m", "second"]);
    run_skron_with_env(skron_repo.path(), ["stash", "push", "-m", "second"]);

    git_with_env(
        git_repo.path(),
        ["stash", "branch", "from-first", "stash@{1}"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["stash", "branch", "from-first", "stash@{1}"],
    );
    assert_eq!(
        git(skron_repo.path(), ["branch", "--show-current"]),
        git(git_repo.path(), ["branch", "--show-current"])
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}

#[test]
fn stash_create_and_store_match_stock_git_stack_behavior() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    assert_eq!(git(git_repo.path(), ["stash", "create"]), "");
    assert_eq!(run_skron(skron_repo.path(), ["stash", "create"]), "");

    write_file(git_repo.path(), "a.txt", "one\ncreated\n");
    write_file(skron_repo.path(), "a.txt", "one\ncreated\n");
    let git_id = git(git_repo.path(), ["stash", "create", "custom"]);
    let skron_id = run_skron(skron_repo.path(), ["stash", "create", "custom"]);
    assert_eq!(git_id.len(), 40);
    assert_eq!(skron_id.len(), 40);
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    assert_eq!(
        run_skron_args(
            skron_repo.path(),
            &["stash", "store", "-m", "stored", &skron_id],
        ),
        git_args(
            git_repo.path(),
            &["stash", "store", "-m", "stored", &git_id],
        )
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    assert_eq!(
        run_skron_args(skron_repo.path(), &["stash", "store", &skron_id]),
        git_args(git_repo.path(), &["stash", "store", &git_id])
    );
    assert_eq!(
        run_skron_args(
            skron_repo.path(),
            &["stash", "store", "--quiet", "--no-quiet", &skron_id],
        ),
        git_args(
            git_repo.path(),
            &["stash", "store", "--quiet", "--no-quiet", &git_id],
        )
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git(git_repo.path(), ["stash", "clear"]);
    run_skron(skron_repo.path(), ["stash", "clear"]);
    assert_eq!(
        run_skron_args(
            skron_repo.path(),
            &[
                "stash",
                "store",
                "--message=custom",
                "--no-message",
                &skron_id,
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
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git(git_repo.path(), ["stash", "clear"]);
    run_skron(skron_repo.path(), ["stash", "clear"]);
    assert_eq!(
        run_skron_args(
            skron_repo.path(),
            &[
                "stash",
                "store",
                "--no-message",
                "--message=after-no",
                &skron_id,
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
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(skron_repo.path(), ["reset", "--hard"]);
    git_with_env(git_repo.path(), ["stash", "apply"]);
    run_skron_with_env(skron_repo.path(), ["stash", "apply"]);
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
}

#[test]
fn stash_store_negative_arguments_match_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "one\ncreated\n");
    write_file(skron_repo.path(), "a.txt", "one\ncreated\n");
    let git_stash_id = git(git_repo.path(), ["stash", "create", "custom"]);
    let skron_stash_id = run_skron(skron_repo.path(), ["stash", "create", "custom"]);
    let git_head_id = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let skron_head_id = run_skron(skron_repo.path(), ["rev-parse", "HEAD"]);

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
    let skron_cases = [
        vec!["stash", "store"],
        vec!["stash", "store", "--message", "msg"],
        vec!["stash", "store", "--bad", skron_stash_id.as_str()],
        vec![
            "stash",
            "store",
            skron_stash_id.as_str(),
            skron_stash_id.as_str(),
        ],
        vec!["stash", "store", "deadbeef"],
        vec!["stash", "store", skron_head_id.as_str()],
    ];

    for (git_args, skron_args) in cases.iter().zip(skron_cases.iter()) {
        assert_eq!(
            run_skron_failure_output(skron_repo.path(), skron_args),
            git_failure_output(git_repo.path(), git_args),
            "stash store failure output should match for {git_args:?}",
        );
    }
}

#[test]
fn stash_pathspec_from_file_limits_saved_changes_like_stock_git() {
    let git_repo = stash_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "b.txt", "one\n");
    write_file(skron_repo.path(), "b.txt", "one\n");
    git(git_repo.path(), ["add", "b.txt"]);
    git(skron_repo.path(), ["add", "b.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "add b"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "add b"]);

    write_file(git_repo.path(), "a.txt", "one\nfrom-file\n");
    write_file(skron_repo.path(), "a.txt", "one\nfrom-file\n");
    write_file(git_repo.path(), "b.txt", "one\nkept-local\n");
    write_file(skron_repo.path(), "b.txt", "one\nkept-local\n");

    write_file(git_repo.path(), "paths.txt", "a.txt\n");
    write_file(skron_repo.path(), "paths.txt", "a.txt\n");
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
    run_skron_with_env(
        skron_repo.path(),
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
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("b.txt")).expect("read skron b"),
        fs::read_to_string(git_repo.path().join("b.txt")).expect("read git b")
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(skron_repo.path(), ["reset", "--hard"]);
    write_file(git_repo.path(), "a.txt", "one\nstdin\n");
    write_file(skron_repo.path(), "a.txt", "one\nstdin\n");
    write_file(git_repo.path(), "b.txt", "one\nstdin-kept\n");
    write_file(skron_repo.path(), "b.txt", "one\nstdin-kept\n");
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
    run_skron_with_stdin(
        skron_repo.path(),
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
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
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
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    let changed = base
        .replace("line 2\n", "line 2 changed\n")
        .replace("line 10\n", "line 10 changed\n");
    write_file(git_repo.path(), "a.txt", &changed);
    write_file(skron_repo.path(), "a.txt", &changed);

    git_with_stdin(
        git_repo.path(),
        ["stash", "push", "--patch", "-m", "patchy"],
        "y\nn\n",
    );
    run_skron_with_stdin(
        skron_repo.path(),
        ["stash", "push", "--patch", "-m", "patchy"],
        "y\nn\n",
    );

    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "show", "--patch"]),
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
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(skron_repo.path());

    let changed = base
        .replace("line 2\n", "line 2 changed\n")
        .replace("line 10\n", "line 10 changed\n");
    write_file(git_repo.path(), "a.txt", &changed);
    write_file(skron_repo.path(), "a.txt", &changed);
    write_file(git_repo.path(), "b.txt", &changed);
    write_file(skron_repo.path(), "b.txt", &changed);
    git_with_stdin(
        git_repo.path(),
        ["stash", "push", "--patch", "-m", "all-a", "--", "a.txt"],
        "a\n",
    );
    run_skron_with_stdin(
        skron_repo.path(),
        ["stash", "push", "--patch", "-m", "all-a", "--", "a.txt"],
        "a\n",
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("b.txt")).expect("read skron b"),
        fs::read_to_string(git_repo.path().join("b.txt")).expect("read git b")
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "show", "--patch"]),
        git(git_repo.path(), ["stash", "show", "--patch"])
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(skron_repo.path(), ["reset", "--hard"]);
    write_file(git_repo.path(), "a.txt", &changed);
    write_file(skron_repo.path(), "a.txt", &changed);
    assert_eq!(
        run_skron_status_with_stdin(
            skron_repo.path(),
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
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron done a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git done a")
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );

    git(git_repo.path(), ["reset", "--hard"]);
    git(skron_repo.path(), ["reset", "--hard"]);
    write_file(git_repo.path(), "a.txt", &changed);
    write_file(skron_repo.path(), "a.txt", &changed);
    write_file(git_repo.path(), "b.txt", &changed);
    write_file(skron_repo.path(), "b.txt", &changed);
    assert_eq!(
        run_skron_status_with_stdin(
            skron_repo.path(),
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
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron quit a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git quit a")
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("b.txt")).expect("read skron quit b"),
        fs::read_to_string(git_repo.path().join("b.txt")).expect("read git quit b")
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["stash", "list"]),
        git(git_repo.path(), ["stash", "list"])
    );
}
