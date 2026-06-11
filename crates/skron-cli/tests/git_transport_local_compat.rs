mod common;

use std::fs;
use std::process::Command;

use tempfile::TempDir;

use common::{
    command_output_with_env, configure_identity, git, git_failure_output, git_status,
    git_status_args, git_with_env, run_skron, run_skron_failure_output, run_skron_status,
    run_skron_with_env, skron_bin,
};

#[test]
fn fetch_local_remote_updates_remote_refs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"main\n").expect("write main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    fs::write(source.join("README.md"), b"main update\n").expect("update main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main update"]);
    git(&source, ["tag", "v2"]);
    git(&source, ["switch", "-c", "feature"]);
    fs::write(source.join("feature.txt"), b"feature\n").expect("write feature");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "feature"]);
    git(&source, ["switch", "main"]);

    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    git(&git_client, ["fetch", "origin"]);
    run_skron(&skron_client, ["fetch", "origin"]);

    assert_eq!(
        run_skron(&skron_client, ["branch", "-r"]),
        git(&git_client, ["branch", "-r"])
    );
    assert_eq!(
        run_skron(&skron_client, ["show-ref", "--tags"]),
        git(&git_client, ["show-ref", "--tags"])
    );
    assert_eq!(
        git(&skron_client, ["rev-parse", "refs/remotes/origin/main"]),
        git(&git_client, ["rev-parse", "refs/remotes/origin/main"])
    );
    assert_eq!(
        git(&skron_client, ["rev-parse", "refs/remotes/origin/feature"]),
        git(&git_client, ["rev-parse", "refs/remotes/origin/feature"])
    );
    assert_eq!(
        git(
            &skron_client,
            ["cat-file", "-p", "refs/remotes/origin/main^{tree}"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "refs/remotes/origin/main^{tree}"]
        )
    );

    let git_file_client = dir.path().join("git-file-client");
    let skron_file_client = dir.path().join("skron-file-client");
    git(dir.path(), ["init", "-b", "main", "git-file-client"]);
    run_skron(dir.path(), ["init", "-b", "main", "skron-file-client"]);
    let source_file_url = format!("file://{}", source.display());
    git(
        &git_file_client,
        ["remote", "add", "origin", &source_file_url],
    );
    run_skron(
        &skron_file_client,
        ["remote", "add", "origin", &source_file_url],
    );
    git(&git_file_client, ["fetch", "--depth", "1", "origin"]);
    run_skron(&skron_file_client, ["fetch", "--depth", "1", "origin"]);
    assert_eq!(
        run_skron(&skron_file_client, ["branch", "-r"]),
        git(&git_file_client, ["branch", "-r"])
    );
    assert_eq!(
        run_skron(&skron_file_client, ["log", "--oneline", "--all"]),
        git(&git_file_client, ["log", "--oneline", "--all"])
    );
    assert_eq!(
        fs::read_to_string(skron_file_client.join(".git/shallow")).expect("skron shallow"),
        fs::read_to_string(git_file_client.join(".git/shallow")).expect("git shallow")
    );
}

#[test]
fn fetch_with_depth_like_stock_git_for_local_remote() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let remote = dir.path().join("remote.git");
    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");

    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file.txt"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    fs::write(source.join("file.txt"), b"next\n").expect("write next");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "next"]);

    git(
        &source,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&source, ["push", "-q", "origin", "main"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    let source_remote_url = format!("file://{}", remote.display());
    git(
        &git_client,
        ["remote", "set-url", "origin", &source_remote_url],
    );
    run_skron(
        &skron_client,
        ["remote", "set-url", "origin", &source_remote_url],
    );

    fs::write(source.join("file.txt"), b"next2\n").expect("write next2");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "next2"]);

    git(&source, ["push", "-q", "origin", "main"]);

    git(&git_client, ["fetch", "--depth=1", "origin", "main"]);
    run_skron(&skron_client, ["fetch", "--depth=1", "origin", "main"]);

    let remote_main = git(&git_client, ["rev-parse", "refs/remotes/origin/main"]);
    assert_eq!(
        git(&skron_client, ["rev-parse", "refs/remotes/origin/main"]),
        remote_main
    );
    assert_eq!(
        git_status_args(
            &skron_client,
            &["cat-file", "-e", &format!("{remote_main}^")]
        ),
        128
    );
    assert_eq!(
        fs::read_to_string(skron_client.join(".git/shallow")).expect("skron shallow"),
        fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow")
    );
    assert_eq!(
        git(
            &skron_client,
            ["cat-file", "-p", &format!("{remote_main}:file.txt")],
        ),
        git(
            &git_client,
            ["cat-file", "-p", &format!("{remote_main}:file.txt")],
        )
    );
    assert_eq!(
        run_skron(&skron_client, ["log", "--oneline", "--all"]),
        git(&git_client, ["log", "--oneline", "--all"])
    );
}

#[test]
fn fetch_depth_all_branches_uses_loose_ref_over_packed_ref_for_local_remote() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let skron_client = dir.path().join("skron-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file.txt"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    git(&source, ["branch", "feature"]);
    git(&source, ["pack-refs", "--all", "--prune"]);
    let packed_feature = git(&source, ["rev-parse", "refs/heads/feature"]);
    git(&source, ["switch", "feature"]);
    fs::write(source.join("feature.txt"), b"feature\n").expect("write feature");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "feature"]);
    let loose_feature = git(&source, ["rev-parse", "refs/heads/feature"]);
    assert_ne!(packed_feature, loose_feature);
    git(&source, ["switch", "main"]);

    run_skron(dir.path(), ["init", "-b", "main", "skron-client"]);
    run_skron(
        &skron_client,
        [
            "remote",
            "add",
            "origin",
            source.to_str().expect("source path"),
        ],
    );
    run_skron(&skron_client, ["fetch", "--depth", "1", "origin"]);

    assert_eq!(
        git(&skron_client, ["rev-parse", "refs/remotes/origin/feature"]),
        loose_feature
    );
}

