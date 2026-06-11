use std::fs;
use std::process::Command;

use tempfile::TempDir;

fn skron_bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_skron-git").unwrap_or(env!("CARGO_BIN_EXE_skron"))
}

#[test]
fn invalid_option_combinations_match_stock_git_failures() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"base\n").expect("write a");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    let remote_dir = TempDir::new().expect("remote dir");
    let remote_path = remote_dir.path().join("remote.git");
    git(
        remote_dir.path(),
        ["init", "--bare", remote_path.to_str().expect("remote path")],
    );
    git(
        repo.path(),
        [
            "remote",
            "add",
            "origin",
            remote_path.to_str().expect("remote path"),
        ],
    );
    git(repo.path(), ["push", "origin", "HEAD:refs/heads/main"]);

    assert_eq!(
        command_output(skron_bin(), repo.path(), &["add"], "skron"),
        command_output("git", repo.path(), &["add"], "git")
    );
    assert_eq!(
        command_output(skron_bin(), repo.path(), &["tag", "-d"], "skron"),
        command_output("git", repo.path(), &["tag", "-d"], "git")
    );
    assert_eq!(
        command_output(skron_bin(), repo.path(), &["rev-parse"], "skron"),
        command_output("git", repo.path(), &["rev-parse"], "git")
    );
    assert_eq!(
        command_output(
            skron_bin(),
            repo.path(),
            &["update-ref", "-d", "refs/heads/missing"],
            "skron"
        ),
        command_output(
            "git",
            repo.path(),
            &["update-ref", "-d", "refs/heads/missing"],
            "git"
        )
    );
    let update_ref_head_repo = git_init();
    configure_identity(update_ref_head_repo.path());
    fs::write(update_ref_head_repo.path().join("a.txt"), b"base\n").expect("write a");
    git(update_ref_head_repo.path(), ["add", "a.txt"]);
    git_with_env(update_ref_head_repo.path(), ["commit", "-m", "base"]);
    assert_eq!(
        command_output(
            skron_bin(),
            update_ref_head_repo.path(),
            &["update-ref", "-d", "HEAD"],
            "skron"
        ),
        command_output(
            "git",
            update_ref_head_repo.path(),
            &["update-ref", "-d", "HEAD"],
            "git"
        )
    );
    assert_eq!(
        command_output(
            skron_bin(),
            repo.path(),
            &["reset", "--", "missing"],
            "skron"
        ),
        command_output("git", repo.path(), &["reset", "--", "missing"], "git")
    );
    assert_eq!(
        command_output(
            skron_bin(),
            repo.path(),
            &["reset", "HEAD", "--", "missing"],
            "skron"
        ),
        command_output(
            "git",
            repo.path(),
            &["reset", "HEAD", "--", "missing"],
            "git"
        )
    );
    assert_eq!(
        command_output(
            skron_bin(),
            repo.path(),
            &["reset", "--mixed", "--", "a.txt"],
            "skron"
        ),
        command_output(
            "git",
            repo.path(),
            &["reset", "--mixed", "--", "a.txt"],
            "git"
        )
    );
    assert_eq!(
        command_output(skron_bin(), repo.path(), &["log", "--format=%Q"], "skron"),
        command_output("git", repo.path(), &["log", "--format=%Q"], "git")
    );
    assert_eq!(
        command_output(
            skron_bin(),
            repo.path(),
            &["log", "--since", "bad"],
            "skron"
        ),
        command_output("git", repo.path(), &["log", "--since", "bad"], "git")
    );
    assert_eq!(
        command_output(
            skron_bin(),
            repo.path(),
            &["show", "--format=%Q", "HEAD"],
            "skron"
        ),
        command_output("git", repo.path(), &["show", "--format=%Q", "HEAD"], "git")
    );
    fs::write(repo.path().join("new.txt"), b"new\n").expect("write new");
    assert_eq!(
        command_output(skron_bin(), repo.path(), &["clean", "-x", "-n"], "skron"),
        command_output("git", repo.path(), &["clean", "-x", "-n"], "git")
    );
    assert_eq!(
        command_output(
            skron_bin(),
            repo.path(),
            &["status", "--porcelain=v2", "--short"],
            "skron"
        ),
        command_output(
            "git",
            repo.path(),
            &["status", "--porcelain=v2", "--short"],
            "git"
        )
    );
    assert_eq!(
        command_output(
            skron_bin(),
            repo.path(),
            &["branch", "-c", "main", "main"],
            "skron"
        ),
        command_output("git", repo.path(), &["branch", "-c", "main", "main"], "git")
    );
    assert_eq!(
        command_output(
            skron_bin(),
            repo.path(),
            &["remote", "set-head", "origin", "-d"],
            "skron"
        ),
        command_output(
            "git",
            repo.path(),
            &["remote", "set-head", "origin", "-d"],
            "git"
        )
    );
    assert_eq!(
        command_output(
            skron_bin(),
            repo.path(),
            &["remote", "set-head", "origin", "main"],
            "skron"
        ),
        command_output(
            "git",
            repo.path(),
            &["remote", "set-head", "origin", "main"],
            "git"
        )
    );
    assert_eq!(
        git(repo.path(), ["symbolic-ref", "refs/remotes/origin/HEAD"]),
        "refs/remotes/origin/main"
    );
    assert_eq!(
        command_output(
            skron_bin(),
            repo.path(),
            &["remote", "set-head", "origin", "-d"],
            "skron"
        ),
        command_output(
            "git",
            repo.path(),
            &["remote", "set-head", "origin", "-d"],
            "git"
        )
    );
    let repo_source = repo.path().to_str().expect("repo source path");
    fs::write(repo.path().join("clone-target-file"), b"occupied\n").expect("write clone target");
    fs::create_dir(repo.path().join("clone-target-dir")).expect("create clone target dir");
    fs::write(repo.path().join("clone-target-dir/occupied"), b"occupied\n")
        .expect("write clone target dir entry");
    for args in [
        [
            "clone",
            "-b",
            "missing",
            repo_source,
            "clone-missing-branch",
        ]
        .as_slice(),
        ["clone", "--depth", "bad", repo_source, "clone-bad-depth"].as_slice(),
        ["clone", "-o", "bad..name", repo_source, "clone-bad-origin"].as_slice(),
        [
            "clone",
            "--reference",
            "missing-reference",
            repo_source,
            "clone-bad-reference",
        ]
        .as_slice(),
        ["clone", "-c", "bad", repo_source, "clone-bad-config"].as_slice(),
        [
            "clone",
            "-c",
            "core.=x",
            repo_source,
            "clone-bad-config-variable",
        ]
        .as_slice(),
        [
            "clone",
            "--bare",
            "--separate-git-dir",
            "separate.git",
            repo_source,
            "clone-bare-separate",
        ]
        .as_slice(),
        [
            "clone",
            "--mirror",
            "--separate-git-dir",
            "separate.git",
            repo_source,
            "clone-mirror-separate",
        ]
        .as_slice(),
        ["clone", repo_source, "clone-target-file"].as_slice(),
        ["clone", repo_source, "clone-target-dir"].as_slice(),
    ] {
        assert_eq!(
            command_output(skron_bin(), repo.path(), args, "skron"),
            command_output("git", repo.path(), args, "git"),
            "clone failure mismatch for {args:?}"
        );
    }

    for args in [
        ["add", "missing"].as_slice(),
        ["add", "-u", "missing"].as_slice(),
        ["add", "-A", "--", "missing"].as_slice(),
        ["add", "--pathspec-from-file", "missing"].as_slice(),
        ["mv", "missing", "dest"].as_slice(),
        ["mv", "a.txt", "b.txt", "c.txt"].as_slice(),
        ["restore", "missing"].as_slice(),
        ["restore", "--staged", "missing"].as_slice(),
        ["restore", "--source", "missing", "a.txt"].as_slice(),
        ["checkout", "HEAD", "--", "missing"].as_slice(),
        ["checkout", "--", "a.txt", "missing"].as_slice(),
        ["checkout", "--detach", "-b", "one"].as_slice(),
        ["checkout", "--orphan", "one", "-b", "two"].as_slice(),
        ["checkout", "--detach", "main", "--", "a.txt"].as_slice(),
        ["checkout", "-b", "new", "missing"].as_slice(),
        ["checkout", "-B", "main", "missing"].as_slice(),
        ["switch", "--detach", "-c", "one"].as_slice(),
        ["switch", "-c", "one", "--detach"].as_slice(),
        ["switch", "--detach", "missing"].as_slice(),
        ["switch", "-c", "new", "missing"].as_slice(),
        ["switch", "--discard-changes", "missing"].as_slice(),
        ["commit", "--amend", "--no-edit", "--only", "missing"].as_slice(),
        ["commit", "-m", "msg", "--", "missing"].as_slice(),
        ["commit", "-m", "msg", "--only", "missing"].as_slice(),
        ["commit", "--fixup", "missing"].as_slice(),
        ["commit", "--fixup=amend:missing"].as_slice(),
        ["commit", "--fixup=reword:missing"].as_slice(),
        ["commit", "--fixup=bad:HEAD"].as_slice(),
        ["commit", "--fixup=amend:HEAD", "-m", "msg"].as_slice(),
        ["commit", "--fixup=reword:HEAD", "-m", "msg"].as_slice(),
        ["commit", "-C", "missing"].as_slice(),
        ["commit", "-c", "missing"].as_slice(),
        ["commit", "-c", "HEAD", "-m", "msg"].as_slice(),
        ["commit", "-c", "HEAD", "-C", "HEAD"].as_slice(),
        ["commit", "-c", "HEAD", "-F", "missing-message"].as_slice(),
        ["commit", "-c", "HEAD", "--fixup", "HEAD"].as_slice(),
        ["commit-tree", "HEAD"].as_slice(),
        ["commit-tree", "-m", "msg", "missing"].as_slice(),
        ["cat-file"].as_slice(),
        ["cat-file", "-t"].as_slice(),
        ["cat-file", "-p"].as_slice(),
        ["cat-file", "-t", "HEAD", "extra"].as_slice(),
        ["checkout-index", "missing"].as_slice(),
        ["rev-parse", "--verify", "HEAD", "extra"].as_slice(),
        ["log", "missing"].as_slice(),
        ["log", "--max-count", "bad"].as_slice(),
        ["show", "missing"].as_slice(),
        ["diff", "missing"].as_slice(),
        ["diff", "HEAD", "missing"].as_slice(),
        ["ls-tree", "missing"].as_slice(),
        ["rev-list", "missing"].as_slice(),
        ["merge-base", "missing", "HEAD"].as_slice(),
        ["merge-base", "--is-ancestor", "missing", "HEAD"].as_slice(),
        ["cherry-pick", "missing"].as_slice(),
        ["cherry-pick", "--abort"].as_slice(),
        ["revert", "missing"].as_slice(),
        ["revert", "--abort"].as_slice(),
        ["grep", "pattern", "missing"].as_slice(),
        ["apply", "missing.patch"].as_slice(),
        ["update-ref", "refs/heads/x"].as_slice(),
        ["symbolic-ref", "HEAD", "refs/heads/new", "extra"].as_slice(),
        ["notes", "show", "missing"].as_slice(),
        ["notes", "add", "-m", "note", "missing"].as_slice(),
        ["notes", "remove", "missing"].as_slice(),
        ["reflog", "show", "missing"].as_slice(),
        ["bisect", "good", "missing"].as_slice(),
        ["bisect", "bad", "missing"].as_slice(),
        ["bisect", "reset", "missing"].as_slice(),
        ["stash", "pop", "stash@{99}"].as_slice(),
        ["stash", "apply", "stash@{99}"].as_slice(),
        ["stash", "drop", "stash@{99}"].as_slice(),
        ["stash", "show", "stash@{99}"].as_slice(),
        ["merge", "missing"].as_slice(),
        ["merge", "--ff-only", "missing"].as_slice(),
        ["merge", "--abort"].as_slice(),
        ["merge", "--continue"].as_slice(),
        ["rebase", "--abort"].as_slice(),
        ["rebase", "--continue"].as_slice(),
        ["branch", "--set-upstream-to", "missing"].as_slice(),
        ["branch", "--set-upstream-to", "origin/missing"].as_slice(),
        ["branch", "--contains", "missing"].as_slice(),
        ["branch", "--merged", "missing"].as_slice(),
        ["branch", "--no-merged", "missing"].as_slice(),
        ["remote", "get-url", "missing"].as_slice(),
        ["remote", "remove", "missing"].as_slice(),
        ["remote", "rename", "missing", "new"].as_slice(),
        ["remote", "set-url", "missing", "url"].as_slice(),
        ["remote", "rename", "origin", "origin"].as_slice(),
        ["remote", "rename", "origin", "bad..name"].as_slice(),
        ["config", "--unset", "missing.key"].as_slice(),
        ["config", "--bool", "bad", "maybe"].as_slice(),
        ["status", "--porcelain=bad"].as_slice(),
        ["status", "--ignored=bad"].as_slice(),
        ["branch", "--unset-upstream"].as_slice(),
        ["branch", "--unset-upstream", "missingbranch"].as_slice(),
        ["push", "origin", "missingbranch"].as_slice(),
        ["push", "--set-upstream", "missingremote", "main"].as_slice(),
        ["fetch", "--depth", "bad", "origin"].as_slice(),
        ["fetch", "--depth", "1", "missingremote"].as_slice(),
        ["fetch", "origin", "missingbranch"].as_slice(),
        ["pull", "--rebase=bad", "origin", "main"].as_slice(),
        ["pull", "--ff-only", "origin", "missingbranch"].as_slice(),
        ["clone", "--depth", "bad", "file:///missing", "target"].as_slice(),
        ["worktree", "remove", "missing"].as_slice(),
        ["worktree", "add", "../wt", "missing"].as_slice(),
        ["worktree", "prune", "--expire", "bad"].as_slice(),
        ["remote", "set-head", "missing", "-a"].as_slice(),
        ["submodule", "status", "missing"].as_slice(),
        ["reset", "--soft", "--", "a.txt"].as_slice(),
        ["reset", "--hard", "--", "a.txt"].as_slice(),
        ["checkout-index", "--all", "a.txt"].as_slice(),
        ["read-tree", "--empty", "HEAD"].as_slice(),
        ["read-tree", "missing"].as_slice(),
        ["branch", "-d"].as_slice(),
        ["branch", "-d", "main", "missing"].as_slice(),
        ["branch", "-m", "missing", "new"].as_slice(),
        ["branch", "-M", "missing", "new"].as_slice(),
        ["branch", "-m", "main", "bad..name"].as_slice(),
        ["tag", "bad..name"].as_slice(),
        ["tag", "-d", "missing"].as_slice(),
        ["tag", "-m", "msg"].as_slice(),
        ["tag", "-a", "v1", "-m", "msg", "missing"].as_slice(),
        ["tag", "-v", "missing"].as_slice(),
        ["for-each-ref", "--format", "%(bad)", "refs/heads"].as_slice(),
        ["show-ref", "--verify", "refs/heads/missing"].as_slice(),
        ["show-ref", "--verify", "refs/heads/main", "refs/tags/v1"].as_slice(),
    ] {
        assert_eq!(
            run_skron_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "failure mismatch for {args:?}"
        );
    }
}

