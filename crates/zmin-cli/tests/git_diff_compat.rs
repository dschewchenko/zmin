mod common;

use std::collections::HashMap;
use std::io::Write;
use std::process::Command;
use std::{fs, path::Path, path::PathBuf};

use flate2::{Compression, write::ZlibEncoder};
use tempfile::TempDir;
use zmin_git_core::{GitHashAlgorithm, GitObjectHash};

use common::{
    RawCommandOutput, command_any_output_with_stdin, command_output_with_env, command_raw_output,
    command_raw_output_with_stdin, configure_identity, git, git_args, git_failure_output, git_init,
    git_status, git_with_env, git_with_stdin, pinned_git_args, pinned_git_init_sha256,
    pinned_git_with_env, required_pinned_stock_git, run_zmin, run_zmin_args,
    run_zmin_failure_output, run_zmin_status, run_zmin_with_env, run_zmin_with_stdin, write_file,
    zmin_bin,
};

fn pinned_stock_git() -> PathBuf {
    required_pinned_stock_git()
}

fn diff_tree_abbrev_tuple(
    zmin: &str,
    stock: &str,
    repo: &Path,
    config: Option<&str>,
    stdin: bool,
    abbrev_args: &[&str],
    head: &str,
) -> (RawCommandOutput, RawCommandOutput) {
    let mut args = vec!["diff-tree"];
    if let Some(config) = config {
        args.splice(0..0, ["-c", config]);
    }
    if stdin {
        args.extend(["--stdin", "--format=%h"]);
    } else {
        args.extend(["--format=%h"]);
    }
    args.extend_from_slice(abbrev_args);
    if stdin {
        args.push("-s");
    } else {
        args.extend(["-s", "HEAD"]);
    }
    let zmin_result = if stdin {
        command_raw_output_with_stdin(zmin, repo, &args, format!("{head}\n").as_bytes(), "zmin")
    } else {
        command_raw_output(zmin, repo, &args, "zmin")
    };
    let stock_result = if stdin {
        command_raw_output_with_stdin(
            stock,
            repo,
            &args,
            format!("{head}\n").as_bytes(),
            "stock Git",
        )
    } else {
        command_raw_output(stock, repo, &args, "stock Git")
    };
    (zmin_result, stock_result)
}

fn command_output(command: &str, cwd: &Path, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(common::test_command_program(command))
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

fn normalize_difftool_dir_output(output: (i32, String, String)) -> (i32, String, String) {
    let stdout = output
        .1
        .lines()
        .map(|line| normalize_difftool_dir_line(line))
        .collect::<Vec<_>>()
        .join("\n");
    (output.0, stdout, output.2)
}

fn normalize_difftool_dir_line(line: &str) -> String {
    if let Some(rest) = line.strip_prefix("L:")
        && let Some(suffix) = rest.rsplit_once("/left/").map(|(_, tail)| tail)
    {
        return format!("L:<TEMP>/left/{suffix}");
    }
    if let Some(rest) = line.strip_prefix("R:")
        && let Some(suffix) = rest.rsplit_once("/right/").map(|(_, tail)| tail)
    {
        return format!("R:<TEMP>/right/{suffix}");
    }
    line.to_owned()
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

fn write_abbrev_collision_object(repo: &Path, kind: &str, content: &[u8]) -> String {
    let mut object = format!("{kind} {}\0", content.len()).into_bytes();
    object.extend_from_slice(content);
    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update(&object);
    let object_id = hasher.finalize();
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&object).expect("compress loose object");
    let compressed = encoder.finish().expect("finish loose object");
    let hex = object_id.to_hex();
    let object_dir = repo.join(".git/objects").join(&hex[..2]);
    fs::create_dir_all(&object_dir).expect("create loose object directory");
    fs::write(object_dir.join(&hex[2..]), compressed).expect("write loose object");
    hex
}

fn write_abbrev_collision_tree(repo: &Path, blob_id: &str, name: &[u8]) -> String {
    write_abbrev_collision_tree_entries(repo, &[(blob_id, name)])
}

fn write_abbrev_collision_tree_entries(repo: &Path, entries: &[(&str, &[u8])]) -> String {
    let mut tree = Vec::new();
    for (blob_id, name) in entries {
        tree.extend_from_slice(b"100644 ");
        tree.extend_from_slice(name);
        tree.push(0);
        tree.extend_from_slice(&hex_to_bytes(blob_id));
    }
    write_abbrev_collision_object(repo, "tree", &tree)
}

fn write_abbrev_collision_commit(repo: &Path, tree_id: &str, parent: Option<&str>) -> String {
    let parent_line = parent.map_or(String::new(), |id| format!("parent {id}\n"));
    let content = format!(
        "tree {tree_id}\n{parent_line}author Diff Collision <diff@example.test> 1700000000 +0000\ncommitter Diff Collision <diff@example.test> 1700000000 +0000\n\ncollision\n"
    );
    write_abbrev_collision_object(repo, "commit", content.as_bytes())
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|chunk| {
            let value = std::str::from_utf8(chunk).expect("hex chunk utf8");
            u8::from_str_radix(value, 16).expect("hex byte")
        })
        .collect()
}

struct NoIndexBlobCandidate {
    content: Vec<u8>,
    id: String,
}

struct NoIndexBlobCollision {
    old_content: Vec<u8>,
    new_content: Vec<u8>,
    prefix: String,
}

fn no_index_blob_id(algorithm: GitHashAlgorithm, content: &[u8]) -> String {
    let mut object = format!("blob {}\0", content.len()).into_bytes();
    object.extend_from_slice(content);
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update(&object);
    hasher.finalize().to_hex()
}

fn no_index_blob_collision(algorithm: GitHashAlgorithm) -> NoIndexBlobCollision {
    let label = match algorithm {
        GitHashAlgorithm::Sha1 => "sha1",
        GitHashAlgorithm::Sha256 => "sha256",
    };
    let mut candidates = HashMap::<String, NoIndexBlobCandidate>::new();
    for number in 0..100_000 {
        let content = format!("no-index-{label}-collision-{number:05}\n").into_bytes();
        let id = no_index_blob_id(algorithm, &content);
        let prefix = id[..7].to_owned();
        if let Some(previous) = candidates.get(&prefix) {
            if previous.id != id {
                return NoIndexBlobCollision {
                    old_content: previous.content.clone(),
                    new_content: content,
                    prefix,
                };
            }
        } else {
            candidates.insert(prefix, NoIndexBlobCandidate { content, id });
        }
    }
    panic!("bounded {label} no-index collision search did not find a pair");
}

struct PatchIndexIds {
    old: String,
    new: String,
}

fn patch_index_ids(stdout: &[u8]) -> PatchIndexIds {
    let line = stdout
        .split(|byte| *byte == b'\n')
        .find(|line| line.starts_with(b"index "))
        .expect("patch index line");
    let range = line
        .split(|byte| byte.is_ascii_whitespace())
        .nth(1)
        .expect("patch index range");
    let separator = range
        .windows(2)
        .position(|pair| pair == b"..")
        .expect("patch index range separator");
    let old = &range[..separator];
    let new = &range[separator + 2..];
    PatchIndexIds {
        old: String::from_utf8(old.to_vec())
            .expect("old patch index id")
            .to_owned(),
        new: String::from_utf8(new.to_vec())
            .expect("new patch index id")
            .to_owned(),
    }
}

#[test]
fn diff_bare_unified_option_keeps_following_rev_operand() {
    let repo = two_commit_repo();
    let args = ["diff", "-U", "HEAD~1..HEAD"];
    assert_eq!(run_zmin(repo.path(), args), git(repo.path(), args));
    let recursive_args = ["diff", "-r", "HEAD~1..HEAD"];
    assert_eq!(
        run_zmin(repo.path(), recursive_args),
        git(repo.path(), recursive_args)
    );
}

#[test]
fn diff_explicit_path_separator_keeps_missing_paths_out_of_revision_parsing() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "tracked.txt", "tracked\n");
    git(repo.path(), ["add", "tracked.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    fs::remove_file(repo.path().join("tracked.txt")).expect("remove tracked file");

    for args in [
        ["diff", "--cached", "--", "tracked.txt"].as_slice(),
        ["diff", "HEAD", "--", "missing.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}",
        );
    }
}

#[test]
fn diff_dirstat_matches_stock_git_for_treeish_pairs() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "root.txt", "1\n2\n3\n");
    write_file(repo.path(), "dir/sub.txt", "a\nb\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    write_file(repo.path(), "root.txt", "1\n2\n3\n4\n5\n6\n");
    write_file(repo.path(), "dir/sub.txt", "a\nb\nc\nd\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "update"]);

    for args in [
        ["diff", "--dirstat", "HEAD~1", "HEAD"].as_slice(),
        ["diff", "--dirstat-by-file", "HEAD~1", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args)
        );
    }
}

#[test]
fn diff_dirstat_family_matches_stock_git_for_porcelain_and_plumbing() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "root.txt", "1\n2\n3\n");
    write_file(repo.path(), "dir/sub.txt", "a\nb\n");
    write_file(repo.path(), "dir/nested/deep.txt", "x\ny\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    write_file(repo.path(), "root.txt", "1\n2\n3\n4\n5\n");
    write_file(repo.path(), "dir/sub.txt", "a\nb\nc\nd\n");
    write_file(repo.path(), "dir/nested/deep.txt", "x\ny\nz\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "update"]);

    write_file(repo.path(), "dir/sub.txt", "a\nb\nc\nd\ne\nf\n");
    write_file(repo.path(), "dir/nested/deep.txt", "x\ny\nz\nq\nr\n");

    for args in [
        ["diff", "-X", "HEAD~1", "HEAD"].as_slice(),
        ["diff", "-X10", "HEAD~1", "HEAD"].as_slice(),
        ["diff", "--dirstat=files", "HEAD~1", "HEAD"].as_slice(),
        ["diff", "--dirstat=files,10", "HEAD~1", "HEAD"].as_slice(),
        ["diff", "--dirstat=cumulative", "HEAD~1", "HEAD"].as_slice(),
        ["diff", "--dirstat-by-file=10,cumulative", "HEAD~1", "HEAD"].as_slice(),
        ["diff", "--cumulative", "HEAD~1", "HEAD"].as_slice(),
        ["diff", "--dirstat", "--cumulative", "HEAD~1", "HEAD"].as_slice(),
        ["diff-files", "-X"].as_slice(),
        ["diff-files", "-X10"].as_slice(),
        ["diff-files", "--dirstat=files"].as_slice(),
        ["diff-files", "--dirstat=files,10"].as_slice(),
        ["diff-files", "--dirstat=cumulative"].as_slice(),
        ["diff-files", "--dirstat-by-file=10,cumulative"].as_slice(),
        ["diff-files", "--cumulative"].as_slice(),
        ["diff-files", "--dirstat", "--cumulative"].as_slice(),
        ["diff-files", "--dirstat-by-file"].as_slice(),
        ["diff-index", "-X", "HEAD"].as_slice(),
        ["diff-index", "-X10", "HEAD"].as_slice(),
        ["diff-index", "--dirstat=files", "HEAD"].as_slice(),
        ["diff-index", "--dirstat=files,10", "HEAD"].as_slice(),
        ["diff-index", "--dirstat=cumulative", "HEAD"].as_slice(),
        ["diff-index", "--dirstat-by-file=10,cumulative", "HEAD"].as_slice(),
        ["diff-index", "--cumulative", "HEAD"].as_slice(),
        ["diff-index", "--dirstat", "--cumulative", "HEAD"].as_slice(),
        ["diff-index", "--dirstat-by-file", "HEAD"].as_slice(),
        ["diff-tree", "--dirstat", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "--dirstat-by-file", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-X", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-X10", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "--dirstat=files", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "--dirstat=files,10", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "--dirstat=cumulative", "HEAD~1", "HEAD"].as_slice(),
        [
            "diff-tree",
            "--dirstat-by-file=10,cumulative",
            "HEAD~1",
            "HEAD",
        ]
        .as_slice(),
        ["diff-tree", "--cumulative", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "--dirstat", "--cumulative", "HEAD~1", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}",
        );
    }
}

