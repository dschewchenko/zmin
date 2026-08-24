mod common;

use std::fs;
use std::io::Write;
use std::process::Command;

use flate2::{Compression, write::ZlibEncoder};
use tempfile::TempDir;
use zmin_git_core::{GitHashAlgorithm, GitObjectHash};

use common::{
    clone_repo_fixture, command_any_output, command_any_output_with_stdin, command_output_with_env,
    command_raw_output, configure_identity, git, git_args, git_failure_output, git_init,
    git_status, git_status_args, git_with_env, pinned_git_args, pinned_git_init_sha256,
    pinned_git_with_env, read_named_files, required_pinned_stock_git, run_zmin, run_zmin_args,
    run_zmin_failure_output, run_zmin_status, run_zmin_status_args, run_zmin_with_env, write_file,
    zmin_bin,
};

fn format_patch_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    write_file(repo.path(), "alpha.txt", "alpha\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "add alpha"]);
    write_file(repo.path(), "alpha.txt", "alpha\nbeta\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "update alpha"]);
    repo
}

fn write_mail_collision_object(repo: &std::path::Path, kind: &str, content: &[u8]) -> String {
    let mut object = format!("{kind} {}\0", content.len()).into_bytes();
    object.extend_from_slice(content);
    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update(&object);
    let id = hasher.finalize().to_hex();
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&object).expect("compress loose object");
    let compressed = encoder.finish().expect("finish loose object");
    let object_dir = repo.join(".git/objects").join(&id[..2]);
    fs::create_dir_all(&object_dir).expect("create loose object directory");
    fs::write(object_dir.join(&id[2..]), compressed).expect("write loose object");
    id
}

fn write_mail_collision_tree(repo: &std::path::Path, blob_id: &str) -> String {
    let mut tree = b"100644 ".to_vec();
    tree.extend_from_slice(b"path.txt");
    tree.push(0);
    tree.extend(
        blob_id
            .as_bytes()
            .chunks_exact(2)
            .map(|chunk| u8::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16).unwrap()),
    );
    write_mail_collision_object(repo, "tree", &tree)
}

fn write_mail_collision_commit(
    repo: &std::path::Path,
    tree_id: &str,
    parent: Option<&str>,
    message: &str,
) -> String {
    let parent_line = parent.map_or(String::new(), |id| format!("parent {id}\n"));
    let content = format!(
        "tree {tree_id}\n{parent_line}author Mail Collision <mail@example.test> 1700000000 +0000\ncommitter Mail Collision <mail@example.test> 1700000000 +0000\n\n{message}\n"
    );
    write_mail_collision_object(repo, "commit", content.as_bytes())
}

fn range_diff_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["checkout", "-b", "old"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "add one"]);
    write_file(repo.path(), "b.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "add two"]);
    git(repo.path(), ["checkout", "main"]);
    git(repo.path(), ["checkout", "-b", "new"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "add one"]);
    write_file(repo.path(), "c.txt", "three\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "add three"]);
    repo
}

fn format_patch_keep_subject_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    write_file(repo.path(), "alpha.txt", "alpha\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "[PATCH] add alpha"]);
    write_file(repo.path(), "alpha.txt", "alpha\nbeta\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "[PATCH] update alpha"]);
    repo
}

fn format_patch_multi_file_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "alpha.txt", "alpha beta\n");
    write_file(repo.path(), "beta.txt", "one two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    write_file(repo.path(), "alpha.txt", "alpha gamma beta\n");
    write_file(repo.path(), "beta.txt", "one changed two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "change"]);
    repo
}

fn format_patch_ignore_if_in_upstream_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "file", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    write_file(repo.path(), "elif", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "Initial"]);

    git(repo.path(), ["checkout", "-b", "side"]);
    write_file(repo.path(), "file", "1\n2\n5\n6\nA\nB\nC\n7\n8\n9\n10\n");
    let elif = repo.path().join("elif");
    let permissions = fs::metadata(&elif).expect("elif metadata").permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut next = permissions;
        next.set_mode(0o100755);
        fs::set_permissions(&elif, next).expect("chmod elif");
    }
    git(repo.path(), ["add", "file", "elif"]);
    git_with_env(repo.path(), ["commit", "-m", "Side changes #1"]);

    write_file(
        repo.path(),
        "file",
        "1\n2\n5\n6\nA\nB\nC\n7\n8\n9\n10\nD\nE\nF\n",
    );
    git(repo.path(), ["add", "file"]);
    git_with_env(repo.path(), ["commit", "-m", "Side changes #2"]);
    git(repo.path(), ["tag", "C2"]);

    git(repo.path(), ["checkout", "main"]);
    let patch = Command::new("git")
        .args(["diff-tree", "-p", "C2"])
        .current_dir(repo.path())
        .output()
        .expect("git diff-tree -p C2");
    assert!(
        patch.status.success(),
        "git diff-tree -p C2 stderr: {}",
        String::from_utf8_lossy(&patch.stderr)
    );
    let (code, _, stderr) = command_any_output_with_stdin(
        "git",
        repo.path(),
        &["apply", "--index"],
        &String::from_utf8_lossy(&patch.stdout),
        "git apply --index",
    );
    assert_eq!(code, 0, "git apply --index stderr: {stderr}");
    git_with_env(
        repo.path(),
        ["commit", "-m", "Main accepts moral equivalent of #2"],
    );

    git(repo.path(), ["checkout", "side"]);
    write_file(
        repo.path(),
        "file",
        "5\n6\n1\n2\n3\nA\n4\nB\nC\n7\n8\n9\n10\nD\nE\nF\n",
    );
    git(repo.path(), ["add", "file"]);
    git_with_env(
        repo.path(),
        [
            "commit",
            "-m",
            "Side changes #3 with \\n backslash-n in it.",
        ],
    );

    git(repo.path(), ["checkout", "-b", "patchid"]);
    write_file(
        repo.path(),
        "file2",
        "5\n6\n1\n2\n3\nA\n4\nB\nC\n7\n8\n9\n10\nD\nE\nF\n",
    );
    write_file(
        repo.path(),
        "file3",
        "1\n2\n3\nA\n4\nB\nC\n7\n8\n9\n10\nD\nE\nF\n5\n6\n",
    );
    write_file(repo.path(), "file", "8\n9\n10\n");
    git(repo.path(), ["add", "file", "file2", "file3"]);
    git_with_env(repo.path(), ["commit", "-m", "patchid 1"]);

    write_file(repo.path(), "file2", "4\nA\nB\n7\n8\n9\n10\n");
    write_file(repo.path(), "file3", "8\n9\n10\n5\n6\n");
    git(repo.path(), ["add", "file2", "file3"]);
    git_with_env(repo.path(), ["commit", "-m", "patchid 2"]);

    write_file(repo.path(), "file", "10\n5\n6\n");
    git(repo.path(), ["add", "file"]);
    git_with_env(repo.path(), ["commit", "-m", "patchid 3"]);

    git(repo.path(), ["checkout", "-b", "empty", "main"]);
    git_with_env(
        repo.path(),
        ["commit", "--allow-empty", "-m", "empty commit"],
    );

    git(repo.path(), ["checkout", "side"]);
    repo
}

fn format_patch_long_subject_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    write_file(repo.path(), "alpha.txt", "alpha\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(
        repo.path(),
        [
            "commit",
            "-m",
            "This is a very long subject line to exercise filename max length behavior exactly",
        ],
    );
    write_file(repo.path(), "beta.txt", "beta\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(
        repo.path(),
        [
            "commit",
            "-m",
            "Second commit with another long title for output filename testing",
        ],
    );
    repo
}

fn format_patch_nested_dir_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    fs::create_dir_all(repo.path().join("src/lib")).expect("create src/lib");
    fs::create_dir_all(repo.path().join("tests/unit")).expect("create tests/unit");
    write_file(repo.path(), "src/lib/alpha.txt", "alpha beta\n");
    write_file(repo.path(), "tests/unit/beta.txt", "one two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    write_file(repo.path(), "src/lib/alpha.txt", "alpha gamma beta\n");
    write_file(repo.path(), "tests/unit/beta.txt", "one changed two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "change"]);
    repo
}

fn format_patch_merge_commit_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "side"]);
    write_file(repo.path(), "side.txt", "side\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "side"]);

    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "main.txt", "main\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);
    git(repo.path(), ["merge", "--no-ff", "-m", "merge", "side"]);
    repo
}

fn normalize_format_patch_version(output: &str) -> String {
    let mut normalized = Vec::new();
    let mut version_line = false;
    for line in output.lines() {
        if version_line {
            normalized.push("git-version");
            version_line = false;
            continue;
        }
        if line.starts_with("Message-ID: <") {
            normalized.push("Message-ID: <git-version>");
        } else if line.starts_with("Content-Type: multipart/mixed; boundary=\"------------") {
            normalized.push("Content-Type: multipart/mixed; boundary=\"------------git-version\"");
        } else if line.starts_with("--------------") {
            let suffix = if line.ends_with("--") { "--" } else { "" };
            normalized.push(if suffix.is_empty() {
                "--------------git-version"
            } else {
                "--------------git-version--"
            });
        } else {
            normalized.push(line);
        }
        version_line = line == "-- ";
    }
    normalized.join("\n")
}

