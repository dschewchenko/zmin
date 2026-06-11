mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    configure_identity, git, git_failure_output, git_init, git_with_env, git_with_stdin, run_skron,
    run_skron_failure_output, run_skron_with_env, run_skron_with_stdin,
};

fn committed_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    run_skron(repo.path(), ["add", "-A"]);
    run_skron_with_env(repo.path(), ["commit", "-m", "initial"]);
    repo
}

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

fn checkout_index_fixture_repo() -> TempDir {
    let repo = git_init();
    fs::create_dir_all(repo.path().join("docs")).expect("create docs");
    fs::write(repo.path().join("README.md"), b"readme\n").expect("write readme");
    fs::write(repo.path().join("docs/guide.md"), b"guide\n").expect("write guide");
    git(repo.path(), ["add", "-A"]);
    fs::remove_file(repo.path().join("README.md")).expect("remove readme");
    fs::remove_file(repo.path().join("docs/guide.md")).expect("remove guide");
    repo
}

#[test]
fn checkout_switches_branches_and_updates_worktree() {
    let repo = committed_repo();
    let default_branch = git(repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    run_skron(repo.path(), ["checkout", "-b", "feature"]);
    assert_eq!(
        git(repo.path(), ["symbolic-ref", "HEAD"]),
        "refs/heads/feature"
    );

    fs::write(repo.path().join("a.txt"), b"feature\n").expect("modify feature file");
    fs::write(repo.path().join("feature.txt"), b"only feature\n").expect("write feature file");
    run_skron(repo.path(), ["add", "-A"]);
    run_skron_with_env(repo.path(), ["commit", "-m", "feature"]);

    run_skron(repo.path(), ["checkout", &default_branch]);
    assert_eq!(
        git(repo.path(), ["symbolic-ref", "HEAD"]),
        format!("refs/heads/{default_branch}")
    );
    assert_eq!(
        fs::read(repo.path().join("a.txt")).expect("read master file"),
        b"hello\n"
    );
    assert!(!repo.path().join("feature.txt").exists());
    assert_eq!(
        run_skron(repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v1", "--branch"])
    );

    run_skron(repo.path(), ["checkout", "feature"]);
    assert_eq!(
        fs::read(repo.path().join("a.txt")).expect("read feature file"),
        b"feature\n"
    );
    assert_eq!(
        fs::read(repo.path().join("feature.txt")).expect("read feature-only file"),
        b"only feature\n"
    );

    run_skron(
        repo.path(),
        ["checkout", "-B", "feature-reset", &default_branch],
    );
    assert_eq!(
        git(repo.path(), ["symbolic-ref", "HEAD"]),
        "refs/heads/feature-reset"
    );
    assert_eq!(
        fs::read(repo.path().join("a.txt")).expect("read reset branch file"),
        b"hello\n"
    );

    let feature_head = git(repo.path(), ["rev-parse", "feature"]);
    run_skron(repo.path(), ["checkout", "--detach", "feature"]);
    assert_eq!(git(repo.path(), ["rev-parse", "HEAD"]), feature_head);
    assert_eq!(
        git(repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]),
        "HEAD"
    );
    assert_eq!(
        run_skron(repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn checkout_paths_match_stock_git_state() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    fs::create_dir_all(git_repo.path().join("dir")).expect("mkdir git dir");
    fs::create_dir_all(skron_repo.path().join("dir")).expect("mkdir skron dir");
    fs::write(git_repo.path().join("a.txt"), b"hello\n").expect("write git a");
    fs::write(skron_repo.path().join("a.txt"), b"hello\n").expect("write skron a");
    fs::write(git_repo.path().join("remove.txt"), b"remove\n").expect("write git remove");
    fs::write(skron_repo.path().join("remove.txt"), b"remove\n").expect("write skron remove");
    fs::write(git_repo.path().join("dir/nested.txt"), b"nested\n").expect("write git nested");
    fs::write(skron_repo.path().join("dir/nested.txt"), b"nested\n").expect("write skron nested");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "initial"]);

    fs::write(git_repo.path().join("a.txt"), b"worktree\n").expect("dirty git a");
    fs::write(skron_repo.path().join("a.txt"), b"worktree\n").expect("dirty skron a");
    git(git_repo.path(), ["checkout", "--", "a.txt"]);
    run_skron(skron_repo.path(), ["checkout", "--", "a.txt"]);
    assert_eq!(
        fs::read(skron_repo.path().join("a.txt")).expect("read skron a"),
        fs::read(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );

    fs::write(git_repo.path().join("a.txt"), b"staged\n").expect("stage git a");
    fs::write(skron_repo.path().join("a.txt"), b"staged\n").expect("stage skron a");
    fs::remove_file(git_repo.path().join("remove.txt")).expect("remove git file");
    fs::remove_file(skron_repo.path().join("remove.txt")).expect("remove skron file");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git(
        git_repo.path(),
        ["checkout", "HEAD", "--", "a.txt", "remove.txt"],
    );
    run_skron(
        skron_repo.path(),
        ["checkout", "HEAD", "--", "a.txt", "remove.txt"],
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read(skron_repo.path().join("remove.txt")).expect("read skron restored"),
        fs::read(git_repo.path().join("remove.txt")).expect("read git restored")
    );
}

#[test]
fn checkout_index_matches_stock_git_for_all_paths_stdin_and_prefix() {
    let git_repo = checkout_index_fixture_repo();
    let skron_repo = checkout_index_fixture_repo();

    git(git_repo.path(), ["checkout-index", "-a"]);
    run_skron(skron_repo.path(), ["checkout-index", "-a"]);
    assert_eq!(
        fs::read(skron_repo.path().join("README.md")).expect("read skron readme"),
        fs::read(git_repo.path().join("README.md")).expect("read git readme")
    );
    assert_eq!(
        fs::read(skron_repo.path().join("docs/guide.md")).expect("read skron guide"),
        fs::read(git_repo.path().join("docs/guide.md")).expect("read git guide")
    );

    fs::remove_file(git_repo.path().join("README.md")).expect("remove git readme");
    fs::remove_file(skron_repo.path().join("README.md")).expect("remove skron readme");
    git(git_repo.path(), ["checkout-index", "README.md"]);
    run_skron(skron_repo.path(), ["checkout-index", "README.md"]);
    assert_eq!(
        fs::read(skron_repo.path().join("README.md")).expect("read skron readme"),
        fs::read(git_repo.path().join("README.md")).expect("read git readme")
    );

    git(
        git_repo.path(),
        [
            "checkout-index",
            "--prefix=out/",
            "README.md",
            "docs/guide.md",
        ],
    );
    run_skron(
        skron_repo.path(),
        [
            "checkout-index",
            "--prefix=out/",
            "README.md",
            "docs/guide.md",
        ],
    );
    assert_eq!(
        fs::read(skron_repo.path().join("out/README.md")).expect("read skron out readme"),
        fs::read(git_repo.path().join("out/README.md")).expect("read git out readme")
    );
    assert_eq!(
        fs::read(skron_repo.path().join("out/docs/guide.md")).expect("read skron out guide"),
        fs::read(git_repo.path().join("out/docs/guide.md")).expect("read git out guide")
    );

    fs::remove_file(git_repo.path().join("docs/guide.md")).expect("remove git guide");
    fs::remove_file(skron_repo.path().join("docs/guide.md")).expect("remove skron guide");
    assert_eq!(
        run_skron_with_stdin(
            skron_repo.path(),
            ["checkout-index", "--stdin"],
            "docs/guide.md\n",
        ),
        git_with_stdin(
            git_repo.path(),
            ["checkout-index", "--stdin"],
            "docs/guide.md\n",
        )
    );
    assert_eq!(
        fs::read(skron_repo.path().join("docs/guide.md")).expect("read skron stdin guide"),
        fs::read(git_repo.path().join("docs/guide.md")).expect("read git stdin guide")
    );
}

#[test]
fn switch_create_matches_stock_git_state() {
    let git_repo = committed_repo();
    let skron_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    git(git_repo.path(), ["switch", "-c", "feature"]);
    run_skron(skron_repo.path(), ["switch", "-c", "feature"]);
    assert_eq!(
        git(skron_repo.path(), ["symbolic-ref", "HEAD"]),
        git(git_repo.path(), ["symbolic-ref", "HEAD"])
    );

    fs::write(git_repo.path().join("a.txt"), b"feature\n").expect("modify git feature");
    fs::write(skron_repo.path().join("a.txt"), b"feature\n").expect("modify skron feature");
    fs::write(git_repo.path().join("feature.txt"), b"feature-only\n").expect("write git feature");
    fs::write(skron_repo.path().join("feature.txt"), b"feature-only\n")
        .expect("write skron feature");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "feature"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "feature"]);

    git(git_repo.path(), ["switch", &default_branch]);
    run_skron(skron_repo.path(), ["switch", &default_branch]);
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read(skron_repo.path().join("a.txt")).expect("read switched file"),
        b"hello\n"
    );
    assert!(!skron_repo.path().join("feature.txt").exists());

    git(git_repo.path(), ["switch", "feature"]);
    run_skron(skron_repo.path(), ["switch", "feature"]);
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        fs::read(skron_repo.path().join("feature.txt")).expect("read feature-only file"),
        b"feature-only\n"
    );
}