#[test]
fn diff_blob_to_blob_operands_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    let left = git_with_stdin(repo.path(), ["hash-object", "-w", "--stdin"], "one\n");
    let right = git_with_stdin(repo.path(), ["hash-object", "-w", "--stdin"], "two\n");

    for args in [
        ["diff", left.as_str(), right.as_str()].as_slice(),
        ["diff", right.as_str(), right.as_str()].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "blob-to-blob diff should match stock Git for {args:?}",
        );
    }
}

#[test]
fn diff_same_tree_pairs_match_stock_git_for_machine_formats() {
    let repo = two_commit_repo();
    let head_tree = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);

    for args in [
        ["diff", "--raw", head_tree.as_str(), head_tree.as_str()].as_slice(),
        [
            "diff",
            "-z",
            "--raw",
            head_tree.as_str(),
            head_tree.as_str(),
        ]
        .as_slice(),
        [
            "diff",
            "--name-status",
            head_tree.as_str(),
            head_tree.as_str(),
        ]
        .as_slice(),
        [
            "diff",
            "-z",
            "--name-status",
            head_tree.as_str(),
            head_tree.as_str(),
        ]
        .as_slice(),
        ["diff", "--numstat", head_tree.as_str(), head_tree.as_str()].as_slice(),
        [
            "diff",
            "-z",
            "--numstat",
            head_tree.as_str(),
            head_tree.as_str(),
        ]
        .as_slice(),
        [
            "diff",
            "-M",
            "--raw",
            head_tree.as_str(),
            head_tree.as_str(),
        ]
        .as_slice(),
        [
            "diff",
            "-M",
            "--name-status",
            head_tree.as_str(),
            head_tree.as_str(),
        ]
        .as_slice(),
        [
            "diff",
            "-M",
            "--numstat",
            head_tree.as_str(),
            head_tree.as_str(),
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "same-tree diff should match stock Git for {args:?}",
        );
    }
}

#[test]
fn diff_cached_raw_z_uses_stock_git_default_abbrev() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    write_file(repo.path(), "b.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    write_file(repo.path(), "a.txt", "one changed\n");
    write_file(repo.path(), "c.txt", "three\n");
    git(repo.path(), ["add", "-A"]);

    for args in [
        ["diff", "--cached", "--raw"].as_slice(),
        ["diff", "--cached", "--raw", "-z"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "cached raw diff should match stock Git for {args:?}",
        );
    }
}

#[test]
fn diff_tree_raw_extends_seed_width_for_colliding_output_objects() {
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    let repo = git_init();
    let left = write_abbrev_collision_object(repo.path(), "blob", b"abbrev-sha1-collision-006687");
    let right = write_abbrev_collision_object(repo.path(), "blob", b"abbrev-sha1-collision-040110");
    assert_eq!(
        &left[..7],
        &right[..7],
        "fixture must collide at seed width"
    );
    let unrelated_old =
        write_abbrev_collision_object(repo.path(), "blob", b"abbrev-sha1-unrelated-old");
    let unrelated_new =
        write_abbrev_collision_object(repo.path(), "blob", b"abbrev-sha1-unrelated-new");
    let old_tree =
        write_abbrev_collision_tree_entries(repo.path(), &[(&left, b"a"), (&unrelated_old, b"b")]);
    let new_tree =
        write_abbrev_collision_tree_entries(repo.path(), &[(&right, b"a"), (&unrelated_new, b"b")]);
    let old_commit = write_abbrev_collision_commit(repo.path(), &old_tree, None);
    let new_commit = write_abbrev_collision_commit(repo.path(), &new_tree, Some(&old_commit));
    for value in ["7", "12", "4", "no"] {
        let config = format!("core.abbrev={value}");
        let args = [
            "-c",
            config.as_str(),
            "diff-tree",
            "--raw",
            "--abbrev",
            "--no-commit-id",
            &old_commit,
            &new_commit,
        ];
        let zmin = command_raw_output(zmin_bin(), repo.path(), &args, "zmin");
        let stock_result = command_raw_output(stock, repo.path(), &args, "stock Git");
        assert_eq!(zmin, stock_result, "core.abbrev={value}");
        let rows = stock_result
            .stdout
            .split(|byte| *byte == b'\n')
            .filter(|row| !row.is_empty())
            .map(|row| {
                row.split(|byte| byte.is_ascii_whitespace())
                    .filter(|field| !field.is_empty())
                    .map(|field| field.len())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let minimum = value.parse::<usize>().unwrap_or(40);
        assert_eq!(rows.len(), 2, "core.abbrev={value}");
        assert!(rows.iter().all(|fields| fields[2] >= minimum));
        assert_eq!(rows[0][2], rows[0][3], "colliding row width");
        assert_eq!(rows[1][2], rows[1][3], "unrelated row width");
        if value == "7" || value == "4" {
            let unrelated_width = value.parse::<usize>().expect("numeric core.abbrev");
            assert_eq!(
                [rows[0][2], rows[0][3], rows[1][2], rows[1][3]],
                [8, 8, unrelated_width, unrelated_width],
                "per-object widths for core.abbrev={value}"
            );
        } else if value == "no" {
            assert_eq!(rows[0][2], 40, "core.abbrev=no");
            assert_eq!(rows[1][2], 40, "core.abbrev=no");
        }
    }
    for args in [
        [
            "diff-tree",
            "--raw",
            "--abbrev=7",
            "--no-commit-id",
            old_commit.as_str(),
            new_commit.as_str(),
        ]
        .as_slice(),
        [
            "-c",
            "core.abbrev=7",
            "log",
            "--raw",
            "--format=%H",
            "--max-count",
            "1",
            new_commit.as_str(),
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_raw_output(zmin_bin(), repo.path(), args, "zmin"),
            command_raw_output(stock, repo.path(), args, "stock Git"),
            "raw history tuple: {args:?}"
        );
    }
}

#[test]
fn root_patch_null_side_uses_default_abbrev_width_sha1_and_sha256() {
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    for sha256 in [false, true] {
        let repo = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        configure_identity(repo.path());
        write_file(repo.path(), "root.txt", "root\n");
        git(repo.path(), ["add", "-A"]);
        git_with_env(repo.path(), ["commit", "-m", "root"]);

        let args = ["diff-tree", "--root", "-p", "--no-commit-id", "HEAD"];
        let zmin = command_raw_output(zmin_bin(), repo.path(), &args, "zmin");
        let expected = command_raw_output(stock, repo.path(), &args, "stock Git");
        assert_eq!(zmin, expected, "root patch tuple: sha256={sha256}");
        let ids = patch_index_ids(&expected.stdout);
        assert_eq!(ids.old.len(), 7, "root null width: sha256={sha256}");
        assert_eq!(ids.new.len(), 7, "root new width: sha256={sha256}");
    }
}

#[test]
fn sha256_diff_and_diff_tree_abbrev_match_pinned_git() {
    let repo = pinned_git_init_sha256();
    pinned_git_args(repo.path(), &["config", "user.name", "Bench"]);
    pinned_git_args(repo.path(), &["config", "user.email", "bench@example.test"]);
    write_file(repo.path(), "a.txt", "one\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_git_with_env(
        repo.path(),
        &["commit", "-m", "base"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
    );
    write_file(repo.path(), "a.txt", "two\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_git_with_env(
        repo.path(),
        &["commit", "-m", "second"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000001 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000001 +0000"),
        ],
    );
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    for args in [
        ["diff", "--raw", "--no-abbrev", "HEAD~1", "HEAD"].as_slice(),
        [
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--raw",
            "--no-abbrev",
            "HEAD",
        ]
        .as_slice(),
        [
            "-c",
            "core.abbrev=12",
            "diff",
            "--raw",
            "--abbrev",
            "HEAD~1",
            "HEAD",
        ]
        .as_slice(),
        [
            "-c",
            "core.abbrev=no",
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--raw",
            "--abbrev",
            "HEAD",
        ]
        .as_slice(),
        [
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--raw",
            "--abbrev=40",
            "HEAD",
        ]
        .as_slice(),
        [
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--raw",
            "--abbrev=63",
            "HEAD",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_raw_output(zmin_bin(), repo.path(), args, "zmin"),
            command_raw_output(stock, repo.path(), args, "stock Git"),
            "SHA-256 diff tuple: {args:?}"
        );
    }
}

#[test]
fn sha256_no_index_raw_abbrev_matches_pinned_git() {
    let repo = pinned_git_init_sha256();
    write_file(repo.path(), "left.txt", "left\n");
    write_file(repo.path(), "right.txt", "right\n");
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    for args in [
        [
            "diff",
            "--no-index",
            "--raw",
            "--abbrev=4",
            "left.txt",
            "right.txt",
        ]
        .as_slice(),
        [
            "diff",
            "--no-index",
            "--raw",
            "--no-abbrev",
            "left.txt",
            "right.txt",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_raw_output(zmin_bin(), repo.path(), args, "zmin"),
            command_raw_output(stock, repo.path(), args, "stock Git"),
            "SHA-256 no-index tuple: {args:?}"
        );
    }
}

#[test]
fn sha1_no_index_patch_abbrev_matches_pinned_git() {
    let repo = git_init();
    write_file(repo.path(), "left.txt", "left\n");
    write_file(repo.path(), "right.txt", "right\n");
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    for args in [
        ["diff", "--no-index", "--patch", "left.txt", "right.txt"].as_slice(),
        [
            "diff",
            "--no-index",
            "--patch",
            "--abbrev=12",
            "left.txt",
            "right.txt",
        ]
        .as_slice(),
        [
            "-c",
            "core.abbrev=12",
            "diff",
            "--no-index",
            "--patch",
            "left.txt",
            "right.txt",
        ]
        .as_slice(),
        [
            "-c",
            "core.abbrev=no",
            "diff",
            "--no-index",
            "--patch",
            "left.txt",
            "right.txt",
        ]
        .as_slice(),
        [
            "diff",
            "--no-index",
            "--patch",
            "--no-abbrev",
            "left.txt",
            "right.txt",
        ]
        .as_slice(),
        [
            "diff",
            "--no-index",
            "--patch",
            "--full-index",
            "left.txt",
            "right.txt",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_raw_output(zmin_bin(), repo.path(), args, "zmin"),
            command_raw_output(stock, repo.path(), args, "stock Git"),
            "SHA-1 no-index patch tuple: {args:?}"
        );
    }
}

#[test]
fn sha256_no_index_patch_abbrev_matches_pinned_git() {
    let repo = pinned_git_init_sha256();
    write_file(repo.path(), "left.txt", "left\n");
    write_file(repo.path(), "right.txt", "right\n");
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    for args in [
        ["diff", "--no-index", "--patch", "left.txt", "right.txt"].as_slice(),
        [
            "diff",
            "--no-index",
            "--patch",
            "--abbrev=12",
            "left.txt",
            "right.txt",
        ]
        .as_slice(),
        [
            "-c",
            "core.abbrev=12",
            "diff",
            "--no-index",
            "--patch",
            "left.txt",
            "right.txt",
        ]
        .as_slice(),
        [
            "-c",
            "core.abbrev=no",
            "diff",
            "--no-index",
            "--patch",
            "left.txt",
            "right.txt",
        ]
        .as_slice(),
        [
            "diff",
            "--no-index",
            "--patch",
            "--no-abbrev",
            "left.txt",
            "right.txt",
        ]
        .as_slice(),
        [
            "diff",
            "--no-index",
            "--patch",
            "--full-index",
            "left.txt",
            "right.txt",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_raw_output(zmin_bin(), repo.path(), args, "zmin"),
            command_raw_output(stock, repo.path(), args, "stock Git"),
            "SHA-256 no-index patch tuple: {args:?}"
        );
    }
}

#[test]
fn no_index_patch_no_abbrev_keeps_fixed_seed_for_sha1_sha256_collisions() {
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    for use_sha256 in [false, true] {
        let (repo, algorithm) = if use_sha256 {
            (pinned_git_init_sha256(), GitHashAlgorithm::Sha256)
        } else {
            (git_init(), GitHashAlgorithm::Sha1)
        };
        let collision = no_index_blob_collision(algorithm);
        write_file(
            repo.path(),
            "old",
            std::str::from_utf8(&collision.old_content).expect("collision fixture utf8"),
        );
        write_file(
            repo.path(),
            "new",
            std::str::from_utf8(&collision.new_content).expect("collision fixture utf8"),
        );
        let no_abbrev = ["diff", "--no-index", "--patch", "--no-abbrev", "old", "new"];
        let zmin = command_raw_output(zmin_bin(), repo.path(), &no_abbrev, "zmin");
        let stock_result = command_raw_output(stock, repo.path(), &no_abbrev, "stock Git");
        assert_eq!(zmin, stock_result, "fixed no-abbrev tuple: {algorithm:?}");
        let ids = patch_index_ids(&stock_result.stdout);
        assert_eq!(ids.old.len(), 7, "old fixed width: {algorithm:?}");
        assert_eq!(ids.new.len(), 7, "new fixed width: {algorithm:?}");
        assert_eq!(
            ids.old, collision.prefix,
            "old collision prefix: {algorithm:?}"
        );
        assert_eq!(
            ids.new, collision.prefix,
            "new collision prefix: {algorithm:?}"
        );

        let full_index = [
            "diff",
            "--no-index",
            "--patch",
            "--no-abbrev",
            "--full-index",
            "old",
            "new",
        ];
        let zmin = command_raw_output(zmin_bin(), repo.path(), &full_index, "zmin");
        let stock_result = command_raw_output(stock, repo.path(), &full_index, "stock Git");
        assert_eq!(
            zmin, stock_result,
            "full-index precedence tuple: {algorithm:?}"
        );
        let ids = patch_index_ids(&stock_result.stdout);
        let full_width = match algorithm {
            GitHashAlgorithm::Sha1 => 40,
            GitHashAlgorithm::Sha256 => 64,
        };
        assert_eq!(ids.old.len(), full_width, "old full width: {algorithm:?}");
        assert_eq!(ids.new.len(), full_width, "new full width: {algorithm:?}");
    }
}

#[test]
fn sha256_no_index_binary_add_delete_patch_matches_pinned_git() {
    let repo = pinned_git_init_sha256();
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    fs::write(repo.path().join("added.bin"), b"new\0payload\n").expect("write added binary");
    fs::write(repo.path().join("deleted.bin"), b"old\0payload\n").expect("write deleted binary");

    for args in [
        [
            "diff",
            "--no-index",
            "--binary",
            "--no-abbrev",
            "/dev/null",
            "added.bin",
        ]
        .as_slice(),
        [
            "diff",
            "--no-index",
            "--binary",
            "--full-index",
            "/dev/null",
            "added.bin",
        ]
        .as_slice(),
        [
            "diff",
            "--no-index",
            "--binary",
            "--no-abbrev",
            "deleted.bin",
            "/dev/null",
        ]
        .as_slice(),
        [
            "diff",
            "--no-index",
            "--binary",
            "--full-index",
            "deleted.bin",
            "/dev/null",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_raw_output(zmin_bin(), repo.path(), args, "zmin"),
            command_raw_output(stock, repo.path(), args, "stock Git"),
            "SHA-256 no-index binary tuple: {args:?}"
        );
    }
}

#[test]
fn diff_stat_scales_graph_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    let big = (0..643).map(|i| format!("big-{i}\n")).collect::<String>();
    let mid = (0..460).map(|i| format!("mid-{i}\n")).collect::<String>();
    write_file(repo.path(), "big.txt", "");
    write_file(repo.path(), "mid.txt", "");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    write_file(repo.path(), "big.txt", &big);
    write_file(repo.path(), "mid.txt", &mid);
    git(repo.path(), ["add", "-A"]);

    for args in [["diff", "--cached", "--stat"].as_slice()] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "stat graph scaling should match stock Git for {args:?}",
        );
    }

    git_with_env(repo.path(), ["commit", "-m", "scaled stat"]);
    let show_args = ["show", "HEAD", "--format=raw", "--stat"];
    assert_eq!(
        run_zmin_args(repo.path(), &show_args),
        git_args(repo.path(), &show_args),
        "stat graph scaling should match stock Git for {show_args:?}",
    );
}