fn normalize_format_patch_named_files(files: Vec<(String, String)>) -> Vec<(String, String)> {
    files
        .into_iter()
        .map(|(name, content)| {
            let normalized = content
                .lines()
                .map(|line| {
                    if line.starts_with("Date: ") {
                        "Date: <normalized-date>".to_owned()
                    } else {
                        line.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            (name, normalize_format_patch_version(&normalized))
        })
        .collect()
}

fn normalize_format_patch_dates(output: &str) -> String {
    output
        .lines()
        .map(|line| {
            if line.starts_with("Date: ") {
                "Date: <normalized-date>".to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn header_block(output: &str, header_name: &str) -> String {
    let mut lines = Vec::new();
    let header_prefix = format!("{header_name}: ");
    let mut collecting = false;
    for line in output.lines() {
        if !collecting {
            if line.starts_with(&header_prefix) {
                lines.push(line);
                collecting = true;
            }
            continue;
        }
        if line.starts_with(' ') {
            lines.push(line);
            continue;
        }
        break;
    }
    lines.join("\n")
}

#[test]
fn format_patch_emits_stock_applicable_mail_patches() {
    let repo = format_patch_fixture_repo();
    let base = git(repo.path(), ["rev-parse", "HEAD~2"]);
    let expected_tree = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);

    let output = run_zmin(
        repo.path(),
        ["format-patch", "-o", "patches", "HEAD~2..HEAD"],
    );
    let patch_names = read_named_files(&repo.path().join("patches"))
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    assert_eq!(
        patch_names,
        vec!["0001-add-alpha.patch", "0002-update-alpha.patch"]
    );
    assert_eq!(
        output,
        "patches/0001-add-alpha.patch\npatches/0002-update-alpha.patch"
    );

    let apply_repo = clone_repo_fixture(repo.path());
    configure_identity(apply_repo.path());
    git(apply_repo.path(), ["reset", "--hard", &base]);
    for patch in patch_names {
        let path = repo.path().join("patches").join(patch);
        let path = path.to_str().expect("patch path utf8");
        git(apply_repo.path(), ["am", path]);
    }
    assert_eq!(
        git(apply_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        expected_tree
    );

    let stdout_patch = run_zmin(repo.path(), ["format-patch", "--stdout", "-1", "HEAD"]);
    assert!(stdout_patch.contains("Subject: [PATCH] update alpha"));
    assert!(stdout_patch.contains("diff --git a/alpha.txt b/alpha.txt"));

    let no_numbered = run_zmin(
        repo.path(),
        ["format-patch", "--stdout", "--no-numbered", "HEAD~2..HEAD"],
    );
    assert!(no_numbered.contains("Subject: [PATCH] add alpha"));
    assert!(no_numbered.contains("Subject: [PATCH] update alpha"));
    assert!(!no_numbered.contains("[PATCH 1/2]"));

    let prefixed_zmin = run_zmin(
        repo.path(),
        [
            "format-patch",
            "--inline",
            "--stdout",
            "--subject-prefix=TESTCASE",
            "HEAD~2..HEAD",
        ],
    );
    let prefixed_stock = git(
        repo.path(),
        [
            "format-patch",
            "--inline",
            "--stdout",
            "--subject-prefix=TESTCASE",
            "HEAD~2..HEAD",
        ],
    );
    assert_eq!(
        normalize_format_patch_version(&prefixed_zmin),
        normalize_format_patch_version(&prefixed_stock)
    );

    git(
        repo.path(),
        ["config", "format.subjectprefix", "DIFFERENT_PREFIX"],
    );
    let configured_zmin = run_zmin(
        repo.path(),
        ["format-patch", "--inline", "--stdout", "-1", "HEAD"],
    );
    let configured_stock = git(
        repo.path(),
        ["format-patch", "--inline", "--stdout", "-1", "HEAD"],
    );
    assert_eq!(
        normalize_format_patch_version(&configured_zmin),
        normalize_format_patch_version(&configured_stock)
    );

    let cover_zmin = run_zmin_with_env(
        repo.path(),
        [
            "format-patch",
            "--stdout",
            "--cover-letter",
            "-n",
            "HEAD~2..HEAD",
        ],
    );
    let cover_stock = git_with_env(
        repo.path(),
        [
            "format-patch",
            "--stdout",
            "--cover-letter",
            "-n",
            "HEAD~2..HEAD",
        ],
    );
    assert_eq!(
        normalize_format_patch_version(&cover_zmin),
        normalize_format_patch_version(&cover_stock)
    );
}

#[test]
fn format_patch_handles_merge_commit_like_stock_git_first_parent_patch() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "side"]);
    write_file(repo.path(), "side.txt", "side\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "side"]);

    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "main.txt", "main\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);
    git(repo.path(), ["merge", "--no-ff", "-m", "merge", "side"]);

    let zmin = run_zmin(repo.path(), ["format-patch", "--stdout", "-1", "HEAD"]);
    let stock = git_args(repo.path(), &["format-patch", "--stdout", "-1", "HEAD"]);
    assert_eq!(
        normalize_format_patch_version(&zmin),
        normalize_format_patch_version(&stock)
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["format-patch", "--stdout", "-1", "HEAD"]),
        git_status(repo.path(), ["format-patch", "--stdout", "-1", "HEAD"])
    );
}

#[test]
fn format_patch_raw_extends_seed_width_for_colliding_blob_objects() {
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    let repo = git_init();
    let left = write_mail_collision_object(repo.path(), "blob", b"abbrev-sha1-collision-006687");
    let right = write_mail_collision_object(repo.path(), "blob", b"abbrev-sha1-collision-040110");
    assert_eq!(
        &left[..7],
        &right[..7],
        "fixture must collide at seed width"
    );
    let left_tree = write_mail_collision_tree(repo.path(), &left);
    let right_tree = write_mail_collision_tree(repo.path(), &right);
    let base = write_mail_collision_commit(repo.path(), &left_tree, None, "base");
    let head = write_mail_collision_commit(repo.path(), &right_tree, Some(&base), "change");
    git(repo.path(), ["update-ref", "refs/heads/main", &head]);
    for value in ["7", "12", "4", "no"] {
        let config = format!("core.abbrev={value}");
        let args = [
            "-c",
            config.as_str(),
            "format-patch",
            "--stdout",
            "--no-signature",
            "--raw",
            "-1",
            &head,
        ];
        let zmin = command_raw_output(zmin_bin(), repo.path(), &args, "zmin");
        let stock = command_raw_output(stock, repo.path(), &args, "stock Git");
        assert_eq!(zmin, stock, "core.abbrev={value}");
        let minimum = value.parse::<usize>().unwrap_or(40);
        let stdout = String::from_utf8(stock.stdout.clone()).expect("stock format-patch stdout");
        let index_ranges = stdout
            .lines()
            .filter_map(|line| line.strip_prefix("index "))
            .filter_map(|line| line.split_whitespace().next())
            .flat_map(|range| range.split_once(".."))
            .flat_map(|(old, new)| [old, new]);
        let mut saw_index = false;
        for id in index_ranges {
            saw_index = true;
            assert!(id.len() >= minimum, "core.abbrev={value}: {id}");
            if value == "no" {
                assert_eq!(id.len(), 40, "core.abbrev=no");
            }
        }
        assert!(saw_index, "raw format-patch must contain an index line");
    }

}

#[test]
fn sha256_format_patch_abbrev_matches_pinned_git() {
    let repo = pinned_git_init_sha256();
    pinned_git_args(repo.path(), &["config", "user.name", "Bench"]);
    pinned_git_args(repo.path(), &["config", "user.email", "bench@example.test"]);
    write_file(repo.path(), "base.txt", "base\n");
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
    write_file(repo.path(), "base.txt", "changed\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_git_with_env(
        repo.path(),
        &["commit", "-m", "changed"],
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
    for (args, expected_width) in [
        (
            [
                "-c",
                "core.abbrev=12",
                "format-patch",
                "--stdout",
                "--no-signature",
                "--raw",
                "-1",
                "HEAD",
            ]
            .as_slice(),
            12,
        ),
        (
            [
                "-c",
                "core.abbrev=no",
                "format-patch",
                "--stdout",
                "--no-signature",
                "--raw",
                "-1",
                "HEAD",
            ]
            .as_slice(),
            64,
        ),
    ] {
        let zmin = command_raw_output(zmin_bin(), repo.path(), args, "zmin");
        let stock_result = command_raw_output(stock, repo.path(), args, "stock Git");
        assert_eq!(zmin, stock_result, "SHA-256 format-patch tuple: {args:?}");
        let output =
            String::from_utf8(stock_result.stdout.clone()).expect("SHA-256 format-patch output");
        let index = output
            .lines()
            .find_map(|line| line.strip_prefix("index "))
            .and_then(|line| line.split_whitespace().next())
            .expect("raw format-patch index line")
            .split_once("..")
            .expect("raw format-patch index range");
        assert_eq!(index.0.len(), expected_width, "SHA-256 old object name");
        assert_eq!(index.1.len(), expected_width, "SHA-256 new object name");
    }
}

#[test]
fn format_patch_binary_summary_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    let mut content = vec![b'a'; 16 * 1024];
    content[128] = 0;
    fs::write(repo.path().join("blob.bin"), content).expect("write binary blob");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "add binary blob"]);
    git(repo.path(), ["repack", "-ad", "--depth=0"]);
    let base = git(repo.path(), ["rev-parse", "HEAD~1"]);
    let expected_tree = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);

    let zmin = run_zmin(repo.path(), ["format-patch", "--stdout", "-1", "HEAD"]);

    assert!(zmin.contains("GIT binary patch"));
    assert!(!zmin.contains("Binary files /dev/null and b/blob.bin differ"));

    let apply_repo = clone_repo_fixture(repo.path());
    configure_identity(apply_repo.path());
    git(apply_repo.path(), ["reset", "--hard", &base]);
    let patch_path = repo.path().join("binary.patch");
    fs::write(&patch_path, zmin).expect("write zmin binary patch");
    git(
        apply_repo.path(),
        ["am", patch_path.to_str().expect("patch path utf8")],
    );
    assert_eq!(
        git(apply_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        expected_tree
    );
}

#[test]
fn format_patch_default_diff_option_family_matches_stock_git() {
    let repo = format_patch_fixture_repo();
    let cases = [
        ["--binary"],
        ["--default-prefix"],
        ["--no-ext-diff"],
        ["--no-textconv"],
        ["--no-color"],
        ["--no-color-moved"],
        ["--no-color-moved-ws"],
        ["--stat"],
        ["--patch"],
    ];

    for extra in cases {
        let mut args = vec!["format-patch", "--stdout"];
        args.extend(extra);
        args.push("-1");
        args.push("HEAD");
        let zmin = run_zmin_args(repo.path(), &args);
        let stock = git_args(repo.path(), &args);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), &args),
            git_status_args(repo.path(), &args),
            "status case mismatch"
        );
    }
}

#[test]
fn format_patch_shared_diff_option_family_matches_stock_git() {
    let repo = format_patch_fixture_repo();
    let cases = [
        ["--abbrev"],
        ["--always"],
        ["--color-moved"],
        ["--color-moved-ws=ignore-space-change"],
        ["--histogram"],
        ["--minimal"],
        ["--patience"],
        ["--indent-heuristic"],
        ["--ignore-all-space"],
        ["--ignore-space-change"],
        ["--ignore-space-at-eol"],
        ["--ignore-blank-lines"],
        ["--ignore-cr-at-eol"],
        ["--inter-hunk-context=0"],
        ["--diff-algorithm=myers"],
        ["--diff-algorithm=minimal"],
        ["--diff-algorithm=patience"],
        ["--diff-algorithm=histogram"],
        ["--anchored=alpha"],
        ["--color=never"],
        ["--ignore-submodules=all"],
        ["--ita-invisible-in-index"],
        ["--irreversible-delete"],
        ["--diff-filter=AM"],
        ["--function-context"],
    ];

    for extra in cases {
        let mut args = vec!["format-patch", "--stdout"];
        args.extend(extra);
        args.push("-1");
        args.push("HEAD");
        let zmin = run_zmin_args(repo.path(), &args);
        let stock = git_args(repo.path(), &args);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), &args),
            git_status_args(repo.path(), &args),
            "status case mismatch"
        );
    }
}

#[test]
fn format_patch_shared_diff_noop_family_matches_stock_git() {
    let repo = format_patch_fixture_repo();
    let cases = [
        ["--exit-code"],
        ["--progress"],
        ["--quiet"],
        ["--find-renames"],
        ["--find-copies"],
        ["--find-copies-harder"],
        ["--rename-empty"],
        ["--break-rewrites"],
        ["--compact-summary"],
        ["--patch-with-stat"],
        ["--output-indicator-new=+"],
        ["--output-indicator-old=-"],
        ["--output-indicator-context= "],
        ["--src-prefix=a/"],
        ["--dst-prefix=b/"],
        ["--line-prefix="],
        ["--relative"],
        ["--root"],
        ["--notes"],
        ["--no-notes"],
        ["--no-thread"],
        ["--no-cover-letter"],
        ["--no-attach"],
        ["--no-binary"],
        ["--no-renames"],
        ["--no-indent-heuristic"],
    ];

    for extra in cases {
        let mut args = vec!["format-patch", "--stdout"];
        args.extend(extra);
        args.push("-1");
        args.push("HEAD");
        let zmin = run_zmin_args(repo.path(), &args);
        let stock = git_args(repo.path(), &args);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), &args),
            git_status_args(repo.path(), &args),
            "status case mismatch"
        );
    }
}

#[test]
fn format_patch_shared_diff_alias_family_matches_stock_git() {
    let repo = format_patch_fixture_repo();
    let cases = [
        ["--text"],
        ["--textconv"],
        ["--unified=3"],
        ["-U3"],
        ["-a"],
        ["-N"],
        ["-M"],
        ["-C"],
        ["-D"],
        ["-B"],
        ["-W"],
        ["--ws-error-highlight=old,new,context"],
        ["--ext-diff"],
        ["--no-relative"],
        ["--no-rename-empty"],
    ];

    for extra in cases {
        let mut args = vec!["format-patch", "--stdout"];
        args.extend(extra);
        args.push("-1");
        args.push("HEAD");
        let zmin = run_zmin_args(repo.path(), &args);
        let stock = git_args(repo.path(), &args);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), &args),
            git_status_args(repo.path(), &args),
            "status case mismatch"
        );
    }
}

#[test]
fn format_patch_helper_free_pickaxe_and_short_diff_family_matches_stock_git() {
    let repo = format_patch_fixture_repo();
    let head_blob = git(repo.path(), ["rev-parse", "HEAD:alpha.txt"]);
    let cases: Vec<Vec<String>> = vec![
        vec!["-q".into()],
        vec!["-u".into()],
        vec!["-w".into()],
        vec!["-b".into()],
        vec!["-S".into(), "beta".into()],
        vec!["-G".into(), "beta".into()],
        vec!["--pickaxe-regex".into(), "-S".into(), "beta".into()],
        vec!["--pickaxe-all".into(), "-S".into(), "beta".into()],
        vec!["-I".into(), "nomatch".into()],
        vec!["--ignore-matching-lines=nomatch".into()],
        vec!["--find-object".into(), head_blob],
        vec!["-l".into(), "10".into()],
    ];

    for extra in cases {
        let mut args = vec!["format-patch".to_owned(), "--stdout".to_owned()];
        args.extend(extra);
        args.push("-1".to_owned());
        args.push("HEAD".to_owned());
        let args_ref = args.iter().map(String::as_str).collect::<Vec<_>>();
        let zmin = run_zmin_args(repo.path(), &args_ref);
        let stock = git_args(repo.path(), &args_ref);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args_ref.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), &args_ref),
            git_status_args(repo.path(), &args_ref),
            "status case mismatch"
        );
    }
}

#[test]
fn format_patch_diff_output_family_matches_stock_git() {
    let repo = format_patch_fixture_repo();
    let cases: Vec<Vec<String>> = vec![
        vec!["--raw".into()],
        vec!["--numstat".into()],
        vec!["--shortstat".into()],
        vec!["--summary".into()],
        vec!["--no-patch".into()],
        vec!["--no-stat".into()],
        vec!["--patch-with-raw".into()],
        vec!["--full-index".into()],
        vec!["-p".into()],
    ];

    for extra in cases {
        let mut args = vec!["format-patch".to_owned(), "--stdout".to_owned()];
        args.extend(extra);
        args.push("-1".to_owned());
        args.push("HEAD".to_owned());
        let args_ref = args.iter().map(String::as_str).collect::<Vec<_>>();
        let zmin = run_zmin_args(repo.path(), &args_ref);
        let stock = git_args(repo.path(), &args_ref);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args_ref.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), &args_ref),
            git_status_args(repo.path(), &args_ref),
            "status case mismatch"
        );
    }
}

#[test]
fn format_patch_invalid_surface_and_output_file_match_stock_git() {
    let repo = format_patch_fixture_repo();
    let invalid_cases: [(&str, &[&str]); 5] = [
        (
            "--check",
            &["format-patch", "--check", "--stdout", "-1", "HEAD"],
        ),
        (
            "--name-only",
            &["format-patch", "--name-only", "--stdout", "-1", "HEAD"],
        ),
        (
            "--name-status",
            &["format-patch", "--name-status", "--stdout", "-1", "HEAD"],
        ),
        (
            "--output with stdout",
            &[
                "format-patch",
                "--output=mail.patch",
                "--stdout",
                "-1",
                "HEAD",
            ],
        ),
        (
            "--output-directory with stdout",
            &[
                "format-patch",
                "--output-directory=patches",
                "--stdout",
                "-1",
                "HEAD",
            ],
        ),
    ];

    for (label, args) in invalid_cases {
        let git_result = git_failure_output(repo.path(), args);
        let zmin_result = run_zmin_failure_output(repo.path(), args);
        assert_eq!(zmin_result, git_result, "args: {args:?}");
        assert!(!label.is_empty());
    }

    let git_repo = clone_repo_fixture(repo.path());
    let zmin_repo = clone_repo_fixture(repo.path());
    let git_args = ["format-patch", "--output=mail.patch", "-1", "HEAD"];
    let git_result = command_any_output("git", git_repo.path(), &git_args, "git");
    let zmin_result = command_any_output(zmin_bin(), zmin_repo.path(), &git_args, "zmin");
    assert_eq!(zmin_result, git_result);
    let git_mail =
        fs::read_to_string(git_repo.path().join("mail.patch")).expect("read git output mail");
    let zmin_mail =
        fs::read_to_string(zmin_repo.path().join("mail.patch")).expect("read zmin output mail");
    assert_eq!(
        normalize_format_patch_version(&zmin_mail),
        normalize_format_patch_version(&git_mail)
    );
}

