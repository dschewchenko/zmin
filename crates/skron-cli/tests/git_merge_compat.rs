mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    configure_identity, git, git_args, git_failure_output, git_init, git_status, git_with_env,
    run_skron, run_skron_args, run_skron_failure_output, run_skron_status, run_skron_with_env,
    write_file,
};

fn two_commit_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"one\n").expect("write first");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    fs::write(repo.path().join("a.txt"), b"two\n").expect("write second");
    fs::write(repo.path().join("b.txt"), b"two\n").expect("write added");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    repo
}

fn committed_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "hello\n");
    run_skron(repo.path(), ["add", "-A"]);
    run_skron_with_env(repo.path(), ["commit", "-m", "initial"]);
    repo
}

#[test]
fn merge_base_matches_stock_git_for_linear_history() {
    let repo = two_commit_repo();
    run_skron(repo.path(), ["tag", "-a", "v2", "-m", "full", "HEAD~1"]);
    let tag_full = git(repo.path(), ["rev-parse", "v2"]);
    let tree_full = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);

    assert_eq!(
        run_skron(repo.path(), ["merge-base", "HEAD", "HEAD~1"]),
        git(repo.path(), ["merge-base", "HEAD", "HEAD~1"])
    );
    assert_eq!(
        run_skron(repo.path(), ["merge-base", "HEAD", "HEAD"]),
        git(repo.path(), ["merge-base", "HEAD", "HEAD"])
    );
    assert_eq!(
        run_skron_status(repo.path(), ["merge-base", "--is-ancestor", "HEAD", "HEAD"]),
        git_status(repo.path(), ["merge-base", "--is-ancestor", "HEAD", "HEAD"])
    );
    assert_eq!(
        run_skron_status(
            repo.path(),
            ["merge-base", "--is-ancestor", "HEAD~1", "HEAD"]
        ),
        git_status(
            repo.path(),
            ["merge-base", "--is-ancestor", "HEAD~1", "HEAD"]
        )
    );
    assert_eq!(
        run_skron_status(
            repo.path(),
            ["merge-base", "--is-ancestor", "HEAD", "HEAD~1"]
        ),
        git_status(
            repo.path(),
            ["merge-base", "--is-ancestor", "HEAD", "HEAD~1"]
        )
    );

    assert_eq!(
        run_skron_status(
            repo.path(),
            ["merge-base", "--is-ancestor", &tag_full, "HEAD"]
        ),
        git_status(
            repo.path(),
            ["merge-base", "--is-ancestor", &tag_full, "HEAD"]
        )
    );

    assert_eq!(
        run_skron_status(
            repo.path(),
            ["merge-base", "--is-ancestor", &tree_full, "HEAD"]
        ),
        git_status(
            repo.path(),
            ["merge-base", "--is-ancestor", &tree_full, "HEAD"]
        )
    );
}