#[test]
fn diff_mixed_tree_and_blob_operands_match_stock_git_usage() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    let blob = git_with_stdin(repo.path(), ["hash-object", "-w", "--stdin"], "two\n");

    for args in [
        ["diff", "HEAD^{tree}", blob.as_str()].as_slice(),
        ["diff", blob.as_str(), "HEAD^{tree}"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "mixed tree/blob diff operands should match stock Git usage for {args:?}",
        );
    }
}

#[test]
fn diff_tree_combined_raw_for_merge_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("a.txt"), b"base\n").expect("write base");
    fs::write(repo.path().join("dir/sub"), b"base\n").expect("write base dir");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "side"]);
    fs::write(repo.path().join("a.txt"), b"side\n").expect("write side");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "side"]);
    let side = git(repo.path(), ["rev-parse", "HEAD"]);

    git(repo.path(), ["checkout", "main"]);
    fs::write(repo.path().join("a.txt"), b"main\n").expect("write main");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);
    let main = git(repo.path(), ["rev-parse", "HEAD"]);

    fs::write(repo.path().join("a.txt"), b"merge result\n").expect("write merge result");
    git(repo.path(), ["add", "-A"]);
    let tree = git(repo.path(), ["write-tree"]);
    let commit_tree_args = [
        "commit-tree",
        tree.as_str(),
        "-p",
        main.as_str(),
        "-p",
        side.as_str(),
    ];
    let (status, merge, stderr) =
        command_any_output_with_stdin("git", repo.path(), &commit_tree_args, "merge\n", "git");
    assert_eq!(status, 0, "commit-tree stderr: {stderr}");
    git(
        repo.path(),
        ["update-ref", "refs/heads/main", merge.as_str()],
    );

    for args in [
        ["diff", "main", "main^", "side"].as_slice(),
        ["diff", "--line-prefix=abc", "main", "main^", "side"].as_slice(),
        ["diff-tree", "-c", "main"].as_slice(),
        ["diff-tree", "-c", "--abbrev", "main"].as_slice(),
        ["diff-tree", "-c", "--raw", "--abbrev=7", "main"].as_slice(),
        ["diff-tree", "--cc", "--raw", "--abbrev=7", "main"].as_slice(),
        ["diff-tree", "--cc", "main"].as_slice(),
        ["diff-tree", "-c", "--stat", "main"].as_slice(),
        ["diff-tree", "--cc", "--stat", "main"].as_slice(),
        ["diff-tree", "--cc", "--patch-with-stat", "main"].as_slice(),
        [
            "diff-tree",
            "--cc",
            "--patch-with-stat",
            "--summary",
            "side",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_tree_remaining_documented_header_and_common_diff_flags_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    git(repo.path(), ["notes", "add", "-m", "note text", "HEAD"]);
    let blob = git(repo.path(), ["rev-parse", "HEAD:a.txt"]);

    for args in [
        ["diff-tree", "--abbrev-commit", "HEAD"].as_slice(),
        ["diff-tree", "--no-abbrev-commit", "HEAD"].as_slice(),
        ["diff-tree", "--oneline", "HEAD"].as_slice(),
        ["diff-tree", "--encoding=UTF-8", "HEAD"].as_slice(),
        ["diff-tree", "--expand-tabs", "HEAD"].as_slice(),
        ["diff-tree", "--no-expand-tabs", "HEAD"].as_slice(),
        ["diff-tree", "--show-notes", "HEAD"].as_slice(),
        ["diff-tree", "--show-notes-by-default", "HEAD"].as_slice(),
        ["diff-tree", "--standard-notes", "HEAD"].as_slice(),
        ["diff-tree", "--no-standard-notes", "HEAD"].as_slice(),
        ["diff-tree", "--show-signature", "HEAD"].as_slice(),
        ["diff-tree", "--no-commit-id", "HEAD"].as_slice(),
        ["diff-tree", "-v", "HEAD"].as_slice(),
        ["diff-tree", "--combined-all-paths", "-c", "HEAD"].as_slice(),
        ["diff-tree", "--dd", "-c", "HEAD"].as_slice(),
        ["diff-tree", "--diff-merges=combined", "HEAD"].as_slice(),
        ["diff-tree", "--function-context", "HEAD"].as_slice(),
        ["diff-tree", "-W", "HEAD"].as_slice(),
        ["diff-tree", "--remerge-diff", "HEAD"].as_slice(),
        ["diff-tree", "--ws-error-highlight=old,new,context", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "diff-tree parity mismatch for {args:?}",
        );
    }

    let find_object_args = vec!["diff-tree", "--find-object", blob.trim(), "HEAD"];
    let find_object_refs = find_object_args.iter().copied().collect::<Vec<_>>();
    assert_eq!(
        command_output(zmin_bin(), repo.path(), &find_object_refs),
        command_output("git", repo.path(), &find_object_refs),
        "diff-tree parity mismatch for --find-object",
    );
}

#[test]
fn diff_and_diff_files_remaining_documented_common_flags_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    write_file(repo.path(), "a.txt", "two\n");
    write_file(repo.path(), "b.txt", "staged\n");
    git(repo.path(), ["add", "b.txt"]);
    let blob = git(repo.path(), ["rev-parse", "HEAD:a.txt"]);

    for args in [
        ["diff", "--function-context"].as_slice(),
        ["diff", "-W"].as_slice(),
        ["diff", "--remerge-diff"].as_slice(),
        ["diff", "--ws-error-highlight=old,new,context"].as_slice(),
        ["diff", "--diff-merges=combined"].as_slice(),
        ["diff", "--dd"].as_slice(),
        ["diff", "--no-diff-merges"].as_slice(),
        ["diff", "-0"].as_slice(),
        ["diff", "--ita-invisible-in-index", "--cached"].as_slice(),
        ["diff", "--combined-all-paths"].as_slice(),
        ["diff-files", "--function-context"].as_slice(),
        ["diff-files", "-W"].as_slice(),
        ["diff-files", "--remerge-diff"].as_slice(),
        ["diff-files", "--ws-error-highlight=old,new,context"].as_slice(),
        ["diff-files", "--diff-merges=combined"].as_slice(),
        ["diff-files", "--dd"].as_slice(),
        ["diff-files", "--check"].as_slice(),
        ["diff-files", "--ita-invisible-in-index"].as_slice(),
        ["diff-files", "--no-diff-merges"].as_slice(),
        ["diff-files", "--combined-all-paths"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "diff-family parity mismatch for {args:?}",
        );
    }

    for args in [
        vec!["diff", "--find-object", blob.trim()],
        vec!["diff-files", "--find-object", blob.trim()],
    ] {
        let refs = args.iter().copied().collect::<Vec<_>>();
        assert_eq!(
            command_output(zmin_bin(), repo.path(), &refs),
            command_output("git", repo.path(), &refs),
            "diff-family parity mismatch for {refs:?}",
        );
    }
}

