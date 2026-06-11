mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{configure_identity, git, git_with_env, run_skron, write_file};
use tempfile::TempDir;

#[test]
fn filter_branch_msg_filter_rewrites_head_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    write_file(&source, "README.md", "hello\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "old subject"]);

    let (git_repo, skron_repo) = rewrite_pair(dir.path(), &source);
    stock_filter_branch(
        &git_repo,
        &["-f", "--msg-filter", "sed s/old/new/", "HEAD"],
        "msg-filter",
    );
    run_skron(
        &skron_repo,
        [
            "filter-branch",
            "-f",
            "--msg-filter",
            "sed s/old/new/",
            "HEAD",
        ],
    );

    assert_eq!(
        git(&skron_repo, ["rev-parse", "HEAD"]),
        git(&git_repo, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&skron_repo, ["log", "--format=%s", "--max-count", "1"]),
        git(&git_repo, ["log", "--format=%s", "--max-count", "1"])
    );
    assert_eq!(
        git(&skron_repo, ["rev-parse", "refs/original/refs/heads/main"]),
        git(&git_repo, ["rev-parse", "refs/original/refs/heads/main"])
    );
    assert_eq!(
        git(&skron_repo, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_repo, ["cat-file", "-p", "HEAD^{tree}"])
    );
}

#[test]
fn filter_branch_tree_filter_rewrites_tree_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    write_file(&source, "README.md", "hello\n");
    write_file(&source, "secret.txt", "remove me\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    write_file(&source, "README.md", "hello again\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "update"]);

    let (git_repo, skron_repo) = rewrite_pair(dir.path(), &source);
    stock_filter_branch(
        &git_repo,
        &["-f", "--tree-filter", "rm -f secret.txt", "HEAD"],
        "tree-filter",
    );
    run_skron(
        &skron_repo,
        [
            "filter-branch",
            "-f",
            "--tree-filter",
            "rm -f secret.txt",
            "HEAD",
        ],
    );

    assert_eq!(
        git(&skron_repo, ["rev-parse", "HEAD"]),
        git(&git_repo, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&skron_repo, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_repo, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&skron_repo, ["ls-tree", "-r", "--name-only", "HEAD"]),
        "README.md"
    );
    assert!(!skron_repo.join("secret.txt").exists());
}

#[test]
fn filter_branch_index_filter_rewrites_index_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    write_file(&source, "README.md", "hello\n");
    write_file(&source, "secret.txt", "remove me\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    write_file(&source, "README.md", "hello again\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "update"]);

    let (git_repo, skron_repo) = rewrite_pair(dir.path(), &source);
    stock_filter_branch(
        &git_repo,
        &[
            "-f",
            "--index-filter",
            "git rm --cached -f secret.txt",
            "HEAD",
        ],
        "index-filter",
    );
    run_skron(
        &skron_repo,
        [
            "filter-branch",
            "-f",
            "--index-filter",
            "git rm --cached -f secret.txt",
            "HEAD",
        ],
    );

    assert_eq!(
        git(&skron_repo, ["rev-parse", "HEAD"]),
        git(&git_repo, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&skron_repo, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_repo, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&skron_repo, ["ls-tree", "-r", "--name-only", "HEAD"]),
        "README.md"
    );
}