#[test]
fn merge_base_multi_commit_modes_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());

    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["branch", "c"]);

    write_file(repo.path(), "shared.txt", "shared\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "shared"]);
    git(repo.path(), ["branch", "a"]);
    git(repo.path(), ["branch", "b"]);

    git(repo.path(), ["checkout", "a"]);
    write_file(repo.path(), "a.txt", "a\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "a"]);

    git(repo.path(), ["checkout", "b"]);
    write_file(repo.path(), "b.txt", "b\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "b"]);

    git(repo.path(), ["checkout", "c"]);
    write_file(repo.path(), "c.txt", "c\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "c"]);

    let plain = run_skron(repo.path(), ["merge-base", "a", "b", "c"]);
    let octopus = run_skron(repo.path(), ["merge-base", "--octopus", "a", "b", "c"]);
    assert_eq!(plain, git(repo.path(), ["merge-base", "a", "b", "c"]));
    assert_eq!(
        octopus,
        git(repo.path(), ["merge-base", "--octopus", "a", "b", "c"])
    );
    assert_ne!(plain, octopus);
    assert_eq!(
        run_skron_status(repo.path(), ["merge-base", "--is-ancestor", "a", "b", "c"]),
        git_status(repo.path(), ["merge-base", "--is-ancestor", "a", "b", "c"])
    );
}

#[test]
fn merge_tree_matches_stock_git_for_trivial_three_tree_merges() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    let base_commit = git(repo.path(), ["rev-parse", "HEAD"]);
    let base_tree = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);

    git(repo.path(), ["switch", "-c", "remote-add"]);
    write_file(repo.path(), "b.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "remote add"]);
    let remote_add_tree = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);

    assert_eq!(
        run_skron_args(
            repo.path(),
            &["merge-tree", &base_tree, &base_tree, &remote_add_tree]
        ),
        git_args(
            repo.path(),
            &["merge-tree", &base_tree, &base_tree, &remote_add_tree]
        )
    );

    git(repo.path(), ["switch", "--detach", &base_commit]);
    git(repo.path(), ["switch", "-c", "ours-change"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "ours change"]);
    let ours_tree = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);

    git(repo.path(), ["switch", "--detach", &base_commit]);
    git(repo.path(), ["switch", "-c", "theirs-change"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "theirs change"]);
    let theirs_tree = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);

    assert_eq!(
        run_skron_args(
            repo.path(),
            &["merge-tree", &base_tree, &base_tree, &theirs_tree]
        ),
        git_args(
            repo.path(),
            &["merge-tree", &base_tree, &base_tree, &theirs_tree]
        )
    );
    assert_eq!(
        run_skron_args(
            repo.path(),
            &["merge-tree", &base_tree, &ours_tree, &theirs_tree]
        ),
        git_args(
            repo.path(),
            &["merge-tree", &base_tree, &ours_tree, &theirs_tree]
        )
    );
}

#[test]
fn merge_ff_only_matches_stock_git_state_and_exit_codes() {
    let git_repo = committed_repo();
    let skron_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    git(git_repo.path(), ["switch", "-c", "feature"]);
    run_skron(skron_repo.path(), ["switch", "-c", "feature"]);
    fs::write(git_repo.path().join("feature.txt"), b"feature\n").expect("write git feature");
    fs::write(skron_repo.path().join("feature.txt"), b"feature\n").expect("write skron feature");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "feature"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "feature"]);

    git(git_repo.path(), ["switch", &default_branch]);
    run_skron(skron_repo.path(), ["switch", &default_branch]);
    git(git_repo.path(), ["merge", "--ff-only", "feature"]);
    run_skron(skron_repo.path(), ["merge", "--ff-only", "feature"]);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read(skron_repo.path().join("feature.txt")).expect("read merged feature"),
        b"feature\n"
    );

    let git_repo = committed_repo();
    let skron_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);
    git(git_repo.path(), ["switch", "-c", "feature"]);
    run_skron(skron_repo.path(), ["switch", "-c", "feature"]);
    fs::write(git_repo.path().join("feature.txt"), b"feature\n").expect("write git feature");
    fs::write(skron_repo.path().join("feature.txt"), b"feature\n").expect("write skron feature");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "feature"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "feature"]);
    git(git_repo.path(), ["switch", &default_branch]);
    run_skron(skron_repo.path(), ["switch", &default_branch]);
    fs::write(git_repo.path().join("main.txt"), b"main\n").expect("write git main");
    fs::write(skron_repo.path().join("main.txt"), b"main\n").expect("write skron main");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "main"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "main"]);

    assert_eq!(
        run_skron_status(skron_repo.path(), ["merge", "--ff-only", "feature"]),
        git_status(git_repo.path(), ["merge", "--ff-only", "feature"])
    );
}