#[test]
fn fetch_with_depth_includes_nested_annotated_tags_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let remote = dir.path().join("remote.git");
    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");

    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    fs::write(source.join("b.txt"), b"next\n").expect("write next");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "next"]);

    git(
        &source,
        [
            "-c",
            "tag.gpgSign=false",
            "tag",
            "-a",
            "v1",
            "-m",
            "v1 tag",
            "HEAD",
        ],
    );
    git(
        &source,
        [
            "-c",
            "tag.gpgSign=false",
            "tag",
            "-a",
            "v1-nested",
            "-m",
            "nested v1 tag",
            "refs/tags/v1",
        ],
    );

    git(
        &source,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&source, ["push", "-q", "origin", "main", "--tags"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    let remote_url = format!("file://{}", remote.display());
    git(&git_client, ["remote", "set-url", "origin", &remote_url]);
    run_skron(&skron_client, ["remote", "set-url", "origin", &remote_url]);

    git(&git_client, ["fetch", "--depth=1", "origin", "main"]);
    run_skron(&skron_client, ["fetch", "--depth=1", "origin", "main"]);

    let nested = git(&source, ["rev-parse", "refs/tags/v1-nested"]);
    let direct = git(&source, ["rev-parse", "refs/tags/v1"]);
    assert_eq!(
        git(&skron_client, ["cat-file", "-p", &nested]),
        git(&git_client, ["cat-file", "-p", &nested])
    );
    assert_eq!(
        git(&skron_client, ["cat-file", "-p", &direct]),
        git(&git_client, ["cat-file", "-p", &direct])
    );
}

#[test]
fn pull_local_remote_fast_forwards_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"main\n").expect("write main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    fs::write(source.join("README.md"), b"main update\n").expect("update main");
    fs::write(source.join("next.txt"), b"next\n").expect("write next");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main update"]);

    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    git(&git_client, ["pull", "--ff-only"]);
    run_skron(&skron_client, ["pull", "--ff-only"]);

    assert_eq!(
        git(&skron_client, ["rev-parse", "HEAD"]),
        git(&git_client, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&skron_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        run_skron(&skron_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_local_remote_replays_local_commit_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    configure_identity(&git_client);
    configure_identity(&skron_client);
    fs::write(git_client.join("local.txt"), b"local\n").expect("write git local");
    fs::write(skron_client.join("local.txt"), b"local\n").expect("write skron local");
    git(&git_client, ["add", "-A"]);
    git(&skron_client, ["add", "-A"]);
    git_with_env(&git_client, ["commit", "-m", "local"]);
    git_with_env(&skron_client, ["commit", "-m", "local"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    git_with_env(&git_client, ["pull", "--rebase"]);
    run_skron_with_env(&skron_client, ["pull", "--rebase"]);

    assert_eq!(
        git(&skron_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&skron_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        run_skron(&skron_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_merges_local_remote_replays_linear_local_commit_like_stock_git() {
    pull_rebase_mode_local_remote_replays_linear_local_commit_like_stock_git(
        "--rebase=merges",
        &[],
    );
}

#[test]
fn pull_rebase_interactive_local_remote_replays_linear_local_commit_like_stock_git() {
    pull_rebase_mode_local_remote_replays_linear_local_commit_like_stock_git(
        "--rebase=interactive",
        &[("GIT_SEQUENCE_EDITOR", ":")],
    );
}

#[test]
fn pull_rebase_interactive_applies_sequence_editor_drop_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    configure_identity(&git_client);
    configure_identity(&skron_client);
    fs::write(git_client.join("local.txt"), b"local\n").expect("write git local");
    fs::write(skron_client.join("local.txt"), b"local\n").expect("write skron local");
    git(&git_client, ["add", "-A"]);
    git(&skron_client, ["add", "-A"]);
    git_with_env(&git_client, ["commit", "-m", "local"]);
    git_with_env(&skron_client, ["commit", "-m", "local"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    let editor = dir.path().join("drop-first-todo.sh");
    fs::write(
        &editor,
        r#"#!/bin/sh
python3 - "$1" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
lines = path.read_text().splitlines()
for index, line in enumerate(lines):
    if line.startswith("pick "):
        lines[index] = "drop " + line.split(" ", 1)[1]
        break
path.write_text("\n".join(lines) + "\n")
PY
"#,
    )
    .expect("write sequence editor");
    let editor_command = format!("sh {}", editor.display());
    let env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ("GIT_SEQUENCE_EDITOR", editor_command.as_str()),
    ];
    command_output_with_env(
        "git",
        &git_client,
        &["pull", "--rebase=interactive"],
        &env,
        "git",
    );
    command_output_with_env(
        skron_bin(),
        &skron_client,
        &["pull", "--rebase=interactive"],
        &env,
        "skron",
    );

    assert_eq!(
        git(&skron_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&skron_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert!(!skron_client.join("local.txt").exists());
    assert_eq!(
        run_skron(&skron_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_interactive_reword_uses_commit_editor_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    configure_identity(&git_client);
    configure_identity(&skron_client);
    fs::write(git_client.join("local.txt"), b"local\n").expect("write git local");
    fs::write(skron_client.join("local.txt"), b"local\n").expect("write skron local");
    git(&git_client, ["add", "-A"]);
    git(&skron_client, ["add", "-A"]);
    git_with_env(&git_client, ["commit", "-m", "local"]);
    git_with_env(&skron_client, ["commit", "-m", "local"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    let sequence_editor = dir.path().join("reword-first-todo.sh");
    fs::write(
        &sequence_editor,
        r#"#!/bin/sh
python3 - "$1" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
lines = path.read_text().splitlines()
for index, line in enumerate(lines):
    if line.startswith("pick "):
        lines[index] = "reword " + line.split(" ", 1)[1]
        break
path.write_text("\n".join(lines) + "\n")
PY
"#,
    )
    .expect("write sequence editor");
    let commit_editor = dir.path().join("rewrite-message.sh");
    fs::write(
        &commit_editor,
        r#"#!/bin/sh
printf 'renamed local\n' > "$1"
"#,
    )
    .expect("write commit editor");
    let sequence_editor_command = format!("sh {}", sequence_editor.display());
    let commit_editor_command = format!("sh {}", commit_editor.display());
    let env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ("GIT_SEQUENCE_EDITOR", sequence_editor_command.as_str()),
        ("GIT_EDITOR", commit_editor_command.as_str()),
    ];
    command_output_with_env(
        "git",
        &git_client,
        &["pull", "--rebase=interactive"],
        &env,
        "git",
    );
    command_output_with_env(
        skron_bin(),
        &skron_client,
        &["pull", "--rebase=interactive"],
        &env,
        "skron",
    );

    assert_eq!(
        git(&skron_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&skron_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        run_skron(&skron_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_interactive_squash_uses_commit_editor_like_stock_git() {
    pull_rebase_interactive_melds_second_commit_like_stock_git("squash", Some("combined local"));
}

#[test]
fn pull_rebase_interactive_fixup_keeps_previous_message_like_stock_git() {
    pull_rebase_interactive_melds_second_commit_like_stock_git("fixup", None);
}

#[test]
fn pull_rebase_interactive_edit_stops_and_continue_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    configure_identity(&git_client);
    configure_identity(&skron_client);
    for client in [&git_client, &skron_client] {
        fs::write(client.join("local-a.txt"), b"local a\n").expect("write local a");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "local a"]);
        fs::write(client.join("local-b.txt"), b"local b\n").expect("write local b");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "local b"]);
    }

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    let sequence_editor = dir.path().join("edit-first-todo.sh");
    fs::write(
        &sequence_editor,
        r#"#!/bin/sh
python3 - "$1" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
lines = path.read_text().splitlines()
for index, line in enumerate(lines):
    if line.startswith("pick "):
        lines[index] = "edit " + line.split(" ", 1)[1]
        break
path.write_text("\n".join(lines) + "\n")
PY
"#,
    )
    .expect("write sequence editor");
    let sequence_editor_command = format!("sh {}", sequence_editor.display());
    let env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ("GIT_SEQUENCE_EDITOR", sequence_editor_command.as_str()),
    ];
    command_output_with_env(
        "git",
        &git_client,
        &["pull", "--rebase=interactive"],
        &env,
        "git",
    );
    command_output_with_env(
        skron_bin(),
        &skron_client,
        &["pull", "--rebase=interactive"],
        &env,
        "skron",
    );

    assert!(
        git_client
            .join(".git/rebase-merge/git-rebase-todo")
            .exists()
    );
    assert!(
        skron_client
            .join(".git/rebase-merge/git-rebase-todo")
            .exists()
    );
    assert_eq!(
        git(&skron_client, ["log", "--format=%s", "--max-count=2"]),
        git(&git_client, ["log", "--format=%s", "--max-count=2"])
    );

    command_output_with_env("git", &git_client, &["rebase", "--continue"], &env, "git");
    command_output_with_env(
        skron_bin(),
        &skron_client,
        &["rebase", "--continue"],
        &env,
        "skron",
    );

    assert!(!skron_client.join(".git/rebase-merge").exists());
    assert_eq!(
        git(&skron_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&skron_client, ["log", "--format=%s", "--max-count=4"]),
        git(&git_client, ["log", "--format=%s", "--max-count=4"])
    );
    assert_eq!(
        run_skron(&skron_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_interactive_edit_abort_restores_original_head_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    configure_identity(&git_client);
    configure_identity(&skron_client);
    for client in [&git_client, &skron_client] {
        fs::write(client.join("local-a.txt"), b"local a\n").expect("write local a");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "local a"]);
        fs::write(client.join("local-b.txt"), b"local b\n").expect("write local b");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "local b"]);
    }

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    let sequence_editor = dir.path().join("edit-first-abort-todo.sh");
    fs::write(
        &sequence_editor,
        r#"#!/bin/sh
python3 - "$1" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
lines = path.read_text().splitlines()
for index, line in enumerate(lines):
    if line.startswith("pick "):
        lines[index] = "edit " + line.split(" ", 1)[1]
        break
path.write_text("\n".join(lines) + "\n")
PY
"#,
    )
    .expect("write sequence editor");
    let sequence_editor_command = format!("sh {}", sequence_editor.display());
    let env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ("GIT_SEQUENCE_EDITOR", sequence_editor_command.as_str()),
    ];
    let git_original = git(&git_client, ["rev-parse", "HEAD"]);
    let skron_original = git(&skron_client, ["rev-parse", "HEAD"]);
    command_output_with_env(
        "git",
        &git_client,
        &["pull", "--rebase=interactive"],
        &env,
        "git",
    );
    command_output_with_env(
        skron_bin(),
        &skron_client,
        &["pull", "--rebase=interactive"],
        &env,
        "skron",
    );
    command_output_with_env("git", &git_client, &["rebase", "--abort"], &env, "git");
    command_output_with_env(
        skron_bin(),
        &skron_client,
        &["rebase", "--abort"],
        &env,
        "skron",
    );

    assert_eq!(git(&git_client, ["rev-parse", "HEAD"]), git_original);
    assert_eq!(git(&skron_client, ["rev-parse", "HEAD"]), skron_original);
    assert!(!skron_client.join(".git/rebase-merge").exists());
    assert_eq!(
        git(&skron_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        run_skron(&skron_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

fn pull_rebase_interactive_melds_second_commit_like_stock_git(
    meld_command: &str,
    editor_message: Option<&str>,
) {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    configure_identity(&git_client);
    configure_identity(&skron_client);
    for client in [&git_client, &skron_client] {
        fs::write(client.join("local-a.txt"), b"local a\n").expect("write local a");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "local a"]);
        fs::write(client.join("local-b.txt"), b"local b\n").expect("write local b");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "local b"]);
    }

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    let sequence_editor = dir.path().join(format!("{meld_command}-second-todo.sh"));
    fs::write(
        &sequence_editor,
        format!(
            r#"#!/bin/sh
python3 - "$1" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
lines = path.read_text().splitlines()
seen = 0
for index, line in enumerate(lines):
    if line.startswith("pick "):
        seen += 1
        if seen == 2:
            lines[index] = "{meld_command} " + line.split(" ", 1)[1]
            break
path.write_text("\n".join(lines) + "\n")
PY
"#
        ),
    )
    .expect("write sequence editor");
    let sequence_editor_command = format!("sh {}", sequence_editor.display());
    let commit_editor_command = if let Some(message) = editor_message {
        let commit_editor = dir.path().join("meld-message.sh");
        fs::write(
            &commit_editor,
            format!(
                r#"#!/bin/sh
printf '{}\n' > "$1"
"#,
                message
            ),
        )
        .expect("write commit editor");
        Some(format!("sh {}", commit_editor.display()))
    } else {
        None
    };
    let mut env = vec![
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ("GIT_SEQUENCE_EDITOR", sequence_editor_command.as_str()),
    ];
    if let Some(editor) = commit_editor_command.as_deref() {
        env.push(("GIT_EDITOR", editor));
    }
    command_output_with_env(
        "git",
        &git_client,
        &["pull", "--rebase=interactive"],
        &env,
        "git",
    );
    command_output_with_env(
        skron_bin(),
        &skron_client,
        &["pull", "--rebase=interactive"],
        &env,
        "skron",
    );

    assert_eq!(
        git(&skron_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&skron_client, ["rev-list", "--count", "origin/main..HEAD"]),
        git(&git_client, ["rev-list", "--count", "origin/main..HEAD"])
    );
    assert_eq!(
        git(&skron_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        run_skron(&skron_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_merges_local_remote_preserves_merge_topology_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    configure_identity(&git_client);
    configure_identity(&skron_client);
    for client in [&git_client, &skron_client] {
        git(client, ["switch", "-c", "side"]);
        fs::write(client.join("side.txt"), b"side\n").expect("write side");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "side"]);
        git(client, ["switch", "main"]);
        fs::write(client.join("local.txt"), b"local\n").expect("write local");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "local"]);
        git_with_env(client, ["merge", "side", "-m", "Merge side"]);
    }

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    let env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
    ];
    command_output_with_env(
        "git",
        &git_client,
        &["pull", "--rebase=merges"],
        &env,
        "git",
    );
    command_output_with_env(
        skron_bin(),
        &skron_client,
        &["pull", "--rebase=merges"],
        &env,
        "skron",
    );

    assert_eq!(
        git(&skron_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&skron_client, ["rev-list", "--parents", "-n", "1", "HEAD"])
            .split_whitespace()
            .count(),
        git(&git_client, ["rev-list", "--parents", "-n", "1", "HEAD"])
            .split_whitespace()
            .count()
    );
    assert_eq!(
        git(&skron_client, ["log", "--format=%s", "--max-count=1"]),
        git(&git_client, ["log", "--format=%s", "--max-count=1"])
    );
    assert_eq!(
        run_skron(&skron_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

fn pull_rebase_mode_local_remote_replays_linear_local_commit_like_stock_git(
    mode: &str,
    extra_env: &[(&str, &str)],
) {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    configure_identity(&git_client);
    configure_identity(&skron_client);
    fs::write(git_client.join("local.txt"), b"local\n").expect("write git local");
    fs::write(skron_client.join("local.txt"), b"local\n").expect("write skron local");
    git(&git_client, ["add", "-A"]);
    git(&skron_client, ["add", "-A"]);
    git_with_env(&git_client, ["commit", "-m", "local"]);
    git_with_env(&skron_client, ["commit", "-m", "local"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    let mut env = vec![
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
    ];
    env.extend_from_slice(extra_env);
    command_output_with_env("git", &git_client, &["pull", mode], &env, "git");
    command_output_with_env(skron_bin(), &skron_client, &["pull", mode], &env, "skron");

    assert_eq!(
        git(&skron_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&skron_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        run_skron(&skron_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_config_replays_local_commit_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    configure_identity(&git_client);
    configure_identity(&skron_client);
    git(&git_client, ["config", "pull.rebase", "true"]);
    run_skron(&skron_client, ["config", "pull.rebase", "true"]);
    fs::write(git_client.join("local.txt"), b"local\n").expect("write git local");
    fs::write(skron_client.join("local.txt"), b"local\n").expect("write skron local");
    git(&git_client, ["add", "-A"]);
    git(&skron_client, ["add", "-A"]);
    git_with_env(&git_client, ["commit", "-m", "local"]);
    git_with_env(&skron_client, ["commit", "-m", "local"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    git_with_env(&git_client, ["pull"]);
    run_skron_with_env(&skron_client, ["pull"]);

    assert_eq!(
        git(&skron_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&skron_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        run_skron(&skron_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_branch_rebase_config_replays_local_commit_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    configure_identity(&git_client);
    configure_identity(&skron_client);
    git(&git_client, ["config", "branch.main.rebase", "true"]);
    run_skron(&skron_client, ["config", "branch.main.rebase", "true"]);
    fs::write(git_client.join("local.txt"), b"local\n").expect("write git local");
    fs::write(skron_client.join("local.txt"), b"local\n").expect("write skron local");
    git(&git_client, ["add", "-A"]);
    git(&skron_client, ["add", "-A"]);
    git_with_env(&git_client, ["commit", "-m", "local"]);
    git_with_env(&skron_client, ["commit", "-m", "local"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    git_with_env(&git_client, ["pull"]);
    run_skron_with_env(&skron_client, ["pull"]);

    assert_eq!(
        git(&skron_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&skron_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        run_skron(&skron_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_false_overrides_config_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    configure_identity(&git_client);
    configure_identity(&skron_client);
    git(&git_client, ["config", "pull.rebase", "true"]);
    run_skron(&skron_client, ["config", "pull.rebase", "true"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    git_with_env(&git_client, ["pull", "--rebase=false"]);
    run_skron_with_env(&skron_client, ["pull", "--rebase=false"]);

    assert_eq!(
        git(&skron_client, ["rev-parse", "HEAD"]),
        git(&git_client, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        run_skron(&skron_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn fetch_pack_copies_local_ref_objects_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "skron-client"]);

    let remote_path = remote.to_str().expect("remote path");
    let expected = git(&git_client, ["fetch-pack", remote_path, "refs/heads/main"]);
    assert_eq!(
        run_skron(
            &skron_client,
            ["fetch-pack", remote_path, "refs/heads/main"]
        ),
        expected
    );
    let fetched = expected
        .split_whitespace()
        .next()
        .expect("fetched object id");
    assert_eq!(
        git(
            &skron_client,
            ["cat-file", "-p", &format!("{fetched}:a.txt")]
        ),
        "hello"
    );
}

#[test]
fn fetch_pack_accepts_thin_and_no_progress_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "skron-client"]);

    let remote_path = remote.to_str().expect("remote path");
    let expected = git(
        &git_client,
        [
            "fetch-pack",
            "--thin",
            "--no-progress",
            remote_path,
            "refs/heads/main",
        ],
    );
    assert_eq!(
        run_skron(
            &skron_client,
            [
                "fetch-pack",
                "--thin",
                "--no-progress",
                remote_path,
                "refs/heads/main",
            ],
        ),
        expected
    );
    let fetched = expected
        .split_whitespace()
        .next()
        .expect("fetched object id");
    assert_eq!(
        git(
            &skron_client,
            ["cat-file", "-p", &format!("{fetched}:a.txt")]
        ),
        "hello"
    );
}

#[test]
fn fetch_pack_include_tag_copies_annotated_tag_objects_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "-c",
            "tag.gpgSign=false",
            "tag",
            "-a",
            "v1",
            "-m",
            "tag message",
        ],
    );
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "--tags"]);
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "skron-client"]);

    let remote_path = remote.to_str().expect("remote path");
    let expected = git(
        &git_client,
        [
            "fetch-pack",
            "--include-tag",
            remote_path,
            "refs/heads/main",
        ],
    );
    assert_eq!(
        run_skron(
            &skron_client,
            [
                "fetch-pack",
                "--include-tag",
                remote_path,
                "refs/heads/main"
            ],
        ),
        expected
    );

    let tag_id = git(&work, ["rev-parse", "refs/tags/v1"]);
    assert_eq!(git(&git_client, ["cat-file", "-t", &tag_id]), "tag");
    assert_eq!(git(&skron_client, ["cat-file", "-t", &tag_id]), "tag");
    assert_eq!(
        git(&skron_client, ["cat-file", "-p", &tag_id]),
        git(&git_client, ["cat-file", "-p", &tag_id])
    );
}

#[test]
fn fetch_pack_include_tag_with_depth_limited_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    fs::write(work.join("b.txt"), b"next\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "next"]);
    git(
        &work,
        [
            "-c",
            "tag.gpgSign=false",
            "tag",
            "-a",
            "v1",
            "-m",
            "tag message",
        ],
    );
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "--tags"]);
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "skron-client"]);

    let remote_path = remote.to_str().expect("remote path");
    let expected = git(
        &git_client,
        [
            "fetch-pack",
            "--depth=1",
            "--include-tag",
            remote_path,
            "refs/heads/main",
        ],
    );
    assert_eq!(
        run_skron(
            &skron_client,
            [
                "fetch-pack",
                "--depth=1",
                "--include-tag",
                remote_path,
                "refs/heads/main",
            ],
        ),
        expected
    );

    let fetched = expected
        .split_whitespace()
        .next()
        .expect("fetched object id");
    let tag_id = git(&work, ["rev-parse", "refs/tags/v1"]);
    assert_eq!(
        git(&skron_client, ["cat-file", "-p", &tag_id]),
        git(&git_client, ["cat-file", "-p", &tag_id])
    );
    assert_eq!(
        git_status_args(&skron_client, &["cat-file", "-e", &format!("{fetched}^")]),
        128
    );
}

#[test]
fn fetch_pack_include_tag_depth_includes_nested_annotated_tags_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"base\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "base"]);
    fs::write(work.join("b.txt"), b"next\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "next"]);
    git(
        &work,
        [
            "-c",
            "tag.gpgSign=false",
            "tag",
            "-a",
            "v1",
            "-m",
            "v1 tag",
            "HEAD",
        ],
    );
    git(
        &work,
        [
            "-c",
            "tag.gpgSign=false",
            "tag",
            "-a",
            "v1-nested",
            "-m",
            "nested v1 tag",
            "refs/tags/v1",
        ],
    );
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "--tags"]);
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "skron-client"]);

    let remote_path = remote.to_str().expect("remote path");
    let expected = git(
        &git_client,
        [
            "fetch-pack",
            "--depth=1",
            "--include-tag",
            remote_path,
            "refs/heads/main",
        ],
    );
    assert_eq!(
        run_skron(
            &skron_client,
            [
                "fetch-pack",
                "--depth=1",
                "--include-tag",
                remote_path,
                "refs/heads/main",
            ],
        ),
        expected
    );

    let nested = git(&work, ["rev-parse", "refs/tags/v1-nested"]);
    let direct = git(&work, ["rev-parse", "refs/tags/v1"]);
    assert_eq!(
        git(&skron_client, ["cat-file", "-p", &nested]),
        git(&git_client, ["cat-file", "-p", &nested])
    );
    assert_eq!(
        git(&skron_client, ["cat-file", "-p", &direct]),
        git(&git_client, ["cat-file", "-p", &direct])
    );
}

#[test]
fn fetch_pack_depth_one_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let skron_client = dir.path().join("skron-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"base\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "base"]);
    fs::write(work.join("b.txt"), b"next\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "next"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "skron-client"]);

    let remote_path = remote.to_str().expect("remote path");
    let expected = git(
        &git_client,
        ["fetch-pack", "--depth=1", remote_path, "refs/heads/main"],
    );
    assert_eq!(
        run_skron(
            &skron_client,
            ["fetch-pack", "--depth=1", remote_path, "refs/heads/main"],
        ),
        expected
    );
    let fetched = expected
        .split_whitespace()
        .next()
        .expect("fetched object id");
    assert_eq!(
        git(
            &skron_client,
            ["cat-file", "-p", &format!("{fetched}:b.txt")]
        ),
        "next"
    );
    assert_eq!(
        fs::read_to_string(skron_client.join(".git/shallow")).expect("skron shallow"),
        fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow")
    );
    assert_eq!(
        git_status_args(&skron_client, &["cat-file", "-e", &format!("{fetched}^")]),
        128
    );
}

#[test]
fn ls_remote_matches_stock_git_for_local_remotes() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "tag message"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);

    assert_eq!(
        run_skron(&work, ["ls-remote", "origin"]),
        git(&work, ["ls-remote", "origin"])
    );
    assert_eq!(
        run_skron(&work, ["ls-remote", "--heads", "origin"]),
        git(&work, ["ls-remote", "--heads", "origin"])
    );
    assert_eq!(
        run_skron(&work, ["ls-remote", "--tags", "origin"]),
        git(&work, ["ls-remote", "--tags", "origin"])
    );
    assert_eq!(
        run_skron(&work, ["ls-remote", "--refs", "origin"]),
        git(&work, ["ls-remote", "--refs", "origin"])
    );
    assert_eq!(
        run_skron(&work, ["ls-remote", "origin", "main", "v*"]),
        git(&work, ["ls-remote", "origin", "main", "v*"])
    );
    assert_eq!(
        run_skron(&work, ["ls-remote", remote.to_str().expect("remote path")]),
        git(&work, ["ls-remote", remote.to_str().expect("remote path")])
    );
    let remote_file_url = format!("file://{}", remote.display());
    assert_eq!(
        run_skron(&work, ["ls-remote", &remote_file_url]),
        git(&work, ["ls-remote", &remote_file_url])
    );
}

#[test]
fn ls_remote_unsupported_remote_helper_failure_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    git(dir.path(), ["init"]);

    assert_eq!(
        run_skron_failure_output(dir.path(), &["ls-remote", "skronproto://example/repo"]),
        git_failure_output(dir.path(), &["ls-remote", "skronproto://example/repo"])
    );
}

#[test]
fn push_local_remote_updates_bare_refs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_remote = dir.path().join("git-remote.git");
    let skron_remote = dir.path().join("skron-remote.git");
    let git_work = dir.path().join("git-work");
    let skron_work = dir.path().join("skron-work");

    git(
        dir.path(),
        ["init", "--bare", git_remote.to_str().expect("git remote")],
    );
    git(
        dir.path(),
        [
            "init",
            "--bare",
            skron_remote.to_str().expect("skron remote"),
        ],
    );
    git(
        dir.path(),
        ["init", "-b", "main", git_work.to_str().expect("git work")],
    );
    run_skron(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            skron_work.to_str().expect("skron work"),
        ],
    );
    configure_identity(&git_work);
    configure_identity(&skron_work);
    git(
        &git_work,
        [
            "remote",
            "add",
            "origin",
            git_remote.to_str().expect("git remote"),
        ],
    );
    run_skron(
        &skron_work,
        [
            "remote",
            "add",
            "origin",
            skron_remote.to_str().expect("skron remote"),
        ],
    );

    fs::write(git_work.join("README.md"), b"main\n").expect("write git main");
    fs::write(skron_work.join("README.md"), b"main\n").expect("write skron main");
    git(&git_work, ["add", "-A"]);
    run_skron(&skron_work, ["add", "-A"]);
    git_with_env(&git_work, ["commit", "-m", "main"]);
    run_skron_with_env(&skron_work, ["commit", "-m", "main"]);
    git(&git_work, ["push", "-u", "origin", "HEAD"]);
    run_skron(&skron_work, ["push", "-u", "origin", "HEAD"]);

    assert_eq!(
        git(&skron_remote, ["rev-parse", "refs/heads/main"]),
        git(&git_remote, ["rev-parse", "refs/heads/main"])
    );
    assert_eq!(
        git(&skron_remote, ["cat-file", "-p", "refs/heads/main^{tree}"]),
        git(&git_remote, ["cat-file", "-p", "refs/heads/main^{tree}"])
    );
    assert_eq!(
        run_skron(&skron_work, ["config", "branch.main.remote"]),
        git(&git_work, ["config", "branch.main.remote"])
    );
    assert_eq!(
        run_skron(&skron_work, ["config", "branch.main.merge"]),
        git(&git_work, ["config", "branch.main.merge"])
    );

    let git_remote_file_url = format!("file://{}", git_remote.display());
    let skron_remote_file_url = format!("file://{}", skron_remote.display());
    git(
        &git_work,
        ["remote", "set-url", "origin", &git_remote_file_url],
    );
    run_skron(
        &skron_work,
        ["remote", "set-url", "origin", &skron_remote_file_url],
    );

    fs::write(git_work.join("next.txt"), b"next\n").expect("write git next");
    fs::write(skron_work.join("next.txt"), b"next\n").expect("write skron next");
    git(&git_work, ["add", "-A"]);
    run_skron(&skron_work, ["add", "-A"]);
    git_with_env(&git_work, ["commit", "-m", "next"]);
    run_skron_with_env(&skron_work, ["commit", "-m", "next"]);
    git(&git_work, ["push"]);
    run_skron(&skron_work, ["push"]);

    assert_eq!(
        git(&skron_remote, ["rev-parse", "refs/heads/main"]),
        git(&git_remote, ["rev-parse", "refs/heads/main"])
    );
    assert_eq!(
        git(&skron_remote, ["cat-file", "-p", "refs/heads/main^{tree}"]),
        git(&git_remote, ["cat-file", "-p", "refs/heads/main^{tree}"])
    );

    git(&git_work, ["checkout", "-b", "feature"]);
    run_skron(&skron_work, ["checkout", "-b", "feature"]);
    fs::write(git_work.join("feature.txt"), b"feature\n").expect("write git feature");
    fs::write(skron_work.join("feature.txt"), b"feature\n").expect("write skron feature");
    git(&git_work, ["add", "-A"]);
    run_skron(&skron_work, ["add", "-A"]);
    git_with_env(&git_work, ["commit", "-m", "feature"]);
    run_skron_with_env(&skron_work, ["commit", "-m", "feature"]);
    git(&git_work, ["push", "origin", "feature"]);
    run_skron(&skron_work, ["push", "origin", "feature"]);
    assert_eq!(
        git(&skron_remote, ["rev-parse", "refs/heads/feature"]),
        git(&git_remote, ["rev-parse", "refs/heads/feature"])
    );
    git(&git_work, ["push", "origin", ":feature"]);
    run_skron(&skron_work, ["push", "origin", ":feature"]);
    assert_eq!(
        git_status(
            &skron_remote,
            ["rev-parse", "--verify", "refs/heads/feature"]
        ),
        git_status(&git_remote, ["rev-parse", "--verify", "refs/heads/feature"])
    );
}

#[test]
fn fetch_and_push_unsupported_remote_helper_failures_match_stock_git() {
    let git_repo = TempDir::new().expect("git repo");
    let skron_repo = TempDir::new().expect("skron repo");
    git(git_repo.path(), ["init", "-b", "main"]);
    git(skron_repo.path(), ["init", "-b", "main"]);
    for repo in [git_repo.path(), skron_repo.path()] {
        git(
            repo,
            ["remote", "add", "origin", "skronproto://example/repo"],
        );
    }

    assert_eq!(
        run_skron_failure_output(skron_repo.path(), &["fetch", "origin", "main"]),
        git_failure_output(git_repo.path(), &["fetch", "origin", "main"])
    );
    assert_eq!(
        run_skron_failure_output(skron_repo.path(), &["push", "origin", "main"]),
        git_failure_output(git_repo.path(), &["push", "origin", "main"])
    );
}

#[test]
fn send_pack_updates_local_bare_ref_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_remote = dir.path().join("git-remote.git");
    let skron_remote = dir.path().join("skron-remote.git");
    let git_work = dir.path().join("git-work");
    let skron_work = dir.path().join("skron-work");
    git(dir.path(), ["init", "--bare", "git-remote.git"]);
    git(dir.path(), ["init", "--bare", "skron-remote.git"]);
    git(dir.path(), ["init", "-b", "main", "git-work"]);
    git(dir.path(), ["init", "-b", "main", "skron-work"]);
    configure_identity(&git_work);
    configure_identity(&skron_work);
    fs::write(git_work.join("a.txt"), b"hello\n").expect("write git");
    fs::write(skron_work.join("a.txt"), b"hello\n").expect("write skron");
    git(&git_work, ["add", "-A"]);
    git(&skron_work, ["add", "-A"]);
    git_with_env(&git_work, ["commit", "-m", "initial"]);
    run_skron_with_env(&skron_work, ["commit", "-m", "initial"]);

    let git_remote_path = git_remote.to_str().expect("git remote");
    let skron_remote_path = skron_remote.to_str().expect("skron remote");
    assert_eq!(
        run_skron(
            &skron_work,
            ["send-pack", skron_remote_path, "refs/heads/main"]
        ),
        git(&git_work, ["send-pack", git_remote_path, "refs/heads/main"])
    );
    assert_eq!(
        git(&skron_remote, ["rev-parse", "refs/heads/main"]),
        git(&skron_work, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&skron_remote, ["cat-file", "-p", "refs/heads/main:a.txt"]),
        "hello"
    );
}

#[test]
fn send_pack_thin_updates_local_bare_ref_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_remote = dir.path().join("git-remote.git");
    let skron_remote = dir.path().join("skron-remote.git");
    let git_work = dir.path().join("git-work");
    let skron_work = dir.path().join("skron-work");
    git(dir.path(), ["init", "--bare", "git-remote.git"]);
    git(dir.path(), ["init", "--bare", "skron-remote.git"]);
    git(dir.path(), ["init", "-b", "main", "git-work"]);
    git(dir.path(), ["init", "-b", "main", "skron-work"]);
    configure_identity(&git_work);
    configure_identity(&skron_work);
    fs::write(git_work.join("a.txt"), b"hello\n").expect("write git");
    fs::write(skron_work.join("a.txt"), b"hello\n").expect("write skron");
    git(&git_work, ["add", "-A"]);
    git(&skron_work, ["add", "-A"]);
    git_with_env(&git_work, ["commit", "-m", "initial"]);
    run_skron_with_env(&skron_work, ["commit", "-m", "initial"]);

    let git_remote_path = git_remote.to_str().expect("git remote");
    let skron_remote_path = skron_remote.to_str().expect("skron remote");
    assert_eq!(
        run_skron(
            &skron_work,
            ["send-pack", "--thin", skron_remote_path, "refs/heads/main"]
        ),
        git(
            &git_work,
            ["send-pack", "--thin", git_remote_path, "refs/heads/main"]
        )
    );
    assert_eq!(
        git(&skron_remote, ["rev-parse", "refs/heads/main"]),
        git(&skron_work, ["rev-parse", "HEAD"])
    );
}

#[test]
fn send_pack_mirror_syncs_heads_tags_and_deletions_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_remote = dir.path().join("git-remote.git");
    let skron_remote = dir.path().join("skron-remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "git-remote.git"]);
    git(dir.path(), ["init", "--bare", "skron-remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("main.txt"), b"main\n").expect("write main");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "main"]);
    git(&work, ["branch", "feature"]);
    git(&work, ["tag", "v1"]);

    let git_remote_path = git_remote.to_str().expect("git remote");
    let skron_remote_path = skron_remote.to_str().expect("skron remote");
    git(&work, ["push", git_remote_path, "HEAD:refs/heads/stale"]);
    git(&work, ["push", skron_remote_path, "HEAD:refs/heads/stale"]);

    assert_eq!(
        run_skron(&work, ["send-pack", "--mirror", skron_remote_path]),
        git(&work, ["send-pack", "--mirror", git_remote_path])
    );
    assert_eq!(
        git(
            &skron_remote,
            ["for-each-ref", "--format=%(refname) %(objectname)", "refs"]
        ),
        git(
            &git_remote,
            ["for-each-ref", "--format=%(refname) %(objectname)", "refs"]
        )
    );
    assert_ne!(
        git_status(&skron_remote, ["rev-parse", "--verify", "refs/heads/stale"]),
        0
    );
}

#[test]
fn send_pack_atomic_rejects_all_updates_when_one_ref_fails() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("main.txt"), b"main\n").expect("write main");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "main"]);
    git(&work, ["branch", "feature"]);
    let remote_path = remote.to_str().expect("remote path");
    git(&work, ["push", remote_path, "HEAD:refs/heads/main"]);

    fs::write(work.join("main.txt"), b"updated\n").expect("update main");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "updated"]);
    let updated = git(&work, ["rev-parse", "HEAD"]);
    fs::write(dir.path().join("foreign.txt"), b"foreign\n").expect("write foreign");
    git(dir.path(), ["init", "-b", "main", "foreign"]);
    let foreign = dir.path().join("foreign");
    configure_identity(&foreign);
    fs::write(foreign.join("foreign.txt"), b"foreign\n").expect("write foreign repo");
    git(&foreign, ["add", "-A"]);
    git_with_env(&foreign, ["commit", "-m", "foreign"]);
    let foreign_id = git(&foreign, ["rev-parse", "HEAD"]);
    git(
        &foreign,
        ["push", "--force", remote_path, "HEAD:refs/heads/main"],
    );

    assert_ne!(
        git_status(
            &work,
            [
                "send-pack",
                "--atomic",
                remote_path,
                "refs/heads/feature",
                "refs/heads/main",
            ],
        ),
        0
    );
    assert_ne!(
        run_skron_status(
            &work,
            [
                "send-pack",
                "--atomic",
                remote_path,
                "refs/heads/feature",
                "refs/heads/main",
            ]
        ),
        0
    );
    assert_ne!(
        git_status(&remote, ["rev-parse", "--verify", "refs/heads/feature"]),
        0
    );
    assert_eq!(git(&remote, ["rev-parse", "refs/heads/main"]), foreign_id);
    assert_ne!(git(&remote, ["rev-parse", "refs/heads/main"]), updated);
}