#[test]
fn filter_branch_env_filter_rewrites_author_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    write_file(&source, "README.md", "hello\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    let (git_repo, skron_repo) = rewrite_pair(dir.path(), &source);
    let filter = "GIT_AUTHOR_NAME=Filtered; GIT_AUTHOR_EMAIL=filtered@example.test; export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL";

    stock_filter_branch(
        &git_repo,
        &["-f", "--env-filter", filter, "HEAD"],
        "env-filter",
    );
    run_skron(
        &skron_repo,
        ["filter-branch", "-f", "--env-filter", filter, "HEAD"],
    );

    assert_eq!(
        git(&skron_repo, ["rev-parse", "HEAD"]),
        git(&git_repo, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(
            &skron_repo,
            ["show", "--pretty=format:%an <%ae>", "--no-patch", "HEAD"]
        ),
        "Filtered <filtered@example.test>"
    );
    assert_eq!(
        git(&skron_repo, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_repo, ["cat-file", "-p", "HEAD^{tree}"])
    );
}

#[test]
fn filter_branch_parent_filter_rewrites_parents_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    write_file(&source, "README.md", "hello\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    write_file(&source, "README.md", "hello again\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "update"]);

    let (git_repo, skron_repo) = rewrite_pair(dir.path(), &source);
    let filter = "sed 's/-p [0-9a-f]*//g'";

    stock_filter_branch(
        &git_repo,
        &["-f", "--parent-filter", filter, "HEAD"],
        "parent-filter",
    );
    run_skron(
        &skron_repo,
        ["filter-branch", "-f", "--parent-filter", filter, "HEAD"],
    );

    assert_eq!(
        git(&skron_repo, ["rev-parse", "HEAD"]),
        git(&git_repo, ["rev-parse", "HEAD"])
    );
    assert_eq!(git(&skron_repo, ["log", "--format=%P", "-1"]), "");
    assert_eq!(
        git(&skron_repo, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_repo, ["cat-file", "-p", "HEAD^{tree}"])
    );
}

#[test]
fn filter_branch_subdirectory_filter_rewrites_tree_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::create_dir_all(source.join("docs")).expect("create docs");
    write_file(&source, "docs/README.md", "hello\n");
    write_file(&source, "root.txt", "drop\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    write_file(&source, "docs/guide.md", "guide\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "guide"]);

    let (git_repo, skron_repo) = rewrite_pair(dir.path(), &source);
    stock_filter_branch(
        &git_repo,
        &["-f", "--subdirectory-filter", "docs", "HEAD"],
        "subdirectory-filter",
    );
    run_skron(
        &skron_repo,
        [
            "filter-branch",
            "-f",
            "--subdirectory-filter",
            "docs",
            "HEAD",
        ],
    );

    assert_eq!(
        git(&skron_repo, ["rev-parse", "HEAD"]),
        git(&git_repo, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&skron_repo, ["ls-tree", "-r", "--name-only", "HEAD"]),
        "README.md\nguide.md"
    );
}

#[test]
fn filter_branch_tag_name_filter_renames_lightweight_tag_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_repo = dir.path().join("git-rewrite");
    let skron_repo = dir.path().join("skron-rewrite");
    for repo in [&git_repo, &skron_repo] {
        git(
            dir.path(),
            ["init", "-b", "main", repo.to_str().expect("repo path")],
        );
        configure_identity(repo);
        write_file(repo, "README.md", "hello\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "base"]);
        git(repo, ["tag", "v1"]);
    }

    stock_filter_branch(
        &git_repo,
        &[
            "-f",
            "--tag-name-filter",
            "sed s/v/release-/",
            "--",
            "--all",
        ],
        "tag-name-filter",
    );
    run_skron(
        &skron_repo,
        [
            "filter-branch",
            "-f",
            "--tag-name-filter",
            "sed s/v/release-/",
            "--",
            "--all",
        ],
    );

    assert_eq!(
        git(&skron_repo, ["show-ref", "--tags"]),
        git(&git_repo, ["show-ref", "--tags"])
    );
    assert_eq!(
        git(&skron_repo, ["rev-parse", "release-1"]),
        git(&git_repo, ["rev-parse", "release-1"])
    );
}

#[test]
fn filter_branch_setup_runs_before_filters_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    write_file(&source, "README.md", "hello\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "subject"]);

    let (git_repo, skron_repo) = rewrite_pair(dir.path(), &source);
    stock_filter_branch(
        &git_repo,
        &[
            "-f",
            "--setup",
            "PREFIX=prep-; export PREFIX",
            "--msg-filter",
            "printf \"%s\" \"$PREFIX\"; cat",
            "HEAD",
        ],
        "setup",
    );
    run_skron(
        &skron_repo,
        [
            "filter-branch",
            "-f",
            "--setup",
            "PREFIX=prep-; export PREFIX",
            "--msg-filter",
            "printf \"%s\" \"$PREFIX\"; cat",
            "HEAD",
        ],
    );

    assert_eq!(
        git(&skron_repo, ["rev-parse", "HEAD"]),
        git(&git_repo, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&skron_repo, ["log", "--format=%B", "-1"]),
        git(&git_repo, ["log", "--format=%B", "-1"])
    );
}