#[test]
fn diff_index_remaining_documented_common_flags_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    write_file(repo.path(), "a.txt", "two\n");
    write_file(repo.path(), "b.txt", "staged\n");
    git(repo.path(), ["add", "b.txt"]);

    for args in [
        ["diff-index", "--function-context", "HEAD"].as_slice(),
        ["diff-index", "-W", "HEAD"].as_slice(),
        ["diff-index", "--remerge-diff", "HEAD"].as_slice(),
        ["diff-index", "--ws-error-highlight=old,new,context", "HEAD"].as_slice(),
        ["diff-index", "--dd", "HEAD"].as_slice(),
        ["diff-index", "--check", "HEAD"].as_slice(),
        ["diff-index", "--ita-invisible-in-index", "HEAD"].as_slice(),
        ["diff-index", "--merge-base", "HEAD"].as_slice(),
        ["diff-index", "--no-diff-merges", "HEAD"].as_slice(),
        ["diff-index", "--combined-all-paths", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "diff-index parity mismatch for {args:?}",
        );
    }
}

#[test]
fn diff_family_final_documented_tail_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    git(repo.path(), ["notes", "add", "-m", "note text", "HEAD"]);

    write_file(repo.path(), "a.txt", "three\n");
    write_file(repo.path(), "b.txt", "staged\n");
    git(repo.path(), ["add", "b.txt"]);
    let blob = git(repo.path(), ["rev-parse", "HEAD:a.txt"]);

    for args in [
        ["diff", "--no-stat"].as_slice(),
        ["diff-files", "--no-stat"].as_slice(),
        ["diff-index", "--no-stat", "HEAD"].as_slice(),
        ["diff-tree", "--no-stat", "HEAD"].as_slice(),
        ["diff-tree", "--check", "HEAD"].as_slice(),
        ["diff-tree", "--ita-invisible-in-index", "HEAD"].as_slice(),
        ["diff-tree", "--merge-base", "HEAD", "HEAD"].as_slice(),
        ["diff-tree", "--no-diff-merges", "HEAD"].as_slice(),
        ["diff-tree", "--no-notes", "--notes", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "diff-family final-tail mismatch for {args:?}",
        );
    }

    let find_object_args = vec!["diff-index", "--find-object", blob.trim(), "HEAD"];
    let find_object_refs = find_object_args.iter().copied().collect::<Vec<_>>();
    assert_eq!(
        command_output(zmin_bin(), repo.path(), &find_object_refs),
        command_output("git", repo.path(), &find_object_refs),
        "diff-family final-tail mismatch for --find-object",
    );
}

#[test]
fn diff_index_combined_tail_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "f.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "side"]);
    write_file(repo.path(), "f.txt", "side\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "side"]);

    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "f.txt", "main\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);

    let _ = git_status(repo.path(), ["merge", "side"]);

    for args in [
        ["diff-index", "-c", "HEAD"].as_slice(),
        ["diff-index", "--cc", "HEAD"].as_slice(),
        ["diff-index", "--diff-merges=combined", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "diff-index combined tail mismatch for {args:?}",
        );
    }
}

#[test]
fn diff_noop_option_surface_matches_stock_git_for_porcelain_and_plumbing() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"one\n").expect("write base");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    fs::write(repo.path().join("a.txt"), b"two\n").expect("write second");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);

    fs::write(repo.path().join("a.txt"), b"three\n").expect("write dirty");

    for args in [
        ["diff", "--rename-empty"].as_slice(),
        ["diff", "--no-rename-empty"].as_slice(),
        ["diff", "-l1"].as_slice(),
        ["diff", "-m"].as_slice(),
        ["diff", "-t"].as_slice(),
        ["diff-files", "--no-renames"].as_slice(),
        ["diff-files", "-l1"].as_slice(),
        ["diff-files", "-t"].as_slice(),
        ["diff-index", "--no-renames", "HEAD"].as_slice(),
        ["diff-index", "-l1", "HEAD"].as_slice(),
        ["diff-index", "-t", "HEAD"].as_slice(),
        ["diff-tree", "--no-renames", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-l1", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-t", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "--always", "HEAD~1", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_unmerged_stage_selectors_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"base\n").expect("write base");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "side"]);
    fs::write(repo.path().join("a.txt"), b"side\n").expect("write side");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "side"]);

    git(repo.path(), ["checkout", "main"]);
    fs::write(repo.path().join("a.txt"), b"main\n").expect("write main");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);

    let _ = git_status(repo.path(), ["merge", "side"]);

    for args in [
        ["diff", "--base"].as_slice(),
        ["diff", "--ours"].as_slice(),
        ["diff", "--theirs"].as_slice(),
        ["diff", "-1"].as_slice(),
        ["diff", "-2"].as_slice(),
        ["diff", "-3"].as_slice(),
        ["diff-files", "--base"].as_slice(),
        ["diff-files", "--ours"].as_slice(),
        ["diff-files", "--theirs"].as_slice(),
        ["diff-files", "-0"].as_slice(),
        ["diff-files", "-1"].as_slice(),
        ["diff-files", "-2"].as_slice(),
        ["diff-files", "-3"].as_slice(),
        ["diff-files", "--cc"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_unmerged_stage_selectors_reject_commit_operands_like_stock_git() {
    let repo = two_commit_repo();
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["diff", "--ours", "HEAD~1", "HEAD"]),
        git_failure_output(repo.path(), &["diff", "--ours", "HEAD~1", "HEAD"])
    );
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["diff", "--cached", "--base"]),
        git_failure_output(repo.path(), &["diff", "--cached", "--base"])
    );
}