#[test]
fn receive_pack_accepts_stock_git_send_pack() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);

    let receive_pack = format!("{} receive-pack", skron_bin());
    let output = Command::new("git")
        .args([
            "send-pack",
            "--receive-pack",
            &receive_pack,
            remote.to_str().expect("remote path"),
            "refs/heads/main",
        ])
        .current_dir(&work)
        .output()
        .expect("git send-pack via skron receive-pack");
    assert!(
        output.status.success(),
        "git send-pack failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        git(&remote, ["rev-parse", "refs/heads/main"]),
        git(&work, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&remote, ["cat-file", "-p", "refs/heads/main:a.txt"]),
        "hello"
    );

    git(&work, ["checkout", "-b", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    let output = Command::new("git")
        .args([
            "send-pack",
            "--receive-pack",
            &receive_pack,
            remote.to_str().expect("remote path"),
            "refs/heads/feature",
        ])
        .current_dir(&work)
        .output()
        .expect("git send-pack feature via skron receive-pack");
    assert!(
        output.status.success(),
        "git send-pack feature failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        git(&remote, ["rev-parse", "refs/heads/feature"]),
        git(&work, ["rev-parse", "feature"])
    );

    let output = Command::new("git")
        .args([
            "send-pack",
            "--receive-pack",
            &receive_pack,
            remote.to_str().expect("remote path"),
            ":refs/heads/feature",
        ])
        .current_dir(&work)
        .output()
        .expect("git send-pack delete via skron receive-pack");
    assert!(
        output.status.success(),
        "git send-pack delete failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_ne!(
        git_status(&remote, ["rev-parse", "--verify", "refs/heads/feature"]),
        0
    );
}

#[test]
fn upload_pack_advertisement_serves_stock_git_ls_remote() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "tag message"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);

    let upload_pack = format!("{} upload-pack", skron_bin());
    let output = Command::new("git")
        .args([
            "ls-remote",
            "--upload-pack",
            &upload_pack,
            remote.to_str().expect("remote path"),
        ])
        .output()
        .expect("git ls-remote via skron upload-pack");
    assert!(
        output.status.success(),
        "git ls-remote failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).expect("stdout utf8");
    assert_eq!(
        actual.trim_end_matches('\n'),
        git(&work, ["ls-remote", remote.to_str().expect("remote path")])
    );
}