#[test]
fn switch_orphan_and_discard_changes_match_stock_git_state() {
    let git_repo = committed_repo();
    let skron_repo = committed_repo();

    git(git_repo.path(), ["switch", "--orphan", "orphan"]);
    run_skron(skron_repo.path(), ["switch", "--orphan", "orphan"]);
    assert_eq!(
        git(skron_repo.path(), ["symbolic-ref", "HEAD"]),
        git(git_repo.path(), ["symbolic-ref", "HEAD"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(run_skron(skron_repo.path(), ["ls-files", "--stage"]), "");
    assert!(!skron_repo.path().join("a.txt").exists());

    let git_dirty = two_commit_repo();
    let skron_dirty = two_commit_repo();
    fs::write(git_dirty.path().join("a.txt"), b"dirty\n").expect("dirty git a");
    fs::write(skron_dirty.path().join("a.txt"), b"dirty\n").expect("dirty skron a");
    assert_eq!(
        run_skron_failure_output(skron_dirty.path(), &["switch", "--orphan", "blocked"]),
        git_failure_output(git_dirty.path(), &["switch", "--orphan", "blocked"])
    );

    let git_force = two_commit_repo();
    let skron_force = two_commit_repo();
    git(git_force.path(), ["branch", "feature", "HEAD~1"]);
    run_skron(skron_force.path(), ["branch", "feature", "HEAD~1"]);
    fs::write(git_force.path().join("a.txt"), b"dirty\n").expect("force dirty git a");
    fs::write(skron_force.path().join("a.txt"), b"dirty\n").expect("force dirty skron a");
    git(git_force.path(), ["switch", "--discard-changes", "feature"]);
    run_skron(
        skron_force.path(),
        ["switch", "--discard-changes", "feature"],
    );
    assert_eq!(
        git(skron_force.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_force.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read(skron_force.path().join("a.txt")).expect("read discarded a"),
        fs::read(git_force.path().join("a.txt")).expect("read git discarded a")
    );
}

#[test]
fn switch_detach_matches_stock_git_for_branch_targets() {
    let git_repo = two_commit_repo();
    let skron_repo = two_commit_repo();

    git(git_repo.path(), ["branch", "feature"]);
    run_skron(skron_repo.path(), ["branch", "feature"]);
    git(git_repo.path(), ["switch", "--detach", "feature"]);
    run_skron(skron_repo.path(), ["switch", "--detach", "feature"]);

    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]),
        "HEAD"
    );
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
}

#[test]
fn restore_staged_and_worktree_match_stock_git() {
    let git_repo = committed_repo();
    let skron_repo = committed_repo();

    fs::write(git_repo.path().join("a.txt"), b"modified\n").expect("modify git a");
    fs::write(skron_repo.path().join("a.txt"), b"modified\n").expect("modify skron a");
    fs::write(git_repo.path().join("b.txt"), b"new\n").expect("write git b");
    fs::write(skron_repo.path().join("b.txt"), b"new\n").expect("write skron b");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);

    git(git_repo.path(), ["restore", "--staged", "a.txt", "b.txt"]);
    run_skron(skron_repo.path(), ["restore", "--staged", "a.txt", "b.txt"]);
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );

    git(git_repo.path(), ["restore", "a.txt"]);
    run_skron(skron_repo.path(), ["restore", "a.txt"]);
    assert_eq!(
        fs::read(skron_repo.path().join("a.txt")).expect("read restored a"),
        b"hello\n"
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn restore_source_can_remove_paths_from_index_and_worktree() {
    let git_repo = two_commit_repo();
    let skron_repo = two_commit_repo();

    git(
        git_repo.path(),
        [
            "restore",
            "--source",
            "HEAD~1",
            "--staged",
            "--worktree",
            "b.txt",
        ],
    );
    run_skron(
        skron_repo.path(),
        [
            "restore",
            "--source",
            "HEAD~1",
            "--staged",
            "--worktree",
            "b.txt",
        ],
    );

    assert!(!skron_repo.path().join("b.txt").exists());
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
}

#[test]
fn reset_modes_match_stock_git_state() {
    for mode in ["--soft", "--mixed", "--hard"] {
        let git_repo = two_commit_repo();
        let skron_repo = two_commit_repo();
        let target = git(git_repo.path(), ["rev-parse", "HEAD~1"]);

        git(git_repo.path(), ["reset", mode, &target]);
        run_skron(skron_repo.path(), ["reset", mode, &target]);

        assert_eq!(
            git(skron_repo.path(), ["rev-parse", "HEAD"]),
            git(git_repo.path(), ["rev-parse", "HEAD"])
        );
        assert_eq!(
            git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
            git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
        );
        assert_eq!(
            git(skron_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
            git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
        );
        if mode == "--hard" {
            assert_eq!(
                fs::read(skron_repo.path().join("a.txt")).expect("read hard-reset file"),
                b"one\n"
            );
            assert!(!skron_repo.path().join("b.txt").exists());
        }
    }
}

#[test]
fn reset_paths_match_stock_git_state() {
    let git_repo = two_commit_repo();
    let skron_repo = two_commit_repo();

    fs::write(git_repo.path().join("a.txt"), b"staged\n").expect("stage git a");
    fs::write(skron_repo.path().join("a.txt"), b"staged\n").expect("stage skron a");
    fs::write(git_repo.path().join("new.txt"), b"new\n").expect("write git new");
    fs::write(skron_repo.path().join("new.txt"), b"new\n").expect("write skron new");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);

    git(git_repo.path(), ["reset", "HEAD", "--", "a.txt", "new.txt"]);
    run_skron(
        skron_repo.path(),
        ["reset", "HEAD", "--", "a.txt", "new.txt"],
    );
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        git(skron_repo.path(), ["diff", "--cached", "--name-status"]),
        git(git_repo.path(), ["diff", "--cached", "--name-status"])
    );

    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git(git_repo.path(), ["reset", "--", "a.txt"]);
    run_skron(skron_repo.path(), ["reset", "--", "a.txt"]);
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
}