#[test]
fn commit_conflict_and_invalid_cleanup_failures_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"base\n").expect("write a");
    git_with_env(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    assert_eq!(
        run_skron_failure_output(
            repo.path(),
            &["commit", "--squash", "HEAD", "--fixup", "HEAD"]
        ),
        git_failure_output(
            repo.path(),
            &["commit", "--squash", "HEAD", "--fixup", "HEAD"]
        ),
    );
    fs::write(repo.path().join("a.txt"), b"dirty\n").expect("write dirty");
    git(repo.path(), ["add", "-A"]);
    assert_eq!(
        run_skron_failure_output(repo.path(), &["commit", "--cleanup", "bogus", "-m", "msg"]),
        git_failure_output(repo.path(), &["commit", "--cleanup", "bogus", "-m", "msg"]),
    );
}

fn git_init() -> TempDir {
    let repo = TempDir::new().expect("temp repo");
    git(repo.path(), ["init"]);
    repo
}

fn configure_identity(cwd: &std::path::Path) {
    git(cwd, ["config", "user.name", "Bench"]);
    git(cwd, ["config", "user.email", "bench@example.test"]);
    git(cwd, ["config", "commit.gpgsign", "false"]);
}

fn command_output(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    label: &str,
) -> (i32, String, String) {
    command_output_with_env(command, cwd, args, &[], label)
}

fn command_output_with_env(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
    label: &str,
) -> (i32, String, String) {
    let output = Command::new(command)
        .args(args)
        .envs(envs.iter().copied())
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|err| panic!("run {label}: {err}"));
    (
        output.status.code().expect("process exit code"),
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

fn run_skron_failure_output(cwd: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    command_failure_output(skron_bin(), cwd, args, "skron")
}

fn git_failure_output(cwd: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    command_failure_output("git", cwd, args, "git")
}

fn command_failure_output(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    label: &str,
) -> (i32, String, String) {
    let output = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|err| panic!("run {label}: {err}"));
    assert!(
        !output.status.success(),
        "{label} unexpectedly succeeded: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    (
        output.status.code().expect("process exit code"),
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

fn git_with_env<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> String {
    let output = Command::new("git")
        .args(args)
        .env("GIT_AUTHOR_NAME", "Bench")
        .env("GIT_AUTHOR_EMAIL", "bench@example.test")
        .env("GIT_COMMITTER_NAME", "Bench")
        .env("GIT_COMMITTER_EMAIL", "bench@example.test")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn git<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> String {
    git_args(cwd, &args)
}

fn git_args(cwd: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}