#[test]
fn upload_pack_serves_stock_git_clone_protocol_v1() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let clone = dir.path().join("clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    fs::create_dir_all(work.join("dir")).expect("create dir");
    fs::write(work.join("dir/b.txt"), b"world\n").expect("write b");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);

    let upload_pack = format!("{} upload-pack", skron_bin());
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.version=0",
            "clone",
            "--no-local",
            "--upload-pack",
            &upload_pack,
            remote.to_str().expect("remote path"),
            clone.to_str().expect("clone path"),
        ])
        .output()
        .expect("git clone via skron upload-pack");
    assert!(
        output.status.success(),
        "git clone failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(clone.join("a.txt")).expect("read a"),
        "hello\n"
    );
    assert_eq!(
        fs::read_to_string(clone.join("dir/b.txt")).expect("read b"),
        "world\n"
    );
    assert_eq!(
        git(&clone, ["rev-parse", "HEAD"]),
        git(&work, ["rev-parse", "HEAD"])
    );
}

#[test]
fn shell_dispatches_stock_git_upload_pack_command() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let clone = dir.path().join("clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);

    let shell_upload_pack = format!("{} shell -c git-upload-pack", skron_bin());
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.version=0",
            "clone",
            "--no-local",
            "--upload-pack",
            shell_upload_pack.as_str(),
            remote.to_str().expect("remote path"),
            clone.to_str().expect("clone path"),
        ])
        .output()
        .expect("git clone via skron shell upload-pack");
    assert!(
        output.status.success(),
        "git clone via shell failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(clone.join("a.txt")).expect("read clone file"),
        "hello\n"
    );
    assert_eq!(
        git(&clone, ["rev-parse", "HEAD"]),
        git(&work, ["rev-parse", "HEAD"])
    );
}
