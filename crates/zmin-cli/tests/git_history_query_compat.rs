mod common;

use std::io::Write;
use std::process::{Command, Stdio};
use std::fs;

use tempfile::TempDir;

use common::{
    clone_repo_fixture, command_any_output, command_output, command_output_with_env,
    configure_identity, git, git_args, git_failure_output, git_init, git_status, git_with_env,
    run_zmin, run_zmin_args, run_zmin_failure_output, run_zmin_status, run_zmin_with_env,
    stock_git_bin, write_file, zmin_bin,
};

fn commit_empty_as(cwd: &std::path::Path, name: &str, email: &str, message: &str) {
    let output = Command::new(stock_git_bin())
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
    let output = Command::new(stock_git_bin())
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

fn git_commit_with_identities(
    cwd: &std::path::Path,
    author_name: &str,
    author_email: &str,
    committer_name: &str,
    committer_email: &str,
    date: &str,
    message: &str,
) {
    let output = Command::new(stock_git_bin())
        .args(["-c", "commit.gpgsign=false", "commit", "-m", message])
        .env("GIT_AUTHOR_NAME", author_name)
        .env("GIT_AUTHOR_EMAIL", author_email)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_NAME", committer_name)
        .env("GIT_COMMITTER_EMAIL", committer_email)
        .env("GIT_COMMITTER_DATE", date)
        .current_dir(cwd)
        .output()
        .expect("git commit with identities");
    assert!(
        output.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_loose_blob(cwd: &std::path::Path, content: &str) {
    let mut child = Command::new(stock_git_bin())
        .args(["hash-object", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .current_dir(cwd)
        .spawn()
        .expect("spawn git hash-object");
    child
        .stdin
        .as_mut()
        .expect("hash-object stdin")
        .write_all(content.as_bytes())
        .expect("write hash-object content");
    let output = child.wait_with_output().expect("wait git hash-object");
    assert!(
        output.status.success(),
        "git hash-object failed: {}",
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

fn blame_line_range_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\ntwo\nthree\nfour\nfive\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "A",
        "a@example.test",
        "1700000000 +0000",
        "base",
    );
    repo
}

fn blame_basic_regex_literal_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(
        repo.path(),
        "a.txt",
        "paren (\nbrace {\nplus a+\nquestion a?\npipe a|\nstar *\nxx\nxy\nx(y)\n",
    );
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "A",
        "a@example.test",
        "1700000000 +0000",
        "base",
    );
    repo
}

fn whatchanged_cases() -> Vec<Vec<&'static str>> {
    if !stock_git_version_at_least(2, 54) {
        return Vec::new();
    }
    let prefix = vec!["whatchanged", "--i-still-use-this"];
    let mut plain = prefix.clone();
    plain.extend(["--max-count", "1"]);
    let mut stat = prefix.clone();
    stat.extend(["--stat", "--max-count", "1"]);
    let mut oneline = prefix;
    oneline.extend(["--oneline", "--max-count", "1"]);
    vec![plain, stat, oneline]
}

fn stock_git_version_at_least(major: u32, minor: u32) -> bool {
    let output = Command::new(stock_git_bin())
        .arg("--version")
        .output()
        .expect("git version");
    let version = String::from_utf8_lossy(&output.stdout);
    let Some(version) = version.split_whitespace().nth(2) else {
        return false;
    };
    let mut parts = version.split('.');
    let actual_major = parts.next().and_then(|value| value.parse::<u32>().ok());
    let actual_minor = parts.next().and_then(|value| value.parse::<u32>().ok());
    match (actual_major, actual_minor) {
        (Some(actual_major), Some(actual_minor)) => (actual_major, actual_minor) >= (major, minor),
        _ => false,
    }
}

#[test]
fn whatchanged_requires_explicit_opt_in_like_git_2_54() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "a\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    let (code, stdout, stderr) = run_zmin_failure_output(repo.path(), &["whatchanged"]);
    assert_eq!(code, 128);
    assert!(stdout.is_empty());
    assert!(stderr.contains("nominated for removal"));
    assert!(stderr.contains("--i-still-use-this"));
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
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn shortlog_documented_option_family_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "a@example.test",
        "1700000000 +0000",
        "feat: init\n\nReviewed-by: Rev <rev@example.test>",
    );
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Bob",
        "b@example.test",
        "1700000600 +0000",
        "fix: second\n\nReviewed-by: Rev <rev@example.test>\nCo-authored-by: Co <co@example.test>",
    );

    for args in [
        ["shortlog", "--group=committer", "HEAD"].as_slice(),
        ["shortlog", "--group=author", "--group=committer", "HEAD"].as_slice(),
        ["shortlog", "--group=committer", "--group=author", "HEAD"].as_slice(),
        ["shortlog", "--group=author", "--group=author", "HEAD"].as_slice(),
        ["shortlog", "--group=trailer:reviewed-by", "-sne", "HEAD"].as_slice(),
        [
            "shortlog",
            "--group=trailer:reviewed-by",
            "--group=format:%an",
            "-sne",
            "HEAD",
        ]
        .as_slice(),
        ["shortlog", "--group=format:%an", "-sn", "HEAD"].as_slice(),
        ["shortlog", "--format=%h %s", "HEAD"].as_slice(),
        ["shortlog", "--date=short", "--group=format:%ad", "-sn", "HEAD"].as_slice(),
        ["shortlog", "--format=%s", "--format=%h", "HEAD"].as_slice(),
        ["shortlog", "--format=%h", "--format=%s", "HEAD"].as_slice(),
        [
            "shortlog",
            "--date=short",
            "--date=human",
            "--group=format:%ad",
            "-sn",
            "HEAD",
        ]
        .as_slice(),
        [
            "shortlog",
            "--date=human",
            "--date=short",
            "--group=format:%ad",
            "-sn",
            "HEAD",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    for args in [
        ["shortlog", "--group=bogus", "HEAD"].as_slice(),
        ["shortlog", "--date=bogus", "--group=format:%ad", "-sn", "HEAD"].as_slice(),
        ["shortlog", "-wbogus", "HEAD"].as_slice(),
        ["shortlog", "--stdin"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }

    let wrap_repo = git_init();
    git(wrap_repo.path(), ["checkout", "-b", "main"]);
    write_file(wrap_repo.path(), "a.txt", "one\n");
    git(wrap_repo.path(), ["add", "-A"]);
    git_commit_with_author(
        wrap_repo.path(),
        "Alice",
        "a@example.test",
        "1700000000 +0000",
        "feat: this is a very long subject line that should wrap in shortlog output once the width is intentionally tiny",
    );

    for args in [
        ["shortlog", "-w20,4,6", "HEAD"].as_slice(),
        ["shortlog", "-w0,4,6", "HEAD"].as_slice(),
        ["shortlog", "-w20,4,6", "-w0,4,6", "HEAD"].as_slice(),
        ["shortlog", "-w0,4,6", "-w20,4,6", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(wrap_repo.path(), args),
            git_args(wrap_repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn shortlog_grep_family_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "a@example.test",
        "1700000000 +0000",
        "feat: Alpha\n\nbody apple banana",
    );
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "a@example.test",
        "1700000600 +0000",
        "fix: beta\n\nbody BANANA carrot",
    );
    write_file(repo.path(), "a.txt", "three\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "a@example.test",
        "1700001200 +0000",
        "chore: gamma\n\nbody carrot delta",
    );

    for args in [
        ["shortlog", "--grep=banana", "HEAD"].as_slice(),
        ["shortlog", "--grep=banana", "-i", "HEAD"].as_slice(),
        ["shortlog", "--grep=banana", "--regexp-ignore-case", "HEAD"].as_slice(),
        ["shortlog", "--grep=banana", "--invert-grep", "HEAD"].as_slice(),
        ["shortlog", "--grep=banana", "--grep=carrot", "--all-match", "HEAD"].as_slice(),
        ["shortlog", "--grep=banana", "--grep=carrot", "HEAD"].as_slice(),
        ["shortlog", "--grep=BA[N]ANA", "-E", "HEAD"].as_slice(),
        ["shortlog", "--grep=BA[N]ANA", "--extended-regexp", "HEAD"].as_slice(),
        ["shortlog", "--grep=BA[N]ANA", "-i", "-E", "HEAD"].as_slice(),
        ["shortlog", "--grep=BA[N]ANA", "-E", "-F", "HEAD"].as_slice(),
        ["shortlog", "--grep=BA[N]ANA", "-F", "-E", "HEAD"].as_slice(),
        ["shortlog", "--grep=banana", "-F", "HEAD"].as_slice(),
        ["shortlog", "--grep=banana", "--fixed-strings", "HEAD"].as_slice(),
        [
            "shortlog",
            "--grep=BA[N]ANA",
            "--extended-regexp",
            "--fixed-strings",
            "HEAD",
        ]
        .as_slice(),
        [
            "shortlog",
            "--grep=BA[N]ANA",
            "--fixed-strings",
            "--extended-regexp",
            "HEAD",
        ]
        .as_slice(),
        ["shortlog", "--grep=ba.+na", "-P", "HEAD"].as_slice(),
        ["shortlog", "--grep=ba.+na", "--perl-regexp", "HEAD"].as_slice(),
        ["shortlog", "--grep=ba.+na", "-P", "-F", "HEAD"].as_slice(),
        ["shortlog", "--grep=ba.+na", "-F", "-P", "HEAD"].as_slice(),
        [
            "shortlog",
            "--grep=ba.+na",
            "--perl-regexp",
            "--fixed-strings",
            "HEAD",
        ]
        .as_slice(),
        [
            "shortlog",
            "--grep=ba.+na",
            "--fixed-strings",
            "--perl-regexp",
            "HEAD",
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
fn shortlog_reflog_option_family_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "a@example.test",
        "1700000000 +0000",
        "feat: one",
    );
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Bob",
        "b@example.test",
        "1700000600 +0000",
        "fix: two",
    );

    for args in [
        ["shortlog", "--reflog", "HEAD"].as_slice(),
        ["shortlog", "--walk-reflogs", "HEAD"].as_slice(),
        ["shortlog", "--grep-reflog=one", "--walk-reflogs", "HEAD"].as_slice(),
        [
            "shortlog",
            "--grep-reflog=one",
            "--walk-reflogs",
            "--grep-reflog=two",
            "HEAD",
        ]
        .as_slice(),
        ["shortlog", "-g", "HEAD"].as_slice(),
        ["shortlog", "-g", "--reflog", "HEAD"].as_slice(),
        ["shortlog", "--reflog", "-g", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    for args in [
        ["shortlog", "--grep-reflog=one", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn log_grep_family_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "a@example.test",
        "1700000000 +0000",
        "feat: Alpha\n\nbody apple banana",
    );
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "a@example.test",
        "1700000600 +0000",
        "fix: beta\n\nbody BANANA carrot",
    );
    write_file(repo.path(), "a.txt", "three\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "a@example.test",
        "1700001200 +0000",
        "chore: gamma\n\nbody carrot delta",
    );

    for args in [
        ["log", "--grep=banana", "--format=%s", "HEAD"].as_slice(),
        ["log", "--grep=banana", "-i", "--format=%s", "HEAD"].as_slice(),
        [
            "log",
            "--grep=banana",
            "--regexp-ignore-case",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        ["log", "--grep=banana", "--invert-grep", "--format=%s", "HEAD"].as_slice(),
        [
            "log",
            "--grep=banana",
            "--grep=carrot",
            "--all-match",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        ["log", "--grep=banana", "--grep=carrot", "--format=%s", "HEAD"].as_slice(),
        ["log", "--grep=BA[N]ANA", "-E", "--format=%s", "HEAD"].as_slice(),
        [
            "log",
            "--grep=BA[N]ANA",
            "--extended-regexp",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        ["log", "--grep=BA[N]ANA", "--basic-regexp", "--format=%s", "HEAD"].as_slice(),
        ["log", "--grep=banana", "-F", "--format=%s", "HEAD"].as_slice(),
        [
            "log",
            "--grep=banana",
            "--fixed-strings",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        [
            "log",
            "--grep=BA[N]ANA",
            "--extended-regexp",
            "--fixed-strings",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        [
            "log",
            "--grep=BA[N]ANA",
            "--fixed-strings",
            "--extended-regexp",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        ["log", "--grep=ba.+na", "-P", "--format=%s", "HEAD"].as_slice(),
        [
            "log",
            "--grep=ba.+na",
            "--perl-regexp",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        ["log", "--grep=ba.+na", "-P", "-F", "--format=%s", "HEAD"].as_slice(),
        ["log", "--grep=ba.+na", "-F", "-P", "--format=%s", "HEAD"].as_slice(),
        [
            "log",
            "--grep=ba.+na",
            "--perl-regexp",
            "--fixed-strings",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        [
            "log",
            "--grep=ba.+na",
            "--fixed-strings",
            "--perl-regexp",
            "--format=%s",
            "HEAD",
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
fn log_grep_reflog_requires_walk_reflogs_and_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "a@example.test",
        "1700000000 +0000",
        "feat: one",
    );
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "a@example.test",
        "1700000600 +0000",
        "fix: two",
    );

    assert_eq!(
        run_zmin_args(
            repo.path(),
            &[
                "log",
                "--walk-reflogs",
                "--grep-reflog=commit",
                "--format=%gd %gs",
                "HEAD",
            ],
        ),
        git_args(
            repo.path(),
            &[
                "log",
                "--walk-reflogs",
                "--grep-reflog=commit",
                "--format=%gd %gs",
                "HEAD",
            ],
        )
    );

    assert_eq!(
        run_zmin_failure_output(repo.path(), &["log", "--grep-reflog=commit", "HEAD"]),
        git_failure_output(repo.path(), &["log", "--grep-reflog=commit", "HEAD"])
    );
}

#[test]
fn log_identity_time_and_parent_filters_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);

    write_file(repo.path(), "a.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_identities(
        repo.path(),
        "Alice",
        "alice@example.test",
        "Carol",
        "carol@example.test",
        "2023-11-14T22:13:20Z",
        "feat: base",
    );

    git(repo.path(), ["checkout", "-b", "topic"]);
    write_file(repo.path(), "topic.txt", "topic\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_identities(
        repo.path(),
        "Alice",
        "alice@example.test",
        "Eve",
        "eve@example.test",
        "2023-11-14T22:33:20Z",
        "feat: topic",
    );

    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "main.txt", "main\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_identities(
        repo.path(),
        "Bob",
        "bob@example.test",
        "Dana",
        "dana@example.test",
        "2023-11-14T22:23:20Z",
        "fix: main",
    );

    let merge = Command::new(stock_git_bin())
        .args(["merge", "--no-ff", "topic", "-m", "merge topic"])
        .env("GIT_AUTHOR_NAME", "Frank")
        .env("GIT_AUTHOR_EMAIL", "frank@example.test")
        .env("GIT_AUTHOR_DATE", "2023-11-14T22:43:20Z")
        .env("GIT_COMMITTER_NAME", "Frank")
        .env("GIT_COMMITTER_EMAIL", "frank@example.test")
        .env("GIT_COMMITTER_DATE", "2023-11-14T22:43:20Z")
        .current_dir(repo.path())
        .output()
        .expect("git merge");
    assert!(
        merge.status.success(),
        "git merge failed: {}",
        String::from_utf8_lossy(&merge.stderr)
    );

    for args in [
        ["log", "--author=Alice", "--format=%s", "HEAD"].as_slice(),
        ["log", "--committer=Eve", "--format=%s", "HEAD"].as_slice(),
        [
            "log",
            "--after=2023-11-14T22:20:00Z",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        [
            "log",
            "--until=2023-11-14T22:20:00Z",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        [
            "log",
            "--before=2023-11-14T22:20:00Z",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        ["log", "--count", "--author=Alice", "HEAD"].as_slice(),
        ["log", "--merges", "--format=%s", "HEAD"].as_slice(),
        ["log", "--no-merges", "--format=%s", "HEAD"].as_slice(),
        ["log", "--max-parents=1", "--format=%s", "HEAD"].as_slice(),
        ["log", "--no-max-parents", "--format=%s", "HEAD"].as_slice(),
        ["log", "--min-parents=2", "--format=%s", "HEAD"].as_slice(),
        ["log", "--no-min-parents", "--format=%s", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn log_notes_and_abbrev_commit_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);

    write_file(repo.path(), "a.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "alice@example.test",
        "1700000000 +0000",
        "feat: base",
    );
    git(repo.path(), ["notes", "add", "-m", "note body"]);

    for args in [
        ["log", "--no-notes", "-1"].as_slice(),
        ["log", "--format=%N", "--notes", "-1"].as_slice(),
        ["log", "--abbrev-commit", "-1"].as_slice(),
        ["log", "--oneline", "--no-abbrev-commit", "-1"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn show_notes_aliases_and_abbrev_commit_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());

    write_file(repo.path(), "a.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "alice@example.test",
        "1700000000 +0000",
        "feat: base",
    );
    git(repo.path(), ["notes", "add", "-m", "note body"]);

    for args in [
        ["show", "--notes", "HEAD"].as_slice(),
        ["show", "--no-notes", "HEAD"].as_slice(),
        ["show", "--show-notes", "HEAD"].as_slice(),
        ["show", "--standard-notes", "HEAD"].as_slice(),
        ["show", "--show-notes-by-default", "HEAD"].as_slice(),
        ["show", "--no-standard-notes", "HEAD"].as_slice(),
        ["show", "--abbrev-commit", "HEAD"].as_slice(),
        ["show", "--oneline", "--no-abbrev-commit", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_line_range_forms_match_stock_git() {
    let git_repo = blame_line_range_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    for args in [
        ["blame", "-L", "3,-2", "a.txt"].as_slice(),
        ["blame", "-L", ",3", "a.txt"].as_slice(),
        ["blame", "-L", "/two/,-1", "a.txt"].as_slice(),
        ["blame", "-L", "2,/four/", "a.txt"].as_slice(),
        ["blame", "-L", "/two/,/four/", "a.txt"].as_slice(),
        ["blame", "-L", "^/two/", "a.txt"].as_slice(),
        ["blame", "-L", ":two", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_reversed_absolute_line_ranges_match_stock_git() {
    let git_repo = blame_line_range_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    for args in [
        ["blame", "-L", "2,1", "a.txt"].as_slice(),
        ["blame", "-L", "4,2", "a.txt"].as_slice(),
        ["blame", "-L", "5,1", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_and_annotate_match_stock_git_for_simple_linear_history() {
    let git_repo = blame_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    for args in [
        ["blame", "a.txt"].as_slice(),
        ["blame", "-l", "a.txt"].as_slice(),
        ["blame", "-p", "a.txt"].as_slice(),
        ["blame", "--incremental", "a.txt"].as_slice(),
        ["blame", "--line-porcelain", "a.txt"].as_slice(),
        ["blame", "-f", "a.txt"].as_slice(),
        ["blame", "-n", "a.txt"].as_slice(),
        ["blame", "-e", "a.txt"].as_slice(),
        ["blame", "--abbrev=12", "a.txt"].as_slice(),
        ["blame", "--no-abbrev", "a.txt"].as_slice(),
        ["blame", "--abbrev=12", "--no-abbrev", "a.txt"].as_slice(),
        ["blame", "--no-abbrev", "--abbrev=12", "a.txt"].as_slice(),
        ["blame", "--date=iso", "a.txt"].as_slice(),
        ["blame", "--date=iso-strict", "a.txt"].as_slice(),
        ["blame", "--date=default", "a.txt"].as_slice(),
        ["blame", "--date=short", "a.txt"].as_slice(),
        ["blame", "--date=raw", "a.txt"].as_slice(),
        ["blame", "--date=unix", "a.txt"].as_slice(),
        ["blame", "--date=rfc", "a.txt"].as_slice(),
        ["blame", "--date=rfc2822", "a.txt"].as_slice(),
        ["blame", "--date=local", "a.txt"].as_slice(),
        ["blame", "-L", "1,1", "a.txt"].as_slice(),
        ["blame", "-w", "a.txt"].as_slice(),
        ["blame", "--root", "a.txt"].as_slice(),
        ["blame", "-b", "a.txt"].as_slice(),
        ["blame", "-c", "a.txt"].as_slice(),
        ["blame", "-s", "a.txt"].as_slice(),
        ["blame", "-t", "a.txt"].as_slice(),
        ["blame", "--show-stats", "a.txt"].as_slice(),
        ["blame", "-M", "a.txt"].as_slice(),
        ["blame", "-C", "a.txt"].as_slice(),
        ["blame", "-L", "2", "a.txt"].as_slice(),
        ["blame", "-L", "2,+1", "a.txt"].as_slice(),
        ["blame", "--no-incremental", "a.txt"].as_slice(),
        ["blame", "--incremental", "--no-incremental", "a.txt"].as_slice(),
        ["blame", "--no-porcelain", "a.txt"].as_slice(),
        ["blame", "--porcelain", "--no-porcelain", "a.txt"].as_slice(),
        ["blame", "--no-line-porcelain", "a.txt"].as_slice(),
        ["blame", "--line-porcelain", "--no-line-porcelain", "a.txt"].as_slice(),
        ["blame", "--no-root", "a.txt"].as_slice(),
        ["blame", "--no-show-stats", "a.txt"].as_slice(),
        ["blame", "--show-stats", "--no-show-stats", "a.txt"].as_slice(),
        ["blame", "--no-show-name", "a.txt"].as_slice(),
        ["blame", "--show-name", "--no-show-name", "a.txt"].as_slice(),
        ["blame", "--no-show-number", "a.txt"].as_slice(),
        ["blame", "--show-number", "--no-show-number", "a.txt"].as_slice(),
        ["blame", "--no-show-email", "a.txt"].as_slice(),
        ["blame", "--show-email", "--no-show-email", "a.txt"].as_slice(),
        ["blame", "--no-progress", "a.txt"].as_slice(),
        ["blame", "--progress", "--no-progress", "a.txt"].as_slice(),
        ["blame", "--no-score-debug", "a.txt"].as_slice(),
        ["blame", "--score-debug", "--no-score-debug", "a.txt"].as_slice(),
        ["blame", "--no-color-lines", "a.txt"].as_slice(),
        ["blame", "--color-lines", "--no-color-lines", "a.txt"].as_slice(),
        ["blame", "--no-color-by-age", "a.txt"].as_slice(),
        ["blame", "--color-by-age", "--no-color-by-age", "a.txt"].as_slice(),
        ["blame", "--no-minimal", "a.txt"].as_slice(),
        ["blame", "--minimal", "--no-minimal", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
    let date_env = [("GIT_TEST_DATE_NOW", "1780000000")];
    for args in [
        ["blame", "--date=relative", "a.txt"].as_slice(),
        ["blame", "--date=human", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            command_output_with_env(zmin_bin(), zmin_repo.path(), args, &date_env, "zmin").1,
            command_output_with_env("git", git_repo.path(), args, &date_env, "git").1,
            "args: {args:?}"
        );
    }
    assert_eq!(
        run_zmin(zmin_repo.path(), ["annotate", "a.txt"]),
        git(git_repo.path(), ["annotate", "a.txt"])
    );

    write_file(git_repo.path(), "contents.txt", "one\nTWO\n");
    write_file(zmin_repo.path(), "contents.txt", "one\nTWO\n");
    let git_contents_path = git_repo.path().join("contents.txt");
    let zmin_contents_path = zmin_repo.path().join("contents.txt");
    let git_contents = git_contents_path.to_string_lossy();
    let zmin_contents = zmin_contents_path.to_string_lossy();
    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &["blame", "--contents", &zmin_contents, "HEAD", "--", "a.txt"],
        ),
        git_args(
            git_repo.path(),
            &["blame", "--contents", &git_contents, "HEAD", "--", "a.txt"],
        )
    );
}

#[test]
fn blame_documented_option_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\ntwo\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);

    write_file(repo.path(), "contents.txt", "uno\ndos\n");
    let head = command_output("git", repo.path(), &["rev-parse", "HEAD"], "git").1;
    write_file(repo.path(), "ignore-revs.txt", &format!("{head}\n"));
    write_file(repo.path(), "revs.txt", &format!("{head}\n"));

    for args in [
        ["blame", "--encoding=none", "a.txt"].as_slice(),
        ["blame", "--first-parent", "a.txt"].as_slice(),
        ["blame", "--ignore-rev", &head, "a.txt"].as_slice(),
        ["blame", "--ignore-revs-file", "ignore-revs.txt", "a.txt"].as_slice(),
        ["blame", "-S", "revs.txt", "a.txt"].as_slice(),
    ] {
        assert_eq!(run_zmin_args(repo.path(), args), git_args(repo.path(), args), "args: {args:?}");
    }

    for args in [
        ["blame", "--reverse", "HEAD..HEAD", "a.txt"].as_slice(),
        ["blame", "-h"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn annotate_documented_option_family_matches_stock_git() {
    let git_repo = blame_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    let head = command_output("git", git_repo.path(), &["rev-parse", "HEAD"], "git").1;

    write_file(git_repo.path(), "ignore-revs.txt", &format!("{head}\n"));
    write_file(zmin_repo.path(), "ignore-revs.txt", &format!("{head}\n"));
    write_file(git_repo.path(), "revs.txt", &format!("{head}\n"));
    write_file(zmin_repo.path(), "revs.txt", &format!("{head}\n"));

    for args in [
        ["annotate", "-l", "a.txt"].as_slice(),
        ["annotate", "-p", "a.txt"].as_slice(),
        ["annotate", "--porcelain", "a.txt"].as_slice(),
        ["annotate", "--incremental", "a.txt"].as_slice(),
        ["annotate", "--line-porcelain", "a.txt"].as_slice(),
        ["annotate", "--date=short", "a.txt"].as_slice(),
        ["annotate", "--encoding=none", "a.txt"].as_slice(),
        ["annotate", "--no-progress", "a.txt"].as_slice(),
        ["annotate", "--root", "a.txt"].as_slice(),
        ["annotate", "--show-stats", "a.txt"].as_slice(),
        ["annotate", "-C", "a.txt"].as_slice(),
        ["annotate", "-L", "1,1", "a.txt"].as_slice(),
        ["annotate", "-M", "a.txt"].as_slice(),
        ["annotate", "-b", "a.txt"].as_slice(),
        ["annotate", "-t", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }

    for args in [
        ["annotate", "--progress", "a.txt"].as_slice(),
        ["annotate", "--color-lines", "a.txt"].as_slice(),
        ["annotate", "--color-by-age", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), zmin_repo.path(), args, "zmin"),
            command_output("git", git_repo.path(), args, "git"),
            "args: {args:?}"
        );
    }

    write_file(git_repo.path(), "contents.txt", "one\nTWO\n");
    write_file(zmin_repo.path(), "contents.txt", "one\nTWO\n");
    let git_contents = git_repo.path().join("contents.txt");
    let zmin_contents = zmin_repo.path().join("contents.txt");
    let git_contents = git_contents.to_string_lossy().into_owned();
    let zmin_contents = zmin_contents.to_string_lossy().into_owned();
    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &["annotate", "--contents", &zmin_contents, "HEAD", "--", "a.txt"],
        ),
        git_args(
            git_repo.path(),
            &["annotate", "--contents", &git_contents, "HEAD", "--", "a.txt"],
        )
    );

    let opt_git_repo = git_init();
    configure_identity(opt_git_repo.path());
    write_file(opt_git_repo.path(), "a.txt", "one\ntwo\n");
    git(opt_git_repo.path(), ["add", "-A"]);
    git_with_env(opt_git_repo.path(), ["commit", "-m", "init"]);
    let opt_zmin_repo = clone_repo_fixture(opt_git_repo.path());
    let opt_head = command_output("git", opt_git_repo.path(), &["rev-parse", "HEAD"], "git").1;
    write_file(opt_git_repo.path(), "ignore-revs.txt", &format!("{opt_head}\n"));
    write_file(opt_zmin_repo.path(), "ignore-revs.txt", &format!("{opt_head}\n"));
    write_file(opt_git_repo.path(), "revs.txt", &format!("{opt_head}\n"));
    write_file(opt_zmin_repo.path(), "revs.txt", &format!("{opt_head}\n"));

    for args in [
        ["annotate", "--first-parent", "a.txt"].as_slice(),
        ["annotate", "--ignore-rev", &opt_head, "a.txt"].as_slice(),
        ["annotate", "--ignore-revs-file", "ignore-revs.txt", "a.txt"].as_slice(),
        ["annotate", "-S", "revs.txt", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(opt_zmin_repo.path(), args),
            git_args(opt_git_repo.path(), args),
            "args: {args:?}"
        );
    }

    for args in [
        ["annotate", "--reverse", "HEAD..HEAD", "a.txt"].as_slice(),
        ["annotate", "-h"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), args),
            git_failure_output(git_repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_invalid_date_format_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "a\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);

    assert_eq!(
        run_zmin_failure_output(repo.path(), &["blame", "--date=bogus", "a.txt"]),
        git_failure_output(repo.path(), &["blame", "--date=bogus", "a.txt"])
    );
}

#[test]
fn blame_unknown_option_matches_stock_git_usage() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "a\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);

    assert_eq!(
        run_zmin_failure_output(repo.path(), &["blame", "--bad", "a.txt"]),
        git_failure_output(repo.path(), &["blame", "--bad", "a.txt"])
    );
}

#[test]
fn blame_zero_line_range_matches_stock_git_failure() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\ntwo\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);

    for args in [
        ["blame", "-L", "0", "a.txt"].as_slice(),
        ["blame", "-L", "1,0", "a.txt"].as_slice(),
        ["blame", "-L", "/one/,0", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_empty_line_range_matches_stock_git_failure() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\ntwo\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);

    for args in [
        ["blame", "-L", "1,+0", "a.txt"].as_slice(),
        ["blame", "-L", "1,-0", "a.txt"].as_slice(),
        ["blame", "-L", "/one/,+0", "a.txt"].as_slice(),
        ["blame", "-L", "/one/,-0", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_missing_function_line_range_matches_stock_git_failure() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "fn one() {\n    alpha();\n}\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);

    assert_eq!(
        run_zmin_failure_output(repo.path(), &["blame", "-L", ":missing", "a.txt"]),
        git_failure_output(repo.path(), &["blame", "-L", ":missing", "a.txt"])
    );
}

#[test]
fn blame_missing_regex_line_range_matches_stock_git_failure() {
    let repo = blame_line_range_fixture_repo();

    assert_eq!(
        run_zmin_failure_output(repo.path(), &["blame", "-L", "/missing/", "a.txt"]),
        git_failure_output(repo.path(), &["blame", "-L", "/missing/", "a.txt"])
    );
}

#[test]
fn blame_missing_end_regex_line_ranges_match_stock_git_failure() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "2,/missing/", "a.txt"].as_slice(),
        ["blame", "-L", "/two/,/missing/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_invalid_regex_line_ranges_match_stock_git_failure() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "/[/", "a.txt"].as_slice(),
        ["blame", "-L", "1,/[/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_unbalanced_bracket_regex_line_ranges_match_stock_git_failure() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "/[a-/", "a.txt"].as_slice(),
        ["blame", "-L", "1,/[a-/", "a.txt"].as_slice(),
        ["blame", "-L", "/one/,/[a-/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_invalid_character_range_regexes_match_stock_git_failure() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "/[z-a]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[b-a]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[a-b-c]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[0-9-a]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[a--]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[[:digit:]-a]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[[:digit:]-[:alpha:]]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[a-[:upper:]]/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_empty_character_class_regexes_match_stock_git_failure() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "/[]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[^]/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_empty_function_and_regex_ranges_match_stock_git_usage() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", ":", "a.txt"].as_slice(),
        ["blame", "-L", "/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_malformed_numeric_line_ranges_match_stock_git_usage() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "abc", "a.txt"].as_slice(),
        ["blame", "-L", "1,abc", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_malformed_count_line_ranges_match_stock_git_usage() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "1,+abc", "a.txt"].as_slice(),
        ["blame", "-L", "1,-abc", "a.txt"].as_slice(),
        ["blame", "-L", "/one/,+abc", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_unterminated_regex_line_ranges_match_stock_git_usage() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "/one", "a.txt"].as_slice(),
        ["blame", "-L", "1,/one", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_empty_end_regex_line_ranges_match_stock_git_failure() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "1,//", "a.txt"].as_slice(),
        ["blame", "-L", "/one/,//", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_empty_start_regex_line_ranges_match_stock_git_failure() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "//", "a.txt"].as_slice(),
        ["blame", "-L", "//,+1", "a.txt"].as_slice(),
        ["blame", "-L", "^//", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_regex_line_ranges_require_comma_before_suffix_like_stock_git() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "/one/+1", "a.txt"].as_slice(),
        ["blame", "-L", "/one/2", "a.txt"].as_slice(),
        ["blame", "-L", "/one//two/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_end_regex_line_ranges_reject_suffix_like_stock_git() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "1,/two/3", "a.txt"].as_slice(),
        ["blame", "-L", "1,/two/+1", "a.txt"].as_slice(),
        ["blame", "-L", "1,/two//four/", "a.txt"].as_slice(),
        ["blame", "-L", "/one/,/two/3", "a.txt"].as_slice(),
        ["blame", "-L", "/one/,/two/+1", "a.txt"].as_slice(),
        ["blame", "-L", "/one/,/two//four/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_basic_regex_literal_metacharacters_match_stock_git() {
    let git_repo = blame_basic_regex_literal_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    for args in [
        ["blame", "-L", "/(/", "a.txt"].as_slice(),
        ["blame", "-L", "/{/", "a.txt"].as_slice(),
        ["blame", "-L", "/a+/", "a.txt"].as_slice(),
        ["blame", "-L", "/a?/", "a.txt"].as_slice(),
        ["blame", "-L", "/a|/", "a.txt"].as_slice(),
        ["blame", "-L", "/a\\+/", "a.txt"].as_slice(),
        ["blame", "-L", "/a\\?/", "a.txt"].as_slice(),
        ["blame", "-L", "/z\\|a/", "a.txt"].as_slice(),
        ["blame", "-L", "/*/", "a.txt"].as_slice(),
        ["blame", "-L", "/x\\(y\\)/", "a.txt"].as_slice(),
        ["blame", "-L", "/x\\(y\\)*/", "a.txt"].as_slice(),
        ["blame", "-L", "/x\\{2\\}/", "a.txt"].as_slice(),
        ["blame", "-L", "/x\\{2,3\\}/", "a.txt"].as_slice(),
        ["blame", "-L", "/[a-b-]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[-a-b]/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_basic_regex_literal_metacharacters_no_match_like_stock_git() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "/(/", "a.txt"].as_slice(),
        ["blame", "-L", "/{/", "a.txt"].as_slice(),
        ["blame", "-L", "/z+/", "a.txt"].as_slice(),
        ["blame", "-L", "/z?/", "a.txt"].as_slice(),
        ["blame", "-L", "/z|/", "a.txt"].as_slice(),
        ["blame", "-L", "/*/", "a.txt"].as_slice(),
        ["blame", "-L", "/q\\(r\\)/", "a.txt"].as_slice(),
        ["blame", "-L", "/q\\{2\\}/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_basic_regex_unbalanced_grouping_matches_stock_git_failure() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "/\\(/", "a.txt"].as_slice(),
        ["blame", "-L", "/\\)/", "a.txt"].as_slice(),
        ["blame", "-L", "/x\\(y/", "a.txt"].as_slice(),
        ["blame", "-L", "/x\\)y/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_basic_regex_invalid_backreferences_match_stock_git_failure() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "/\\1/", "a.txt"].as_slice(),
        ["blame", "-L", "/x\\1/", "a.txt"].as_slice(),
        ["blame", "-L", "/\\(x\\)\\2/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_invalid_posix_character_classes_match_stock_git_failure() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "/[[:word:]]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[[:ascii:]]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[[:bogus:]]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[[:digit:][:bogus:]]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[[:bogus:][:digit:]]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[[:bogus:]/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_invalid_posix_collating_elements_match_stock_git_failure() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "/[[.bogus.]]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[[=bogus=]]/", "a.txt"].as_slice(),
        ["blame", "-L", "/[[.ch.]]/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_basic_regex_invalid_intervals_match_stock_git_failure() {
    let repo = blame_line_range_fixture_repo();

    for args in [
        ["blame", "-L", "/\\{/", "a.txt"].as_slice(),
        ["blame", "-L", "/a\\{x\\}/", "a.txt"].as_slice(),
        ["blame", "-L", "/a\\{2/", "a.txt"].as_slice(),
        ["blame", "-L", "/a\\{,2\\}/", "a.txt"].as_slice(),
        ["blame", "-L", "/a\\{3,2\\}/", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn blame_progress_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "a\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);

    assert_eq!(
        command_output(
            zmin_bin(),
            repo.path(),
            &["blame", "--progress", "a.txt"],
            "zmin"
        ),
        command_output("git", repo.path(), &["blame", "--progress", "a.txt"], "git")
    );
}

#[test]
fn blame_minimal_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "a\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);

    assert_eq!(
        command_output(
            zmin_bin(),
            repo.path(),
            &["blame", "--minimal", "a.txt"],
            "zmin"
        ),
        command_output("git", repo.path(), &["blame", "--minimal", "a.txt"], "git")
    );
}

#[test]
fn blame_color_lines_non_tty_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "a\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);

    assert_eq!(
        command_output(
            zmin_bin(),
            repo.path(),
            &["blame", "--color-lines", "a.txt"],
            "zmin"
        ),
        command_output(
            "git",
            repo.path(),
            &["blame", "--color-lines", "a.txt"],
            "git"
        )
    );
}

#[test]
fn blame_color_by_age_small_file_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "a\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);

    assert_eq!(
        command_output(
            zmin_bin(),
            repo.path(),
            &["blame", "--color-by-age", "a.txt"],
            "zmin"
        ),
        command_output(
            "git",
            repo.path(),
            &["blame", "--color-by-age", "a.txt"],
            "git"
        )
    );
}

#[test]
fn blame_score_debug_small_file_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "a\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);

    assert_eq!(
        command_output(
            zmin_bin(),
            repo.path(),
            &["blame", "--score-debug", "a.txt"],
            "zmin"
        ),
        command_output(
            "git",
            repo.path(),
            &["blame", "--score-debug", "a.txt"],
            "git"
        )
    );
}

#[test]
fn blame_line_regex_and_function_ranges_match_stock_git() {
    let git_repo = git_init();
    configure_identity(git_repo.path());
    write_file(
        git_repo.path(),
        "a.txt",
        "fn one() {\n    a\n}\n\nfn two() {\n    b\n}\n",
    );
    git(git_repo.path(), ["add", "-A"]);
    git_commit_with_author(
        git_repo.path(),
        "A",
        "a@example.test",
        "1700000000 +0000",
        "one",
    );
    write_file(
        git_repo.path(),
        "a.txt",
        "fn one() {\n    a\n}\n\nfn two() {\n    B\n}\n",
    );
    git(git_repo.path(), ["add", "-A"]);
    git_commit_with_author(
        git_repo.path(),
        "B",
        "b@example.test",
        "1700000100 +0000",
        "two",
    );
    let zmin_repo = clone_repo_fixture(git_repo.path());

    for args in [
        ["blame", "-L", "/fn two/,+2", "a.txt"].as_slice(),
        ["blame", "-L", "/^fn two/,6", "a.txt"].as_slice(),
        ["blame", "-L", ":two", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
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
    let cherry_pick = Command::new(stock_git_bin())
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    write_file(repo.path(), "dirty.txt", "dirty\n");
    assert_eq!(
        run_zmin(repo.path(), ["describe", "--dirty"]),
        git(repo.path(), ["describe", "--dirty"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["describe", "--dirty=.modified"]),
        git(repo.path(), ["describe", "--dirty=.modified"])
    );
}

#[test]
fn describe_always_matches_stock_git_without_names() {
    let repo = git_init();
    configure_identity(repo.path());
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "base"]);

    assert_eq!(
        run_zmin(repo.path(), ["describe", "--always"]),
        git(repo.path(), ["describe", "--always"])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["describe"]),
        git_status(repo.path(), ["describe"])
    );
}

#[test]
fn describe_additional_documented_options_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "base"]);
    let base = git(repo.path(), ["rev-parse", "HEAD"]);
    git(repo.path(), ["checkout", "-b", "side"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "side1"]);
    git_with_env(repo.path(), ["tag", "-a", "v2.0.0", "-m", "version2"]);
    git(repo.path(), ["checkout", "main"]);
    git(repo.path(), ["merge", "--no-ff", "-m", "merge", "side"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "after"]);

    for args in [
        ["describe", "--contains", &base].as_slice(),
        ["describe", "--first-parent", "HEAD~1"].as_slice(),
        ["describe", "--candidates=0", "HEAD"].as_slice(),
        ["describe", "--debug", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), repo.path(), args, "zmin"),
            command_any_output("git", repo.path(), args, "git"),
            "args: {args:?}"
        );
    }
}

#[test]
fn describe_broken_matches_stock_git_for_corrupt_index() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "base"]);
    git_with_env(repo.path(), ["tag", "-a", "v1.0.0", "-m", "version"]);
    let index_path = repo.path().join(".git/index");
    let backup = repo.path().join(".git/index.bak");
    fs::copy(&index_path, &backup).expect("backup index");
    fs::write(&index_path, b"broken").expect("corrupt index");

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["describe", "--broken"], "zmin"),
        command_any_output("git", repo.path(), &["describe", "--broken"], "git")
    );

    fs::rename(&backup, &index_path).expect("restore index");
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
        run_zmin(repo.path(), ["last-modified", "--recursive"]),
        format!("{latest}\ta.txt\n{initial}\tdir/b.txt")
    );
    assert_eq!(
        run_zmin(repo.path(), ["last-modified"]),
        format!("{latest}\ta.txt\n{initial}\tdir")
    );
    assert_eq!(
        run_zmin(repo.path(), ["last-modified", "-z", "--", "a.txt"]),
        format!("{latest}\ta.txt\0")
    );
}

#[test]
fn add_commit_rev_list_and_log_match_stock_git_state() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "hello\n");
    write_file(zmin_repo.path(), "a.txt", "hello\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);

    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );

    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "initial"]);
    assert_eq!(
        git(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );

    write_file(git_repo.path(), "a.txt", "changed\n");
    write_file(zmin_repo.path(), "a.txt", "changed\n");
    write_file(git_repo.path(), "b.txt", "new\n");
    write_file(zmin_repo.path(), "b.txt", "new\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "second"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "second"]);

    assert_eq!(
        git(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["rev-list", "--max-count", "2", "HEAD"]),
        git(git_repo.path(), ["rev-list", "--max-count", "2", "HEAD"])
    );

    let mut history_cases = vec![
        vec!["rev-list", "--max-count", "2", "HEAD"],
        vec!["rev-list", "--all"],
        vec!["rev-list", "HEAD~1..HEAD"],
        vec!["rev-list", "HEAD", "^HEAD~1"],
        vec!["rev-list", "HEAD", "--not", "HEAD~1"],
        vec!["rev-list", "--count", "HEAD"],
        vec!["rev-list", "--parents", "HEAD"],
        vec!["rev-list", "--parents", "--max-count", "1", "HEAD"],
        vec!["rev-list", "-1", "HEAD"],
        vec!["rev-list", "--objects", "HEAD"],
        vec!["rev-list", "--objects", "--no-object-names", "HEAD"],
        vec!["rev-list", "--objects", "--count", "HEAD"],
        vec!["rev-list", "--objects", "--all"],
        vec!["rev-list", "--objects", "--no-object-names", "--all"],
        vec!["rev-list", "--objects", "--reverse", "HEAD"],
        vec!["rev-list", "--reverse", "HEAD"],
        vec!["rev-list", "--reverse", "--max-count", "2", "HEAD"],
        vec!["rev-list", "--count", "--max-count", "1", "HEAD"],
        vec!["log", "--max-count", "2"],
        vec!["log", "-1", "--format=%H"],
        vec!["log", "-z", "-1", "--format=%H%x00%P%x00%D%x00%s"],
        vec!["log", "--reverse", "--format=%s"],
        vec!["log", "--stat", "--max-count", "1"],
        vec!["log", "--numstat", "--format=%H", "--max-count", "1"],
        vec!["log", "--shortstat", "--max-count", "1"],
        vec!["log", "--raw", "--format=%H", "--max-count", "1"],
        vec!["log", "--summary", "--format=%H", "--max-count", "1"],
        vec!["log", "--name-only", "--format=%H", "--max-count", "1"],
        vec!["log", "--name-status", "--format=%H", "--max-count", "1"],
        vec!["log", "--parents", "--oneline", "--max-count", "1"],
    ];
    history_cases.extend(whatchanged_cases());
    for args in history_cases {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args.as_slice()),
            git_args(git_repo.path(), args.as_slice()),
            "args: {args:?}"
        );
    }

    let git_blob = git(git_repo.path(), ["hash-object", "-w", "a.txt"]);
    let zmin_blob = git(zmin_repo.path(), ["hash-object", "-w", "a.txt"]);
    assert_eq!(zmin_blob, git_blob);
    git(git_repo.path(), ["tag", "blob-tag", &git_blob]);
    git(zmin_repo.path(), ["tag", "blob-tag", &zmin_blob]);

    for args in [
        ["log", "--all", "--format=%H"].as_slice(),
        ["rev-list", "--objects", "--all", "--max-count", "2"].as_slice(),
        ["log", "--format=%H", "--max-count", "1"].as_slice(),
        ["log", "--format=%h %s", "--max-count", "1"].as_slice(),
        ["log", "--pretty=format:%an <%ae>", "--max-count", "1"].as_slice(),
        ["log", "--pretty=oneline", "--max-count", "1"].as_slice(),
        ["rev-parse", "HEAD"].as_slice(),
        ["rev-parse", "--short=12", "HEAD"].as_slice(),
        ["rev-parse", "--short=100", "HEAD"].as_slice(),
        ["rev-parse", "--show-object-format"].as_slice(),
        ["show-ref", "--heads"].as_slice(),
        ["show-ref", "--head"].as_slice(),
        ["show-ref", "--hash=12"].as_slice(),
        ["log", "--oneline", "--max-count", "2", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }

    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
}

#[test]
fn log_decoration_order_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["branch", "feature"]);
    git(repo.path(), ["tag", "v1"]);
    git(
        repo.path(),
        ["remote", "add", "origin", "https://example.test/repo.git"],
    );
    git(
        repo.path(),
        ["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    git(
        repo.path(),
        [
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );

    for args in [
        ["log", "-1", "--format=%D"].as_slice(),
        [
            "log",
            "-z",
            "-1",
            "--format=%H%x00%h%x00%P%x00%D%x00%s%x00%an%x00%ae%x00%ad",
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
fn log_decorate_boolean_values_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["tag", "v1"]);

    for args in [
        ["log", "--decorate=yes", "--oneline", "-1"].as_slice(),
        ["log", "--decorate=on", "--oneline", "-1"].as_slice(),
        ["log", "--decorate=1", "--oneline", "-1"].as_slice(),
        ["log", "--decorate=off", "--oneline", "-1"].as_slice(),
        ["log", "--decorate=0", "--oneline", "-1"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn log_diff_merges_m_alias_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "a.txt", "base\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "base"]);
        git(repo, ["checkout", "-b", "side"]);
        write_file(repo, "a.txt", "side\n");
        git(repo, ["commit", "-am", "side"]);
        git(repo, ["checkout", "main"]);
        write_file(repo, "a.txt", "main\n");
        git(repo, ["commit", "-am", "main"]);
        git(repo, ["merge", "-s", "ours", "side", "-m", "merge"]);
    }

    for args in [
        [
            "log",
            "-1",
            "--format=%s",
            "--diff-merges=separate",
            "--stat",
        ]
        .as_slice(),
        ["log", "-1", "--format=%s", "--diff-merges=on", "--stat"].as_slice(),
        ["log", "-1", "--format=%s", "--diff-merges=m", "--stat"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn log_and_show_ide_formats_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["checkout", "-b", "feature"]);
    write_file(repo.path(), "a.txt", "two\n");
    write_file(repo.path(), "b.txt", "new\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "feature"]);
    git(repo.path(), ["checkout", "main"]);
    git(
        repo.path(),
        ["remote", "add", "origin", "https://example.test/repo.git"],
    );
    git(
        repo.path(),
        ["update-ref", "refs/remotes/origin/main", "HEAD"],
    );

    for args in [
        [
            "log",
            "--branches",
            "--remotes",
            "-z",
            "--max-count=5",
            "--format=%H%x00%h%x00%P%x00%D%x00%s%x00%an%x00%ae%x00%ct",
        ]
        .as_slice(),
        [
            "show",
            "--format=%H%x00%s",
            "--name-status",
            "-z",
            "--max-count=1",
            "HEAD",
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
fn log_default_short_hash_matches_stock_git_with_unrelated_object_prefix_collision() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    write_loose_blob(repo.path(), "zmin-abbrev-collision-12562\n");
    write_loose_blob(repo.path(), "zmin-abbrev-collision-14850\n");

    for args in [
        ["log", "-1", "--format=%h", "HEAD"].as_slice(),
        ["log", "-1", "--oneline", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn show_root_commit_patch_respects_log_showroot_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "one\n");
    write_file(zmin_repo.path(), "a.txt", "one\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "root"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "root"]);

    for args in [
        ["show", "HEAD"].as_slice(),
        ["show", "--root", "HEAD"].as_slice(),
        ["show", "--format=raw", "HEAD"].as_slice(),
        ["show", "--format=raw", "--root", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }

    git(git_repo.path(), ["config", "log.showroot", "false"]);
    run_zmin(zmin_repo.path(), ["config", "log.showroot", "false"]);
    for args in [
        ["show", "HEAD"].as_slice(),
        ["show", "--root", "HEAD"].as_slice(),
        ["show", "--format=raw", "HEAD"].as_slice(),
        ["show", "--format=raw", "--root", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args with log.showroot=false: {args:?}"
        );
    }
}

#[test]
fn show_empty_root_commit_does_not_print_empty_patch_separator() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    git_with_env(
        git_repo.path(),
        ["commit", "--allow-empty", "-m", "initial"],
    );
    git_with_env(
        zmin_repo.path(),
        ["commit", "--allow-empty", "-m", "initial"],
    );

    assert_eq!(
        run_zmin_args(zmin_repo.path(), ["show", "HEAD"].as_slice()),
        git_args(git_repo.path(), ["show", "HEAD"].as_slice())
    );
}

#[test]
fn rev_list_symmetric_difference_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    git(git_repo.path(), ["checkout", "-b", "main"]);
    git(zmin_repo.path(), ["checkout", "-b", "main"]);

    write_file(git_repo.path(), "base.txt", "base\n");
    write_file(zmin_repo.path(), "base.txt", "base\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "base"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "base"]);

    git(git_repo.path(), ["checkout", "-b", "left"]);
    git(zmin_repo.path(), ["checkout", "-b", "left"]);
    write_file(git_repo.path(), "left.txt", "left\n");
    write_file(zmin_repo.path(), "left.txt", "left\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "left"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "left"]);

    git(git_repo.path(), ["checkout", "main"]);
    git(zmin_repo.path(), ["checkout", "main"]);
    write_file(git_repo.path(), "right.txt", "right\n");
    write_file(zmin_repo.path(), "right.txt", "right\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "right"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "right"]);

    for args in [
        ["rev-list", "left...main"].as_slice(),
        ["rev-list", "--count", "left...main"].as_slice(),
        ["rev-list", "--reverse", "left...main"].as_slice(),
        ["rev-list", "--objects", "left...main"].as_slice(),
        ["rev-list", "--objects", "--no-object-names", "left...main"].as_slice(),
        ["rev-list", "--not", "left...main", "main"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn log_relative_since_matches_stock_git_for_recent_commits() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    write_file(git_repo.path(), "a.txt", "now\n");
    write_file(zmin_repo.path(), "a.txt", "now\n");
    git(git_repo.path(), ["add", "-A"]);
    git(zmin_repo.path(), ["add", "-A"]);
    git(git_repo.path(), ["commit", "-m", "recent"]);
    run_zmin(zmin_repo.path(), ["commit", "-m", "recent"]);

    for args in [
        ["log", "--since", "yesterday", "--format=%s"].as_slice(),
        ["log", "--since", "1.week.ago", "--format=%s"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn log_no_walk_author_date_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    write_file(git_repo.path(), "a.txt", "one\n");
    write_file(zmin_repo.path(), "a.txt", "one\n");
    git(git_repo.path(), ["add", "-A"]);
    git(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "one"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "one"]);
    write_file(git_repo.path(), "a.txt", "two\n");
    write_file(zmin_repo.path(), "a.txt", "two\n");
    git(git_repo.path(), ["add", "-A"]);
    git(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "two"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "two"]);

    assert_eq!(
        run_zmin_args(
            zmin_repo.path(),
            &["log", "--no-walk", "--format=%ad", "HEAD"]
        ),
        git_args(
            git_repo.path(),
            &["log", "--no-walk", "--format=%ad", "HEAD"]
        )
    );
}

#[test]
fn log_date_formats_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    write_file(git_repo.path(), "a.txt", "one\n");
    write_file(zmin_repo.path(), "a.txt", "one\n");
    git(git_repo.path(), ["add", "-A"]);
    git(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "one"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "one"]);

    for mode in [
        "default",
        "default-local",
        "local",
        "iso",
        "iso-local",
        "iso-strict",
        "iso-strict-local",
        "rfc",
        "rfc-local",
        "rfc2822",
        "rfc2822-local",
        "short",
        "short-local",
        "unix",
        "unix-local",
        "raw",
        "raw-local",
    ] {
        let date_arg = format!("--date={mode}");
        let args = ["log", "-1", date_arg.as_str(), "--format=%ad|%cd"];
        assert_eq!(
            run_zmin_args(zmin_repo.path(), &args),
            git_args(git_repo.path(), &args),
            "date mode: {mode}"
        );
    }

    let date_env = [("GIT_TEST_DATE_NOW", "1780000000")];
    for mode in ["relative", "relative-local", "human", "human-local"] {
        let date_arg = format!("--date={mode}");
        let args = ["log", "-1", date_arg.as_str(), "--format=%ad|%cd"];
        assert_eq!(
            command_output_with_env(zmin_bin(), zmin_repo.path(), &args, &date_env, "zmin").1,
            command_output_with_env("git", git_repo.path(), &args, &date_env, "git").1,
            "date mode: {mode}"
        );
    }

    for mode in [
        "format:%Y-%m-%d %H:%M:%S %z",
        "format-local:%Y-%m-%d %H:%M:%S %z",
    ] {
        let date_arg = format!("--date={mode}");
        let args = ["log", "-1", date_arg.as_str(), "--format=%ad|%cd"];
        assert_eq!(
            run_zmin_args(zmin_repo.path(), &args),
            git_args(git_repo.path(), &args),
            "date mode: {mode}"
        );
    }

    let separate_date_args = ["log", "-1", "--date", "iso", "--format=%ad|%cd"];
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &separate_date_args),
        git_args(git_repo.path(), &separate_date_args),
        "date mode separate value"
    );
}

#[test]
fn log_invalid_date_format_matches_stock_git_failure() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);

    let args = ["log", "-1", "--date=bad", "--format=%ad"];
    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );
}

#[test]
fn log_missing_date_value_matches_stock_git_failure() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);

    let args = ["log", "-1", "--date", "--format=%ad"];
    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );
}

#[test]
fn rev_list_accepts_dashdash_separator_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    write_file(git_repo.path(), "a.txt", "one\n");
    write_file(zmin_repo.path(), "a.txt", "one\n");
    git(git_repo.path(), ["add", "-A"]);
    git(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "one"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "one"]);

    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["rev-list", "--objects", "HEAD", "--"]),
        git_args(git_repo.path(), &["rev-list", "--objects", "HEAD", "--"])
    );
}