#[test]
fn log_merge_diff_modes_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"base\n").expect("write base");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "side"]);
    fs::write(repo.path().join("a.txt"), b"side\n").expect("write side");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "side"]);
    let side = git(repo.path(), ["rev-parse", "HEAD"]);

    git(repo.path(), ["checkout", "main"]);
    fs::write(repo.path().join("a.txt"), b"main\n").expect("write main");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);
    let main = git(repo.path(), ["rev-parse", "HEAD"]);

    fs::write(repo.path().join("a.txt"), b"merge result\n").expect("write merge result");
    git(repo.path(), ["add", "-A"]);
    let tree = git(repo.path(), ["write-tree"]);
    let commit_tree_args = [
        "commit-tree",
        tree.as_str(),
        "-p",
        main.as_str(),
        "-p",
        side.as_str(),
    ];
    let (status, merge, stderr) =
        command_any_output_with_stdin("git", repo.path(), &commit_tree_args, "merge\n", "git");
    assert_eq!(status, 0, "commit-tree stderr: {stderr}");
    git(
        repo.path(),
        ["update-ref", "refs/heads/main", merge.as_str()],
    );

    for args in [
        ["log", "-m", "main"].as_slice(),
        [
            "log",
            "--no-diff-merges",
            "-p",
            "--first-parent",
            "-n",
            "2",
            "main",
        ]
        .as_slice(),
        [
            "log",
            "--diff-merges=off",
            "-p",
            "--first-parent",
            "-n",
            "2",
            "main",
        ]
        .as_slice(),
        [
            "log",
            "--first-parent",
            "--diff-merges=off",
            "-p",
            "-n",
            "2",
            "main",
        ]
        .as_slice(),
        ["log", "-p", "--first-parent", "-n", "2", "main"].as_slice(),
        ["log", "-p", "--diff-merges=first-parent", "-n", "3", "main"].as_slice(),
        ["log", "--diff-merges=first-parent", "-n", "3", "main"].as_slice(),
        ["log", "--dd", "-n", "3", "main"].as_slice(),
        ["log", "-m", "-p", "--first-parent", "-n", "2", "main"].as_slice(),
        ["log", "-m", "-p", "-n", "3", "main"].as_slice(),
        ["log", "--cc", "-m", "-p", "-n", "3", "main"].as_slice(),
        ["log", "-c", "-m", "-p", "-n", "3", "main"].as_slice(),
        ["log", "-p", "--diff-merges=separate", "-n", "3", "main"].as_slice(),
        ["log", "-p", "--diff-merges=on", "-n", "3", "main"].as_slice(),
        ["log", "-p", "--diff-merges=1", "-n", "3", "main"].as_slice(),
        ["log", "-p", "--diff-merges=combined", "-n", "3", "main"].as_slice(),
        [
            "log",
            "-p",
            "--diff-merges=dense-combined",
            "-n",
            "3",
            "main",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    git(repo.path(), ["config", "log.diffMerges", "1"]);
    for args in [
        ["log", "-p", "-n", "3", "main"].as_slice(),
        ["log", "-n", "1", "main"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    git(repo.path(), ["config", "--unset", "log.diffMerges"]);
    git(repo.path(), ["config", "log.diffMerges", "first-parent"]);
    assert_eq!(
        run_zmin_args(
            repo.path(),
            &["log", "-p", "--diff-merges=on", "-n", "3", "main"]
        ),
        git_args(
            repo.path(),
            &["log", "-p", "--diff-merges=on", "-n", "3", "main"]
        )
    );
    assert_eq!(
        run_zmin_args(repo.path(), &["log", "-p", "-m", "-n", "3", "main"]),
        git_args(repo.path(), &["log", "-p", "-m", "-n", "3", "main"])
    );
    git(repo.path(), ["config", "--unset", "log.diffMerges"]);
    git(repo.path(), ["config", "log.diffMerges", "wrong-value"]);
    let zmin = run_zmin_failure_output(repo.path(), &["log"]);
    let git = git_failure_output(repo.path(), &["log"]);
    assert_eq!(zmin.0, git.0);
    assert!(zmin.1.is_empty());
    assert!(zmin.2.contains("log.diffMerges"));
    assert!(git.2.contains("log.diff"));
}

#[test]
fn log_decorate_and_notes_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"base\n").expect("write base");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["branch", "side"]);
    git(repo.path(), ["notes", "add", "-m", "note"]);

    for args in [
        ["log", "--decorate", "--all"].as_slice(),
        ["log", "--decorate=true", "--all"].as_slice(),
        ["log", "--decorate=full", "--all"].as_slice(),
        ["log", "--decorate", "--clear-decorations", "--all"].as_slice(),
        ["log", "--decorate=full", "--clear-decorations", "--all"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn log_decorate_full_orders_same_target_local_branches_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"base\n").expect("write base");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["branch", "oldbase"]);
    git(repo.path(), ["branch", "keep-base"]);

    assert_eq!(
        run_zmin_args(repo.path(), &["log", "--decorate=full", "--all"]),
        common::git_args(repo.path(), &["log", "--decorate=full", "--all"])
    );
}

#[test]
fn diff_tree_pretty_notes_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"base\n").expect("write base");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    fs::write(repo.path().join("a.txt"), b"update\n").expect("write update");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "update"]);
    git(repo.path(), ["notes", "add", "-m", "note"]);

    for args in [
        ["diff-tree", "--pretty", "HEAD"].as_slice(),
        ["diff-tree", "--pretty", "--notes", "HEAD"].as_slice(),
        ["diff-tree", "--format=%N", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_tree_stdin_log_format_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"base\n").expect("write base");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "side"]);
    fs::write(repo.path().join("side.txt"), b"side\n").expect("write side");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "side"]);

    git(repo.path(), ["checkout", "main"]);
    fs::create_dir_all(repo.path().join("dir")).expect("create main dir");
    fs::write(repo.path().join("dir/sub"), b"main\n").expect("write main dir");
    fs::write(repo.path().join("main.txt"), b"main\n").expect("write main");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);
    git(repo.path(), ["merge", "--no-ff", "side", "-m", "merge"]);
    let revs = git(repo.path(), ["rev-list", "main"]);
    let args = ["diff-tree", "--stdin", "--format=%s", "-s"];

    assert_eq!(
        command_any_output_with_stdin(zmin_bin(), repo.path(), &args, &revs, "zmin"),
        command_any_output_with_stdin("git", repo.path(), &args, &revs, "git")
    );

    let revs = git(repo.path(), ["rev-list", "main^"]);
    let args = [
        "diff-tree",
        "-r",
        "--stdin",
        "--name-only",
        "--format=%s",
        "dir",
    ];
    assert_eq!(
        command_any_output_with_stdin(zmin_bin(), repo.path(), &args, &revs, "zmin"),
        command_any_output_with_stdin("git", repo.path(), &args, &revs, "git")
    );
}

#[test]
fn diff_tree_format_abbrev_policy_is_validated_before_stdin() {
    let stock = pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    let repo = two_commit_repo();
    let head = git(repo.path(), ["rev-parse", "HEAD"]);

    for stdin in [false, true] {
        for request_abbrev in [false, true] {
            for value in ["12", "4", "no"] {
                let config = format!("core.abbrev={value}");
                let abbrev_args: &[&str] = if request_abbrev { &["--abbrev"] } else { &[] };
                let (zmin, expected) = diff_tree_abbrev_tuple(
                    zmin_bin(),
                    stock,
                    repo.path(),
                    Some(&config),
                    stdin,
                    &abbrev_args,
                    &head,
                );
                assert_eq!(
                    zmin, expected,
                    "core.abbrev={value}, stdin={stdin}, request_abbrev={request_abbrev}"
                );
                assert_eq!(zmin.status, 0, "core.abbrev={value}, stdin={stdin}");
                let width = zmin
                    .stdout
                    .split(|byte| *byte == b'\n')
                    .next()
                    .expect("formatted diff-tree output")
                    .iter()
                    .copied()
                    .filter(|byte| *byte != b'\r')
                    .count();
                assert_eq!(
                    width,
                    if !request_abbrev || value == "no" {
                        40
                    } else {
                        value.parse().unwrap()
                    },
                    "core.abbrev={value}, stdin={stdin}, request_abbrev={request_abbrev}"
                );
            }
        }

        for abbrev_args in [
            &[][..],
            &["--abbrev"][..],
            &["--abbrev=7"][..],
            &["--abbrev=12"][..],
            &["--abbrev=4"][..],
            &["--abbrev=0"][..],
            &["--abbrev=+1"][..],
            &["--abbrev=-1"][..],
            &["--abbrev=12junk"][..],
            &["--abbrev=999999999999999999999999"][..],
            &["--no-abbrev"][..],
            &["--no-abbrev", "--abbrev=12"][..],
            &["--abbrev=12", "--no-abbrev"][..],
            &["--abbrev=4", "--abbrev=12"][..],
        ] {
            let (zmin, expected) = diff_tree_abbrev_tuple(
                zmin_bin(),
                stock,
                repo.path(),
                None,
                stdin,
                abbrev_args,
                &head,
            );
            assert_eq!(zmin, expected, "abbrev args={abbrev_args:?}, stdin={stdin}");
        }

        let ulong_max = if std::mem::size_of::<std::os::raw::c_ulong>() == 4 {
            u32::MAX as u128
        } else {
            u64::MAX as u128
        };
        for value in [ulong_max, ulong_max + 1] {
            let argument = format!("--abbrev=-{value}");
            let abbrev_args = [argument.as_str()];
            let (zmin, expected) = diff_tree_abbrev_tuple(
                zmin_bin(),
                stock,
                repo.path(),
                None,
                stdin,
                &abbrev_args,
                &head,
            );
            assert_eq!(
                zmin, expected,
                "negative c_ulong boundary={value}, stdin={stdin}"
            );
        }

        for (config, abbrev_args) in [
            ("core.abbrev= 12", &["--abbrev"][..]),
            ("core.abbrev=+12", &[][..]),
            ("core.abbrev=\t12", &["--abbrev"][..]),
        ] {
            let (zmin, expected) = diff_tree_abbrev_tuple(
                zmin_bin(),
                stock,
                repo.path(),
                Some(config),
                stdin,
                abbrev_args,
                &head,
            );
            assert_eq!(zmin, expected, "config={config:?}, stdin={stdin}");
        }

        for request_abbrev in [false, true] {
            for configs in [
                ("core.abbrev=bogus", "core.abbrev=12"),
                ("core.abbrev=12", "core.abbrev=bogus"),
            ] {
                let mut args = vec!["-c", configs.0, "-c", configs.1, "diff-tree"];
                if stdin {
                    args.extend(["--stdin", "--format=%h"]);
                } else {
                    args.extend(["--format=%h"]);
                }
                if request_abbrev {
                    args.push("--abbrev");
                }
                if stdin {
                    args.push("-s");
                } else {
                    args.extend(["-s", "HEAD"]);
                }
                let zmin = if stdin {
                    command_raw_output_with_stdin(
                        zmin_bin(),
                        repo.path(),
                        &args,
                        format!("{head}\n").as_bytes(),
                        "zmin",
                    )
                } else {
                    command_raw_output(zmin_bin(), repo.path(), &args, "zmin")
                };
                let expected = if stdin {
                    command_raw_output_with_stdin(
                        stock,
                        repo.path(),
                        &args,
                        format!("{head}\n").as_bytes(),
                        "stock Git",
                    )
                } else {
                    command_raw_output(stock, repo.path(), &args, "stock Git")
                };
                assert_eq!(zmin, expected, "configs={configs:?}, stdin={stdin}");
                assert_ne!(zmin.status, 0, "configs={configs:?}, stdin={stdin}");
            }
        }
    }
}

