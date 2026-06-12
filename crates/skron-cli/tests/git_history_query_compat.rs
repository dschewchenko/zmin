mod common;

use std::process::Command;

use tempfile::TempDir;

use common::{
    clone_repo_fixture, configure_identity, git, git_args, git_init, git_status, git_with_env,
    run_skron, run_skron_args, run_skron_status, run_skron_with_env, write_file,
};

fn commit_empty_as(cwd: &std::path::Path, name: &str, email: &str, message: &str) {
    let output = Command::new("git")
        .args([
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            message,
        ])
        .env("GIT_AUTHOR_NAME", name)
        .env("GIT_AUTHOR_EMAIL", email)
        .env("GIT_COMMITTER_NAME", name)
        .env("GIT_COMMITTER_EMAIL", email)
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .current_dir(cwd)
        .output()
        .expect("commit empty as");
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_commit_with_author(
    cwd: &std::path::Path,
    name: &str,
    email: &str,
    date: &str,
    message: &str,
) {
    let output = Command::new("git")
        .args(["-c", "commit.gpgsign=false", "commit", "-m", message])
        .env("GIT_AUTHOR_NAME", name)
        .env("GIT_AUTHOR_EMAIL", email)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_NAME", name)
        .env("GIT_COMMITTER_EMAIL", email)
        .env("GIT_COMMITTER_DATE", date)
        .current_dir(cwd)
        .output()
        .expect("git commit with author");
    assert!(
        output.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn blame_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\ntwo\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "A",
        "a@example.test",
        "1700000000 +0000",
        "one",
    );
    write_file(repo.path(), "a.txt", "one\nTWO\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "B",
        "b@example.test",
        "1700000100 +0000",
        "two",
    );
    repo
}

#[test]
fn shortlog_matches_stock_git_for_author_summaries() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    commit_empty_as(repo.path(), "Alice", "a@example.test", "first subject");
    commit_empty_as(repo.path(), "Bob", "b@example.test", "second subject");
    commit_empty_as(repo.path(), "Alice", "a@example.test", "third subject");

    for args in [
        ["shortlog", "HEAD"].as_slice(),
        ["shortlog", "-s", "HEAD"].as_slice(),
        ["shortlog", "-sn", "HEAD"].as_slice(),
        ["shortlog", "-se", "HEAD"].as_slice(),
        ["shortlog", "--no-merges", "HEAD"].as_slice(),
        ["shortlog", "HEAD~2..HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_and_annotate_match_stock_git_for_simple_linear_history() {
    let git_repo = blame_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    for args in [
        ["blame", "a.txt"].as_slice(),
        ["blame", "-l", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
    assert_eq!(
        run_skron(skron_repo.path(), ["annotate", "a.txt"]),
        git(git_repo.path(), ["annotate", "a.txt"])
    );
}

#[test]
fn cherry_matches_stock_git_for_patch_equivalence_and_upstream_default() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "upstream"]);
    write_file(repo.path(), "a.txt", "alpha\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "add alpha"]);

    git(repo.path(), ["checkout", "-b", "topic", "main"]);
    let cherry_pick = Command::new("git")
        .args(["-c", "commit.gpgsign=false", "cherry-pick", "upstream"])
        .env("GIT_AUTHOR_NAME", "Bench")
        .env("GIT_AUTHOR_EMAIL", "bench@example.test")
        .env("GIT_COMMITTER_NAME", "Bench")
        .env("GIT_COMMITTER_EMAIL", "bench@example.test")
        .env("GIT_COMMITTER_DATE", "1700000001 +0000")
        .current_dir(repo.path())
        .output()
        .expect("git cherry-pick");
    assert!(
        cherry_pick.status.success(),
        "git cherry-pick failed: {}",
        String::from_utf8_lossy(&cherry_pick.stderr)
    );
    write_file(repo.path(), "b.txt", "beta\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "add beta"]);
    git(
        repo.path(),
        ["branch", "--set-upstream-to", "upstream", "topic"],
    );

    for args in [
        ["cherry"].as_slice(),
        ["cherry", "upstream", "topic"].as_slice(),
        ["cherry", "-v", "upstream", "topic"].as_slice(),
        ["cherry", "--abbrev", "upstream", "topic"].as_slice(),
        ["cherry", "--abbrev=12", "upstream", "topic"].as_slice(),
        ["cherry", "upstream", "topic", "HEAD~1"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn describe_matches_stock_git_for_tags_refs_and_dirty_worktrees() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "base"]);
    git_with_env(repo.path(), ["tag", "-a", "v1.0.0", "-m", "version"]);
    write_file(repo.path(), "next.txt", "next\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "next"]);
    git(repo.path(), ["tag", "lightweight"]);

    for args in [
        ["describe"].as_slice(),
        ["describe", "--long"].as_slice(),
        ["describe", "--abbrev=0"].as_slice(),
        ["describe", "--abbrev=12"].as_slice(),
        ["describe", "--tags"].as_slice(),
        ["describe", "--all"].as_slice(),
        ["describe", "--match", "v*"].as_slice(),
        ["describe", "--exclude", "light*"].as_slice(),
        ["describe", "--always"].as_slice(),
        ["describe", "v1.0.0"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    write_file(repo.path(), "dirty.txt", "dirty\n");
    assert_eq!(
        run_skron(repo.path(), ["describe", "--dirty"]),
        git(repo.path(), ["describe", "--dirty"])
    );
    assert_eq!(
        run_skron(repo.path(), ["describe", "--dirty=.modified"]),
        git(repo.path(), ["describe", "--dirty=.modified"])
    );
}

#[test]
fn describe_always_matches_stock_git_without_names() {
    let repo = git_init();
    configure_identity(repo.path());
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "base"]);

    assert_eq!(
        run_skron(repo.path(), ["describe", "--always"]),
        git(repo.path(), ["describe", "--always"])
    );
    assert_eq!(
        run_skron_status(repo.path(), ["describe"]),
        git_status(repo.path(), ["describe"])
    );
}

#[test]
fn last_modified_reports_latest_commit_per_path() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    write_file(repo.path(), "dir/b.txt", "b\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let initial = git(repo.path(), ["rev-parse", "HEAD"]);

    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "modify a"]);
    let latest = git(repo.path(), ["rev-parse", "HEAD"]);

    assert_eq!(
        run_skron(repo.path(), ["last-modified", "--recursive"]),
        format!("{latest}\ta.txt\n{initial}\tdir/b.txt")
    );
    assert_eq!(
        run_skron(repo.path(), ["last-modified"]),
        format!("{latest}\ta.txt\n{initial}\tdir")
    );
    assert_eq!(
        run_skron(repo.path(), ["last-modified", "-z", "--", "a.txt"]),
        format!("{latest}\ta.txt\0")
    );
}

#[test]
fn add_commit_rev_list_and_log_match_stock_git_state() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "hello\n");
    write_file(skron_repo.path(), "a.txt", "hello\n");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);

    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );

    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "initial"]);
    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );

    write_file(git_repo.path(), "a.txt", "changed\n");
    write_file(skron_repo.path(), "a.txt", "changed\n");
    write_file(git_repo.path(), "b.txt", "new\n");
    write_file(skron_repo.path(), "b.txt", "new\n");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "second"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "second"]);

    assert_eq!(
        git(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        git(skron_repo.path(), ["rev-list", "--max-count", "2", "HEAD"]),
        git(git_repo.path(), ["rev-list", "--max-count", "2", "HEAD"])
    );

    for args in [
        ["rev-list", "--max-count", "2", "HEAD"].as_slice(),
        ["rev-list", "--all"].as_slice(),
        ["rev-list", "HEAD~1..HEAD"].as_slice(),
        ["rev-list", "HEAD", "^HEAD~1"].as_slice(),
        ["rev-list", "HEAD", "--not", "HEAD~1"].as_slice(),
        ["rev-list", "--count", "HEAD"].as_slice(),
        ["rev-list", "--parents", "HEAD"].as_slice(),
        ["rev-list", "--parents", "--max-count", "1", "HEAD"].as_slice(),
        ["rev-list", "-1", "HEAD"].as_slice(),
        ["rev-list", "--objects", "HEAD"].as_slice(),
        ["rev-list", "--objects", "--no-object-names", "HEAD"].as_slice(),
        ["rev-list", "--objects", "--count", "HEAD"].as_slice(),
        ["rev-list", "--objects", "--all"].as_slice(),
        ["rev-list", "--objects", "--no-object-names", "--all"].as_slice(),
        ["rev-list", "--objects", "--reverse", "HEAD"].as_slice(),
        ["rev-list", "--reverse", "HEAD"].as_slice(),
        ["rev-list", "--reverse", "--max-count", "2", "HEAD"].as_slice(),
        ["rev-list", "--count", "--max-count", "1", "HEAD"].as_slice(),
        ["log", "--max-count", "2"].as_slice(),
        ["log", "-1", "--format=%H"].as_slice(),
        ["log", "--reverse", "--format=%s"].as_slice(),
        ["log", "--stat", "--max-count", "1"].as_slice(),
        ["log", "--numstat", "--format=%H", "--max-count", "1"].as_slice(),
        ["log", "--shortstat", "--max-count", "1"].as_slice(),
        ["log", "--raw", "--format=%H", "--max-count", "1"].as_slice(),
        ["log", "--summary", "--format=%H", "--max-count", "1"].as_slice(),
        ["log", "--name-only", "--format=%H", "--max-count", "1"].as_slice(),
        ["log", "--name-status", "--format=%H", "--max-count", "1"].as_slice(),
        ["log", "--parents", "--oneline", "--max-count", "1"].as_slice(),
        ["whatchanged", "--max-count", "1"].as_slice(),
        ["whatchanged", "--stat", "--max-count", "1"].as_slice(),
        ["whatchanged", "--oneline", "--max-count", "1"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }

    let git_blob = git(git_repo.path(), ["hash-object", "-w", "a.txt"]);
    let skron_blob = git(skron_repo.path(), ["hash-object", "-w", "a.txt"]);
    assert_eq!(skron_blob, git_blob);
    git(git_repo.path(), ["tag", "blob-tag", &git_blob]);
    git(skron_repo.path(), ["tag", "blob-tag", &skron_blob]);

    for args in [
        ["log", "--all", "--format=%H"].as_slice(),
        ["rev-list", "--objects", "--all", "--max-count", "2"].as_slice(),
        ["log", "--format=%H", "--max-count", "1"].as_slice(),
        ["log", "--format=%h %s", "--max-count", "1"].as_slice(),
        ["log", "--pretty=format:%an <%ae>", "--max-count", "1"].as_slice(),
        ["log", "--pretty=oneline", "--max-count", "1"].as_slice(),
        ["rev-parse", "HEAD"].as_slice(),
        ["rev-parse", "--short=12", "HEAD"].as_slice(),
        ["rev-parse", "--show-object-format"].as_slice(),
        ["show-ref", "--heads"].as_slice(),
        ["show-ref", "--head"].as_slice(),
        ["show-ref", "--hash=12"].as_slice(),
        ["log", "--oneline", "--max-count", "2", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }

    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
}

#[test]
fn rev_list_symmetric_difference_matches_stock_git() {
    let git_repo = git_init();
    let skron_repo = clone_repo_fixture(git_repo.path());
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "base.txt", "base\n");
    write_file(skron_repo.path(), "base.txt", "base\n");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "base"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "base"]);

    git(git_repo.path(), ["checkout", "-b", "left"]);
    git(skron_repo.path(), ["checkout", "-b", "left"]);
    write_file(git_repo.path(), "left.txt", "left\n");
    write_file(skron_repo.path(), "left.txt", "left\n");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "left"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "left"]);

    git(git_repo.path(), ["checkout", "main"]);
    git(skron_repo.path(), ["checkout", "main"]);
    write_file(git_repo.path(), "right.txt", "right\n");
    write_file(skron_repo.path(), "right.txt", "right\n");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "right"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "right"]);

    for args in [
        ["rev-list", "left...main"].as_slice(),
        ["rev-list", "--count", "left...main"].as_slice(),
        ["rev-list", "--reverse", "left...main"].as_slice(),
        ["rev-list", "--objects", "left...main"].as_slice(),
        ["rev-list", "--objects", "--no-object-names", "left...main"].as_slice(),
        ["rev-list", "--not", "left...main", "main"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn log_relative_since_matches_stock_git_for_recent_commits() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());
    write_file(git_repo.path(), "a.txt", "now\n");
    write_file(skron_repo.path(), "a.txt", "now\n");
    git(git_repo.path(), ["add", "-A"]);
    git(skron_repo.path(), ["add", "-A"]);
    git(git_repo.path(), ["commit", "-m", "recent"]);
    run_skron(skron_repo.path(), ["commit", "-m", "recent"]);

    for args in [
        ["log", "--since", "yesterday", "--format=%s"].as_slice(),
        ["log", "--since", "1.week.ago", "--format=%s"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(skron_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
}