#[test]
fn filter_branch_temp_dir_option_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    write_file(&source, "README.md", "hello\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    let (git_repo, skron_repo) = rewrite_pair(dir.path(), &source);
    let git_temp = dir.path().join("git-temp");
    let skron_temp = dir.path().join("skron-temp");
    fs::create_dir_all(&git_temp).expect("create git temp");
    fs::create_dir_all(&skron_temp).expect("create skron temp");
    write_file(dir.path(), "git-temp/leftover.txt", "leftover\n");
    write_file(dir.path(), "skron-temp/leftover.txt", "leftover\n");

    stock_filter_branch(
        &git_repo,
        &[
            "-f",
            "-d",
            git_temp.to_str().expect("git temp"),
            "--msg-filter",
            "cat",
            "HEAD",
        ],
        "temp-dir",
    );
    run_skron(
        &skron_repo,
        [
            "filter-branch",
            "-f",
            "-d",
            skron_temp.to_str().expect("skron temp"),
            "--msg-filter",
            "cat",
            "HEAD",
        ],
    );

    assert_eq!(
        git(&skron_repo, ["rev-parse", "HEAD"]),
        git(&git_repo, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&skron_repo, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_repo, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert!(!git_temp.exists());
    assert!(!skron_temp.exists());
}

#[test]
fn filter_branch_commit_filter_passthrough_rewrites_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    write_file(&source, "README.md", "hello\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    write_file(&source, "README.md", "hello again\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "update"]);

    let (git_repo, skron_repo) = rewrite_pair(dir.path(), &source);
    let filter = "GIT_COMMITTER_NAME=Filtered; export GIT_COMMITTER_NAME; git commit-tree \"$@\"";

    stock_filter_branch(
        &git_repo,
        &["-f", "--commit-filter", filter, "HEAD"],
        "commit-filter",
    );
    run_skron(
        &skron_repo,
        ["filter-branch", "-f", "--commit-filter", filter, "HEAD"],
    );

    assert_eq!(
        git(&skron_repo, ["rev-parse", "HEAD"]),
        git(&git_repo, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(
            &skron_repo,
            ["show", "--pretty=format:%cn <%ce>", "--no-patch", "HEAD"]
        ),
        git(
            &git_repo,
            ["show", "--pretty=format:%cn <%ce>", "--no-patch", "HEAD"]
        )
    );
}

#[test]
fn filter_branch_commit_filter_skip_commit_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    write_file(&source, "README.md", "base\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    write_file(&source, "README.md", "middle\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "middle"]);
    let middle = git(&source, ["rev-parse", "HEAD"]);
    write_file(&source, "README.md", "top\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "top"]);

    let (git_repo, skron_repo) = rewrite_pair(dir.path(), &source);
    let filter = format!(
        "if test \"$GIT_COMMIT\" = \"{middle}\"; then skip_commit \"$@\"; else git commit-tree \"$@\"; fi"
    );

    stock_filter_branch(
        &git_repo,
        &["-f", "--commit-filter", &filter, "HEAD"],
        "commit-filter-skip",
    );
    run_skron(
        &skron_repo,
        ["filter-branch", "-f", "--commit-filter", &filter, "HEAD"],
    );

    assert_eq!(
        git(&skron_repo, ["rev-parse", "HEAD"]),
        git(&git_repo, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&skron_repo, ["log", "--format=%P", "-1"]),
        git(&git_repo, ["log", "--format=%P", "-1"])
    );
    assert_eq!(
        git(&skron_repo, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_repo, ["cat-file", "-p", "HEAD^{tree}"])
    );
}