#[test]
fn rev_list_children_matches_stock_git_for_merge_graph() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"base\n").expect("write base");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "side"]);
    fs::write(repo.path().join("side.txt"), b"side\n").expect("write side");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "side"]);

    git(repo.path(), ["checkout", "main"]);
    fs::write(repo.path().join("main.txt"), b"main\n").expect("write main");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);
    git(repo.path(), ["merge", "--no-ff", "side", "-m", "merge"]);

    for args in [
        ["rev-list", "--children", "HEAD"].as_slice(),
        ["rev-list", "--children", "--reverse", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn show_patch_with_stat_matches_stock_git() {
    let repo = two_commit_repo();

    for args in [
        ["show", "--patch-with-stat", "HEAD"].as_slice(),
        ["show", "--patch-with-stat", "--summary", "HEAD"].as_slice(),
        ["show", "--patch-with-raw", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn show_raw_abbrev_controls_match_stock_git() {
    let repo = two_commit_repo();

    for args in [
        ["show", "--raw", "--abbrev=7", "HEAD"].as_slice(),
        ["show", "--raw", "--no-abbrev", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn show_multiple_commits_with_pathspec_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("dir/sub"), b"base\n").expect("write base");
    fs::write(repo.path().join("file0"), b"base\n").expect("write file");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "side"]);
    fs::write(repo.path().join("dir/sub"), b"side\n").expect("write side");
    fs::write(repo.path().join("side.txt"), b"side\n").expect("write side file");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "side"]);

    git(repo.path(), ["checkout", "main"]);
    fs::write(repo.path().join("dir/sub"), b"main\n").expect("write main");
    fs::write(repo.path().join("main.txt"), b"main\n").expect("write main file");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);

    let args = [
        "show",
        "--name-only",
        "--format=<%s>",
        "side",
        "main",
        "--",
        "dir",
    ];
    assert_eq!(
        run_zmin_args(repo.path(), &args),
        common::git_args(repo.path(), &args)
    );
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_raw_after_branch_checkout_uses_index_blob_ids_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
    }

    write_file(git_repo.path(), "a.txt", "base\n");
    git(git_repo.path(), ["add", "a.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    git(git_repo.path(), ["branch", "initial"]);
    git(git_repo.path(), ["checkout", "-b", "side"]);
    write_file(git_repo.path(), "a.txt", "side\n");
    git(git_repo.path(), ["add", "a.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "side"]);
    git(git_repo.path(), ["checkout", "main"]);
    git(git_repo.path(), ["checkout", "side"]);

    write_file(zmin_repo.path(), "a.txt", "base\n");
    run_zmin(zmin_repo.path(), ["add", "a.txt"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "initial"]);
    run_zmin(zmin_repo.path(), ["branch", "initial"]);
    run_zmin(zmin_repo.path(), ["checkout", "-b", "side"]);
    write_file(zmin_repo.path(), "a.txt", "side\n");
    run_zmin(zmin_repo.path(), ["add", "a.txt"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "side"]);
    run_zmin(zmin_repo.path(), ["checkout", "main"]);
    run_zmin(zmin_repo.path(), ["checkout", "side"]);

    let args = ["diff", "--raw", "--no-abbrev", "initial"];
    let zmin = run_zmin(zmin_repo.path(), args);
    let git = git(git_repo.path(), args);
    assert_eq!(zmin, git);
    assert!(!zmin.contains(" 0000000000000000000000000000000000000000 M\ta.txt"));
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
            run_zmin_args(repo.path(), args),
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
        ["log", "-Sneedle", "HEAD~1..HEAD"].as_slice(),
        ["log", "-S", "needle", "HEAD~1..HEAD"].as_slice(),
        ["log", "-Gchanged", "HEAD~1..HEAD"].as_slice(),
        ["log", "-Sneedle", "-p", "HEAD~1..HEAD"].as_slice(),
        ["log", "-Gchanged", "-p", "HEAD~1..HEAD"].as_slice(),
        ["log", "-Sneedle", "HEAD", "--max-count=1"].as_slice(),
        ["log", "-Sneedle", "HEAD", "-n", "1"].as_slice(),
        ["log", "--pickaxe-all", "-Sneedle", "-p", "HEAD~1..HEAD"].as_slice(),
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
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
    let raw = Command::new(common::stock_git_bin())
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
            command_any_output_with_stdin(zmin_bin(), repo.path(), args, &raw, "zmin"),
            command_any_output_with_stdin("git", repo.path(), args, &raw, "git"),
            "args: {args:?}"
        );
    }

    let raw = Command::new(common::stock_git_bin())
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
        let output = command_any_output_with_stdin(zmin_bin(), repo.path(), args, &raw, "zmin");
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
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    assert_eq!(
        run_zmin_status(repo.path(), ["diff", "--cached", "--quiet"]),
        git_status(repo.path(), ["diff", "--cached", "--quiet"])
    );
    assert_eq!(
        run_zmin_status(
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
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    assert_eq!(
        run_zmin_status(repo.path(), ["diff", "--quiet"]),
        git_status(repo.path(), ["diff", "--quiet"])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["diff", "--name-only", "--exit-code"]),
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
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    assert_eq!(
        run_zmin_status(repo.path(), ["diff", "--quiet"]),
        git_status(repo.path(), ["diff", "--quiet"])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["diff", "--cached", "--quiet"]),
        git_status(repo.path(), ["diff", "--cached", "--quiet"])
    );
}

#[test]
fn diff_name_only_and_name_status_match_stock_git_for_cached_and_worktree() {
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
        ["diff", "--cached", "--name-status", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    fs::write(repo.path().join("a.txt"), b"unstaged\n").expect("unstaged modify a");
    fs::remove_file(repo.path().join("c.txt")).expect("unstaged remove c");
    fs::write(repo.path().join("untracked.txt"), b"ignored by diff\n").expect("write untracked");

    for args in [
        ["diff", "--name-status"].as_slice(),
        ["diff", "--name-only"].as_slice(),
        ["diff", "--name-status", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
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
        ["diff", "--cached", "--name-status", "--diff-filter=D*"].as_slice(),
        ["diff", "--cached", "--name-status", "--diff-filter=M*"].as_slice(),
        ["diff", "--cached", "--name-status", "--diff-filter=A*"].as_slice(),
        ["diff", "--cached", "--name-status", "--diff-filter=DM*"].as_slice(),
        ["diff", "--cached", "--name-status", "--diff-filter=d*"].as_slice(),
        ["diff", "--cached", "--name-status", "--diff-filter=*D"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_filter_invalid_change_classes_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"old\n").expect("write");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    fs::write(repo.path().join("a.txt"), b"new\n").expect("rewrite");
    git(repo.path(), ["add", "-A"]);

    for args in [
        ["diff", "--cached", "--name-status", "--diff-filter=Z"].as_slice(),
        ["diff", "--cached", "--name-status", "--diff-filter=@"].as_slice(),
        ["diff", "--cached", "--name-status", "--diff-filter=A@"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
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
            command_output(zmin_bin(), repo.path(), args),
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
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    let sub = repo.path().join("sub");
    let args = ["diff", "--cached", "--relative", "--name-only"];
    assert_eq!(
        command_output(zmin_bin(), &sub, &args),
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
            command_output(zmin_bin(), repo.path(), args),
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
            command_output(zmin_bin(), &subdir, args),
            command_output("git", &subdir, args),
            "subdir args: {args:?}"
        );
    }
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "relative-change"]);
    assert_eq!(
        command_output(
            zmin_bin(),
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
        ["diff", "--cached", "--patch", "--line-prefix=| | | "].as_slice(),
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
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_output_and_plumbing_line_prefix_match_stock_git() {
    let repo = two_commit_repo();

    fs::write(repo.path().join("a.txt"), b"worktree change\n").expect("modify tracked file");
    fs::write(repo.path().join("b.txt"), b"staged change\n").expect("modify second tracked file");

    for (git_args, zmin_args, git_output, zmin_output) in [
        (
            ["diff", "--output=git-diff.patch", "HEAD~1", "HEAD"].as_slice(),
            ["diff", "--output=zmin-diff.patch", "HEAD~1", "HEAD"].as_slice(),
            "git-diff.patch",
            "zmin-diff.patch",
        ),
        (
            ["diff-files", "--output", "git-diff-files.patch", "-p"].as_slice(),
            ["diff-files", "--output", "zmin-diff-files.patch", "-p"].as_slice(),
            "git-diff-files.patch",
            "zmin-diff-files.patch",
        ),
        (
            ["diff-index", "--output=git-diff-index.patch", "-p", "HEAD"].as_slice(),
            ["diff-index", "--output=zmin-diff-index.patch", "-p", "HEAD"].as_slice(),
            "git-diff-index.patch",
            "zmin-diff-index.patch",
        ),
        (
            [
                "diff-tree",
                "--output",
                "git-diff-tree.patch",
                "-p",
                "HEAD~1",
                "HEAD",
            ]
            .as_slice(),
            [
                "diff-tree",
                "--output",
                "zmin-diff-tree.patch",
                "-p",
                "HEAD~1",
                "HEAD",
            ]
            .as_slice(),
            "git-diff-tree.patch",
            "zmin-diff-tree.patch",
        ),
    ] {
        let zmin = command_output(zmin_bin(), repo.path(), zmin_args);
        let git = command_output("git", repo.path(), git_args);
        assert_eq!(zmin, git, "git args: {git_args:?}");
        assert_eq!(
            fs::read_to_string(repo.path().join(zmin_output)).expect("read zmin redirected output"),
            fs::read_to_string(repo.path().join(git_output)).expect("read git redirected output"),
            "git args: {git_args:?}"
        );
    }

    for args in [
        ["diff-files", "-p", "--line-prefix=LP "].as_slice(),
        ["diff-index", "-p", "--line-prefix=LP ", "HEAD"].as_slice(),
        ["diff-tree", "-p", "--line-prefix=LP ", "HEAD~1", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_prefix_config_matches_stock_git_for_porcelain() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("file0"), b"old\n").expect("write file");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    fs::write(repo.path().join("file0"), b"new\n").expect("rewrite file");

    for args in [
        ["-c", "diff.noPrefix", "diff"].as_slice(),
        ["-c", "diff.noPrefix=true", "diff"].as_slice(),
        ["-c", "diff.mnemonicPrefix", "diff"].as_slice(),
        ["-c", "diff.mnemonicPrefix=true", "diff"].as_slice(),
        ["-c", "diff.srcPrefix=x/", "diff"].as_slice(),
        ["-c", "diff.dstPrefix=y/", "diff"].as_slice(),
        [
            "-c",
            "diff.dstPrefix=y/",
            "-c",
            "diff.srcPrefix=x/",
            "-c",
            "diff.noPrefix=true",
            "diff",
        ]
        .as_slice(),
        [
            "-c",
            "diff.dstPrefix=x/",
            "-c",
            "diff.srcPrefix=y/",
            "-c",
            "diff.mnemonicPrefix=true",
            "diff",
        ]
        .as_slice(),
        ["-c", "diff.noPrefix=true", "diff-files", "-p"].as_slice(),
        ["diff-files", "-p", "--no-prefix"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_no_rename_abbreviation_is_rejected_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("file0"), b"old\n").expect("write file");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    fs::write(repo.path().join("file0"), b"new\n").expect("rewrite file");

    let zmin = run_zmin_failure_output(repo.path(), ["diff", "--no-rename"].as_slice());
    let git = git_failure_output(repo.path(), ["diff", "--no-rename"].as_slice());

    assert_eq!(zmin.0, git.0);
    assert_eq!(zmin.1, git.1);
    assert!(zmin.2.contains("error: invalid option: --no-rename"));
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    let invalid_regex = ["diff", "--cached", "--ignore-matching-lines=^[124-9"];
    let (status, _stdout, stderr) = command_output(&zmin_bin(), repo.path(), &invalid_regex);
    assert_eq!(status, 129);
    assert!(stderr.contains("invalid regex given to -I: "), "{stderr}");

    git_with_env(repo.path(), ["commit", "-m", "noise"]);
    let numbered = (1..=50).map(|n| format!("{n}\n")).collect::<String>();
    write_file(repo.path(), "file1", &numbered);
    git(repo.path(), ["add", "file1"]);
    let ignored_numbered = (1..=50)
        .map(|n| {
            if n == 13 {
                "ten and three\n".to_owned()
            } else if matches!(n.to_string().as_bytes()[0], b'1' | b'2' | b'4'..=b'9') {
                format!("{n} \n")
            } else {
                format!("{n}\n")
            }
        })
        .collect::<String>();
    write_file(repo.path(), "file1", &ignored_numbered);
    for args in [
        [
            "diff",
            "--raw",
            "--ignore-blank-lines",
            "-Iten.*e",
            "-I^[124-9]",
        ]
        .as_slice(),
        [
            "diff",
            "--name-only",
            "--ignore-blank-lines",
            "-Iten.*e",
            "-I^[124-9]",
        ]
        .as_slice(),
        [
            "diff",
            "--name-status",
            "--ignore-blank-lines",
            "-Iten.*e",
            "-I^[124-9]",
        ]
        .as_slice(),
    ] {
        let zmin = run_zmin_args(repo.path(), args);
        assert!(!zmin.contains("file1"), "args: {args:?}\n{zmin}");
    }
    write_file(repo.path(), "file2", "");
    git(repo.path(), ["add", "file2"]);
    fs::remove_file(repo.path().join("file2")).expect("remove file2");
    fs::create_dir(repo.path().join("file2")).expect("mkdir file2");
    for args in [
        ["diff", "--raw", "--ignore-blank-lines", "-I.*"].as_slice(),
        ["diff", "--name-only", "--ignore-blank-lines", "-I.*"].as_slice(),
        ["diff", "--name-status", "--ignore-blank-lines", "-I.*"].as_slice(),
    ] {
        let zmin = run_zmin_args(repo.path(), args);
        assert!(zmin.contains("file2"), "args: {args:?}\n{zmin}");
    }

    for args in [
        ["log", "-IDEBUG|TRACE", "-p", "HEAD~1..HEAD"].as_slice(),
        ["log", "-I", "DEBUG|TRACE", "-p", "HEAD~1..HEAD"].as_slice(),
        [
            "log",
            "--ignore-matching-lines=DEBUG|TRACE",
            "-p",
            "HEAD~1..HEAD",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_tree_rename_similarity_ignores_crlf_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "sample", "one\ntwo\nthree\n");
    git(repo.path(), ["add", "sample"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    fs::write(repo.path().join("elpmas"), b"one\r\ntwo\r\nthree\r\n").expect("write crlf copy");
    git(repo.path(), ["add", "elpmas"]);
    fs::remove_file(repo.path().join("sample")).expect("remove sample");
    git_with_env(repo.path(), ["commit", "-a", "-m", "rename with crlf"]);

    let args = ["diff-tree", "-M", "-r", "--name-status", "HEAD^", "HEAD"];
    assert_eq!(run_zmin(repo.path(), args), git(repo.path(), args));
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_color_words_and_word_diff_regex_match_stock_git_for_diff_family() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(
        repo.path().join("a.txt"),
        b"ctx1\nhello brave world\nsecond line\nctx2\n",
    )
    .expect("write base");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    fs::write(
        repo.path().join("a.txt"),
        b"ctx1\nhello bright world\nsecond line plus\nctx2\n",
    )
    .expect("write committed change");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "change"]);
    fs::write(
        repo.path().join("a.txt"),
        b"ctx1\nhello bolder world\nsecond line plus more\nctx2\n",
    )
    .expect("write worktree change");

    for args in [
        ["diff", "--color-words", "HEAD~1", "HEAD"].as_slice(),
        ["diff", "--color-words=[a-z]+", "HEAD~1", "HEAD"].as_slice(),
        [
            "diff",
            "--word-diff-regex=[a-z]+",
            "--word-diff=plain",
            "HEAD~1",
            "HEAD",
        ]
        .as_slice(),
        ["diff-files", "-p", "--color-words"].as_slice(),
        ["diff-files", "-p", "--color-words=[a-z]+"].as_slice(),
        [
            "diff-files",
            "-p",
            "--word-diff-regex=[a-z]+",
            "--word-diff=plain",
        ]
        .as_slice(),
        ["diff-index", "-p", "--color-words", "HEAD"].as_slice(),
        ["diff-index", "-p", "--color-words=[a-z]+", "HEAD"].as_slice(),
        [
            "diff-index",
            "-p",
            "--word-diff-regex=[a-z]+",
            "--word-diff=plain",
            "HEAD",
        ]
        .as_slice(),
        ["diff-tree", "-p", "--color-words", "HEAD~1", "HEAD"].as_slice(),
        ["diff-tree", "-p", "--color-words=[a-z]+", "HEAD~1", "HEAD"].as_slice(),
        [
            "diff-tree",
            "-p",
            "--word-diff-regex=[a-z]+",
            "--word-diff=plain",
            "HEAD~1",
            "HEAD",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
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
            command_output(zmin_bin(), repo.path(), args),
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
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
    let output = Command::new(common::stock_git_bin())
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
            run_zmin_args(&super_repo, &args),
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
            run_zmin_args(&super_repo, &args),
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
        command_output(zmin_bin(), repo.path(), &["diff", "--check", "--", "a.txt"]),
        command_output("git", repo.path(), &["diff", "--check", "--", "a.txt"])
    );

    fs::write(repo.path().join("a.txt"), b"dirty \nnext\t\n").expect("write whitespace");
    assert_eq!(
        command_output(zmin_bin(), repo.path(), &["diff", "--check"]),
        command_output("git", repo.path(), &["diff", "--check"])
    );
    assert_eq!(
        command_output(
            zmin_bin(),
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
                zmin_bin(),
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
            command_output(zmin_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "args: {args:?}"
        );
    }

    assert_eq!(
        command_output(
            zmin_bin(),
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
            command_output(zmin_bin(), repo.path(), args),
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
            command_output(zmin_bin(), repo.path(), args),
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
            command_output(zmin_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "args: {args:?}"
        );
    }

    fs::create_dir(repo.path().join("empty")).expect("create empty dir");
    fs::create_dir(repo.path().join("right-only")).expect("create right-only dir");
    fs::write(repo.path().join("right-only/sub.txt"), b"new\n").expect("write added only");
    for args in [
        ["diff", "--no-index", "--raw", "empty", "right-only"].as_slice(),
        [
            "diff",
            "--no-index",
            "--raw",
            "--abbrev=4",
            "empty",
            "right-only",
        ]
        .as_slice(),
        [
            "diff",
            "--no-index",
            "--raw",
            "--no-abbrev",
            "empty",
            "right-only",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), repo.path(), args),
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
        run_zmin(
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
        run_zmin(
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
    git(repo.path(), ["config", "diff.tool", "zmintest"]);
    git(repo.path(), ["config", "difftool.zmintest.cmd", command]);
    git(repo.path(), ["config", "difftool.prompt", "false"]);

    assert_eq!(
        run_zmin(repo.path(), ["difftool", "--no-prompt", "a.txt"]),
        git(repo.path(), ["difftool", "--no-prompt", "a.txt"])
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            ["difftool", "--no-prompt", "--tool=zmintest", "a.txt"]
        ),
        git(
            repo.path(),
            ["difftool", "--no-prompt", "--tool=zmintest", "a.txt"]
        )
    );
}

#[test]
#[cfg(not(windows))]
fn difftool_additional_documented_options_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "old\n");
    write_file(repo.path(), "b.txt", "oldb\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    write_file(repo.path(), "a.txt", "new\n");
    write_file(repo.path(), "b.txt", "newb\n");

    let command = "printf 'L:'; cat \"$LOCAL\"; printf 'R:'; cat \"$REMOTE\"";
    git(repo.path(), ["config", "diff.tool", "zmintest"]);
    git(repo.path(), ["config", "diff.guitool", "zmintest"]);
    git(repo.path(), ["config", "difftool.zmintest.cmd", command]);
    git(repo.path(), ["config", "difftool.prompt", "false"]);

    for args in [
        ["difftool", "--tool-help"].as_slice(),
        ["difftool", "--gui", "-y", "a.txt"].as_slice(),
        ["difftool", "--no-gui", "-y", "a.txt"].as_slice(),
        ["difftool", "-g", "-y", "a.txt"].as_slice(),
        ["difftool", "--rotate-to=b.txt", "-y", "-t", "zmintest"].as_slice(),
        ["difftool", "--skip-to=b.txt", "-y", "-t", "zmintest"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "args: {args:?}"
        );
    }

    git(
        repo.path(),
        ["config", "difftool.fail.cmd", "printf fail; exit 7"],
    );
    for args in [
        ["difftool", "-y", "-t", "fail", "a.txt"].as_slice(),
        ["difftool", "-y", "-t", "fail", "--trust-exit-code", "a.txt"].as_slice(),
        [
            "difftool",
            "-y",
            "-t",
            "fail",
            "--no-trust-exit-code",
            "a.txt",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), repo.path(), args),
            command_output("git", repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
#[cfg(not(windows))]
fn difftool_tool_help_does_not_depend_on_stock_git_runtime() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "old\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["config", "difftool.zmintest.cmd", "printf x"]);

    let expected = command_output("git", repo.path(), &["difftool", "--tool-help"]);
    let actual = command_output_with_env(
        zmin_bin(),
        repo.path(),
        &["difftool", "--tool-help"],
        &[
            ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
            ("GIT_BIN", "/definitely/missing/git"),
        ],
        "zmin difftool --tool-help",
    );
    assert_eq!(actual, expected);
}

#[test]
#[cfg(not(windows))]
fn difftool_dir_diff_and_symlink_modes_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "a.txt", "old\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
        write_file(repo, "a.txt", "new\n");
        let command = "printf 'L:%s\\nR:%s\\n' \"$LOCAL\" \"$REMOTE\"; test -L \"$REMOTE/a.txt\" && echo SYMLINK || echo COPY; cat \"$REMOTE/a.txt\"";
        git(repo, ["config", "diff.tool", "zmintest"]);
        git(repo, ["config", "difftool.zmintest.cmd", command]);
        git(repo, ["config", "difftool.prompt", "false"]);
    }

    for args in [
        ["difftool", "--dir-diff", "-y", "-t", "zmintest"].as_slice(),
        ["difftool", "-d", "-y", "-t", "zmintest"].as_slice(),
        [
            "difftool",
            "--symlinks",
            "--dir-diff",
            "-y",
            "-t",
            "zmintest",
        ]
        .as_slice(),
        [
            "difftool",
            "--no-symlinks",
            "--dir-diff",
            "-y",
            "-t",
            "zmintest",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            normalize_difftool_dir_output(command_output(zmin_bin(), zmin_repo.path(), args)),
            normalize_difftool_dir_output(command_output("git", git_repo.path(), args)),
            "args: {args:?}"
        );
    }
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn diff_raw_treeish_to_worktree_hashes_worktree_objects_like_stock_git() {
    let repo = two_commit_repo();
    fs::write(repo.path().join("a.txt"), b"staged\n").expect("modify staged file");
    git(repo.path(), ["add", "a.txt"]);
    fs::write(repo.path().join("c.txt"), b"added\n").expect("write added");
    git(repo.path(), ["add", "c.txt"]);
    fs::remove_file(repo.path().join("b.txt")).expect("delete tracked file");

    for args in [
        ["diff", "--raw", "HEAD"].as_slice(),
        ["diff", "--raw", "--abbrev=4", "HEAD"].as_slice(),
        ["diff", "--raw", "--no-abbrev", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    fs::write(repo.path().join("a.txt"), b"worktree\n").expect("dirty staged file");
    assert_eq!(
        run_zmin_args(repo.path(), &["diff", "--raw", "HEAD"]),
        common::git_args(repo.path(), &["diff", "--raw", "HEAD"])
    );
}

#[test]
fn diff_index_merge_option_treats_missing_worktree_files_like_stock_git() {
    let repo = two_commit_repo();
    fs::remove_file(repo.path().join("a.txt")).expect("remove tracked file");

    for args in [
        ["diff-index", "HEAD"].as_slice(),
        ["diff-index", "-m", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
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
        [
            "diff-tree",
            "--pretty",
            "--root",
            "--stat",
            "--compact-summary",
            "HEAD~1",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
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
        ["diff-tree", "HEAD~1"].as_slice(),
        ["diff-tree", "--root", "HEAD~1"].as_slice(),
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
            run_zmin_args(repo.path(), args),
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
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    assert_eq!(
        run_zmin_status(repo.path(), ["diff-files", "--quiet"]),
        git_status(repo.path(), ["diff-files", "--quiet"])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["diff-files", "--exit-code"]),
        git_status(repo.path(), ["diff-files", "--exit-code"])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["diff-index", "--exit-code", "HEAD"]),
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
            run_zmin_args(repo.path(), args),
            common::git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    assert_eq!(
        run_zmin_status(
            repo.path(),
            ["diff-index", "--cached", "--exit-code", "HEAD"]
        ),
        git_status(
            repo.path(),
            ["diff-index", "--cached", "--exit-code", "HEAD"]
        )
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["diff-tree", "--exit-code", "HEAD~1", "HEAD"]),
        git_status(repo.path(), ["diff-tree", "--exit-code", "HEAD~1", "HEAD"])
    );
}

#[test]
fn diff_plumbing_reports_read_tree_stat_dirty_entries_until_refresh_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "path0", "0\n");
        write_file(repo, "path2/file2", "2\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
        fs::remove_file(repo.join(".git/index")).expect("remove index");
    }

    git(git_repo.path(), ["read-tree", "HEAD"]);
    run_zmin(zmin_repo.path(), ["read-tree", "HEAD"]);

    for args in [
        ["diff-files"].as_slice(),
        ["diff-files", "--raw", "--no-abbrev"].as_slice(),
        ["diff-files", "--name-only"].as_slice(),
        ["diff-files", "--patch-with-raw"].as_slice(),
        ["diff-files", "-p"].as_slice(),
        ["diff-files", "--stat"].as_slice(),
        ["diff-index", "HEAD"].as_slice(),
        ["diff-index", "--raw", "--no-abbrev", "HEAD"].as_slice(),
        ["diff-index", "--name-only", "HEAD"].as_slice(),
        ["diff-index", "--patch-with-raw", "HEAD"].as_slice(),
        ["diff-index", "-p", "HEAD"].as_slice(),
        ["diff-index", "--stat", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }

    let raw = run_zmin_args(zmin_repo.path(), &["diff-files", "--raw", "--no-abbrev"]);
    assert!(
        raw.contains(" 0000000000000000000000000000000000000000 M\tpath0"),
        "diff-files raw should report a zero worktree object id before refresh: {raw}"
    );

    git(git_repo.path(), ["update-index", "--refresh"]);
    run_zmin(zmin_repo.path(), ["update-index", "--refresh"]);
    for args in [
        ["diff-files"].as_slice(),
        ["diff-files", "--raw", "--no-abbrev"].as_slice(),
        ["diff-files", "--name-only"].as_slice(),
        ["diff-index", "HEAD"].as_slice(),
        ["diff-index", "--raw", "--no-abbrev", "HEAD"].as_slice(),
        ["diff-index", "--name-only", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args after refresh: {args:?}"
        );
    }
}

#[test]
fn diff_files_reports_plain_worktree_symlink_cacheinfo_as_stat_dirty_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "core.symlinks", "false"]);
        write_file(repo, "path0sym", "target");
        let oid = git_with_stdin(repo, ["hash-object", "-w", "--stdin"], "target");
        git(
            repo,
            [
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("120000,{oid},path0sym"),
            ],
        );
    }

    assert_eq!(
        run_zmin(zmin_repo.path(), ["diff-files", "--raw", "--no-abbrev"]),
        git(git_repo.path(), ["diff-files", "--raw", "--no-abbrev"])
    );
    run_zmin(zmin_repo.path(), ["update-index", "--refresh"]);
    git(git_repo.path(), ["update-index", "--refresh"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["diff-files", "--raw", "--no-abbrev"]),
        git(git_repo.path(), ["diff-files", "--raw", "--no-abbrev"])
    );
}

#[test]
fn diff_files_reports_read_tree_symlink_entries_as_stat_dirty_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    git(git_repo.path(), ["config", "core.symlinks", "false"]);
    run_zmin(zmin_repo.path(), ["config", "core.symlinks", "false"]);

    for repo in [git_repo.path(), zmin_repo.path()] {
        for (path, target) in [
            ("path0", "hello path0\n"),
            ("path0sym", "hello path0"),
            ("path2/file2", "hello path2/file2\n"),
            ("path2/file2sym", "hello path2/file2"),
            ("path3/file3", "hello path3/file3\n"),
            ("path3/file3sym", "hello path3/file3"),
            ("path3/subp3/file3", "hello path3/subp3/file3\n"),
            ("path3/subp3/file3sym", "hello path3/subp3/file3"),
        ] {
            write_file(repo, path, target);
        }
    }

    git(
        git_repo.path(),
        [
            "add",
            "path0",
            "path2/file2",
            "path3/file3",
            "path3/subp3/file3",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "add",
            "path0",
            "path2/file2",
            "path3/file3",
            "path3/subp3/file3",
        ],
    );
    for path in [
        "path0sym",
        "path2/file2sym",
        "path3/file3sym",
        "path3/subp3/file3sym",
    ] {
        let target = fs::read_to_string(git_repo.path().join(path)).expect("read git fixture");
        let git_oid = git_with_stdin(git_repo.path(), ["hash-object", "-w", "--stdin"], &target);
        git(
            git_repo.path(),
            [
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("120000,{git_oid},{path}"),
            ],
        );
        git(git_repo.path(), ["update-index", path]);

        let target = fs::read_to_string(zmin_repo.path().join(path)).expect("read zmin fixture");
        let zmin_oid =
            run_zmin_with_stdin(zmin_repo.path(), ["hash-object", "-w", "--stdin"], &target);
        run_zmin(
            zmin_repo.path(),
            [
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("120000,{zmin_oid},{path}"),
            ],
        );
        run_zmin(zmin_repo.path(), ["update-index", path]);
    }
    git(
        git_repo.path(),
        [
            "update-index",
            "--add",
            "path0",
            "path0sym",
            "path2/file2",
            "path2/file2sym",
            "path3/file3",
            "path3/file3sym",
            "path3/subp3/file3",
            "path3/subp3/file3sym",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "update-index",
            "--add",
            "path0",
            "path0sym",
            "path2/file2",
            "path2/file2sym",
            "path3/file3",
            "path3/file3sym",
            "path3/subp3/file3",
            "path3/subp3/file3sym",
        ],
    );
    let git_tree = git(git_repo.path(), ["write-tree"]);
    fs::remove_file(git_repo.path().join(".git/index")).expect("remove git index");
    git(git_repo.path(), ["read-tree", &git_tree]);
    let zmin_tree = run_zmin(zmin_repo.path(), ["write-tree"]);
    fs::remove_file(zmin_repo.path().join(".git/index")).expect("remove zmin index");
    run_zmin(zmin_repo.path(), ["read-tree", &zmin_tree]);

    assert_eq!(
        run_zmin(zmin_repo.path(), ["diff-files", "--raw", "--no-abbrev"]),
        git(git_repo.path(), ["diff-files", "--raw", "--no-abbrev"])
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
        run_zmin(repo.path(), ["diff", "context.txt"]),
        git(repo.path(), ["diff", "context.txt"])
    );
    git(repo.path(), ["add", "-A"]);
    assert_eq!(
        run_zmin(repo.path(), ["diff", "--cached", "context.txt"]),
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

    assert_eq!(run_zmin(repo.path(), ["diff"]), git(repo.path(), ["diff"]));
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
        run_zmin(repo.path(), ["diff", ".changelog.yml"]),
        git(repo.path(), ["diff", ".changelog.yml"])
    );
}

#[test]
fn diff_patch_matches_stock_git_for_extensionless_hunk_headers() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(
        repo.path().join("plain"),
        concat!("A\n", "B\n", "C\n", "D\n", "E\n", "F\n"),
    )
    .expect("write plain");
    git(repo.path(), ["add", "plain"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    fs::write(
        repo.path().join("plain"),
        concat!("A\n", "B\n", "C\n", "changed\n", "E\n", "F\n", "G\n", "H\n"),
    )
    .expect("rewrite plain");

    assert_eq!(
        run_zmin(repo.path(), ["diff", "plain"]),
        git(repo.path(), ["diff", "plain"])
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

    assert_eq!(run_zmin(repo.path(), ["diff"]), git(repo.path(), ["diff"]));
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
        run_zmin(repo.path(), ["diff", "--stat"]),
        git(repo.path(), ["diff", "--stat"])
    );
}

#[test]
fn t1050_diff_big_file_threshold_marks_text_as_binary_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("large.txt"), b"old text\n").expect("write fixture");
    git(repo.path(), ["add", "large.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    fs::write(repo.path().join("large.txt"), b"old text\nnew text\n").expect("rewrite fixture");

    assert_eq!(
        run_zmin(
            repo.path(),
            [
                "-c",
                "core.autocrlf=false",
                "-c",
                "core.bigFileThreshold=1",
                "diff",
                "--",
                "large.txt",
            ]
        ),
        git(
            repo.path(),
            [
                "-c",
                "core.autocrlf=false",
                "-c",
                "core.bigFileThreshold=1",
                "diff",
                "--",
                "large.txt",
            ]
        )
    );
}