#[test]
fn format_patch_mail_render_family_matches_stock_git() {
    let repo = format_patch_fixture_repo();
    let cases: Vec<Vec<String>> = vec![
        vec!["--signoff".into()],
        vec!["-s".into()],
        vec!["--zero-commit".into()],
        vec!["--reroll-count=2".into()],
        vec!["-v".into(), "2".into()],
        vec!["--no-signature".into()],
        vec!["--signature=custom".into()],
        vec!["--start-number".into(), "7".into()],
        vec!["--rfc".into()],
    ];

    for extra in cases {
        let mut args = vec!["format-patch".to_owned(), "--stdout".to_owned()];
        args.extend(extra);
        args.push("HEAD~2..HEAD".to_owned());
        let args_ref = args.iter().map(String::as_str).collect::<Vec<_>>();
        let zmin = run_zmin_args(repo.path(), &args_ref);
        let stock = git_args(repo.path(), &args_ref);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args_ref.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), &args_ref),
            git_status_args(repo.path(), &args_ref),
            "status case mismatch"
        );
    }
}

#[test]
fn format_patch_notes_family_matches_stock_git() {
    let repo = format_patch_fixture_repo();
    git(
        repo.path(),
        [
            "notes",
            "--ref",
            "test",
            "add",
            "-m",
            "test message",
            "HEAD",
        ],
    );
    git(
        repo.path(),
        ["notes", "add", "-m", "notes config message", "HEAD"],
    );
    git(
        repo.path(),
        [
            "notes",
            "--ref",
            "note1",
            "add",
            "-m",
            "this is note 1",
            "HEAD",
        ],
    );
    git(
        repo.path(),
        [
            "notes",
            "--ref",
            "note2",
            "add",
            "-m",
            "this is note 2",
            "HEAD",
        ],
    );

    let cases: Vec<Vec<&str>> = vec![
        vec![
            "format-patch",
            "-1",
            "--signoff",
            "--stdout",
            "--notes=test",
        ],
        vec!["format-patch", "-1", "--stdout", "--notes"],
        vec!["format-patch", "-1", "--stdout", "--no-notes"],
        vec!["format-patch", "-1", "--stdout", "--notes", "--no-notes"],
        vec!["format-patch", "-1", "--stdout", "--no-notes", "--notes"],
        vec!["format-patch", "-1", "--stdout", "--notes=note1"],
        vec!["format-patch", "-1", "--stdout", "--notes=note2"],
        vec![
            "format-patch",
            "-1",
            "--stdout",
            "--notes=note1",
            "--notes=note2",
        ],
        vec![
            "format-patch",
            "-1",
            "--stdout",
            "--no-notes",
            "--notes=note2",
        ],
    ];

    for args in cases {
        let zmin = run_zmin_args(repo.path(), &args);
        let stock = git_args(repo.path(), &args);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), &args),
            git_status_args(repo.path(), &args),
            "status case mismatch: {}",
            args.join(" ")
        );
    }

    let config_cases: Vec<Vec<(&str, &str)>> = vec![
        vec![("format.notes", "true")],
        vec![("format.notes", "note1")],
        vec![("format.notes", "note2"), ("format.notes", "note1")],
    ];
    let args = ["format-patch", "-1", "--stdout"];
    for config in config_cases {
        let git_repo = clone_repo_fixture(repo.path());
        let zmin_repo = clone_repo_fixture(repo.path());
        for (key, value) in &config {
            git(git_repo.path(), ["config", "--add", key, value]);
            git(zmin_repo.path(), ["config", "--add", key, value]);
        }
        let zmin = run_zmin_args(zmin_repo.path(), &args);
        let stock = git_args(git_repo.path(), &args);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "config case: {:?}",
            config
        );
        assert_eq!(
            run_zmin_status_args(zmin_repo.path(), &args),
            git_status_args(git_repo.path(), &args),
            "config status mismatch: {:?}",
            config
        );
    }
}

#[test]
fn format_patch_output_directory_family_matches_stock_git() {
    let repo = format_patch_ignore_if_in_upstream_fixture_repo();
    let cases: Vec<(&str, Vec<&str>, Option<(&str, &str)>, &str)> = vec![
        (
            "cli outdir simple",
            vec!["format-patch", "-o", "patches", "origin/main..origin/side"],
            None,
            "patches",
        ),
        (
            "cli outdir nested existing",
            vec![
                "format-patch",
                "-o",
                "existing-dir/patches",
                "origin/main..origin/side",
            ],
            None,
            "existing-dir/patches",
        ),
        (
            "cli outdir nested missing",
            vec![
                "format-patch",
                "-o",
                "non-existing-dir/patches",
                "origin/main..origin/side",
            ],
            None,
            "non-existing-dir/patches",
        ),
        (
            "config outdir",
            vec!["format-patch", "origin/main..origin/side"],
            Some(("format.outputDirectory", "patches")),
            "patches",
        ),
        (
            "cli overrides config outdir",
            vec!["format-patch", "origin/main..origin/side", "-o", "patchset"],
            Some(("format.outputDirectory", "patches")),
            "patchset",
        ),
    ];

    for (label, args, config, outdir) in cases {
        let git_repo = clone_repo_fixture(repo.path());
        let zmin_repo = clone_repo_fixture(repo.path());
        if let Some((key, value)) = config {
            git(git_repo.path(), ["config", key, value]);
            git(zmin_repo.path(), ["config", key, value]);
        }
        if label == "cli outdir nested existing" {
            fs::create_dir_all(git_repo.path().join("existing-dir"))
                .expect("create git existing dir");
            fs::create_dir_all(zmin_repo.path().join("existing-dir"))
                .expect("create zmin existing dir");
        }
        let git_result = command_any_output("git", git_repo.path(), &args, "git");
        let zmin_result = command_any_output(zmin_bin(), zmin_repo.path(), &args, "zmin");
        assert_eq!(zmin_result.0, git_result.0, "{label}: status");
        assert_eq!(zmin_result.2, git_result.2, "{label}: stderr");

        let git_files = read_named_files(&git_repo.path().join(outdir));
        let zmin_files = read_named_files(&zmin_repo.path().join(outdir));
        assert_eq!(zmin_files.len(), git_files.len(), "{label}: file count");
        for ((zmin_name, _zmin_contents), (git_name, _git_contents)) in
            zmin_files.iter().zip(git_files.iter())
        {
            assert_eq!(zmin_name, git_name, "{label}: filename");
        }

        let git_stdout = git_result
            .1
            .lines()
            .map(|line| line.rsplit('/').next().unwrap_or(line).to_owned())
            .collect::<Vec<_>>();
        let zmin_stdout = zmin_result
            .1
            .lines()
            .map(|line| line.rsplit('/').next().unwrap_or(line).to_owned())
            .collect::<Vec<_>>();
        assert_eq!(zmin_stdout, git_stdout, "{label}: stdout filenames");

        if label == "cli overrides config outdir" {
            assert!(
                !zmin_repo.path().join("patches").exists(),
                "{label}: config outdir should stay unused"
            );
        }
    }
}

#[test]
fn format_patch_notes_config_no_notes_and_path_output_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "a\n");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    write_file(repo.path(), "a.txt", "a\nb\n");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "change"]);
    git(
        repo.path(),
        [
            "notes",
            "--ref",
            "note1",
            "add",
            "-m",
            "this is note 1",
            "HEAD",
        ],
    );
    git(
        repo.path(),
        [
            "notes",
            "--ref",
            "note2",
            "add",
            "-m",
            "this is note 2",
            "HEAD",
        ],
    );
    git(repo.path(), ["config", "format.notes", "note1"]);
    git(repo.path(), ["config", "--add", "format.notes", "note2"]);

    let note_cases: Vec<Vec<&str>> = vec![
        vec!["format-patch", "-1", "--stdout", "--no-notes"],
        vec![
            "format-patch",
            "-1",
            "--stdout",
            "--no-notes",
            "--notes=note2",
        ],
    ];
    for args in note_cases {
        let stock = git_args(repo.path(), &args);
        let zmin = run_zmin_args(repo.path(), &args);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args.join(" ")
        );
    }

    let subdir = repo.path().join("sub/dir");
    fs::create_dir_all(&subdir).expect("create subdir");
    let stock = command_any_output("git", &subdir, &["format-patch", "-1"], "git");
    let zmin = command_any_output(zmin_bin(), &subdir, &["format-patch", "-1"], "zmin");
    assert_eq!(zmin.0, stock.0, "subdir status");
    assert_eq!(zmin.1, stock.1, "subdir stdout");
    assert_eq!(zmin.2, stock.2, "subdir stderr");
}

#[test]
fn format_patch_unified_context_and_default_filename_limit_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "file", "1\n2\n3\n4\n");
    git(repo.path(), ["add", "file"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    write_file(repo.path(), "file", "1\n2\n3\n4\n5\n6\n");
    git(repo.path(), ["add", "file"]);
    git_with_env(repo.path(), ["commit", "-m", "c1"]);
    write_file(repo.path(), "file", "1\n2\n3\n4\n5\n6\n7\n8\n");
    git(repo.path(), ["add", "file"]);
    git_with_env(repo.path(), ["commit", "-m", "c2"]);

    let stock = git_args(repo.path(), &["format-patch", "-U4", "-2", "--stdout"]);
    let zmin = run_zmin_args(repo.path(), &["format-patch", "-U4", "-2", "--stdout"]);
    assert_eq!(
        normalize_format_patch_version(&zmin),
        normalize_format_patch_version(&stock),
        "unified context"
    );

    let long_subject = "This is an excessively long subject line for a message due to the habit some projects have of not having a short, one-line subject at the start of the commit message, but rather sticking a whole paragraph right at the start as the only thing in the commit message. It had better not become the filename for the patch.";
    write_file(repo.path(), "file", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    git(repo.path(), ["add", "file"]);
    git_with_env(repo.path(), ["commit", "-m", long_subject]);

    let git_repo = clone_repo_fixture(repo.path());
    let zmin_repo = clone_repo_fixture(repo.path());
    let git_result = command_any_output(
        "git",
        git_repo.path(),
        &["format-patch", "-o", "patches", "-1"],
        "git",
    );
    let zmin_result = command_any_output(
        zmin_bin(),
        zmin_repo.path(),
        &["format-patch", "-o", "patches", "-1"],
        "zmin",
    );
    assert_eq!(zmin_result.0, git_result.0, "filename limit status");
    assert_eq!(zmin_result.2, git_result.2, "filename limit stderr");
    assert_eq!(zmin_result.1, git_result.1, "filename limit stdout");
    let git_files = read_named_files(&git_repo.path().join("patches"));
    let zmin_files = read_named_files(&zmin_repo.path().join("patches"));
    assert_eq!(
        zmin_files.len(),
        git_files.len(),
        "filename limit file count"
    );
    for ((zmin_name, _), (git_name, _)) in zmin_files.iter().zip(git_files.iter()) {
        assert_eq!(zmin_name, git_name, "filename limit filename");
    }
}

#[test]
fn format_patch_upstream_style_signoff_footer_family_matches_stock_git() {
    let cases: [(&str, &[&str], Option<(&str, &str)>, bool); 8] = [
        (
            "existing footer signoff keeps adjacency",
            &["subject", "", "body", "", "Signed-off-by: my@house"],
            None,
            true,
        ),
        (
            "existing matching signoff is not duplicated",
            &[
                "subject",
                "",
                "body",
                "",
                "Signed-off-by: C O Mitter <committer@example.com>",
            ],
            None,
            true,
        ),
        (
            "existing matching signoff without trailing newline is not duplicated",
            &[
                "subject",
                "",
                "Signed-off-by: C O Mitter <committer@example.com>",
            ],
            None,
            false,
        ),
        (
            "middle signoff paragraph does not make trailing text footer",
            &[
                "subject",
                "",
                "Signed-off-by: my@house",
                "",
                "A lot of houses.",
            ],
            None,
            true,
        ),
        (
            "garbage inside conforming footer still counts as footer",
            &[
                "subject",
                "",
                "body",
                "",
                "Tested-by: my@house",
                "Some Trash",
                "Signed-off-by: C O Mitter <committer@example.com>",
            ],
            None,
            true,
        ),
        (
            "wrapped text before trailing signoff stays outside footer",
            &[
                "subject",
                "",
                "My unfortunate",
                "Signed-off-by: example happens to be wrapped here.",
            ],
            None,
            true,
        ),
        (
            "footer duplicate is suppressed even with trailing bug trailers",
            &[
                "subject",
                "",
                "body",
                "",
                "Reviewed-id: Noone",
                "Tested-by: my@house",
                "Change-id: Ideadbeef",
                "Signed-off-by: C O Mitter <committer@example.com>",
                "Bug: 1234",
            ],
            None,
            true,
        ),
        (
            "configured custom trailer makes trailing block footer",
            &["subject", "", "Myfooter: x", "Some Trash"],
            Some(("trailer.Myfooter.ifexists", "add")),
            true,
        ),
    ];

    for (label, message_lines, config, trailing_newline) in cases {
        let git_repo = git_init();
        configure_identity(git_repo.path());
        git(git_repo.path(), ["checkout", "-b", "main"]);
        write_file(git_repo.path(), "file.txt", "base\n");
        git(git_repo.path(), ["add", "file.txt"]);
        git_with_env(git_repo.path(), ["commit", "-m", "base"]);
        write_file(git_repo.path(), "file.txt", "base\nnext\n");
        git(git_repo.path(), ["add", "file.txt"]);
        let mut message = message_lines.join("\n");
        if trailing_newline {
            message.push('\n');
        }
        write_file(git_repo.path(), "msg.txt", &message);
        if let Some((key, value)) = config {
            git(git_repo.path(), ["config", key, value]);
        }
        git_with_env(git_repo.path(), ["commit", "-F", "msg.txt"]);

        let args = ["format-patch", "--stdout", "--signoff", "HEAD^..HEAD"];
        let git_output = git_args(git_repo.path(), &args);
        let zmin_output = run_zmin_args(git_repo.path(), &args);
        assert_eq!(
            normalize_format_patch_version(&zmin_output),
            normalize_format_patch_version(&git_output),
            "{label}"
        );
        assert_eq!(
            run_zmin_status_args(git_repo.path(), &args),
            git_status_args(git_repo.path(), &args),
            "status mismatch: {label}"
        );
    }
}

#[test]
fn format_patch_keep_subject_family_matches_stock_git() {
    let repo = format_patch_keep_subject_fixture_repo();
    let cases = [["--keep-subject"], ["-k"]];

    for extra in cases {
        let mut args = vec!["format-patch", "--stdout"];
        args.extend(extra);
        args.push("HEAD~2..HEAD");
        let zmin = run_zmin_args(repo.path(), &args);
        let stock = git_args(repo.path(), &args);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), &args),
            git_status_args(repo.path(), &args),
            "status case mismatch"
        );
    }
}