#[test]
fn filter_branch_state_branch_initial_run_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    write_file(&source, "README.md", "one\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "one"]);
    write_file(&source, "README.md", "two\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "two"]);

    let (git_repo, skron_repo) = rewrite_pair(dir.path(), &source);
    stock_filter_branch(
        &git_repo,
        &[
            "-f",
            "--state-branch",
            "refs/heads/filter-state",
            "--msg-filter",
            "cat",
            "HEAD",
        ],
        "state-branch-initial",
    );
    run_skron(
        &skron_repo,
        [
            "filter-branch",
            "-f",
            "--state-branch",
            "refs/heads/filter-state",
            "--msg-filter",
            "cat",
            "HEAD",
        ],
    );

    assert_eq!(
        git(&skron_repo, ["show", "refs/heads/filter-state:filter.map"]),
        git(&git_repo, ["show", "refs/heads/filter-state:filter.map"])
    );
    assert_eq!(
        git(
            &skron_repo,
            [
                "show",
                "--format=%s",
                "--no-patch",
                "refs/heads/filter-state"
            ]
        ),
        git(
            &git_repo,
            [
                "show",
                "--format=%s",
                "--no-patch",
                "refs/heads/filter-state"
            ]
        )
    );
}

#[test]
fn filter_branch_state_branch_repeated_run_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    write_file(&source, "README.md", "one\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "one"]);
    write_file(&source, "README.md", "two\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "two"]);
    write_file(&source, "README.md", "three\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "three"]);

    let (git_repo, skron_repo) = rewrite_pair(dir.path(), &source);
    for repo in [&git_repo, &skron_repo] {
        let run = |args: &[&str]| {
            if repo == &git_repo {
                stock_filter_branch(repo, args, "state-branch-repeat");
            } else {
                run_skron_args(repo, args);
            }
        };
        run(&[
            "-f",
            "--state-branch",
            "refs/heads/filter-state",
            "--msg-filter",
            "sed s/two/TWO/",
            "HEAD~1..HEAD",
        ]);
        run(&[
            "-f",
            "--state-branch",
            "refs/heads/filter-state",
            "--msg-filter",
            "sed s/three/THREE/",
            "HEAD~1..HEAD",
        ]);
    }

    assert_eq!(
        git(&skron_repo, ["show", "refs/heads/filter-state:filter.map"]),
        git(&git_repo, ["show", "refs/heads/filter-state:filter.map"])
    );
    assert_eq!(
        git(
            &skron_repo,
            [
                "show",
                "--format=%s",
                "--no-patch",
                "refs/heads/filter-state"
            ]
        ),
        git(
            &git_repo,
            [
                "show",
                "--format=%s",
                "--no-patch",
                "refs/heads/filter-state"
            ]
        )
    );
    assert_eq!(
        git(
            &skron_repo,
            ["rev-list", "--count", "refs/heads/filter-state"]
        ),
        git(
            &git_repo,
            ["rev-list", "--count", "refs/heads/filter-state"]
        )
    );
}

fn rewrite_pair(root: &Path, source: &Path) -> (PathBuf, PathBuf) {
    git(
        root,
        [
            "clone",
            source.to_str().expect("source path"),
            "git-rewrite",
        ],
    );
    git(
        root,
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-rewrite",
        ],
    );
    (root.join("git-rewrite"), root.join("skron-rewrite"))
}

fn stock_filter_branch(repo: &Path, args: &[&str], label: &str) {
    let mut git_args = vec!["-c", "commit.gpgsign=false", "filter-branch"];
    git_args.extend_from_slice(args);
    let output = Command::new("git")
        .args(git_args)
        .env("FILTER_BRANCH_SQUELCH_WARNING", "1")
        .current_dir(repo)
        .output()
        .unwrap_or_else(|err| panic!("run git filter-branch {label}: {err}"));
    assert!(
        output.status.success(),
        "git filter-branch {label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_skron_args(repo: &Path, args: &[&str]) {
    let mut skron_args = vec!["filter-branch"];
    skron_args.extend_from_slice(args);
    let output = Command::new(common::skron_bin())
        .args(skron_args)
        .current_dir(repo)
        .output()
        .unwrap_or_else(|err| panic!("run skron filter-branch: {err}"));
    assert!(
        output.status.success(),
        "skron filter-branch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