#[test]
fn merge_no_ff_creates_merge_commit_for_fast_forwardable_branch() {
    let git_repo = committed_repo();
    let skron_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    for repo in [git_repo.path(), skron_repo.path()] {
        git(repo, ["switch", "-c", "feature"]);
        write_file(repo, "feature.txt", "feature\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "feature"]);
        git(repo, ["switch", &default_branch]);
    }

    let git_out = git_args(git_repo.path(), &["merge", "--no-ff", "feature"]);
    let skron_out = run_skron_args(skron_repo.path(), &["merge", "--no-ff", "feature"]);
    assert_eq!(skron_out.lines().next(), git_out.lines().next());
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_repo.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(skron_repo.path(), ["rev-parse", "feature^{tree}"])
    );
    assert_eq!(
        git(skron_repo.path(), ["rev-list", "--parents", "-1", "HEAD"])
            .split_whitespace()
            .count(),
        3
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn merge_no_commit_leaves_merge_state_without_advancing_head() {
    let git_repo = committed_repo();
    let skron_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    for repo in [git_repo.path(), skron_repo.path()] {
        git(repo, ["switch", "-c", "feature"]);
        write_file(repo, "feature.txt", "feature\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "feature"]);
        git(repo, ["switch", &default_branch]);
        write_file(repo, "main.txt", "main\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "main"]);
    }

    let git_head_before = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let skron_head_before = git(skron_repo.path(), ["rev-parse", "HEAD"]);
    git(git_repo.path(), ["merge", "--no-commit", "feature"]);
    run_skron(skron_repo.path(), ["merge", "--no-commit", "feature"]);

    assert_eq!(git(git_repo.path(), ["rev-parse", "HEAD"]), git_head_before);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        skron_head_before
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join(".git/MERGE_HEAD")).expect("skron MERGE_HEAD"),
        fs::read_to_string(git_repo.path().join(".git/MERGE_HEAD")).expect("git MERGE_HEAD")
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
    assert_eq!(
        git(skron_repo.path(), ["write-tree"]),
        git(git_repo.path(), ["write-tree"])
    );
    assert!(skron_repo.path().join("feature.txt").exists());
}

#[test]
fn merge_squash_stages_result_without_merge_head() {
    let git_repo = committed_repo();
    let skron_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    for repo in [git_repo.path(), skron_repo.path()] {
        git(repo, ["switch", "-c", "feature"]);
        write_file(repo, "feature.txt", "feature\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "feature"]);
        git(repo, ["switch", &default_branch]);
        write_file(repo, "main.txt", "main\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "main"]);
    }

    let git_head_before = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let skron_head_before = git(skron_repo.path(), ["rev-parse", "HEAD"]);
    let git_out = git_args(git_repo.path(), &["merge", "--squash", "feature"]);
    let skron_out = run_skron_args(skron_repo.path(), &["merge", "--squash", "feature"]);

    assert_eq!(skron_out.lines().next(), git_out.lines().next());
    assert_eq!(git(git_repo.path(), ["rev-parse", "HEAD"]), git_head_before);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        skron_head_before
    );
    assert!(!skron_repo.path().join(".git/MERGE_HEAD").exists());
    assert!(skron_repo.path().join(".git/SQUASH_MSG").exists());
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
    assert_eq!(
        git(skron_repo.path(), ["write-tree"]),
        git(git_repo.path(), ["write-tree"])
    );
}

#[test]
fn merge_non_ff_clean_changes_creates_stock_compatible_merge_commit() {
    let git_repo = committed_repo();
    let skron_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    for repo in [git_repo.path(), skron_repo.path()] {
        git(repo, ["switch", "-c", "feature"]);
        write_file(repo, "feature.txt", "feature\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "feature"]);
        git(repo, ["switch", &default_branch]);
        write_file(repo, "main.txt", "main\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "main"]);
    }

    git_with_env(git_repo.path(), ["merge", "feature"]);
    run_skron_with_env(skron_repo.path(), ["merge", "feature"]);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_repo.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(skron_repo.path(), ["rev-list", "--parents", "-1", "HEAD"]),
        git(git_repo.path(), ["rev-list", "--parents", "-1", "HEAD"])
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read(skron_repo.path().join("feature.txt")).expect("read feature"),
        b"feature\n"
    );
    assert_eq!(
        fs::read(skron_repo.path().join("main.txt")).expect("read main"),
        b"main\n"
    );
}

#[test]
fn merge_strategy_output_matches_stock_git_for_default_ort_and_recursive() {
    for strategy in [None, Some("ort"), Some("recursive")] {
        let git_repo = committed_repo();
        let skron_repo = committed_repo();
        let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

        for repo in [git_repo.path(), skron_repo.path()] {
            git(repo, ["switch", "-c", "feature"]);
            write_file(repo, "feature.txt", "feature\n");
            git(repo, ["add", "-A"]);
            git_with_env(repo, ["commit", "-m", "feature"]);
            git(repo, ["switch", &default_branch]);
            write_file(repo, "main.txt", "main\n");
            git(repo, ["add", "-A"]);
            git_with_env(repo, ["commit", "-m", "main"]);
        }

        let git_out = match strategy {
            Some(name) => git_args(git_repo.path(), &["merge", "-s", name, "feature"]),
            None => git_args(git_repo.path(), &["merge", "feature"]),
        };
        let skron_out = match strategy {
            Some(name) => run_skron_args(skron_repo.path(), &["merge", "-s", name, "feature"]),
            None => run_skron_args(skron_repo.path(), &["merge", "feature"]),
        };
        assert_eq!(
            skron_out.lines().next(),
            git_out.lines().next(),
            "strategy={strategy:?}"
        );
        assert_eq!(
            git(skron_repo.path(), ["rev-parse", "HEAD^{tree}"]),
            git(git_repo.path(), ["rev-parse", "HEAD^{tree}"])
        );
        assert_eq!(
            git(skron_repo.path(), ["rev-list", "--parents", "-1", "HEAD"])
                .split_whitespace()
                .count(),
            3
        );
    }
}

#[test]
fn merge_conflict_leaves_unmerged_index_and_merge_state_like_stock_git() {
    let git_repo = committed_repo();
    let skron_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    for repo in [git_repo.path(), skron_repo.path()] {
        git(repo, ["switch", "-c", "feature"]);
        write_file(repo, "a.txt", "feature\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "feature"]);
        git(repo, ["switch", &default_branch]);
        write_file(repo, "a.txt", "main\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "main"]);
    }

    let git_failure = git_failure_output(git_repo.path(), &["merge", "feature"]);
    let skron_failure = run_skron_failure_output(skron_repo.path(), &["merge", "feature"]);
    assert_eq!(skron_failure.0, git_failure.0);
    assert_eq!(
        git(skron_repo.path(), ["ls-files", "-u"]),
        git(git_repo.path(), ["ls-files", "-u"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join(".git/MERGE_HEAD")).expect("skron MERGE_HEAD"),
        fs::read_to_string(git_repo.path().join(".git/MERGE_HEAD")).expect("git MERGE_HEAD")
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("skron conflicted file"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("git conflicted file")
    );

    write_file(git_repo.path(), "a.txt", "resolved\n");
    write_file(skron_repo.path(), "a.txt", "resolved\n");
    git(git_repo.path(), ["add", "a.txt"]);
    run_skron(skron_repo.path(), ["add", "a.txt"]);
    git_with_env(
        git_repo.path(),
        ["-c", "core.editor=:", "merge", "--continue"],
    );
    run_skron_with_env(skron_repo.path(), ["merge", "--continue"]);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_repo.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(skron_repo.path(), ["rev-list", "--parents", "-1", "HEAD"])
            .split_whitespace()
            .count(),
        3
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
    assert!(!skron_repo.path().join(".git/MERGE_HEAD").exists());
}

#[test]
fn merge_abort_during_conflict_restores_head_like_stock_git() {
    let git_repo = committed_repo();
    let skron_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    for repo in [git_repo.path(), skron_repo.path()] {
        git(repo, ["switch", "-c", "feature"]);
        write_file(repo, "a.txt", "feature\n");
        write_file(repo, "feature.txt", "feature only\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "feature"]);
        git(repo, ["switch", &default_branch]);
        write_file(repo, "a.txt", "main\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "main"]);
    }

    let _ = git_failure_output(git_repo.path(), &["merge", "feature"]);
    let _ = run_skron_failure_output(skron_repo.path(), &["merge", "feature"]);
    git(git_repo.path(), ["merge", "--abort"]);
    run_skron(skron_repo.path(), ["merge", "--abort"]);

    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        skron_repo.path().join("feature.txt").exists(),
        git_repo.path().join("feature.txt").exists()
    );
    assert!(!skron_repo.path().join(".git/MERGE_HEAD").exists());
}

#[test]
fn merge_binary_conflict_matches_stock_git_index_and_worktree() {
    let git_repo = committed_repo();
    let skron_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("bin.dat"), b"base\0x").expect("write base binary");
        git(repo, ["add", "bin.dat"]);
        git_with_env(repo, ["commit", "-m", "base binary"]);
        git(repo, ["switch", "-c", "feature"]);
        fs::write(repo.join("bin.dat"), b"feature\0x").expect("write feature binary");
        git(repo, ["add", "bin.dat"]);
        git_with_env(repo, ["commit", "-m", "feature binary"]);
        git(repo, ["switch", &default_branch]);
        fs::write(repo.join("bin.dat"), b"main\0x").expect("write main binary");
        git(repo, ["add", "bin.dat"]);
        git_with_env(repo, ["commit", "-m", "main binary"]);
    }

    let git_failure = git_failure_output(git_repo.path(), &["merge", "feature"]);
    let skron_failure = run_skron_failure_output(skron_repo.path(), &["merge", "feature"]);
    assert_eq!(skron_failure.0, git_failure.0);
    assert_eq!(
        git(skron_repo.path(), ["ls-files", "-u"]),
        git(git_repo.path(), ["ls-files", "-u"])
    );
    assert_eq!(
        fs::read(skron_repo.path().join("bin.dat")).expect("read skron binary"),
        fs::read(git_repo.path().join("bin.dat")).expect("read git binary")
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
}

#[test]
fn merge_modify_delete_conflicts_match_stock_git_index_and_worktree() {
    for delete_on_feature in [true, false] {
        let git_repo = committed_repo();
        let skron_repo = committed_repo();
        let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

        for repo in [git_repo.path(), skron_repo.path()] {
            write_file(repo, "a.txt", "base\n");
            git(repo, ["add", "a.txt"]);
            git_with_env(repo, ["commit", "-m", "base file"]);
            git(repo, ["switch", "-c", "feature"]);
            if delete_on_feature {
                fs::remove_file(repo.join("a.txt")).expect("delete feature file");
                git(repo, ["rm", "a.txt"]);
                git_with_env(repo, ["commit", "-m", "delete feature"]);
                git(repo, ["switch", &default_branch]);
                write_file(repo, "a.txt", "main\n");
                git(repo, ["add", "a.txt"]);
                git_with_env(repo, ["commit", "-m", "modify main"]);
            } else {
                write_file(repo, "a.txt", "feature\n");
                git(repo, ["add", "a.txt"]);
                git_with_env(repo, ["commit", "-m", "modify feature"]);
                git(repo, ["switch", &default_branch]);
                fs::remove_file(repo.join("a.txt")).expect("delete main file");
                git(repo, ["rm", "a.txt"]);
                git_with_env(repo, ["commit", "-m", "delete main"]);
            }
        }

        let git_failure = git_failure_output(git_repo.path(), &["merge", "feature"]);
        let skron_failure = run_skron_failure_output(skron_repo.path(), &["merge", "feature"]);
        assert_eq!(skron_failure.0, git_failure.0);
        assert_eq!(
            git(skron_repo.path(), ["ls-files", "-u"]),
            git(git_repo.path(), ["ls-files", "-u"]),
            "delete_on_feature={delete_on_feature}"
        );
        assert_eq!(
            fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron a"),
            fs::read_to_string(git_repo.path().join("a.txt")).expect("read git a"),
            "delete_on_feature={delete_on_feature}"
        );
        assert_eq!(
            run_skron(skron_repo.path(), ["status", "--porcelain=v1"]),
            git(git_repo.path(), ["status", "--porcelain=v1"]),
            "delete_on_feature={delete_on_feature}"
        );
    }
}

#[test]
fn merge_rename_delete_conflicts_match_stock_git_index_and_worktree() {
    for rename_on_feature in [true, false] {
        let git_repo = committed_repo();
        let skron_repo = committed_repo();
        let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

        for repo in [git_repo.path(), skron_repo.path()] {
            write_file(repo, "a.txt", "base\n");
            git(repo, ["add", "a.txt"]);
            git_with_env(repo, ["commit", "-m", "base file"]);
            git(repo, ["switch", "-c", "feature"]);
            if rename_on_feature {
                git(repo, ["mv", "a.txt", "b.txt"]);
                git_with_env(repo, ["commit", "-m", "rename feature"]);
                git(repo, ["switch", &default_branch]);
                fs::remove_file(repo.join("a.txt")).expect("delete main file");
                git(repo, ["rm", "a.txt"]);
                git_with_env(repo, ["commit", "-m", "delete main"]);
            } else {
                fs::remove_file(repo.join("a.txt")).expect("delete feature file");
                git(repo, ["rm", "a.txt"]);
                git_with_env(repo, ["commit", "-m", "delete feature"]);
                git(repo, ["switch", &default_branch]);
                git(repo, ["mv", "a.txt", "b.txt"]);
                git_with_env(repo, ["commit", "-m", "rename main"]);
            }
        }

        let git_failure = git_failure_output(git_repo.path(), &["merge", "feature"]);
        let skron_failure = run_skron_failure_output(skron_repo.path(), &["merge", "feature"]);
        assert_eq!(skron_failure.0, git_failure.0);
        assert_eq!(
            git(skron_repo.path(), ["ls-files", "-u"]),
            git(git_repo.path(), ["ls-files", "-u"]),
            "rename_on_feature={rename_on_feature}"
        );
        assert_eq!(
            fs::read_to_string(skron_repo.path().join("b.txt")).expect("read skron b"),
            fs::read_to_string(git_repo.path().join("b.txt")).expect("read git b"),
            "rename_on_feature={rename_on_feature}"
        );
        assert_eq!(
            run_skron(skron_repo.path(), ["status", "--porcelain=v1"]),
            git(git_repo.path(), ["status", "--porcelain=v1"]),
            "rename_on_feature={rename_on_feature}"
        );
        assert!(!skron_repo.path().join("a.txt").exists());
    }
}

#[test]
fn merge_ours_strategy_matches_stock_git_tree_and_parents() {
    let git_repo = committed_repo();
    let skron_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    for repo in [git_repo.path(), skron_repo.path()] {
        git(repo, ["switch", "-c", "feature"]);
        write_file(repo, "feature.txt", "feature\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "feature"]);
        git(repo, ["switch", &default_branch]);
        write_file(repo, "main.txt", "main\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "main"]);
    }

    git_with_env(git_repo.path(), ["merge", "-s", "ours", "feature"]);
    run_skron_with_env(skron_repo.path(), ["merge", "-s", "ours", "feature"]);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_repo.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(skron_repo.path(), ["rev-parse", "HEAD^1^{tree}"])
    );
    assert_eq!(
        git(skron_repo.path(), ["rev-list", "--parents", "-1", "HEAD"])
            .split_whitespace()
            .count(),
        3
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
    assert!(!skron_repo.path().join("feature.txt").exists());
}

#[test]
fn merge_abort_and_continue_without_merge_match_stock_git_failures() {
    let git_repo = committed_repo();
    let skron_repo = committed_repo();

    assert_eq!(
        run_skron_failure_output(skron_repo.path(), &["merge", "--abort"]),
        git_failure_output(git_repo.path(), &["merge", "--abort"])
    );
    assert_eq!(
        run_skron_failure_output(skron_repo.path(), &["merge", "--continue"]),
        git_failure_output(git_repo.path(), &["merge", "--continue"])
    );
}