#[test]
fn format_patch_mail_header_family_matches_stock_git() {
    let repo = format_patch_fixture_repo();
    write_file(repo.path(), "custom-signature.txt", "custom sig\n");
    let cases: Vec<Vec<String>> = vec![
        vec!["--to=a@example.test".into()],
        vec!["--cc=c@example.test".into()],
        vec!["--add-header=X-Test: 1".into()],
        vec!["--in-reply-to=<msgid@example.test>".into()],
        vec!["--thread".into()],
        vec!["--from=Sender <sender@example.test>".into()],
        vec![
            "--from=Sender <sender@example.test>".into(),
            "--force-in-body-from".into(),
        ],
        vec![
            "--from=Sender <sender@example.test>".into(),
            "--no-force-in-body-from".into(),
        ],
        vec!["--signature-file=custom-signature.txt".into()],
        vec!["--encode-email-headers".into()],
        vec!["--no-encode-email-headers".into()],
    ];

    for extra in cases {
        let mut args = vec!["format-patch".to_owned(), "--stdout".to_owned()];
        args.extend(extra);
        args.push("-1".to_owned());
        args.push("HEAD".to_owned());
        let args_ref = args.iter().map(String::as_str).collect::<Vec<_>>();
        let zmin = run_zmin_args(repo.path(), &args_ref);
        let stock = git_args(repo.path(), &args_ref);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args_ref.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), &args_ref),
            git_status_args(repo.path(), &args_ref),
            "status case mismatch"
        );
    }
}

#[test]
fn format_patch_multiline_subject_and_header_folding_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "file", "base\n");
    git(repo.path(), ["add", "file"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    write_file(repo.path(), "file", "base\nnext\n");
    git(repo.path(), ["add", "file"]);
    git_with_env(repo.path(), ["commit", "-m", "one\ntwo\nthree\n\nbody\n"]);

    let long_subject = "foo bar ".repeat(64).trim_end().to_owned();
    write_file(repo.path(), "long.txt", "long\n");
    git(repo.path(), ["add", "long.txt"]);
    git_with_env(repo.path(), ["commit", "-m", &long_subject]);

    let long_author = "Foo Bar ".repeat(24).trim_end().to_owned();
    write_file(repo.path(), "author.txt", "author\n");
    git(repo.path(), ["add", "author.txt"]);
    command_output_with_env(
        "git",
        repo.path(),
        &["commit", "-m", "author-check"],
        &[
            ("GIT_AUTHOR_NAME", &long_author),
            ("GIT_AUTHOR_EMAIL", "author@example.com"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
        "git commit long author",
    );

    let multiline_stock = git_args(repo.path(), &["format-patch", "--stdout", "-1", "HEAD~2"]);
    let multiline_zmin = run_zmin_args(repo.path(), &["format-patch", "--stdout", "-1", "HEAD~2"]);
    assert_eq!(
        header_block(&multiline_zmin, "Subject"),
        header_block(&multiline_stock, "Subject"),
        "multi-line subject"
    );

    let long_subject_stock = git_args(repo.path(), &["format-patch", "--stdout", "-1", "HEAD~1"]);
    let long_subject_zmin =
        run_zmin_args(repo.path(), &["format-patch", "--stdout", "-1", "HEAD~1"]);
    assert_eq!(
        header_block(&long_subject_zmin, "Subject"),
        header_block(&long_subject_stock, "Subject"),
        "long ascii subject folding"
    );

    let long_author_stock = git_args(repo.path(), &["format-patch", "--stdout", "-1", "HEAD"]);
    let long_author_zmin = run_zmin_args(repo.path(), &["format-patch", "--stdout", "-1", "HEAD"]);
    assert_eq!(
        header_block(&long_author_zmin, "From"),
        header_block(&long_author_stock, "From"),
        "long ascii from folding"
    );
}

#[test]
fn format_patch_encode_email_headers_override_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "file", "base\n");
    git(repo.path(), ["add", "file"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    write_file(repo.path(), "file", "base\nnext\n");
    git(repo.path(), ["add", "file"]);
    git_with_env(repo.path(), ["commit", "-m", "Foö"]);
    git(
        repo.path(),
        ["config", "format.encodeEmailHeaders", "false"],
    );

    let args = ["format-patch", "--encode-email-headers", "-1", "--stdout"];
    let stock = git_args(repo.path(), &args);
    let zmin = run_zmin_args(repo.path(), &args);
    assert_eq!(
        normalize_format_patch_version(&zmin),
        normalize_format_patch_version(&stock)
    );
}

#[test]
fn format_patch_non_encoded_utf8_from_and_in_body_headers_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "file", "base\n");
    git(repo.path(), ["add", "file"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    write_file(repo.path(), "file", "base\nnext\n");
    git(repo.path(), ["add", "file"]);
    let long_utf8_author = "Foö Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar Foo Bar";
    command_output_with_env(
        "git",
        repo.path(),
        &["commit", "-m", "author-check"],
        &[
            ("GIT_AUTHOR_NAME", long_utf8_author),
            ("GIT_AUTHOR_EMAIL", "author@example.com"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
        "git commit utf8 long author",
    );

    let no_encode_args = [
        "format-patch",
        "--no-encode-email-headers",
        "--stdout",
        "-1",
        "HEAD",
    ];
    let stock = git_args(repo.path(), &no_encode_args);
    let zmin = run_zmin_args(repo.path(), &no_encode_args);
    assert_eq!(header_block(&zmin, "From"), header_block(&stock, "From"));

    git(repo.path(), ["reset", "--hard", "HEAD~1"]);
    write_file(repo.path(), "file", "base\nbody\n");
    git(repo.path(), ["add", "file"]);
    command_output_with_env(
        "git",
        repo.path(),
        &["commit", "-m", "exotic"],
        &[
            ("GIT_AUTHOR_NAME", "éxötìc"),
            ("GIT_AUTHOR_EMAIL", "author@example.com"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "C O Mitter"),
            ("GIT_COMMITTER_EMAIL", "committer@example.com"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
        "git commit exotic author",
    );

    let body_from_args = ["format-patch", "-1", "--stdout", "--from"];
    let stock = git_args(repo.path(), &body_from_args);
    let zmin = run_zmin_args(repo.path(), &body_from_args);
    assert_eq!(
        normalize_format_patch_version(&zmin),
        normalize_format_patch_version(&stock)
    );
}

#[test]
fn format_patch_pathspec_and_diff_relative_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "base.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "pathspec", "main"]);
    write_file(repo.path(), "file_a", "file_a 1\n");
    write_file(repo.path(), "file_b", "file_b 1\n");
    git(repo.path(), ["add", "file_a", "file_b"]);
    git_with_env(repo.path(), ["commit", "-m", "pathspec_initial"]);
    write_file(repo.path(), "file_a", "file_a 1\nfile_a 2\n");
    git(repo.path(), ["add", "file_a"]);
    git_with_env(repo.path(), ["commit", "-m", "pathspec_a"]);
    write_file(repo.path(), "file_b", "file_b 1\nfile_b 2\n");
    git(repo.path(), ["add", "file_b"]);
    git_with_env(repo.path(), ["commit", "-m", "pathspec_b"]);

    let pathspec_args = ["format-patch", "--stdout", "main..pathspec", "--", "file_a"];
    let stock = git_args(repo.path(), &pathspec_args);
    let zmin = run_zmin_args(repo.path(), &pathspec_args);
    assert_eq!(
        normalize_format_patch_version(&zmin),
        normalize_format_patch_version(&stock)
    );

    fs::create_dir_all(repo.path().join("subdir")).expect("create subdir");
    write_file(repo.path(), "subdir/file2", "other content\n");
    git(repo.path(), ["add", "subdir/file2"]);
    git_with_env(repo.path(), ["commit", "-m", "msg"]);
    let expect = git_args(
        repo.path(),
        &["format-patch", "--relative=subdir", "--stdout", "-1"],
    );
    git(repo.path(), ["config", "diff.relative", "true"]);
    let actual = command_any_output(
        zmin_bin(),
        &repo.path().join("subdir"),
        &["format-patch", "--stdout", "-1"],
        "zmin format-patch diff.relative",
    );
    assert_eq!(actual.0, 0);
    assert_eq!(
        normalize_format_patch_version(&actual.1),
        normalize_format_patch_version(&expect)
    );
}

#[test]
fn format_patch_signature_config_family_matches_stock_git() {
    let repo = format_patch_fixture_repo();
    write_file(
        repo.path(),
        "mail-signature",
        "Test User <test.email@kernel.org>\ncustom sig\n",
    );
    let cases: [(&str, Option<(&str, &str)>, &[&str]); 6] = [
        (
            "format.signature config",
            Some(("format.signature", "config sig")),
            &["format-patch", "--stdout", "-1", "HEAD"],
        ),
        (
            "empty format.signature suppresses signature",
            Some(("format.signature", "")),
            &["format-patch", "--stdout", "-1", "HEAD"],
        ),
        (
            "signature flag overrides format.signature",
            Some(("format.signature", "config sig")),
            &[
                "format-patch",
                "--stdout",
                "--signature=override",
                "-1",
                "HEAD",
            ],
        ),
        (
            "empty signature flag suppresses signature",
            None,
            &["format-patch", "--stdout", "--signature=", "-1", "HEAD"],
        ),
        (
            "format.signaturefile config",
            Some(("format.signaturefile", "mail-signature")),
            &["format-patch", "--stdout", "-1", "HEAD"],
        ),
        (
            "signature-file explicit path",
            None,
            &[
                "format-patch",
                "--stdout",
                "--signature-file=mail-signature",
                "-1",
                "HEAD",
            ],
        ),
    ];

    for (label, config, args) in cases {
        if let Some((key, value)) = config {
            git(repo.path(), ["config", key, value]);
        }
        let zmin = run_zmin_args(repo.path(), args);
        let stock = git_args(repo.path(), args);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "{label}"
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), args),
            git_status_args(repo.path(), args),
            "status mismatch: {label}"
        );
        if let Some((key, _)) = config {
            git(repo.path(), ["config", "--unset-all", key]);
        }
    }
}

#[test]
fn format_patch_upstream_style_address_header_family_matches_expected_headers() {
    let repo = format_patch_fixture_repo();
    let cases: Vec<(&str, Vec<(&str, &str)>, Vec<&str>, &[&str])> = vec![
        (
            "additional command line cc rfc822",
            vec![("format.headers", "Cc: R E Cipient <rcipient@example.com>")],
            vec![
                "format-patch",
                "--stdout",
                "--cc=S. E. Cipient <scipient@example.com>",
                "HEAD~2..HEAD",
            ],
            &[
                "Cc: R E Cipient <rcipient@example.com>,",
                " \"S. E. Cipient\" <scipient@example.com>",
            ],
        ),
        (
            "command line to rfc822",
            vec![],
            vec![
                "format-patch",
                "--stdout",
                "--to=R. E. Cipient <rcipient@example.com>",
                "HEAD~2..HEAD",
            ],
            &["To: \"R. E. Cipient\" <rcipient@example.com>"],
        ),
        (
            "command line to rfc2047",
            vec![],
            vec![
                "format-patch",
                "--stdout",
                "--to=R Ä Cipient <rcipient@example.com>",
                "HEAD~2..HEAD",
            ],
            &["To: =?UTF-8?q?R=20=C3=84=20Cipient?= <rcipient@example.com>"],
        ),
        (
            "config to rfc822",
            vec![("format.to", "R. E. Cipient <rcipient@example.com>")],
            vec!["format-patch", "--stdout", "HEAD~2..HEAD"],
            &["To: \"R. E. Cipient\" <rcipient@example.com>"],
        ),
        (
            "config to rfc2047",
            vec![("format.to", "R Ä Cipient <rcipient@example.com>")],
            vec!["format-patch", "--stdout", "HEAD~2..HEAD"],
            &["To: =?UTF-8?q?R=20=C3=84=20Cipient?= <rcipient@example.com>"],
        ),
    ];

    for (label, configs, args, expected_headers) in cases {
        let zmin_repo = clone_repo_fixture(repo.path());
        for (key, value) in &configs {
            git(zmin_repo.path(), ["config", key, value]);
        }

        let zmin = run_zmin_args(zmin_repo.path(), &args);
        let header_block = zmin
            .split("\n\n")
            .next()
            .expect("format-patch output should contain headers");
        for expected_header in expected_headers {
            assert!(
                header_block.contains(expected_header),
                "{label}: missing expected header {expected_header:?}\n{header_block}"
            );
        }
        assert_eq!(run_zmin_status_args(zmin_repo.path(), &args), 0, "{label}");
    }
}

#[test]
fn format_patch_upstream_style_from_header_family_matches_expected_headers() {
    let repo = format_patch_fixture_repo();
    let cases: Vec<(&str, Option<&str>, Vec<&str>, &[&str])> = vec![
        (
            "quotes dot in from header",
            Some("Foo B. Bar"),
            vec!["format-patch", "--stdout", "-1", "HEAD"],
            &["From: \"Foo B. Bar\" <author@example.com>"],
        ),
        (
            "quotes double quote in from header",
            Some("Foo \"The Baz\" Bar"),
            vec!["format-patch", "--stdout", "-1", "HEAD"],
            &["From: \"Foo \\\"The Baz\\\" Bar\" <author@example.com>"],
        ),
        (
            "uses rfc2047 for non ascii from header",
            Some("Föo Bar"),
            vec!["format-patch", "--stdout", "-1", "HEAD"],
            &["From: =?UTF-8?q?F=C3=B6o=20Bar?= <author@example.com>"],
        ),
        (
            "from applies to cover letter",
            None,
            vec![
                "format-patch",
                "--cover-letter",
                "--stdout",
                "--from=Foo Bar <author@example.com>",
                "HEAD~1",
            ],
            &["From: Foo Bar <author@example.com>"],
        ),
        (
            "from omits redundant in body header",
            Some("A U Thor"),
            vec![
                "format-patch",
                "--stdout",
                "--from=A U Thor <author@example.com>",
                "-1",
                "HEAD",
            ],
            &["From: A U Thor <author@example.com>"],
        ),
        (
            "force in body from keeps redundant header",
            Some("A U Thor"),
            vec![
                "format-patch",
                "--stdout",
                "--force-in-body-from",
                "--from=A U Thor <author@example.com>",
                "-1",
                "HEAD",
            ],
            &["From: A U Thor <author@example.com>"],
        ),
    ];

    for (label, author_name, args, expected_parts) in cases {
        let zmin_repo = clone_repo_fixture(repo.path());
        if let Some(author_name) = author_name {
            write_file(zmin_repo.path(), "extra.txt", "content\n");
            git(zmin_repo.path(), ["add", "extra.txt"]);
            command_output_with_env(
                "git",
                zmin_repo.path(),
                &["commit", "-m", "author-check"],
                &[
                    ("GIT_AUTHOR_NAME", author_name),
                    ("GIT_AUTHOR_EMAIL", "author@example.com"),
                    ("GIT_AUTHOR_DATE", "1700000000 +0000"),
                    ("GIT_COMMITTER_NAME", "Bench"),
                    ("GIT_COMMITTER_EMAIL", "bench@example.test"),
                    ("GIT_COMMITTER_DATE", "1700000000 +0000"),
                ],
                "git",
            );
        }
        let zmin = run_zmin_args(zmin_repo.path(), &args);
        for expected_part in expected_parts {
            assert!(
                zmin.contains(expected_part),
                "{label}: missing expected fragment {expected_part:?}\n{zmin}"
            );
        }
        if label == "from omits redundant in body header" {
            assert_eq!(zmin.matches("\nFrom: ").count(), 1, "{label}\n{zmin}");
        }
        if label == "force in body from keeps redundant header" {
            assert_eq!(zmin.matches("\nFrom: ").count(), 2, "{label}\n{zmin}");
        }
        assert_eq!(run_zmin_status_args(zmin_repo.path(), &args), 0, "{label}");
    }
}

#[test]
fn format_patch_upstream_style_subject_prefix_family_matches_expected_output() {
    let repo = format_patch_keep_subject_fixture_repo();
    let cases: Vec<(&str, Vec<&str>, &str)> = vec![
        (
            "subject prefix adds separator space",
            vec![
                "format-patch",
                "-n",
                "-1",
                "--stdout",
                "--subject-prefix=PREFIX",
            ],
            "Subject: [PREFIX 1/1] [PATCH] update alpha",
        ),
        (
            "empty subject prefix has no extra space",
            vec!["format-patch", "-n", "-1", "--stdout", "--subject-prefix="],
            "Subject: [1/1] [PATCH] update alpha",
        ),
        (
            "rfc default prefix",
            vec!["format-patch", "-n", "-1", "--stdout", "--rfc"],
            "Subject: [RFC PATCH 1/1] [PATCH] update alpha",
        ),
        (
            "rfc custom token",
            vec!["format-patch", "-n", "-1", "--stdout", "--rfc=WIP"],
            "Subject: [WIP PATCH 1/1] [PATCH] update alpha",
        ),
        (
            "rfc append variant",
            vec!["format-patch", "-n", "-1", "--stdout", "--rfc=-(WIP)"],
            "Subject: [PATCH (WIP) 1/1] [PATCH] update alpha",
        ),
    ];

    for (label, args, expected_subject) in cases {
        let zmin = run_zmin_args(repo.path(), &args);
        let subject = zmin
            .lines()
            .find(|line| line.starts_with("Subject: "))
            .expect("subject header");
        assert_eq!(subject, expected_subject, "{label}\n{zmin}");
        assert_eq!(run_zmin_status_args(repo.path(), &args), 0, "{label}");
    }
}

#[test]
fn format_patch_upstream_style_subject_prefix_conflicts_match_expected_errors() {
    let repo = format_patch_keep_subject_fixture_repo();
    let cases: Vec<(&str, Vec<&str>)> = vec![
        (
            "subject-prefix and keep-subject conflict",
            vec![
                "format-patch",
                "-1",
                "--stdout",
                "--subject-prefix=MYPREFIX",
                "-k",
            ],
        ),
        (
            "empty subject-prefix and keep-subject conflict",
            vec!["format-patch", "-1", "--stdout", "--subject-prefix=", "-k"],
        ),
        (
            "rfc and keep-subject conflict",
            vec!["format-patch", "-1", "--stdout", "--rfc", "-k"],
        ),
    ];

    for (label, args) in cases {
        let failure = run_zmin_failure_output(repo.path(), &args);
        assert_eq!(
            failure,
            (
                128,
                String::new(),
                "fatal: options '--subject-prefix/--rfc' and '-k' cannot be used together"
                    .to_owned(),
            ),
            "{label}"
        );
    }
}

#[test]
fn format_patch_upstream_style_rfc_prefix_variants_match_expected_output() {
    let repo = format_patch_fixture_repo();
    let cases: Vec<(&str, Vec<&str>, &str, Option<(&str, &str)>)> = vec![
        (
            "rfc then no-rfc",
            vec!["format-patch", "-n", "-1", "--stdout", "--rfc", "--no-rfc"],
            "Subject: [PATCH 1/1] update alpha",
            None,
        ),
        (
            "rfc then empty rfc",
            vec!["format-patch", "-n", "-1", "--stdout", "--rfc", "--rfc="],
            "Subject: [PATCH 1/1] update alpha",
            None,
        ),
        (
            "rfc does not overwrite configured prefix",
            vec!["format-patch", "-n", "-1", "--stdout", "--rfc"],
            "Subject: [RFC PATCH foobar 1/1] update alpha",
            Some(("format.subjectprefix", "PATCH foobar")),
        ),
        (
            "rfc argument order independent",
            vec![
                "format-patch",
                "-n",
                "-1",
                "--stdout",
                "--rfc",
                "--subject-prefix=PATCH foobar",
            ],
            "Subject: [RFC PATCH foobar 1/1] update alpha",
            None,
        ),
    ];

    for (label, args, expected_subject, config) in cases {
        let zmin_repo = clone_repo_fixture(repo.path());
        if let Some((key, value)) = config {
            git(zmin_repo.path(), ["config", key, value]);
        }
        let zmin = run_zmin_args(zmin_repo.path(), &args);
        let subject = zmin
            .lines()
            .find(|line| line.starts_with("Subject: "))
            .expect("subject header");
        assert_eq!(subject, expected_subject, "{label}\n{zmin}");
    }
}

#[test]
fn format_patch_word_diff_order_and_reverse_family_matches_stock_git() {
    let repo = format_patch_multi_file_fixture_repo();
    write_file(repo.path(), "order.txt", "beta.txt\nalpha.txt\n");
    let cases: Vec<Vec<String>> = vec![
        vec!["--word-diff".into()],
        vec!["--word-diff=plain".into()],
        vec!["--word-diff=porcelain".into()],
        vec!["--word-diff=none".into()],
        vec!["--submodule=log".into()],
        vec!["--submodule=short".into()],
        vec!["--submodule=diff".into()],
        vec!["-O".into(), "order.txt".into()],
        vec!["--skip-to=beta.txt".into()],
        vec!["--rotate-to=beta.txt".into()],
        vec!["-O".into(), "order.txt".into(), "--skip-to=beta.txt".into()],
        vec![
            "-O".into(),
            "order.txt".into(),
            "--rotate-to=beta.txt".into(),
        ],
        vec!["-R".into()],
    ];

    for extra in cases {
        let mut args = vec!["format-patch".to_owned(), "--stdout".to_owned()];
        args.extend(extra);
        args.push("-1".to_owned());
        args.push("HEAD".to_owned());
        let args_ref = args.iter().map(String::as_str).collect::<Vec<_>>();
        let zmin = run_zmin_args(repo.path(), &args_ref);
        let stock = git_args(repo.path(), &args_ref);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args_ref.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), &args_ref),
            git_status_args(repo.path(), &args_ref),
            "status case mismatch"
        );
    }
}

