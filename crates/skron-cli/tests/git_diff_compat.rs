mod common;

use std::process::Command;
use std::{fs, path::Path};

use tempfile::TempDir;

use common::{
    command_any_output_with_stdin, configure_identity, git, git_args, git_init, git_status,
    git_with_env, run_skron, run_skron_args, run_skron_status, skron_bin, write_file,
};

fn command_output(command: &str, cwd: &Path, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
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

#[test]
fn diff_reverse_matches_stock_git_for_porcelain_and_plumbing() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"old\n").expect("write a");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    fs::write(repo.path().join("a.txt"), b"new\n").expect("modify a");

    for args in [
        ["diff", "-R", "--raw", "--no-abbrev"].as_slice(),
        ["diff", "-R", "-p"].as_slice(),
        ["diff-files", "-R", "--raw", "--no-abbrev"].as_slice(),
        ["diff-files", "-R", "-p"].as_slice(),
        ["diff-index", "-R", "--raw", "--no-abbrev", "HEAD"].as_slice(),
        ["diff-index", "-R", "-p", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    git(repo.path(), ["add", "-A"]);
    for args in [
        ["diff", "--cached", "-R", "--raw", "--no-abbrev"].as_slice(),
        ["diff", "--cached", "-R", "-p"].as_slice(),
        [
            "diff-index",
            "--cached",
            "-R",
            "--raw",
            "--no-abbrev",
            "HEAD",
        ]
        .as_slice(),
        ["diff-index", "--cached", "-R", "-p", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    git_with_env(repo.path(), ["commit", "-m", "second"]);
    for args in [
        ["diff", "-R", "--raw", "HEAD~1", "HEAD"].as_slice(),
        ["diff", "-R", "-p", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-R", "--raw", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-R", "-p", "HEAD~1", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_pickaxe_matches_stock_git_for_porcelain_and_plumbing() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"needle\nbase\n").expect("write a");
    fs::write(repo.path().join("b.txt"), b"plain\n").expect("write b");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    fs::write(repo.path().join("a.txt"), b"needle\nneedle\nbase\n").expect("modify a");
    fs::write(repo.path().join("b.txt"), b"plain\nchanged\n").expect("modify b");

    for args in [
        ["diff", "-Sneedle", "--name-only"].as_slice(),
        ["diff", "-Gregexless", "--name-only"].as_slice(),
        ["diff", "-Gchanged", "--name-only"].as_slice(),
        ["diff", "--pickaxe-all", "-Sneedle", "--name-only"].as_slice(),
        ["diff", "--pickaxe-regex", "-Sneed.e", "--name-only"].as_slice(),
        ["diff-files", "-Sneedle", "--name-only"].as_slice(),
        ["diff-index", "-Sneedle", "--name-only", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    for args in [
        ["diff", "-Sneedle", "--name-only", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-Sneedle", "--name-only", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-Gchanged", "--name-only", "HEAD~1", "HEAD"].as_slice(),
        [
            "diff-tree",
            "--pickaxe-all",
            "-Sneedle",
            "--name-only",
            "HEAD~1",
            "HEAD",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_order_file_matches_stock_git_for_porcelain_and_plumbing() {
    let repo = git_init();
    configure_identity(repo.path());
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(repo.path().join(name), b"old\n").expect("write file");
    }
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(repo.path().join(name), b"new\n").expect("modify file");
    }
    fs::write(repo.path().join("order.txt"), b"c.txt\na.txt\n").expect("write order");

    for args in [
        ["diff", "-Oorder.txt", "--name-only"].as_slice(),
        ["diff-files", "-Oorder.txt", "--name-only"].as_slice(),
        ["diff-index", "-Oorder.txt", "--name-only", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    for args in [
        ["diff", "-Oorder.txt", "--name-only", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-Oorder.txt", "--name-only", "HEAD~1", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_skip_and_rotate_match_stock_git_for_porcelain_and_plumbing() {
    let repo = git_init();
    configure_identity(repo.path());
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(repo.path().join(name), b"old\n").expect("write file");
    }
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(repo.path().join(name), b"new\n").expect("modify file");
    }

    for args in [
        ["diff", "--skip-to=b.txt", "--name-only"].as_slice(),
        ["diff", "--rotate-to=b.txt", "--name-only"].as_slice(),
        ["diff-files", "--skip-to=b.txt", "--name-only"].as_slice(),
        ["diff-index", "--rotate-to=b.txt", "--name-only", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    for args in [
        ["diff", "--skip-to=b.txt", "--name-only", "HEAD~1", "HEAD"].as_slice(),
        [
            "diff-tree",
            "--rotate-to=b.txt",
            "--name-only",
            "HEAD~1",
            "HEAD",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_pairs_matches_stock_git_for_raw_diff_input() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"one\n").expect("write a");
    fs::write(repo.path().join("c.txt"), b"gone\n").expect("write c");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    fs::write(repo.path().join("a.txt"), b"two\n").expect("modify a");
    fs::write(repo.path().join("b.txt"), b"new\n").expect("write b");
    fs::remove_file(repo.path().join("c.txt")).expect("remove c");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);

    let old = git(repo.path(), ["rev-parse", "HEAD~1"]);
    let new = git(repo.path(), ["rev-parse", "HEAD"]);
    let raw = Command::new("git")
        .args(["diff-tree", "-z", "-r", "--raw", &old, &new])
        .current_dir(repo.path())
        .output()
        .expect("git diff-tree raw");
    assert!(raw.status.success(), "git diff-tree failed");
    let raw = String::from_utf8(raw.stdout).expect("raw diff utf8");

    for args in [
        ["diff-pairs", "-z"].as_slice(),
        ["diff-pairs", "-z", "--raw"].as_slice(),
        ["diff-pairs", "-z", "--name-only"].as_slice(),
        ["diff-pairs", "-z", "--name-status"].as_slice(),
        ["diff-pairs", "-z", "--numstat"].as_slice(),
        ["diff-pairs", "-z", "-p"].as_slice(),
        ["diff-pairs", "-z", "--stat"].as_slice(),
        ["diff-pairs", "-z", "--shortstat"].as_slice(),
        ["diff-pairs", "-z", "--summary"].as_slice(),
    ] {
        assert_eq!(
            command_any_output_with_stdin(skron_bin(), repo.path(), args, &raw, "skron"),
            command_any_output_with_stdin("git", repo.path(), args, &raw, "git"),
            "args: {args:?}"
        );
    }

    let raw = Command::new("git")
        .args(["diff-tree", "-r", "--raw", &old, &new])
        .current_dir(repo.path())
        .output()
        .expect("git diff-tree raw");
    assert!(raw.status.success(), "git diff-tree failed");
    let raw = String::from_utf8(raw.stdout).expect("raw diff utf8");

    for args in [
        ["diff-pairs"].as_slice(),
        ["diff-pairs", "--raw"].as_slice(),
        ["diff-pairs", "--name-only"].as_slice(),
        ["diff-pairs", "--name-status"].as_slice(),
        ["diff-pairs", "--numstat"].as_slice(),
        ["diff-pairs", "-p"].as_slice(),
        ["diff-pairs", "--stat"].as_slice(),
        ["diff-pairs", "--shortstat"].as_slice(),
        ["diff-pairs", "--summary"].as_slice(),
    ] {
        let output = command_any_output_with_stdin(skron_bin(), repo.path(), args, &raw, "skron");
        assert_eq!(output.0, 0, "args: {args:?}");
        assert!(
            output.2.is_empty(),
            "stderr should be empty for args {args:?}: {}",
            output.2
        );
        assert!(
            !output.1.contains("working without -z is not supported"),
            "non-z diff-pairs should not fall back to usage error for args {args:?}"
        );
    }
}

#[test]
fn diff_name_status_matches_stock_git_for_cached_and_worktree() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"old\n").expect("write a");
    fs::write(repo.path().join("b.txt"), b"remove\n").expect("write b");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    fs::write(repo.path().join("a.txt"), b"staged\n").expect("stage modify a");
    fs::write(repo.path().join("c.txt"), b"added\n").expect("stage add c");
    fs::remove_file(repo.path().join("b.txt")).expect("remove b");
    git(repo.path(), ["add", "-A"]);

    for args in [
        ["diff", "--cached", "--name-status"].as_slice(),
        ["diff", "--cached", "--name-only"].as_slice(),
        ["diff", "--cached", "--stat"].as_slice(),
        ["diff", "--cached", "--numstat"].as_slice(),
        ["diff", "--cached", "--shortstat"].as_slice(),
        ["diff", "--cached", "--raw"].as_slice(),
        ["diff", "--cached", "--summary"].as_slice(),
        ["diff", "--cached"].as_slice(),
        ["diff", "--cached", "--name-status", "a.txt"].as_slice(),
        ["diff", "--cached", "--raw", "a.txt"].as_slice(),
        ["diff", "--cached", "--summary", "a.txt"].as_slice(),
        ["diff", "--cached", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    assert_eq!(
        run_skron_status(repo.path(), ["diff", "--cached", "--quiet"]),
        git_status(repo.path(), ["diff", "--cached", "--quiet"])
    );
    assert_eq!(
        run_skron_status(
            repo.path(),
            ["diff", "--cached", "--name-only", "--exit-code"]
        ),
        git_status(
            repo.path(),
            ["diff", "--cached", "--name-only", "--exit-code"]
        )
    );

    fs::write(repo.path().join("a.txt"), b"unstaged\n").expect("unstaged modify a");
    fs::remove_file(repo.path().join("c.txt")).expect("unstaged remove c");
    fs::write(repo.path().join("untracked.txt"), b"ignored by diff\n").expect("write untracked");

    for args in [
        ["diff", "--name-status"].as_slice(),
        ["diff", "--name-only"].as_slice(),
        ["diff", "--stat"].as_slice(),
        ["diff", "--numstat"].as_slice(),
        ["diff", "--shortstat"].as_slice(),
        ["diff", "--raw"].as_slice(),
        ["diff", "--summary"].as_slice(),
        ["diff"].as_slice(),
        ["diff", "--name-status", "a.txt"].as_slice(),
        ["diff", "--raw", "a.txt"].as_slice(),
        ["diff", "--summary", "a.txt"].as_slice(),
        ["diff", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    assert_eq!(
        run_skron_status(repo.path(), ["diff", "--quiet"]),
        git_status(repo.path(), ["diff", "--quiet"])
    );
    assert_eq!(
        run_skron_status(repo.path(), ["diff", "--name-only", "--exit-code"]),
        git_status(repo.path(), ["diff", "--name-only", "--exit-code"])
    );

    git(repo.path(), ["restore", "--staged", "."]);
    git(repo.path(), ["restore", "."]);
    fs::remove_file(repo.path().join("untracked.txt")).expect("remove untracked");
    for args in [
        ["diff", "--stat"].as_slice(),
        ["diff", "--shortstat"].as_slice(),
        ["diff", "--cached", "--stat"].as_slice(),
        ["diff", "--cached", "--shortstat"].as_slice(),
        ["diff", "--raw"].as_slice(),
        ["diff", "--cached", "--raw"].as_slice(),
        ["diff", "--summary"].as_slice(),
        ["diff", "--cached", "--summary"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    assert_eq!(
        run_skron_status(repo.path(), ["diff", "--quiet"]),
        git_status(repo.path(), ["diff", "--quiet"])
    );
    assert_eq!(
        run_skron_status(repo.path(), ["diff", "--cached", "--quiet"]),
        git_status(repo.path(), ["diff", "--cached", "--quiet"])
    );
}

#[test]
fn diff_filter_matches_stock_git_for_cached_formats() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("mod.txt"), b"old\n").expect("write mod");
    fs::write(repo.path().join("del.txt"), b"gone\n").expect("write del");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    fs::write(repo.path().join("mod.txt"), b"new\n").expect("modify");
    fs::write(repo.path().join("add.txt"), b"add\n").expect("add");
    fs::remove_file(repo.path().join("del.txt")).expect("delete");
    git(repo.path(), ["add", "-A"]);

    for args in [
        ["diff", "--cached", "--name-status", "--diff-filter=A"].as_slice(),
        ["diff", "--cached", "--name-status", "--diff-filter=D"].as_slice(),
        ["diff", "--cached", "--name-status", "--diff-filter=M"].as_slice(),
        ["diff", "--cached", "--name-status", "--diff-filter=a"].as_slice(),
        ["diff", "--cached", "--stat", "--diff-filter=AD"].as_slice(),
        ["diff", "--cached", "--patch", "--diff-filter=A"].as_slice(),
        ["diff", "--cached", "--numstat", "--diff-filter=DM"].as_slice(),
        ["diff", "--cached", "--shortstat", "--diff-filter=m"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_no_patch_matches_stock_git_exit_behavior() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"old\n").expect("write");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    fs::write(repo.path().join("a.txt"), b"new\n").expect("rewrite");
    git(repo.path(), ["add", "-A"]);

    for args in [
        ["diff", "--cached", "--no-patch"].as_slice(),
        ["diff", "--cached", "--no-patch", "--exit-code"].as_slice(),
        ["diff", "--cached", "--no-patch", "--quiet"].as_slice(),
    ] {
        assert_eq!(
            command_output(skron_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_relative_matches_stock_git_for_cached_formats() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("sub")).expect("mkdir");
    fs::write(repo.path().join("root.txt"), b"root\n").expect("write root");
    fs::write(repo.path().join("sub/file.txt"), b"old\n").expect("write sub");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    fs::write(repo.path().join("root.txt"), b"root changed\n").expect("rewrite root");
    fs::write(repo.path().join("sub/file.txt"), b"new\n").expect("rewrite sub");
    git(repo.path(), ["add", "-A"]);

    for args in [
        ["diff", "--cached", "--relative=sub", "--name-only"].as_slice(),
        ["diff", "--cached", "--relative=sub", "--name-status"].as_slice(),
        ["diff", "--cached", "--relative=sub", "--stat"].as_slice(),
        ["diff", "--cached", "--relative=sub", "--patch"].as_slice(),
        [
            "diff",
            "--cached",
            "--relative=sub",
            "--no-relative",
            "--name-only",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    let sub = repo.path().join("sub");
    let args = ["diff", "--cached", "--relative", "--name-only"];
    assert_eq!(
        command_output(skron_bin(), &sub, &args),
        command_output("git", &sub, &args)
    );
}

#[test]
fn diff_text_treats_binary_as_text_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("bin.dat"), b"one\0old\nsame\n").expect("write binary");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    fs::write(repo.path().join("bin.dat"), b"one\0new\nsame\n").expect("rewrite binary");
    git(repo.path(), ["add", "-A"]);

    for args in [
        ["diff", "--cached", "--text"].as_slice(),
        ["diff", "--cached", "-a"].as_slice(),
    ] {
        assert_eq!(
            command_output(skron_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "args: {args:?}"
        );
    }

    let subdir = repo.path().join("sub");
    fs::create_dir_all(&subdir).expect("create subdir");
    write_file(repo.path(), "sub/relative.txt", "old\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "relative-base"]);
    write_file(repo.path(), "sub/relative.txt", "new\n");
    for args in [
        ["diff-files", "--relative", "--name-only"].as_slice(),
        ["diff-index", "--relative", "--name-only", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_output(skron_bin(), &subdir, args),
            command_output("git", &subdir, args),
            "subdir args: {args:?}"
        );
    }
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "relative-change"]);
    assert_eq!(
        command_output(
            skron_bin(),
            &subdir,
            &["diff-tree", "--relative", "--name-only", "HEAD~1", "HEAD"],
        ),
        command_output(
            "git",
            &subdir,
            &["diff-tree", "--relative", "--name-only", "HEAD~1", "HEAD"],
        )
    );
}

#[test]
fn diff_patch_format_options_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(
        repo.path().join("fmt.txt"),
        "one\n two\nthree\nfour\nfive\nsix\nseven\n",
    )
    .expect("write fmt");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    fs::write(
        repo.path().join("fmt.txt"),
        "one\n  two\nthree changed\nfour\nfive\nsix changed\nseven\n",
    )
    .expect("rewrite fmt");
    git(repo.path(), ["add", "-A"]);

    for args in [
        ["diff", "--cached", "--patch", "--no-prefix"].as_slice(),
        [
            "diff",
            "--cached",
            "--patch",
            "--src-prefix=old/",
            "--dst-prefix=new/",
        ]
        .as_slice(),
        ["diff", "--cached", "--patch", "--unified=1"].as_slice(),
        ["diff", "--cached", "--patch", "-U1"].as_slice(),
        [
            "diff",
            "--cached",
            "--patch",
            "--output-indicator-new=>",
            "--output-indicator-old=<",
            "--output-indicator-context==",
        ]
        .as_slice(),
        ["diff", "--cached", "--patch", "--output-indicator-new="].as_slice(),
        [
            "diff",
            "--cached",
            "--patch",
            "--unified=1",
            "--inter-hunk-context=20",
        ]
        .as_slice(),
        ["diff", "--cached", "--patch", "--full-index"].as_slice(),
        ["diff", "--cached", "-u"].as_slice(),
        ["diff", "--cached", "--patch-with-stat"].as_slice(),
        ["diff", "--cached", "--patch-with-raw"].as_slice(),
        ["diff", "--cached", "-s"].as_slice(),
        ["diff", "--cached", "--raw", "--abbrev=4"].as_slice(),
        ["diff", "--cached", "--raw", "-z"].as_slice(),
        ["diff", "--cached", "--name-only", "-z"].as_slice(),
        ["diff", "--cached", "--name-status", "-z"].as_slice(),
        ["diff", "--cached", "--numstat", "-z"].as_slice(),
        ["diff", "--cached", "--raw", "--no-abbrev"].as_slice(),
        ["diff", "--cached", "--no-ext-diff", "--no-textconv"].as_slice(),
        ["diff", "--cached", "--no-color", "--no-color-moved"].as_slice(),
        ["diff", "--cached", "--color=never"].as_slice(),
        ["diff", "--cached", "--color"].as_slice(),
        ["diff", "--cached", "--color=always"].as_slice(),
        ["diff", "--cached", "--color=auto"].as_slice(),
        ["diff", "--cached", "--color=always", "--no-color"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_whitespace_options_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("space.txt"), "one \n two\ncrlf\r\n").expect("write");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    fs::write(repo.path().join("space.txt"), "one\n  two\ncrlf\n").expect("rewrite");
    git(repo.path(), ["add", "-A"]);

    for args in [
        ["diff", "--cached", "--patch", "--ignore-space-at-eol"].as_slice(),
        ["diff", "--cached", "--patch", "--ignore-cr-at-eol"].as_slice(),
        ["diff", "--cached", "--patch", "--ignore-space-change"].as_slice(),
        ["diff", "--cached", "--patch", "-b"].as_slice(),
        ["diff", "--cached", "--patch", "--ignore-all-space"].as_slice(),
        ["diff", "--cached", "--patch", "-w"].as_slice(),
        ["diff", "--cached", "--stat", "-w"].as_slice(),
        ["diff", "--cached", "--stat", "--ignore-cr-at-eol"].as_slice(),
        ["diff", "--cached", "--numstat", "-w"].as_slice(),
        ["diff", "--cached", "--shortstat", "-w"].as_slice(),
        ["diff", "--cached", "--ignore-blank-lines"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_ignore_matching_lines_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(
        repo.path().join("noise.txt"),
        "keep\nDEBUG old\nTRACE old\nkeep2\n",
    )
    .expect("write");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    fs::write(
        repo.path().join("noise.txt"),
        "keep\nDEBUG new\nTRACE new\nkeep2\n",
    )
    .expect("rewrite");
    git(repo.path(), ["add", "-A"]);

    for args in [
        ["diff", "--cached", "-IDEBUG|TRACE"].as_slice(),
        ["diff", "--cached", "-I", "DEBUG|TRACE"].as_slice(),
        ["diff", "--cached", "--ignore-matching-lines=DEBUG|TRACE"].as_slice(),
        ["diff", "--cached", "--ignore-matching-lines", "DEBUG|TRACE"].as_slice(),
        ["diff", "--cached", "--stat", "-IDEBUG|TRACE"].as_slice(),
        ["diff", "--cached", "--numstat", "-IDEBUG|TRACE"].as_slice(),
        ["diff", "--cached", "--shortstat", "-IDEBUG|TRACE"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn plumbing_diff_whitespace_and_text_options_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "space.txt", "one \nDEBUG old\nkeep\n");
    fs::write(repo.path().join("bin.dat"), b"one\0old\nsame\n").expect("write binary");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    write_file(repo.path(), "space.txt", "one\nDEBUG new\nkeep\n");
    fs::write(repo.path().join("bin.dat"), b"one\0new\nsame\n").expect("rewrite binary");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "change"]);

    write_file(repo.path(), "space.txt", "one\nDEBUG newer\nkeep\n");
    fs::write(repo.path().join("bin.dat"), b"one\0newer\nsame\n").expect("rewrite binary again");

    for args in [
        ["diff-files", "-p", "--ignore-space-at-eol"].as_slice(),
        ["diff-files", "-p", "-IDEBUG"].as_slice(),
        ["diff-files", "--stat", "-w"].as_slice(),
        ["diff-files", "-p", "--text"].as_slice(),
        ["diff-files", "-p", "--no-color", "--no-ext-diff"].as_slice(),
        ["diff-files", "-p", "--color=never", "--no-textconv"].as_slice(),
        ["diff-files", "-p", "--color=always"].as_slice(),
        ["diff-files", "--name-only", "--diff-filter=M"].as_slice(),
        ["diff-files", "--name-only", "--diff-filter=A"].as_slice(),
        ["diff-index", "-p", "--ignore-space-change", "HEAD"].as_slice(),
        ["diff-index", "--numstat", "-IDEBUG", "HEAD"].as_slice(),
        ["diff-index", "-p", "-a", "HEAD"].as_slice(),
        ["diff-index", "-p", "--no-color-moved", "HEAD"].as_slice(),
        ["diff-index", "--name-status", "--diff-filter=M", "HEAD"].as_slice(),
        ["diff-index", "--name-status", "--diff-filter=A", "HEAD"].as_slice(),
        [
            "diff-tree",
            "-p",
            "--ignore-matching-lines=DEBUG",
            "HEAD~1",
            "HEAD",
        ]
        .as_slice(),
        ["diff-tree", "--stat", "-w", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-p", "--text", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-p", "--color=never", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-p", "--color=always", "HEAD~1", "HEAD"].as_slice(),
        [
            "diff-tree",
            "--name-only",
            "--diff-filter=M",
            "HEAD~1",
            "HEAD",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_find_renames_exact_matches_stock_git_for_cached_formats() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("old.txt"), b"same\n").expect("write old");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["mv", "old.txt", "new.txt"]);

    for args in [
        ["diff", "--cached", "-M", "--name-status"].as_slice(),
        ["diff", "--cached", "-M", "--name-only"].as_slice(),
        ["diff", "--cached", "-M", "--summary"].as_slice(),
        ["diff", "--cached", "-M", "--raw"].as_slice(),
        ["diff", "--cached", "-M", "--raw", "-z"].as_slice(),
        ["diff", "--cached", "-M", "--stat"].as_slice(),
        ["diff", "--cached", "-M", "--numstat"].as_slice(),
        ["diff", "--cached", "-M", "--numstat", "-z"].as_slice(),
        ["diff", "--cached", "-M", "--name-status", "-z"].as_slice(),
        ["diff", "--cached", "-M"].as_slice(),
        ["diff", "--cached", "--find-renames=100%", "--name-status"].as_slice(),
        ["diff-index", "--cached", "-M", "--name-status", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    fs::write(
        repo.path().join("similar.txt"),
        b"same\nsame\nchanged\nsame\n",
    )
    .expect("write similar");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    git(repo.path(), ["mv", "similar.txt", "similar-new.txt"]);
    fs::write(
        repo.path().join("similar-new.txt"),
        b"same\nsame\nCHANGED\nsame\n",
    )
    .expect("rewrite similar");
    git(repo.path(), ["add", "-A"]);

    for args in [
        ["diff", "--cached", "--find-renames=50%", "--name-status"].as_slice(),
        ["diff", "--cached", "-M5", "--name-status"].as_slice(),
        ["diff", "--cached", "-M90%", "--name-status"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_find_copies_exact_matches_stock_git_for_cached_formats() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("old.txt"), b"same\n").expect("write old");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    fs::copy(repo.path().join("old.txt"), repo.path().join("copy.txt")).expect("copy file");
    git(repo.path(), ["add", "copy.txt"]);

    for args in [
        [
            "diff",
            "--cached",
            "-C",
            "--find-copies-harder",
            "--name-status",
        ]
        .as_slice(),
        [
            "diff",
            "--cached",
            "-C",
            "--find-copies-harder",
            "--name-only",
        ]
        .as_slice(),
        [
            "diff",
            "--cached",
            "-C",
            "--find-copies-harder",
            "--summary",
        ]
        .as_slice(),
        ["diff", "--cached", "-C", "--find-copies-harder", "--raw"].as_slice(),
        ["diff", "--cached", "-C", "--find-copies-harder", "--stat"].as_slice(),
        [
            "diff",
            "--cached",
            "-C",
            "--find-copies-harder",
            "--numstat",
        ]
        .as_slice(),
        ["diff", "--cached", "-C", "--find-copies-harder"].as_slice(),
        [
            "diff-index",
            "--cached",
            "-C",
            "--find-copies-harder",
            "--name-status",
            "HEAD",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    fs::write(
        repo.path().join("copy-similar.txt"),
        b"same\nsame\nCHANGED\nsame\n",
    )
    .expect("write similar copy");
    git(repo.path(), ["add", "copy-similar.txt"]);
    for args in [
        [
            "diff",
            "--cached",
            "--find-copies=50%",
            "--find-copies-harder",
            "--name-status",
        ]
        .as_slice(),
        [
            "diff",
            "--cached",
            "-C90%",
            "--find-copies-harder",
            "--name-status",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_word_diff_plain_matches_stock_git_for_text_changes() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(
        repo.path().join("a.txt"),
        b"ctx1\nhello brave world\nsecond line\nctx2\n",
    )
    .expect("write old");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    fs::write(
        repo.path().join("a.txt"),
        b"ctx1\nhello bright world\nsecond line plus\nctx2\n",
    )
    .expect("write new");

    for args in [
        ["diff", "--word-diff=plain"].as_slice(),
        ["diff", "--word-diff=porcelain"].as_slice(),
        ["diff", "--word-diff=color"].as_slice(),
        ["diff", "--word-diff"].as_slice(),
        ["diff-files", "--word-diff=plain", "-p"].as_slice(),
        ["diff-files", "--word-diff=porcelain", "-p"].as_slice(),
        ["diff-files", "--word-diff=color", "-p"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_algorithm_flags_match_stock_git_for_supported_modes() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "alpha\ncommon\nbeta\ncommon\ngamma\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    write_file(
        repo.path(),
        "a.txt",
        "alpha\ncommon\nbeta changed\ncommon\ngamma\n",
    );
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "change"]);
    write_file(
        repo.path(),
        "a.txt",
        "alpha\ncommon\nbeta changed again\ncommon\ngamma\n",
    );

    for args in [
        ["diff", "--minimal"].as_slice(),
        ["diff", "--patience"].as_slice(),
        ["diff", "--histogram"].as_slice(),
        ["diff", "--diff-algorithm=myers"].as_slice(),
        ["diff", "--diff-algorithm=minimal"].as_slice(),
        ["diff", "--anchored=common"].as_slice(),
        ["diff-files", "-p", "--patience"].as_slice(),
        ["diff-index", "-p", "--diff-algorithm=histogram", "HEAD"].as_slice(),
        ["diff-tree", "-p", "--anchored=common", "HEAD~1", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    for args in [
        ["diff", "--diff-algorithm=invalid"].as_slice(),
        ["diff", "--word-diff=bad"].as_slice(),
        ["diff", "--submodule=bad"].as_slice(),
        ["diff", "--ignore-submodules=bad"].as_slice(),
    ] {
        assert_eq!(
            command_output(skron_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_break_rewrites_matches_stock_git_for_complete_rewrites() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(
        repo.path().join("f.txt"),
        (1..=100)
            .map(|value| format!("{value}\n"))
            .collect::<String>(),
    )
    .expect("write base");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    fs::write(
        repo.path().join("f.txt"),
        (201..=300)
            .map(|value| format!("{value}\n"))
            .collect::<String>(),
    )
    .expect("rewrite");

    for args in [
        ["diff", "-B"].as_slice(),
        ["diff", "--break-rewrites"].as_slice(),
        ["diff-files", "-p", "-B"].as_slice(),
        ["diff-index", "-p", "HEAD", "-B"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_irreversible_delete_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("deleted.txt"), b"old\nline\n").expect("write deleted");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    fs::remove_file(repo.path().join("deleted.txt")).expect("remove deleted");

    for args in [
        ["diff", "-D"].as_slice(),
        ["diff", "--irreversible-delete"].as_slice(),
        ["diff", "--stat", "-D"].as_slice(),
        ["diff-files", "-p", "-D"].as_slice(),
        ["diff-index", "-p", "HEAD", "-D"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_submodule_gitlink_patch_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let submodule = dir.path().join("submodule");
    fs::create_dir(&submodule).expect("create submodule");
    git(&submodule, ["init"]);
    configure_identity(&submodule);
    write_file(&submodule, "sub.txt", "one\n");
    git(&submodule, ["add", "-A"]);
    git_with_env(&submodule, ["commit", "-m", "one"]);
    let first = git(&submodule, ["rev-parse", "HEAD"]);
    write_file(&submodule, "sub.txt", "two\n");
    git(&submodule, ["add", "-A"]);
    git_with_env(&submodule, ["commit", "-m", "two"]);
    let second = git(&submodule, ["rev-parse", "HEAD"]);
    git(&submodule, ["checkout", &first]);

    let super_repo = dir.path().join("super");
    fs::create_dir(&super_repo).expect("create super");
    git(&super_repo, ["init"]);
    configure_identity(&super_repo);
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            submodule.to_str().expect("submodule path"),
            "deps/sub",
        ])
        .current_dir(&super_repo)
        .output()
        .expect("git submodule add");
    assert!(
        output.status.success(),
        "git submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_with_env(&super_repo, ["commit", "-m", "submodule"]);
    git(&submodule, ["checkout", &second]);
    git(&super_repo.join("deps/sub"), ["checkout", &second]);

    for args in [
        vec!["diff"],
        vec!["diff", "--submodule=short"],
        vec!["diff", "--submodule=log"],
        vec!["diff", "--submodule=diff"],
        vec!["diff", "--name-status", "--ignore-submodules"],
        vec!["diff", "--name-status", "--ignore-submodules=all"],
        vec!["diff", "--name-status", "--ignore-submodules=dirty"],
        vec!["diff", "--name-status", "--ignore-submodules=none"],
        vec!["diff-files", "-p", "--submodule=short"],
        vec!["diff-files", "-p", "--submodule=log"],
        vec!["diff-files", "-p", "--submodule=diff"],
        vec!["diff-files", "--name-status", "--ignore-submodules"],
        vec!["diff-files", "--name-status", "--ignore-submodules=dirty"],
        vec!["diff-index", "-p", "--submodule=short", "HEAD"],
        vec!["diff-index", "-p", "--submodule=log", "HEAD"],
        vec!["diff-index", "-p", "--submodule=diff", "HEAD"],
        vec!["diff-index", "--name-status", "--ignore-submodules", "HEAD"],
        vec![
            "diff-index",
            "--name-status",
            "--ignore-submodules=dirty",
            "HEAD",
        ],
    ] {
        assert_eq!(
            run_skron_args(&super_repo, &args),
            git_args(&super_repo, &args),
            "args: {args:?}"
        );
    }
    git(&super_repo, ["add", "deps/sub"]);
    git_with_env(&super_repo, ["commit", "-m", "update submodule"]);
    let updated = git(&super_repo, ["rev-parse", "HEAD"]);
    let previous = git(&super_repo, ["rev-parse", "HEAD^"]);
    for args in [
        vec!["diff-tree", "-p", "--submodule=short", &previous, &updated],
        vec!["diff-tree", "-p", "--submodule=log", &previous, &updated],
        vec!["diff-tree", "-p", "--submodule=diff", &previous, &updated],
        vec![
            "diff-tree",
            "-r",
            "--name-status",
            "--ignore-submodules",
            &previous,
            &updated,
        ],
        vec![
            "diff-tree",
            "-r",
            "--name-status",
            "--ignore-submodules=none",
            &previous,
            &updated,
        ],
    ] {
        assert_eq!(
            run_skron_args(&super_repo, &args),
            git_args(&super_repo, &args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_check_and_no_index_errors_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"clean\n").expect("write a");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    assert_eq!(
        command_output(
            skron_bin(),
            repo.path(),
            &["diff", "--check", "--", "a.txt"]
        ),
        command_output("git", repo.path(), &["diff", "--check", "--", "a.txt"])
    );

    fs::write(repo.path().join("a.txt"), b"dirty \nnext\t\n").expect("write whitespace");
    assert_eq!(
        command_output(skron_bin(), repo.path(), &["diff", "--check"]),
        command_output("git", repo.path(), &["diff", "--check"])
    );
    assert_eq!(
        command_output(
            skron_bin(),
            repo.path(),
            &["diff", "--no-index", "a.txt", "missing"],
        ),
        command_output(
            "git",
            repo.path(),
            &["diff", "--no-index", "a.txt", "missing"]
        )
    );
    #[cfg(not(windows))]
    {
        assert_eq!(
            command_output(
                skron_bin(),
                repo.path(),
                &["diff", "--no-index", "a.txt", "/dev/null"],
            ),
            command_output(
                "git",
                repo.path(),
                &["diff", "--no-index", "a.txt", "/dev/null"]
            )
        );
    }
}

#[test]
fn diff_no_index_format_options_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"one\n").expect("write a");
    fs::write(repo.path().join("b.txt"), b"two\n").expect("write b");

    for args in [
        ["diff", "--no-index", "a.txt", "b.txt"].as_slice(),
        ["diff", "--no-index", "-R", "a.txt", "b.txt"].as_slice(),
        ["diff", "--no-index", "--stat", "a.txt", "b.txt"].as_slice(),
        ["diff", "--no-index", "-R", "--stat", "a.txt", "b.txt"].as_slice(),
        ["diff", "--no-index", "--numstat", "a.txt", "b.txt"].as_slice(),
        ["diff", "--no-index", "--shortstat", "a.txt", "b.txt"].as_slice(),
        ["diff", "--no-index", "--name-only", "a.txt", "b.txt"].as_slice(),
        ["diff", "--no-index", "-R", "--name-only", "a.txt", "b.txt"].as_slice(),
        ["diff", "--no-index", "--name-status", "a.txt", "b.txt"].as_slice(),
        [
            "diff",
            "--no-index",
            "-R",
            "--name-status",
            "a.txt",
            "b.txt",
        ]
        .as_slice(),
        ["diff", "--no-index", "--raw", "a.txt", "b.txt"].as_slice(),
        ["diff", "--no-index", "-R", "--raw", "a.txt", "b.txt"].as_slice(),
        ["diff", "--no-index", "--summary", "a.txt", "b.txt"].as_slice(),
        ["diff", "--no-index", "--patch-with-stat", "a.txt", "b.txt"].as_slice(),
        ["diff", "--no-index", "--patch-with-raw", "a.txt", "b.txt"].as_slice(),
        ["diff", "--no-index", "--no-patch", "a.txt", "b.txt"].as_slice(),
        ["diff", "--no-index", "--quiet", "a.txt", "b.txt"].as_slice(),
    ] {
        assert_eq!(
            command_output(skron_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "args: {args:?}"
        );
    }

    assert_eq!(
        command_output(
            skron_bin(),
            repo.path(),
            &["diff", "--no-index", "-D", "a.txt", "/dev/null"],
        ),
        command_output(
            "git",
            repo.path(),
            &["diff", "--no-index", "-D", "a.txt", "/dev/null"]
        )
    );
}

#[test]
fn diff_no_index_file_directory_mismatch_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("dir/a.txt"), b"dir\n").expect("write dir file");
    fs::write(repo.path().join("file.txt"), b"file\n").expect("write file");

    for args in [
        ["diff", "--no-index", "dir", "file.txt"].as_slice(),
        ["diff", "--no-index", "file.txt", "dir"].as_slice(),
    ] {
        assert_eq!(
            command_output(skron_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_no_index_binary_patch_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.bin"), b"abc\0old\n").expect("write a");
    fs::write(repo.path().join("b.bin"), b"abc\0new\n").expect("write b");

    for args in [
        ["diff", "--no-index", "--binary", "a.bin", "b.bin"].as_slice(),
        ["diff", "--no-index", "-R", "--binary", "a.bin", "b.bin"].as_slice(),
    ] {
        assert_eq!(
            command_output(skron_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_no_index_directory_options_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("a/nested")).expect("create left dir");
    fs::create_dir_all(repo.path().join("b/nested")).expect("create right dir");
    fs::write(repo.path().join("a/same.txt"), b"same\n").expect("write same left");
    fs::write(repo.path().join("b/same.txt"), b"same\n").expect("write same right");
    fs::write(repo.path().join("a/mod.txt"), b"left\n").expect("write mod left");
    fs::write(repo.path().join("b/mod.txt"), b"right\n").expect("write mod right");
    fs::write(repo.path().join("a/del.txt"), b"old\n").expect("write deleted");
    fs::write(repo.path().join("b/add.txt"), b"new\n").expect("write added");
    fs::write(repo.path().join("a/nested/file.txt"), b"left\n").expect("write nested left");
    fs::write(repo.path().join("b/nested/file.txt"), b"right\n").expect("write nested right");

    for args in [
        ["diff", "--no-index", "a", "b"].as_slice(),
        ["diff", "--no-index", "-R", "a", "b"].as_slice(),
        ["diff", "--no-index", "--stat", "a", "b"].as_slice(),
        ["diff", "--no-index", "-R", "--stat", "a", "b"].as_slice(),
        ["diff", "--no-index", "--numstat", "a", "b"].as_slice(),
        ["diff", "--no-index", "-R", "--numstat", "a", "b"].as_slice(),
        ["diff", "--no-index", "--shortstat", "a", "b"].as_slice(),
        ["diff", "--no-index", "--name-only", "a", "b"].as_slice(),
        ["diff", "--no-index", "-R", "--name-only", "a", "b"].as_slice(),
        ["diff", "--no-index", "--name-status", "a", "b"].as_slice(),
        ["diff", "--no-index", "-R", "--name-status", "a", "b"].as_slice(),
        ["diff", "--no-index", "--raw", "a", "b"].as_slice(),
        ["diff", "--no-index", "-R", "--raw", "a", "b"].as_slice(),
        ["diff", "--no-index", "--summary", "a", "b"].as_slice(),
        ["diff", "--no-index", "--patch-with-stat", "a", "b"].as_slice(),
        ["diff", "--no-index", "-R", "--patch-with-stat", "a", "b"].as_slice(),
        ["diff", "--no-index", "--patch-with-raw", "a", "b"].as_slice(),
        ["diff", "--no-index", "-R", "--patch-with-raw", "a", "b"].as_slice(),
    ] {
        assert_eq!(
            command_output(skron_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
#[cfg(not(windows))]
fn difftool_extcmd_matches_stock_git_for_worktree_and_cached_changes() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "old\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    let extcmd = "sh -c 'printf \"L:\"; cat \"$1\"; printf \"R:\"; cat \"$2\"' _";
    write_file(repo.path(), "a.txt", "worktree\n");
    assert_eq!(
        run_skron(
            repo.path(),
            ["difftool", "--no-prompt", "--extcmd", extcmd, "a.txt"]
        ),
        git(
            repo.path(),
            ["difftool", "--no-prompt", "--extcmd", extcmd, "a.txt"]
        )
    );

    write_file(repo.path(), "a.txt", "staged\n");
    git(repo.path(), ["add", "a.txt"]);
    write_file(repo.path(), "a.txt", "dirty\n");
    assert_eq!(
        run_skron(
            repo.path(),
            [
                "difftool",
                "--cached",
                "--no-prompt",
                "--extcmd",
                extcmd,
                "a.txt",
            ]
        ),
        git(
            repo.path(),
            [
                "difftool",
                "--cached",
                "--no-prompt",
                "--extcmd",
                extcmd,
                "a.txt",
            ]
        )
    );
}

#[test]
#[cfg(not(windows))]
fn difftool_uses_configured_default_tool_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "old\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    write_file(repo.path(), "a.txt", "worktree\n");
    let command = "printf 'L:'; cat \"$LOCAL\"; printf 'R:'; cat \"$REMOTE\"";
    git(repo.path(), ["config", "diff.tool", "skrontest"]);
    git(repo.path(), ["config", "difftool.skrontest.cmd", command]);
    git(repo.path(), ["config", "difftool.prompt", "false"]);

    assert_eq!(
        run_skron(repo.path(), ["difftool", "--no-prompt", "a.txt"]),
        git(repo.path(), ["difftool", "--no-prompt", "a.txt"])
    );
    assert_eq!(
        run_skron(
            repo.path(),
            ["difftool", "--no-prompt", "--tool=skrontest", "a.txt"]
        ),
        git(
            repo.path(),
            ["difftool", "--no-prompt", "--tool=skrontest", "a.txt"]
        )
    );
}

#[test]
fn diff_treeish_forms_match_stock_git() {
    let repo = two_commit_repo();

    for args in [
        ["diff", "--name-status", "HEAD~1", "HEAD"].as_slice(),
        ["diff", "--stat", "HEAD~1", "HEAD"].as_slice(),
        ["diff", "--raw", "HEAD~1", "HEAD"].as_slice(),
        ["diff", "HEAD~1", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    fs::write(repo.path().join("a.txt"), b"worktree\n").expect("modify worktree");
    for args in [
        ["diff", "--name-status", "HEAD"].as_slice(),
        ["diff", "--stat", "HEAD"].as_slice(),
        ["diff", "HEAD", "a.txt"].as_slice(),
        ["diff", "HEAD", "--", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    git(repo.path(), ["add", "-A"]);
    for args in [
        ["diff", "--cached", "--name-status", "HEAD"].as_slice(),
        ["diff", "--cached", "--stat", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_tree_non_recursive_directory_entries_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("deps")).expect("create deps");
    fs::write(repo.path().join("deps/a.txt"), b"one\n").expect("write nested");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    fs::write(repo.path().join("deps/a.txt"), b"two\n").expect("rewrite nested");
    fs::create_dir(repo.path().join("added")).expect("create added");
    fs::write(repo.path().join("added/b.txt"), b"added\n").expect("write added");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "nested"]);

    for args in [
        ["diff-tree", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "--raw", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-R", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-R", "--raw", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "--name-status", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-R", "--name-status", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "--name-only", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-R", "--name-only", "HEAD~1", "HEAD"].as_slice(),
        [
            "diff-tree",
            "--name-status",
            "--diff-filter=A",
            "HEAD~1",
            "HEAD",
        ]
        .as_slice(),
        [
            "diff-tree",
            "--name-status",
            "--diff-filter=M",
            "HEAD~1",
            "HEAD",
        ]
        .as_slice(),
        ["diff-tree", "-r", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-p", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "--stat", "HEAD~1", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn plumbing_diff_commands_match_stock_git() {
    let repo = two_commit_repo();

    for args in [
        ["diff-tree", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "HEAD"].as_slice(),
        ["diff-tree", "--name-status", "HEAD"].as_slice(),
        ["diff-tree", "-z", "--name-status", "HEAD"].as_slice(),
        ["diff-tree", "-z", "--raw", "HEAD"].as_slice(),
        ["diff-tree", "--raw", "--abbrev=4", "HEAD"].as_slice(),
        ["diff-tree", "--raw", "--no-abbrev", "HEAD"].as_slice(),
        ["diff-tree", "--no-patch", "HEAD"].as_slice(),
        ["diff-tree", "-s", "HEAD"].as_slice(),
        ["diff-tree", "--patch-with-raw", "HEAD"].as_slice(),
        ["diff-tree", "--patch-with-stat", "HEAD"].as_slice(),
        ["diff-tree", "--full-index", "-p", "HEAD"].as_slice(),
        ["diff-tree", "--abbrev=4", "-p", "HEAD"].as_slice(),
        ["diff-tree", "--no-prefix", "-p", "HEAD"].as_slice(),
        [
            "diff-tree",
            "--src-prefix=old/",
            "--dst-prefix=new/",
            "-p",
            "HEAD",
        ]
        .as_slice(),
        ["diff-tree", "-U0", "HEAD~1", "HEAD"].as_slice(),
        [
            "diff-tree",
            "--unified=1",
            "--inter-hunk-context=20",
            "HEAD~1",
            "HEAD",
        ]
        .as_slice(),
        [
            "diff-tree",
            "-p",
            "--output-indicator-new=>",
            "--output-indicator-old=<",
            "--output-indicator-context==",
            "HEAD~1",
            "HEAD",
        ]
        .as_slice(),
        ["diff-tree", "-u", "HEAD"].as_slice(),
        ["diff-tree", "--name-status", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "--stat", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-p", "HEAD~1", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    fs::write(repo.path().join("a.txt"), b"worktree\n").expect("modify worktree");
    for args in [
        ["diff-files"].as_slice(),
        ["diff-files", "--name-status"].as_slice(),
        ["diff-files", "-z", "--name-status"].as_slice(),
        ["diff-files", "-z", "--name-only"].as_slice(),
        ["diff-files", "-z", "--numstat"].as_slice(),
        ["diff-files", "--raw", "--abbrev=4"].as_slice(),
        ["diff-files", "--raw", "--no-abbrev"].as_slice(),
        ["diff-files", "--stat"].as_slice(),
        ["diff-files", "--no-patch"].as_slice(),
        ["diff-files", "-s"].as_slice(),
        ["diff-files", "--patch-with-raw"].as_slice(),
        ["diff-files", "--patch-with-stat"].as_slice(),
        ["diff-files", "--full-index", "-p"].as_slice(),
        ["diff-files", "--abbrev=4", "-p"].as_slice(),
        ["diff-files", "--no-prefix", "-p"].as_slice(),
        ["diff-files", "--src-prefix=old/", "--dst-prefix=new/", "-p"].as_slice(),
        ["diff-files", "-U0"].as_slice(),
        ["diff-files", "--unified=1", "--inter-hunk-context=20"].as_slice(),
        [
            "diff-files",
            "-p",
            "--output-indicator-new=>",
            "--output-indicator-old=<",
            "--output-indicator-context==",
        ]
        .as_slice(),
        ["diff-files", "-p"].as_slice(),
        ["diff-files", "-u"].as_slice(),
        ["diff-files", "-q"].as_slice(),
        ["diff-index", "HEAD"].as_slice(),
        ["diff-index", "--name-status", "HEAD"].as_slice(),
        ["diff-index", "-z", "--name-status", "HEAD"].as_slice(),
        ["diff-index", "-z", "--raw", "HEAD"].as_slice(),
        ["diff-index", "--raw", "--abbrev=4", "HEAD"].as_slice(),
        ["diff-index", "--raw", "--no-abbrev", "HEAD"].as_slice(),
        ["diff-index", "--no-patch", "HEAD"].as_slice(),
        ["diff-index", "-s", "HEAD"].as_slice(),
        ["diff-index", "--patch-with-raw", "HEAD"].as_slice(),
        ["diff-index", "--patch-with-stat", "HEAD"].as_slice(),
        ["diff-index", "--full-index", "-p", "HEAD"].as_slice(),
        ["diff-index", "--abbrev=4", "-p", "HEAD"].as_slice(),
        ["diff-index", "--no-prefix", "-p", "HEAD"].as_slice(),
        [
            "diff-index",
            "--src-prefix=old/",
            "--dst-prefix=new/",
            "-p",
            "HEAD",
        ]
        .as_slice(),
        ["diff-index", "-U0", "HEAD"].as_slice(),
        [
            "diff-index",
            "--unified=1",
            "--inter-hunk-context=20",
            "HEAD",
        ]
        .as_slice(),
        [
            "diff-index",
            "-p",
            "--output-indicator-new=>",
            "--output-indicator-old=<",
            "--output-indicator-context==",
            "HEAD",
        ]
        .as_slice(),
        ["diff-index", "-p", "HEAD"].as_slice(),
        ["diff-index", "-u", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    assert_eq!(
        run_skron_status(repo.path(), ["diff-files", "--quiet"]),
        git_status(repo.path(), ["diff-files", "--quiet"])
    );
    assert_eq!(
        run_skron_status(repo.path(), ["diff-files", "--exit-code"]),
        git_status(repo.path(), ["diff-files", "--exit-code"])
    );
    assert_eq!(
        run_skron_status(repo.path(), ["diff-index", "--exit-code", "HEAD"]),
        git_status(repo.path(), ["diff-index", "--exit-code", "HEAD"])
    );

    git(repo.path(), ["add", "-A"]);
    for args in [
        ["diff-index", "--cached", "HEAD"].as_slice(),
        ["diff-index", "--cached", "--name-status", "HEAD"].as_slice(),
        ["diff-index", "--cached", "-z", "--name-status", "HEAD"].as_slice(),
        ["diff-index", "--cached", "-z", "--numstat", "HEAD"].as_slice(),
        ["diff-index", "--cached", "--stat", "HEAD"].as_slice(),
        ["diff-index", "--cached", "--no-patch", "HEAD"].as_slice(),
        ["diff-index", "--cached", "--patch-with-raw", "HEAD"].as_slice(),
        ["diff-index", "--cached", "--patch-with-stat", "HEAD"].as_slice(),
        ["diff-index", "--cached", "--raw", "--abbrev=4", "HEAD"].as_slice(),
        ["diff-index", "--cached", "--full-index", "-p", "HEAD"].as_slice(),
        ["diff-index", "--cached", "-p", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    assert_eq!(
        run_skron_status(
            repo.path(),
            ["diff-index", "--cached", "--exit-code", "HEAD"]
        ),
        git_status(
            repo.path(),
            ["diff-index", "--cached", "--exit-code", "HEAD"]
        )
    );
    assert_eq!(
        run_skron_status(repo.path(), ["diff-tree", "--exit-code", "HEAD~1", "HEAD"]),
        git_status(repo.path(), ["diff-tree", "--exit-code", "HEAD~1", "HEAD"])
    );
}

#[test]
fn diff_patch_hunks_match_stock_git_for_context_and_no_newline() {
    let repo = git_init();
    configure_identity(repo.path());
    let initial = (1..=20)
        .map(|idx| format!("line {idx}\n"))
        .collect::<String>();
    fs::write(repo.path().join("context.txt"), initial).expect("write context");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    let mut changed = (1..=19)
        .map(|idx| match idx {
            5 => "line five changed\n".to_owned(),
            17 => "line seventeen changed\n".to_owned(),
            _ => format!("line {idx}\n"),
        })
        .collect::<String>();
    changed.push_str("line twenty changed without newline");
    fs::write(repo.path().join("context.txt"), changed).expect("rewrite context");

    assert_eq!(
        run_skron(repo.path(), ["diff", "context.txt"]),
        git(repo.path(), ["diff", "context.txt"])
    );
    git(repo.path(), ["add", "-A"]);
    assert_eq!(
        run_skron(repo.path(), ["diff", "--cached", "context.txt"]),
        git(repo.path(), ["diff", "--cached", "context.txt"])
    );
}

#[test]
fn diff_patch_matches_stock_git_for_markdown_empty_and_binary_deletes() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(
        repo.path().join("README.md"),
        concat!(
            "# This repository has been moved to GitHub.\n",
            "\n",
            "Please see https://github.com/atlassian/atlascode\n",
            "\n",
            "## Issues\n",
            "\n",
            "[GitHub](https://github.com/atlassian/atlascode/issues) will track new issues.\n",
            "\n",
            "[Bitbucket](https://bitbucket.org/atlassianlabs/atlascode/issues) will track old issues.\n",
            "\n",
            "## Pull Requests\n",
            "\n",
            "Please use [GitHub for Pull Requests](https://github.com/atlassian/atlascode/pulls). "
        ),
    )
    .expect("write readme");
    fs::write(repo.path().join("empty.txt"), b"").expect("write empty");
    fs::write(repo.path().join("image.bin"), b"\0binary\n").expect("write binary");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    fs::write(
        repo.path().join("README.md"),
        concat!(
            "# This repository has been moved to GitHub.\n",
            "\n",
            "Please see https://github.com/atlassian/atlascode\n",
            "\n",
            "## Issues\n",
            "\n",
            "[GitHub](https://github.com/atlassian/atlascode/issues) will track new issues.\n",
            "\n",
            "[Bitbucket](https://bitbucket.org/atlassianlabs/atlascode/issues) will track old issues.\n",
            "\n",
            "## Pull Requests\n",
            "\n",
            "Please use [GitHub for Pull Requests](https://github.com/atlassian/atlascode/pulls). worktree diff\n"
        ),
    )
    .expect("rewrite readme");
    fs::remove_file(repo.path().join("empty.txt")).expect("remove empty");
    fs::remove_file(repo.path().join("image.bin")).expect("remove binary");

    assert_eq!(run_skron(repo.path(), ["diff"]), git(repo.path(), ["diff"]));
}

#[test]
fn diff_patch_matches_stock_git_for_yaml_hunk_headers() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(
        repo.path().join(".changelog.yml"),
        concat!(
            "groups:\n",
            "  -\n",
            "    name: DOCS\n",
            "    labels:\n",
            "      - kind/docs\n",
            "\n",
            "# regex indicating which labels to skip for the changelog\n",
            "skip-labels: skip-changelog|backport\\/.+\n"
        ),
    )
    .expect("write changelog config");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    fs::write(
        repo.path().join(".changelog.yml"),
        concat!(
            "groups:\n",
            "  -\n",
            "    name: DOCS\n",
            "    labels:\n",
            "      - kind/docs\n",
            "\n",
            "# regex indicating which labels to skip for the changelog\n",
            "skip-labels: skip-changelog|backport\\/.+\n",
            "worktree-diff: true\n"
        ),
    )
    .expect("rewrite changelog config");

    assert_eq!(
        run_skron(repo.path(), ["diff", ".changelog.yml"]),
        git(repo.path(), ["diff", ".changelog.yml"])
    );
}

#[test]
fn diff_patch_matches_stock_git_for_json_and_repeated_blank_alignment() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(
        repo.path().join(".cspell.json"),
        concat!(
            "{\n",
            "  \"words\": [\n",
            "    \"DNSSEC\",\n",
            "    \"Merch\"\n",
            "  ]\n",
            "}\n"
        ),
    )
    .expect("write json");
    fs::write(
        repo.path().join("merch.md"),
        concat!(
            "**FreeWear.org**\n",
            "**URL:** https://www.freewear.org/Codeberg\n",
            "**Products:** T-shirts\n",
            "**Reviewed by contributors/community:** Yes\n",
            "\n",
            "---\n",
            "\n",
            "**HELLOTUX**\n"
        ),
    )
    .expect("write markdown");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    fs::write(
        repo.path().join(".cspell.json"),
        concat!(
            "{\n",
            "  \"words\": [\n",
            "    \"DNSSEC\",\n",
            "    \"HELLOTUX\",\n",
            "    \"Merch\"\n",
            "  ]\n",
            "}\n"
        ),
    )
    .expect("rewrite json");
    fs::write(
        repo.path().join("merch.md"),
        concat!(
            "**FreeWear.org**\n",
            "\n",
            "- **URL:** [freewear.org/Codeberg](https://freewear.org/Codeberg)\n",
            "- **Products:** T-shirts\n",
            "- **Reviewed by contributors/community:** Yes\n",
            "\n",
            "---\n",
            "\n",
            "**HELLOTUX**\n"
        ),
    )
    .expect("rewrite markdown");

    assert_eq!(run_skron(repo.path(), ["diff"]), git(repo.path(), ["diff"]));
}

#[test]
fn diff_stat_binary_rows_align_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("short.txt"), b"old\n").expect("write text");
    fs::write(repo.path().join("image.bin"), b"\0binary payload\n").expect("write binary");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    fs::write(repo.path().join("short.txt"), b"old\nnew\n").expect("rewrite text");
    fs::remove_file(repo.path().join("image.bin")).expect("remove binary");

    assert_eq!(
        run_skron(repo.path(), ["diff", "--stat"]),
        git(repo.path(), ["diff", "--stat"])
    );
}