#[test]
fn format_patch_mail_series_tail_family_matches_stock_git() {
    let repo = format_patch_fixture_repo();
    write_file(
        repo.path(),
        "desc.txt",
        "Desc file title\n\nDesc file body\n",
    );
    let base = git(repo.path(), ["rev-parse", "HEAD~2"]);
    let success_cases: Vec<Vec<String>> = vec![
        vec!["--base".into(), base.clone(), "HEAD~2..HEAD".into()],
        vec!["--no-base".into(), "HEAD~2..HEAD".into()],
        vec![
            "--cover-letter".into(),
            "--cover-from-description=message".into(),
            "HEAD~2..HEAD".into(),
        ],
        vec![
            "--cover-letter".into(),
            "--cover-from-description=subject".into(),
            "HEAD~2..HEAD".into(),
        ],
        vec![
            "--cover-letter".into(),
            "--cover-from-description=auto".into(),
            "HEAD~2..HEAD".into(),
        ],
        vec![
            "--cover-letter".into(),
            "--cover-from-description=none".into(),
            "HEAD~2..HEAD".into(),
        ],
        vec![
            "--cover-letter".into(),
            "--description-file=desc.txt".into(),
            "HEAD~2..HEAD".into(),
        ],
        vec!["--filename-max-length=40".into(), "HEAD~2..HEAD".into()],
        vec!["--ignore-if-in-upstream".into(), "HEAD~2..HEAD".into()],
        vec![
            "--interdiff".into(),
            "HEAD~1".into(),
            "-1".into(),
            "HEAD".into(),
        ],
        vec![
            "--range-diff".into(),
            "HEAD~1".into(),
            "-1".into(),
            "HEAD".into(),
        ],
        vec![
            "--range-diff".into(),
            "HEAD~1".into(),
            "--creation-factor=70".into(),
            "-1".into(),
            "HEAD".into(),
        ],
    ];

    for extra in success_cases {
        let mut args = vec!["format-patch".to_owned(), "--stdout".to_owned()];
        args.extend(extra);
        let args_ref = args.iter().map(String::as_str).collect::<Vec<_>>();
        let zmin = run_zmin_args(repo.path(), &args_ref);
        let stock = git_args(repo.path(), &args_ref);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args_ref.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), &args_ref),
            git_status_args(repo.path(), &args_ref),
            "status case mismatch"
        );
    }

    git(repo.path(), ["checkout", "-b", "upstream", "HEAD~2"]);
    git(repo.path(), ["checkout", "-b", "local", "upstream"]);
    write_file(repo.path(), "n1.txt", "n1\n");
    git(repo.path(), ["add", "n1.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "N1"]);
    write_file(repo.path(), "n2.txt", "n2\n");
    git(repo.path(), ["add", "n2.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "N2"]);
    git(repo.path(), ["branch", "--set-upstream-to=upstream"]);

    for args in [
        vec!["format-patch", "--stdout", "--base=auto", "-2"],
        vec!["format-patch", "--stdout", "-1"],
    ] {
        let git_result = git_args(repo.path(), &args);
        let zmin_result = run_zmin_args(repo.path(), &args);
        assert_eq!(
            normalize_format_patch_version(&zmin_result),
            normalize_format_patch_version(&git_result),
            "args: {args:?}"
        );
    }

    git(repo.path(), ["config", "format.useAutoBase", "true"]);
    let git_result = git_args(repo.path(), &["format-patch", "--stdout", "-1"]);
    let zmin_result = run_zmin_args(repo.path(), &["format-patch", "--stdout", "-1"]);
    assert_eq!(
        normalize_format_patch_version(&zmin_result),
        normalize_format_patch_version(&git_result),
        "format.useAutoBase=true"
    );

    git(
        repo.path(),
        ["config", "--replace-all", "format.useAutoBase", "whenAble"],
    );
    let git_result = git_args(repo.path(), &["format-patch", "--stdout", "-1"]);
    let zmin_result = run_zmin_args(repo.path(), &["format-patch", "--stdout", "-1"]);
    assert_eq!(
        normalize_format_patch_version(&zmin_result),
        normalize_format_patch_version(&git_result),
        "format.useAutoBase=whenAble"
    );
}

#[test]
fn format_patch_default_history_selection_skips_merge_commits_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "file", "1\n2\n3\n");
    git(repo.path(), ["add", "file"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "feature"]);
    write_file(repo.path(), "file", "1\n2\n3\nAnother line\n");
    git(repo.path(), ["commit", "-am", "Feature branch change #1"]);
    write_file(
        repo.path(),
        "file",
        "1\n2\n3\nAnother line\nYet another line\n",
    );
    git(repo.path(), ["commit", "-am", "Feature branch change #2"]);

    git(repo.path(), ["checkout", "-b", "merger", "main"]);
    git(repo.path(), ["merge", "--no-ff", "feature", "-m", "merge"]);

    let args = ["format-patch", "-3", "--stdout"];
    let zmin = run_zmin_args(repo.path(), &args);
    let stock = git_args(repo.path(), &args);
    assert_eq!(
        normalize_format_patch_version(&zmin),
        normalize_format_patch_version(&stock)
    );
}

#[test]
fn format_patch_ignore_if_in_upstream_matches_stock_git_on_upstream_fixture() {
    let repo = format_patch_ignore_if_in_upstream_fixture_repo();
    for args in [
        vec!["format-patch", "--stdout", "main..side"],
        vec![
            "format-patch",
            "--stdout",
            "--ignore-if-in-upstream",
            "main..side",
        ],
        vec![
            "format-patch",
            "--stdout",
            "--ignore-if-in-upstream",
            "v2..v1",
        ],
    ] {
        if args.last() == Some(&"v2..v1") {
            git(repo.path(), ["tag", "-a", "v1", "-m", "tag", "side"]);
            git(repo.path(), ["tag", "-a", "v2", "-m", "tag", "main"]);
        }
        let zmin = run_zmin_args(repo.path(), &args);
        let stock = git_args(repo.path(), &args);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "args: {args:?}"
        );
    }
}

#[test]
fn format_patch_ignore_if_in_upstream_upstream_style_matches_stock_git() {
    let repo = format_patch_ignore_if_in_upstream_fixture_repo();
    let args = [
        "format-patch",
        "--stdout",
        "--ignore-if-in-upstream",
        "main..side",
    ];
    let zmin = run_zmin_args(repo.path(), &args);
    let stock = git_args(repo.path(), &args);
    assert_eq!(
        normalize_format_patch_version(&zmin),
        normalize_format_patch_version(&stock)
    );
    assert_eq!(
        run_zmin_status_args(repo.path(), &args),
        git_status_args(repo.path(), &args)
    );
}

#[test]
fn format_patch_word_diff_color_and_regex_family_matches_stock_git() {
    let repo = format_patch_multi_file_fixture_repo();
    let success_cases: Vec<Vec<String>> = vec![
        vec!["--word-diff=color".into()],
        vec!["--color-words".into()],
        vec!["--color-words=[a-z]+".into()],
        vec![
            "--word-diff-regex=[a-z]+".into(),
            "--word-diff=plain".into(),
        ],
        vec![
            "--word-diff-regex=[a-z]+".into(),
            "--word-diff=porcelain".into(),
        ],
        vec![
            "--word-diff-regex=[a-z]+".into(),
            "--word-diff=color".into(),
        ],
    ];

    for extra in success_cases {
        let mut args = vec!["format-patch".to_owned(), "--stdout".to_owned()];
        args.extend(extra);
        args.push("-1".to_owned());
        args.push("HEAD".to_owned());
        let args_ref = args.iter().map(String::as_str).collect::<Vec<_>>();
        let zmin = run_zmin_args(repo.path(), &args_ref);
        let stock = git_args(repo.path(), &args_ref);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args_ref.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), &args_ref),
            git_status_args(repo.path(), &args_ref),
            "status case mismatch"
        );
    }

    let invalid_args = [
        "format-patch",
        "--stdout",
        "--word-diff-regex=[",
        "--word-diff=plain",
        "-1",
        "HEAD",
    ];
    let git_result = git_failure_output(repo.path(), &invalid_args);
    let zmin_result = run_zmin_failure_output(repo.path(), &invalid_args);
    assert_eq!(zmin_result, git_result, "args: {invalid_args:?}");
}

#[test]
fn format_patch_prefix_null_and_dirstat_family_matches_stock_git() {
    let multi_file_repo = format_patch_multi_file_fixture_repo();
    let nested_repo = format_patch_nested_dir_fixture_repo();

    let multi_file_cases: Vec<Vec<String>> = vec![vec!["--no-prefix".into()], vec!["-z".into()]];
    for extra in multi_file_cases {
        let mut args = vec!["format-patch".to_owned(), "--stdout".to_owned()];
        args.extend(extra);
        args.push("-1".to_owned());
        args.push("HEAD".to_owned());
        let args_ref = args.iter().map(String::as_str).collect::<Vec<_>>();
        let zmin = run_zmin_args(multi_file_repo.path(), &args_ref);
        let stock = git_args(multi_file_repo.path(), &args_ref);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args_ref.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(multi_file_repo.path(), &args_ref),
            git_status_args(multi_file_repo.path(), &args_ref),
            "status case mismatch"
        );
    }

    let nested_cases: Vec<Vec<String>> = vec![
        vec!["--dirstat".into()],
        vec!["--dirstat=files".into()],
        vec!["--dirstat=files,0".into()],
        vec!["--dirstat=files,10".into()],
        vec!["--dirstat-by-file".into()],
    ];
    for extra in nested_cases {
        let mut args = vec!["format-patch".to_owned(), "--stdout".to_owned()];
        args.extend(extra);
        args.push("-1".to_owned());
        args.push("HEAD".to_owned());
        let args_ref = args.iter().map(String::as_str).collect::<Vec<_>>();
        let zmin = run_zmin_args(nested_repo.path(), &args_ref);
        let stock = git_args(nested_repo.path(), &args_ref);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args_ref.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(nested_repo.path(), &args_ref),
            git_status_args(nested_repo.path(), &args_ref),
            "status case mismatch"
        );
    }
}

#[test]
fn format_patch_merge_diff_and_dirstat_alias_family_matches_stock_git() {
    let merge_repo = format_patch_merge_commit_fixture_repo();
    let merge_cases: Vec<Vec<String>> = vec![
        vec!["-m".into()],
        vec!["-c".into()],
        vec!["-t".into()],
        vec!["--dd".into()],
        vec!["--diff-merges=first-parent".into()],
        vec!["--diff-merges=separate".into()],
        vec!["--diff-merges=combined".into()],
        vec!["--diff-merges=dense-combined".into()],
        vec!["--diff-merges=off".into()],
        vec!["--no-diff-merges".into()],
        vec!["--combined-all-paths".into(), "-c".into()],
        vec![
            "--combined-all-paths".into(),
            "--diff-merges=combined".into(),
        ],
    ];
    for extra in merge_cases {
        let mut args = vec!["format-patch".to_owned(), "--stdout".to_owned()];
        args.extend(extra);
        args.push("HEAD".to_owned());
        args.push("^HEAD^1".to_owned());
        args.push("^HEAD^2".to_owned());
        let args_ref = args.iter().map(String::as_str).collect::<Vec<_>>();
        let zmin = run_zmin_args(merge_repo.path(), &args_ref);
        let stock = git_args(merge_repo.path(), &args_ref);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args_ref.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(merge_repo.path(), &args_ref),
            git_status_args(merge_repo.path(), &args_ref),
            "status case mismatch"
        );
    }

    for args in [
        vec![
            "format-patch",
            "--stdout",
            "--combined-all-paths",
            "HEAD",
            "^HEAD^1",
            "^HEAD^2",
        ],
        vec![
            "format-patch",
            "--stdout",
            "--remerge-diff",
            "HEAD",
            "^HEAD^1",
            "^HEAD^2",
        ],
    ] {
        let git_result = git_failure_output(merge_repo.path(), &args);
        let zmin_result = run_zmin_failure_output(merge_repo.path(), &args);
        assert_eq!(zmin_result, git_result, "args: {args:?}");
    }

    let nested_repo = format_patch_nested_dir_fixture_repo();
    let dirstat_cases: Vec<Vec<String>> = vec![
        vec!["--dirstat=cumulative".into()],
        vec!["--cumulative".into()],
        vec!["-X".into()],
        vec!["-X10".into()],
        vec!["--dirstat-by-file=10,cumulative".into()],
    ];
    for extra in dirstat_cases {
        let mut args = vec!["format-patch".to_owned(), "--stdout".to_owned()];
        args.extend(extra);
        args.push("-1".to_owned());
        args.push("HEAD".to_owned());
        let args_ref = args.iter().map(String::as_str).collect::<Vec<_>>();
        let zmin = run_zmin_args(nested_repo.path(), &args_ref);
        let stock = git_args(nested_repo.path(), &args_ref);
        assert_eq!(
            normalize_format_patch_version(&zmin),
            normalize_format_patch_version(&stock),
            "case: {}",
            args_ref.join(" ")
        );
        assert_eq!(
            run_zmin_status_args(nested_repo.path(), &args_ref),
            git_status_args(nested_repo.path(), &args_ref),
            "status case mismatch"
        );
    }
}

#[test]
fn format_patch_cover_letter_subject_output_and_empty_default_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "f", "a\n");
    git(repo.path(), ["add", "f"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["checkout", "-b", "rebuild-1"]);
    write_file(repo.path(), "f", "a\nb\n");
    git(repo.path(), ["add", "f"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "f", "a\nb\nc\n");
    git(repo.path(), ["add", "f"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    git(
        repo.path(),
        ["config", "branch.rebuild-1.description", "Café?\n\nbody"],
    );

    let cover_utf8_args = [
        "format-patch",
        "--stdout",
        "--cover-letter",
        "--cover-from-description",
        "subject",
        "--encode-email-headers",
        "main",
    ];
    let stock = git_args(repo.path(), &cover_utf8_args);
    let zmin = run_zmin_args(repo.path(), &cover_utf8_args);
    assert_eq!(
        normalize_format_patch_dates(&normalize_format_patch_version(&zmin)),
        normalize_format_patch_dates(&normalize_format_patch_version(&stock))
    );

    let zmin_out_repo = clone_repo_fixture(repo.path());
    let zmin_stdout = run_zmin_args(
        zmin_out_repo.path(),
        &["format-patch", "--cover-letter", "-3", "--stdout", "HEAD"],
    );
    let _ = command_any_output(
        zmin_bin(),
        zmin_out_repo.path(),
        &[
            "format-patch",
            "--cover-letter",
            "-3",
            "--output=outfile",
            "HEAD",
        ],
        "zmin format-patch output",
    );
    let zmin_file =
        fs::read_to_string(zmin_out_repo.path().join("outfile")).expect("read zmin outfile");
    assert_eq!(
        normalize_format_patch_dates(&normalize_format_patch_version(&zmin_file)),
        normalize_format_patch_dates(&normalize_format_patch_version(&zmin_stdout))
    );

    let empty_repo = git_init();
    configure_identity(empty_repo.path());
    write_file(empty_repo.path(), "f", "a\n");
    git(empty_repo.path(), ["add", "f"]);
    git_with_env(empty_repo.path(), ["commit", "-m", "base"]);
    let stock_empty = git_args(
        empty_repo.path(),
        &["format-patch", "--stdout", "--cover-letter"],
    );
    let zmin_empty = run_zmin_args(
        empty_repo.path(),
        &["format-patch", "--stdout", "--cover-letter"],
    );
    assert_eq!(zmin_empty, stock_empty);
    assert!(zmin_empty.is_empty());
}

#[test]
fn am_applies_stock_format_patch_mail_like_stock_git() {
    let repo = format_patch_fixture_repo();
    let base = git(repo.path(), ["rev-parse", "HEAD~2"]);
    git(
        repo.path(),
        ["format-patch", "-o", "stock-patches", "HEAD~2..HEAD"],
    );
    let patch_names = read_named_files(&repo.path().join("stock-patches"))
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>();

    let git_apply = clone_repo_fixture(repo.path());
    let zmin_apply = clone_repo_fixture(repo.path());
    configure_identity(git_apply.path());
    configure_identity(zmin_apply.path());
    git(git_apply.path(), ["reset", "--hard", &base]);
    git(zmin_apply.path(), ["reset", "--hard", &base]);

    for patch in patch_names {
        let path = repo.path().join("stock-patches").join(patch);
        let path = path.to_str().expect("patch path utf8");
        git(git_apply.path(), ["am", path]);
        run_zmin_with_env(zmin_apply.path(), ["am", path]);
    }

    assert_eq!(
        git(zmin_apply.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_apply.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(zmin_apply.path(), ["log", "--format=%an <%ae>%n%s", "-2"]),
        git(git_apply.path(), ["log", "--format=%an <%ae>%n%s", "-2"])
    );
    assert_eq!(git(zmin_apply.path(), ["status", "--short"]), "");
}

#[test]
fn format_patch_single_since_revision_matches_stock_git() {
    let repo = format_patch_fixture_repo();
    let args = ["format-patch", "--stdout", "HEAD~1"];
    let zmin = run_zmin_args(repo.path(), &args);
    let stock = git_args(repo.path(), &args);

    assert_eq!(
        normalize_format_patch_version(&zmin),
        normalize_format_patch_version(&stock)
    );
    assert_eq!(
        run_zmin_status_args(repo.path(), &args),
        git_status_args(repo.path(), &args)
    );
}

#[test]
fn format_patch_output_directory_writes_cover_letter_like_stock_git() {
    let repo = format_patch_fixture_repo();

    let zmin_dir = repo.path().join("zmin-patches");
    let stock_dir = repo.path().join("stock-patches");
    fs::create_dir_all(&zmin_dir).expect("create zmin patch dir");
    fs::create_dir_all(&stock_dir).expect("create stock patch dir");

    let args = [
        "format-patch",
        "--cover-letter",
        "-o",
        "zmin-patches",
        "HEAD~1",
    ];
    run_zmin_args(repo.path(), &args);
    git(
        repo.path(),
        [
            "format-patch",
            "--cover-letter",
            "-o",
            "stock-patches",
            "HEAD~1",
        ],
    );

    assert_eq!(
        normalize_format_patch_named_files(read_named_files(&zmin_dir)),
        normalize_format_patch_named_files(read_named_files(&stock_dir)),
        "cover-letter file output mismatch"
    );
}

#[test]
fn format_patch_respects_format_numbered_config_like_stock_git() {
    let repo = format_patch_fixture_repo();
    git(repo.path(), ["config", "format.numbered", "true"]);

    let args = ["format-patch", "--stdout", "HEAD^"];
    let zmin = run_zmin_args(repo.path(), &args);
    let stock = git_args(repo.path(), &args);

    assert_eq!(
        normalize_format_patch_version(&zmin),
        normalize_format_patch_version(&stock)
    );
    assert_eq!(
        run_zmin_status_args(repo.path(), &args),
        git_status_args(repo.path(), &args)
    );
}

#[test]
fn format_patch_commit_list_format_auto_cover_letter_matches_stock_git() {
    let repo = format_patch_fixture_repo();
    let stock_args = [
        "format-patch",
        "--commit-list-format=log:[%(count)/%(total)] %s (%an)",
        "-o",
        "stock-patches",
        "HEAD~1",
    ];
    let (status, _, stderr) = command_any_output("git", repo.path(), &stock_args, "git");
    if status != 0 && stderr.contains("unrecognized argument: --commit-list-format") {
        return;
    }

    let zmin_dir = repo.path().join("zmin-patches");
    let stock_dir = repo.path().join("stock-patches");
    fs::create_dir_all(&zmin_dir).expect("create zmin patch dir");
    fs::create_dir_all(&stock_dir).expect("create stock patch dir");

    let args = [
        "format-patch",
        "--commit-list-format=log:[%(count)/%(total)] %s (%an)",
        "-o",
        "zmin-patches",
        "HEAD~1",
    ];
    run_zmin_args(repo.path(), &args);
    git(repo.path(), stock_args);

    assert_eq!(
        normalize_format_patch_named_files(read_named_files(&zmin_dir)),
        normalize_format_patch_named_files(read_named_files(&stock_dir)),
        "commit-list-format file output mismatch"
    );
}

#[test]
fn format_patch_filename_max_length_matches_stock_git() {
    let repo = format_patch_long_subject_fixture_repo();

    let zmin_dir = repo.path().join("zmin-patches");
    let stock_dir = repo.path().join("stock-patches");
    fs::create_dir_all(&zmin_dir).expect("create zmin patch dir");
    fs::create_dir_all(&stock_dir).expect("create stock patch dir");

    let args = [
        "format-patch",
        "-o",
        "zmin-patches",
        "--filename-max-length=15",
        "HEAD~2..HEAD",
    ];
    run_zmin_args(repo.path(), &args);
    git(
        repo.path(),
        [
            "format-patch",
            "-o",
            "stock-patches",
            "--filename-max-length=15",
            "HEAD~2..HEAD",
        ],
    );

    let zmin_files = read_named_files(&zmin_dir)
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    let stock_files = read_named_files(&stock_dir)
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>();

    assert_eq!(
        zmin_files, stock_files,
        "filename-max-length names mismatch"
    );
}

fn am_single_patch_fixture() -> (TempDir, String, String) {
    let repo = format_patch_fixture_repo();
    let base = git(repo.path(), ["rev-parse", "HEAD~1"]);
    git(
        repo.path(),
        ["format-patch", "-o", "stock-patches", "HEAD~1"],
    );
    let patch_name = read_named_files(&repo.path().join("stock-patches"))
        .into_iter()
        .map(|(name, _)| name)
        .next()
        .expect("single am patch");
    let patch_path = repo.path().join("stock-patches").join(patch_name);
    (repo, base, patch_path.to_string_lossy().into_owned())
}

fn am_conflict_patch_fixture() -> (TempDir, String, String) {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\nbase\n");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    write_file(repo.path(), "a.txt", "one\nupstream\n");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "upstream"]);
    git(repo.path(), ["format-patch", "-o", "patches", "HEAD~1"]);
    let base = git(repo.path(), ["rev-parse", "HEAD~1"]);
    let patch_name = read_named_files(&repo.path().join("patches"))
        .into_iter()
        .map(|(name, _)| name)
        .next()
        .expect("conflict patch");
    let patch_path = repo.path().join("patches").join(patch_name);
    (repo, base, patch_path.to_string_lossy().into_owned())
}

fn am_directory_patch_fixture() -> (TempDir, String, String) {
    let source = git_init();
    configure_identity(source.path());
    git(source.path(), ["checkout", "-b", "main"]);
    write_file(source.path(), "base.txt", "base\n");
    git(source.path(), ["add", "base.txt"]);
    git_with_env(source.path(), ["commit", "-m", "base"]);
    write_file(source.path(), "alpha.txt", "alpha\n");
    git(source.path(), ["add", "alpha.txt"]);
    git_with_env(source.path(), ["commit", "-m", "add alpha"]);
    git(
        source.path(),
        ["format-patch", "-o", "stock-patches", "HEAD~1"],
    );
    let patch_name = read_named_files(&source.path().join("stock-patches"))
        .into_iter()
        .map(|(name, _)| name)
        .next()
        .expect("directory patch");
    let source_patch_path = source.path().join("stock-patches").join(patch_name);

    let target = git_init();
    configure_identity(target.path());
    git(target.path(), ["checkout", "-b", "main"]);
    write_file(target.path(), "base.txt", "base\n");
    fs::create_dir_all(target.path().join("subdir")).expect("create subdir");
    git(target.path(), ["add", "base.txt"]);
    git_with_env(target.path(), ["commit", "-m", "base"]);
    let patch_path = target.path().join("directory.patch");
    fs::copy(&source_patch_path, &patch_path).expect("copy directory patch");

    (
        target,
        patch_path.to_string_lossy().into_owned(),
        "subdir".into(),
    )
}

fn am_empty_mail_fixture() -> (TempDir, String) {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "base.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    let mail = repo.path().join("empty-mail.txt");
    fs::write(
        &mail,
        concat!(
            "From nobody Mon Sep 17 00:00:00 2001\n",
            "From: Bench <bench@example.test>\n",
            "Date: Mon, 1 Jan 2024 00:00:00 +0000\n",
            "Subject: [PATCH] empty message\n",
            "\n",
            "This mail has no diff.\n",
        ),
    )
    .expect("write empty am mail");
    (repo, mail.to_string_lossy().into_owned())
}

#[test]
fn am_option_surface_batch_matches_stock_git() {
    let (source, base, patch_path) = am_single_patch_fixture();
    let patch = patch_path.as_str();
    let success_cases: [(&str, &[&str]); 46] = [
        ("--quiet", &["am", "--quiet", patch]),
        ("-q", &["am", "-q", patch]),
        ("--utf8", &["am", "--utf8", patch]),
        ("-u", &["am", "-u", patch]),
        ("--no-utf8", &["am", "--no-utf8", patch]),
        ("--keep", &["am", "--keep", patch]),
        ("-k", &["am", "-k", patch]),
        ("--keep-non-patch", &["am", "--keep-non-patch", patch]),
        ("--signoff", &["am", "--signoff", patch]),
        ("-s", &["am", "-s", patch]),
        ("--keep-cr", &["am", "--keep-cr", patch]),
        ("--no-keep-cr", &["am", "--no-keep-cr", patch]),
        ("--message-id", &["am", "--message-id", patch]),
        ("-m", &["am", "-m", patch]),
        ("--no-message-id", &["am", "--no-message-id", patch]),
        ("--scissors", &["am", "--scissors", patch]),
        ("-c", &["am", "-c", patch]),
        ("--no-scissors", &["am", "--no-scissors", patch]),
        ("--quoted-cr=warn", &["am", "--quoted-cr=warn", patch]),
        ("--quoted-cr=nowarn", &["am", "--quoted-cr=nowarn", patch]),
        ("--quoted-cr=strip", &["am", "--quoted-cr=strip", patch]),
        ("--3way", &["am", "--3way", patch]),
        ("-3", &["am", "-3", patch]),
        ("--no-3way", &["am", "--no-3way", patch]),
        (
            "--ignore-space-change",
            &["am", "--ignore-space-change", patch],
        ),
        ("--ignore-whitespace", &["am", "--ignore-whitespace", patch]),
        ("--whitespace=warn", &["am", "--whitespace=warn", patch]),
        ("-C1", &["am", "-C1", patch]),
        ("-p1", &["am", "-p1", patch]),
        ("--include=alpha.txt", &["am", "--include=alpha.txt", patch]),
        ("--exclude=alpha.txt", &["am", "--exclude=alpha.txt", patch]),
        (
            "--patch-format=mboxrd",
            &["am", "--patch-format=mboxrd", patch],
        ),
        ("--patch-format=mbox", &["am", "--patch-format=mbox", patch]),
        ("--patch-format=hg", &["am", "--patch-format=hg", patch]),
        (
            "--patch-format=stgit",
            &["am", "--patch-format=stgit", patch],
        ),
        ("--empty=stop", &["am", "--empty=stop", patch]),
        ("--empty=drop", &["am", "--empty=drop", patch]),
        ("--reject", &["am", "--reject", patch]),
        ("--gpg-sign", &["am", "--gpg-sign", patch]),
        ("--no-gpg-sign", &["am", "--no-gpg-sign", patch]),
        ("-S", &["am", "-S", patch]),
        ("--rerere-autoupdate", &["am", "--rerere-autoupdate", patch]),
        (
            "--no-rerere-autoupdate",
            &["am", "--no-rerere-autoupdate", patch],
        ),
        ("--resolvemsg=hello", &["am", "--resolvemsg=hello", patch]),
        ("--no-verify", &["am", "--no-verify", patch]),
        ("-n", &["am", "-n", patch]),
    ];
    for (label, args) in success_cases {
        let git_apply = clone_repo_fixture(source.path());
        let zmin_apply = clone_repo_fixture(source.path());
        configure_identity(git_apply.path());
        configure_identity(zmin_apply.path());
        git(git_apply.path(), ["reset", "--hard", &base]);
        git(zmin_apply.path(), ["reset", "--hard", &base]);

        let git_result = command_any_output("git", git_apply.path(), args, "git");
        let zmin_result = command_any_output(zmin_bin(), zmin_apply.path(), args, "zmin");

        assert_eq!(zmin_result, git_result, "args: {args:?}");
        assert_eq!(
            git(zmin_apply.path(), ["rev-parse", "HEAD^{tree}"]),
            git(git_apply.path(), ["rev-parse", "HEAD^{tree}"]),
            "tree args: {args:?}"
        );
        assert_eq!(
            git(zmin_apply.path(), ["log", "--format=%s%n%B", "-1"]),
            git(git_apply.path(), ["log", "--format=%s%n%B", "-1"]),
            "log args: {args:?}"
        );
        assert_eq!(
            git(zmin_apply.path(), ["status", "--short"]),
            git(git_apply.path(), ["status", "--short"]),
            "status args: {args:?}"
        );
        assert!(!label.is_empty());
    }

    let invalid_cases: [(&str, &[&str]); 2] = [
        ("--interactive", &["am", "--interactive", patch]),
        ("-i", &["am", "-i", patch]),
    ];
    for (label, args) in invalid_cases {
        let git_apply = clone_repo_fixture(source.path());
        let zmin_apply = clone_repo_fixture(source.path());
        configure_identity(git_apply.path());
        configure_identity(zmin_apply.path());
        git(git_apply.path(), ["reset", "--hard", &base]);
        git(zmin_apply.path(), ["reset", "--hard", &base]);

        let git_result = command_any_output("git", git_apply.path(), args, "git");
        let zmin_result = command_any_output(zmin_bin(), zmin_apply.path(), args, "zmin");
        assert_eq!(zmin_result, git_result, "args: {args:?}");
        assert_eq!(
            git(zmin_apply.path(), ["status", "--short"]),
            git(git_apply.path(), ["status", "--short"]),
            "status args: {args:?}"
        );
        assert!(!label.is_empty());
    }

    let git_apply = clone_repo_fixture(source.path());
    let zmin_apply = clone_repo_fixture(source.path());
    configure_identity(git_apply.path());
    configure_identity(zmin_apply.path());
    git(git_apply.path(), ["reset", "--hard", &base]);
    git(zmin_apply.path(), ["reset", "--hard", &base]);
    let patch_stdin = fs::read_to_string(patch).expect("read stgit-series patch");
    let git_result = command_any_output_with_stdin(
        "git",
        git_apply.path(),
        &["am", "--patch-format=stgit-series"],
        &patch_stdin,
        "git",
    );
    let zmin_result = command_any_output_with_stdin(
        zmin_bin(),
        zmin_apply.path(),
        &["am", "--patch-format=stgit-series"],
        &patch_stdin,
        "zmin",
    );
    assert_eq!(
        zmin_result,
        git_result,
        "args: {:?}",
        ["am", "--patch-format=stgit-series"]
    );
    assert_eq!(
        git(zmin_apply.path(), ["status", "--short"]),
        git(git_apply.path(), ["status", "--short"]),
        "status args: {:?}",
        ["am", "--patch-format=stgit-series"]
    );
}

#[test]
fn am_directory_ignore_date_and_reject_tail_matches_stock_git() {
    let (repo, patch_path, directory) = am_directory_patch_fixture();
    let patch = patch_path.as_str();
    let directory_equals = format!("--directory={directory}");
    let directory_unused = "--directory=unused".to_owned();
    for args in [
        ["am", directory_equals.as_str(), patch].as_slice(),
        ["am", "--directory", directory.as_str(), patch].as_slice(),
        [
            "am",
            directory_unused.as_str(),
            directory_equals.as_str(),
            patch,
        ]
        .as_slice(),
    ] {
        let git_repo = clone_repo_fixture(repo.path());
        let zmin_repo = clone_repo_fixture(repo.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        let git_result = command_any_output("git", git_repo.path(), args, "git");
        let zmin_result = command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin");
        assert_eq!(zmin_result, git_result, "directory args: {args:?}");
        assert_eq!(
            git(zmin_repo.path(), ["rev-parse", "HEAD^{tree}"]),
            git(git_repo.path(), ["rev-parse", "HEAD^{tree}"]),
            "directory tree args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            git(git_repo.path(), ["status", "--short"]),
            "directory status args: {args:?}"
        );
    }

    let (source, base, patch_path) = am_single_patch_fixture();
    let patch = patch_path.as_str();
    let envs = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1900000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1800000000 +0000"),
    ];
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    git(git_repo.path(), ["reset", "--hard", &base]);
    git(zmin_repo.path(), ["reset", "--hard", &base]);
    let git_result = command_output_with_env(
        "git",
        git_repo.path(),
        &["am", "--ignore-date", patch],
        &envs,
        "git",
    );
    let zmin_result = command_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &["am", "--ignore-date", patch],
        &envs,
        "zmin",
    );
    assert_eq!(zmin_result.0, git_result.0);
    assert_eq!(zmin_result.1, git_result.1);
    assert_eq!(
        git(
            zmin_repo.path(),
            [
                "log",
                "-1",
                "--format=%an <%ae>%n%cn <%ce>%n%cd%n%B",
                "--date=raw"
            ]
        ),
        git(
            git_repo.path(),
            [
                "log",
                "-1",
                "--format=%an <%ae>%n%cn <%ce>%n%cd%n%B",
                "--date=raw"
            ]
        )
    );
    let zmin_author = git(
        zmin_repo.path(),
        ["log", "-1", "--format=%ad", "--date=raw"],
    );
    let git_author = git(git_repo.path(), ["log", "-1", "--format=%ad", "--date=raw"]);
    assert_eq!(zmin_author, git_author);

    let git_combo_repo = clone_repo_fixture(source.path());
    let zmin_combo_repo = clone_repo_fixture(source.path());
    configure_identity(git_combo_repo.path());
    configure_identity(zmin_combo_repo.path());
    git(git_combo_repo.path(), ["reset", "--hard", &base]);
    git(zmin_combo_repo.path(), ["reset", "--hard", &base]);
    let combo_args = [
        "am",
        "--ignore-date",
        "--committer-date-is-author-date",
        patch,
    ];
    let git_combo_result =
        command_output_with_env("git", git_combo_repo.path(), &combo_args, &envs, "git");
    let zmin_combo_result = command_output_with_env(
        zmin_bin(),
        zmin_combo_repo.path(),
        &combo_args,
        &envs,
        "zmin",
    );
    assert_eq!(zmin_combo_result, git_combo_result);
    assert_eq!(
        git(
            zmin_combo_repo.path(),
            ["log", "-1", "--format=%ad%n%cd%n%B", "--date=raw"]
        ),
        git(
            git_combo_repo.path(),
            ["log", "-1", "--format=%ad%n%cd%n%B", "--date=raw"]
        )
    );

    let (source, base, patch_path) = am_conflict_patch_fixture();
    let patch = patch_path.as_str();
    for args in [
        ["am", "--reject", patch].as_slice(),
        ["am", "--reject", "--reject", patch].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["reset", "--hard", &base]);
        git(zmin_repo.path(), ["reset", "--hard", &base]);
        write_file(git_repo.path(), "a.txt", "one\nlocal\n");
        write_file(zmin_repo.path(), "a.txt", "one\nlocal\n");
        git(git_repo.path(), ["add", "a.txt"]);
        git(zmin_repo.path(), ["add", "a.txt"]);
        git_with_env(git_repo.path(), ["commit", "-m", "local"]);
        git_with_env(zmin_repo.path(), ["commit", "-m", "local"]);
        let git_result = command_any_output("git", git_repo.path(), args, "git");
        let zmin_result = command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin");
        assert_eq!(zmin_result, git_result, "reject args: {args:?}");
        assert_eq!(
            fs::read_to_string(zmin_repo.path().join("a.txt.rej")).expect("zmin reject file"),
            fs::read_to_string(git_repo.path().join("a.txt.rej")).expect("git reject file"),
            "reject file args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            git(git_repo.path(), ["status", "--short"]),
            "reject status args: {args:?}"
        );
    }
}

#[test]
fn am_empty_mail_family_matches_stock_git() {
    let (source, mail_path) = am_empty_mail_fixture();
    let mail = mail_path.as_str();

    for (label, args) in [
        ("empty-keep", ["am", "--empty=keep", mail].as_slice()),
        ("empty-drop", ["am", "--empty=drop", mail].as_slice()),
        ("empty-stop", ["am", "--empty=stop", mail].as_slice()),
    ] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());

        let git_result = command_any_output("git", git_repo.path(), args, "git");
        let zmin_result = command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin");

        assert_eq!(zmin_result, git_result, "case {label}: args {args:?}");
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            git(git_repo.path(), ["status", "--short"]),
            "status case {label}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["log", "--format=%s%n%B", "-1"]),
            git(git_repo.path(), ["log", "--format=%s%n%B", "-1"]),
            "log case {label}"
        );
    }

    let session_cases: [(&str, &[&str]); 10] = [
        ("show-raw", &["am", "--show-current-patch=raw"]),
        ("show-diff", &["am", "--show-current-patch=diff"]),
        ("allow-empty", &["am", "--allow-empty"]),
        ("continue", &["am", "--continue"]),
        ("resolved", &["am", "--resolved"]),
        ("-r", &["am", "-r"]),
        ("retry", &["am", "--retry"]),
        ("skip", &["am", "--skip"]),
        ("abort", &["am", "--abort"]),
        ("quit", &["am", "--quit"]),
    ];

    for (label, args) in session_cases {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        let _ = command_any_output("git", git_repo.path(), &["am", "--empty=stop", mail], "git");
        let _ = command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &["am", "--empty=stop", mail],
            "zmin",
        );

        let git_result = command_any_output("git", git_repo.path(), args, "git");
        let zmin_result = command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin");

        assert_eq!(zmin_result, git_result, "case {label}: args {args:?}");
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            git(git_repo.path(), ["status", "--short"]),
            "status case {label}"
        );
    }
}

#[test]
fn am_committer_date_is_author_date_matches_stock_git() {
    let (source, base, patch_path) = am_single_patch_fixture();
    let patch = patch_path.as_str();
    let args = ["am", "--committer-date-is-author-date", patch];
    let envs = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000200 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000300 +0000"),
    ];

    let git_apply = clone_repo_fixture(source.path());
    let zmin_apply = clone_repo_fixture(source.path());
    configure_identity(git_apply.path());
    configure_identity(zmin_apply.path());
    git(git_apply.path(), ["reset", "--hard", &base]);
    git(zmin_apply.path(), ["reset", "--hard", &base]);

    let git_result = command_output_with_env("git", git_apply.path(), &args, &envs, "git");
    let zmin_result = command_output_with_env(zmin_bin(), zmin_apply.path(), &args, &envs, "zmin");
    assert_eq!(zmin_result, git_result);
    assert_eq!(
        git(
            zmin_apply.path(),
            ["log", "-1", "--format=%ad%n%cd%n%s%n%B", "--date=raw"]
        ),
        git(
            git_apply.path(),
            ["log", "-1", "--format=%ad%n%cd%n%s%n%B", "--date=raw"]
        )
    );
    assert_eq!(
        git(zmin_apply.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_apply.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(zmin_apply.path(), ["status", "--short"]),
        git(git_apply.path(), ["status", "--short"])
    );
}

#[test]
fn am_resume_only_flags_without_session_match_stock_git() {
    let (_source, _base, patch_path) = am_single_patch_fixture();
    let patch = patch_path.as_str();
    let failure_cases: [(&str, &[&str]); 10] = [
        ("--allow-empty", &["am", "--allow-empty", patch]),
        ("--abort", &["am", "--abort"]),
        ("--quit", &["am", "--quit"]),
        ("--skip", &["am", "--skip"]),
        ("--continue", &["am", "--continue"]),
        ("--resolved", &["am", "--resolved"]),
        ("-r", &["am", "-r"]),
        ("--retry", &["am", "--retry"]),
        (
            "--show-current-patch=raw",
            &["am", "--show-current-patch=raw"],
        ),
        (
            "--show-current-patch=diff",
            &["am", "--show-current-patch=diff"],
        ),
    ];
    for (label, args) in failure_cases {
        let repo = git_init();
        configure_identity(repo.path());
        write_file(repo.path(), "a.txt", "base\n");
        git(repo.path(), ["add", "a.txt"]);
        git_with_env(repo.path(), ["commit", "-m", "base"]);

        let git_result = git_failure_output(repo.path(), args);
        let zmin_result = run_zmin_failure_output(repo.path(), args);
        assert_eq!(zmin_result, git_result, "args: {args:?}");
        assert_eq!(git(repo.path(), ["status", "--short"]), "");
        assert!(!label.is_empty());
    }
}

#[test]
fn am_conflict_session_family_matches_stock_git() {
    let (source, base, patch_path) = am_conflict_patch_fixture();
    let patch = patch_path.as_str();

    let conflict_cases: [(&str, &[&str]); 7] = [
        ("initial", &["am", patch]),
        ("show-raw", &["am", "--show-current-patch=raw"]),
        ("show-diff", &["am", "--show-current-patch=diff"]),
        ("retry", &["am", "--retry"]),
        ("continue", &["am", "--continue"]),
        ("resolved", &["am", "--resolved"]),
        ("-r", &["am", "-r"]),
    ];

    let git_apply = clone_repo_fixture(source.path());
    let zmin_apply = clone_repo_fixture(source.path());
    configure_identity(git_apply.path());
    configure_identity(zmin_apply.path());
    git(git_apply.path(), ["reset", "--hard", &base]);
    git(zmin_apply.path(), ["reset", "--hard", &base]);
    write_file(git_apply.path(), "a.txt", "one\nlocal\n");
    write_file(zmin_apply.path(), "a.txt", "one\nlocal\n");
    git(git_apply.path(), ["add", "a.txt"]);
    git(zmin_apply.path(), ["add", "a.txt"]);
    git_with_env(git_apply.path(), ["commit", "-m", "local"]);
    git_with_env(zmin_apply.path(), ["commit", "-m", "local"]);

    for (label, args) in conflict_cases {
        let git_result = command_any_output("git", git_apply.path(), args, "git");
        let zmin_result = command_any_output(zmin_bin(), zmin_apply.path(), args, "zmin");
        assert_eq!(zmin_result, git_result, "case {label}: args {args:?}");
        assert_eq!(
            git(zmin_apply.path(), ["status", "--short"]),
            git(git_apply.path(), ["status", "--short"]),
            "status case {label}"
        );
    }

    for (label, cleanup_args) in [
        ("skip", ["am", "--skip"].as_slice()),
        ("abort", ["am", "--abort"].as_slice()),
        ("quit", ["am", "--quit"].as_slice()),
    ] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["reset", "--hard", &base]);
        git(zmin_repo.path(), ["reset", "--hard", &base]);
        write_file(git_repo.path(), "a.txt", "one\nlocal\n");
        write_file(zmin_repo.path(), "a.txt", "one\nlocal\n");
        git(git_repo.path(), ["add", "a.txt"]);
        git(zmin_repo.path(), ["add", "a.txt"]);
        git_with_env(git_repo.path(), ["commit", "-m", "local"]);
        git_with_env(zmin_repo.path(), ["commit", "-m", "local"]);
        let _ = command_any_output("git", git_repo.path(), &["am", patch], "git");
        let _ = command_any_output(zmin_bin(), zmin_repo.path(), &["am", patch], "zmin");

        let git_result = command_any_output("git", git_repo.path(), cleanup_args, "git");
        let zmin_result = command_any_output(zmin_bin(), zmin_repo.path(), cleanup_args, "zmin");
        assert_eq!(
            zmin_result, git_result,
            "case {label}: args {cleanup_args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            git(git_repo.path(), ["status", "--short"]),
            "status case {label}"
        );
    }
}

#[test]
fn range_diff_matches_stock_git_for_patch_equivalence() {
    let repo = range_diff_fixture_repo();
    for args in [
        ["range-diff", "main..old", "main..new"].as_slice(),
        ["range-diff", "main", "old", "new"].as_slice(),
        ["range-diff", "--no-dual-color", "main..old", "main..new"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn range_diff_documented_tail_matches_stock_git() {
    let repo = range_diff_fixture_repo();
    for args in [
        [
            "range-diff",
            "--creation-factor=70",
            "main..old",
            "main..new",
        ]
        .as_slice(),
        ["range-diff", "--left-only", "main..old", "main..new"].as_slice(),
        ["range-diff", "--right-only", "main..old", "main..new"].as_slice(),
        ["range-diff", "--notes", "main..old", "main..new"].as_slice(),
        ["range-diff", "--no-notes", "main..old", "main..new"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn sha256_range_diff_abbrev_matches_pinned_git() {
    let repo = pinned_git_init_sha256();
    pinned_git_args(repo.path(), &["config", "user.name", "Bench"]);
    pinned_git_args(repo.path(), &["config", "user.email", "bench@example.test"]);
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
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
    pinned_git_args(repo.path(), &["checkout", "-b", "old"]);
    write_file(repo.path(), "old.txt", "old\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_git_with_env(
        repo.path(),
        &["commit", "-m", "old"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000001 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000001 +0000"),
        ],
    );
    pinned_git_args(repo.path(), &["checkout", "main"]);
    pinned_git_args(repo.path(), &["checkout", "-b", "new"]);
    write_file(repo.path(), "new.txt", "new\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_git_with_env(
        repo.path(),
        &["commit", "-m", "new"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000002 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000002 +0000"),
        ],
    );
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    for args in [
        ["range-diff", "main..old", "main..new"].as_slice(),
        ["range-diff", "--no-dual-color", "main..old", "main..new"].as_slice(),
    ] {
        assert_eq!(
            command_raw_output(zmin_bin(), repo.path(), args, "zmin"),
            command_raw_output(stock, repo.path(), args, "stock Git"),
            "SHA-256 range-diff tuple: {args:?}"
        );
    }
}
