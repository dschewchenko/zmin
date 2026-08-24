mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use flate2::{Compression, write::ZlibEncoder};
use tempfile::TempDir;
use zmin_git_core::{GitHashAlgorithm, GitObjectHash};

use common::{
    RawCommandOutput, clone_repo_fixture, command_any_output, command_any_output_with_stdin,
    command_output, command_output_with_env, command_raw_output, command_stdout_bytes,
    configure_identity, git, git_args, git_failure_output, git_init, git_status, git_with_env,
    pinned_git_args, pinned_git_init_sha256, pinned_git_with_env, required_pinned_stock_git,
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

fn loose_object_id(kind: &str, content: &[u8]) -> String {
    loose_object_id_for_algorithm(GitHashAlgorithm::Sha1, kind, content)
}

fn loose_object_id_for_algorithm(
    algorithm: GitHashAlgorithm,
    kind: &str,
    content: &[u8],
) -> String {
    let mut object = format!("{kind} {}\0", content.len()).into_bytes();
    object.extend_from_slice(content);
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update(&object);
    hasher.finalize().to_hex()
}

fn decode_hex_object_id(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("hex pair"), 16)
                .expect("hex object id")
        })
        .collect()
}

fn write_loose_object(repo: &std::path::Path, kind: &str, content: &[u8]) -> String {
    write_loose_object_for_algorithm(repo, GitHashAlgorithm::Sha1, kind, content)
}

fn write_loose_object_for_algorithm(
    repo: &std::path::Path,
    algorithm: GitHashAlgorithm,
    kind: &str,
    content: &[u8],
) -> String {
    let mut object = format!("{kind} {}\0", content.len()).into_bytes();
    object.extend_from_slice(content);
    let id = loose_object_id_for_algorithm(algorithm, kind, content);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&object).expect("compress loose object");
    let compressed = encoder.finish().expect("finish loose object");
    let object_dir = repo.join(".git/objects").join(&id[..2]);
    fs::create_dir_all(&object_dir).expect("create loose object directory");
    fs::write(object_dir.join(&id[2..]), compressed).expect("write loose object");
    id
}

fn pinned_sha1_repo() -> TempDir {
    let repo = TempDir::new().expect("temp SHA-1 repo");
    let output = Command::new(required_pinned_stock_git())
        .arg("init")
        .current_dir(repo.path())
        .output()
        .expect("run pinned Git SHA-1 init");
    assert!(
        output.status.success(),
        "pinned Git SHA-1 init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    repo
}

fn configure_pinned_identity(repo: &std::path::Path) {
    pinned_git_args(repo, &["config", "user.name", "Bench"]);
    pinned_git_args(repo, &["config", "user.email", "bench@example.test"]);
    pinned_git_args(repo, &["config", "commit.gpgsign", "false"]);
}

fn pinned_commit(repo: &std::path::Path, message: &str, date: &str) {
    let env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", date),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", date),
    ];
    pinned_git_with_env(repo, &["commit", "-m", message], &env);
}

fn pinned_history_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);

    write_file(repo.path(), "-ps", "base option path\n");
    write_file(repo.path(), "base.txt", "base\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "base", "1700000000 +0000");

    pinned_git_args(repo.path(), &["checkout", "-b", "side"]);
    write_file(repo.path(), "side.txt", "side\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "side", "1700000100 +0000");

    pinned_git_args(repo.path(), &["checkout", "main"]);
    write_file(repo.path(), "-ps", "main option path\n");
    write_file(repo.path(), "main.txt", "main\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "main", "1700000200 +0000");
    pinned_git_with_env(
        repo.path(),
        &["merge", "--no-ff", "-m", "merge", "side"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000300 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000300 +0000"),
        ],
    );

    pinned_git_args(
        repo.path(),
        &["tag", "-a", "annotated", "-m", "annotated", "HEAD~1"],
    );
    pinned_git_args(repo.path(), &["tag", "lightweight", "HEAD~2"]);
    pinned_git_args(repo.path(), &["pack-refs", "--all", "--prune"]);
    pinned_git_args(repo.path(), &["update-ref", "refs/heads/loose", "HEAD"]);
    pinned_git_args(repo.path(), &["repack", "-ad"]);
    write_file(repo.path(), "loose.txt", "loose object\n");
    pinned_git_args(repo.path(), &["hash-object", "-w", "loose.txt"]);

    repo
}

fn pinned_disk_reachable_loose_fixture(sha256: bool) -> TempDir {
    let repo = pinned_history_fixture(sha256);
    write_file(repo.path(), "reachable-loose.txt", "reachable loose\n");
    pinned_git_args(repo.path(), &["add", "reachable-loose.txt"]);
    pinned_commit(repo.path(), "reachable-loose", "1700000400 +0000");
    repo
}

fn pinned_path_history_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);

    write_file(repo.path(), "path.txt", "root\n");
    write_file(repo.path(), "noise.txt", "root\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "R", "1700000000 +0000");

    pinned_git_args(repo.path(), &["checkout", "-b", "side"]);
    write_file(repo.path(), "path.txt", "side-one\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "S1", "1700000100 +0000");
    write_file(repo.path(), "side-noise.txt", "side-two\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "S2", "1700000200 +0000");

    pinned_git_args(repo.path(), &["checkout", "main"]);
    write_file(repo.path(), "main-noise.txt", "main-one\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "M1", "1700000150 +0000");
    pinned_git_with_env(
        repo.path(),
        &["merge", "--no-ff", "-m", "P", "side"],
        &[
            ("GIT_AUTHOR_NAME", "Merge"),
            ("GIT_AUTHOR_EMAIL", "merge@example.test"),
            ("GIT_AUTHOR_DATE", "1700000300 +0000"),
            ("GIT_COMMITTER_NAME", "Merge"),
            ("GIT_COMMITTER_EMAIL", "merge@example.test"),
            ("GIT_COMMITTER_DATE", "1700000300 +0000"),
        ],
    );

    write_file(repo.path(), "noise.txt", "after-merge\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "N1", "1700000400 +0000");
    pinned_git_args(repo.path(), &["rm", "path.txt"]);
    pinned_commit(repo.path(), "D", "1700000500 +0000");
    write_file(repo.path(), "noise.txt", "after-delete\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "N2", "1700000600 +0000");
    write_file(repo.path(), "path.txt", "readded\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "Readd", "1700000700 +0000");

    repo
}

fn pinned_reused_subtree_path_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "first/shared.txt", "same subtree\n");
    write_file(repo.path(), "second/shared.txt", "same subtree\n");
    write_file(repo.path(), "noise.txt", "noise\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "reused subtree", "1700000000 +0000");
    repo
}

fn pinned_path_object_edge_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "path.txt", "root\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "R", "1700000000 +0000");
    write_file(repo.path(), "noise.txt", "middle\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "M", "1700000050 +0000");
    write_file(repo.path(), "path.txt", "tip\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "S", "1700000100 +0000");
    let tip = pinned_git_args(repo.path(), &["rev-parse", "HEAD"])
        .trim()
        .to_owned();
    pinned_git_args(repo.path(), &["checkout", "--orphan", "other"]);
    pinned_git_args(repo.path(), &["rm", "-rf", "."]);
    write_file(repo.path(), "other.txt", "other\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "N", "1700000200 +0000");
    pinned_git_args(repo.path(), &["checkout", "main"]);
    pinned_git_args(repo.path(), &["update-ref", "refs/heads/tip", tip.as_str()]);
    repo
}

fn pinned_author_date_children_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "history.txt", "root\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "root", "1700000000 +0000");
    pinned_git_args(repo.path(), &["checkout", "-b", "side"]);
    write_file(repo.path(), "history.txt", "side\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    git_commit_with_split_dates(
        repo.path(),
        "Side Author",
        "side@example.test",
        "1700000400 +0000",
        "Side Committer",
        "side-committer@example.test",
        "1700000100 +0000",
        "side",
    );
    pinned_git_args(repo.path(), &["checkout", "main"]);
    write_file(repo.path(), "history.txt", "main\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    git_commit_with_split_dates(
        repo.path(),
        "Main Author",
        "main@example.test",
        "1700000050 +0000",
        "Main Committer",
        "main-committer@example.test",
        "1700000300 +0000",
        "main",
    );
    repo
}

fn pinned_linear_children_grep_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "history.txt", "R\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "R", "1700000000 +0000");
    for (subject, timestamp) in [
        ("A", "1700000100 +0000"),
        ("B", "1700000200 +0000"),
        ("C", "1700000300 +0000"),
    ] {
        let content = format!("{subject}\n");
        write_file(repo.path(), "history.txt", &content);
        pinned_git_args(repo.path(), &["add", "-A"]);
        pinned_commit(repo.path(), subject, timestamp);
    }
    repo
}

fn pinned_sparse_explicit_root_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "path.txt", "path root\n");
    write_file(repo.path(), "noise.txt", "noise root\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "R", "1700000000 +0000");
    write_file(repo.path(), "noise.txt", "noise head\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "N", "1700000100 +0000");
    repo
}

fn pinned_all_treesame_merge_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "path.txt", "0\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "R", "1700000000 +0000");

    pinned_git_args(repo.path(), &["checkout", "-b", "side"]);
    write_file(repo.path(), "side.txt", "side\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "S", "1700000100 +0000");

    pinned_git_args(repo.path(), &["checkout", "main"]);
    write_file(repo.path(), "main.txt", "main\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "D", "1700000200 +0000");
    pinned_git_with_env(
        repo.path(),
        &["merge", "--no-ff", "-m", "M", "side"],
        &[
            ("GIT_AUTHOR_NAME", "Merge"),
            ("GIT_AUTHOR_EMAIL", "merge@example.test"),
            ("GIT_AUTHOR_DATE", "1700000300 +0000"),
            ("GIT_COMMITTER_NAME", "Merge"),
            ("GIT_COMMITTER_EMAIL", "merge@example.test"),
            ("GIT_COMMITTER_DATE", "1700000300 +0000"),
        ],
    );
    write_file(repo.path(), "path.txt", "1\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "E", "1700000400 +0000");
    repo
}

fn pinned_first_parent_changed_merge_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "p", "0\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "R", "1700000000 +0000");

    pinned_git_args(repo.path(), &["checkout", "-b", "side"]);
    write_file(repo.path(), "p", "1\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "S", "1700000100 +0000");

    pinned_git_args(repo.path(), &["checkout", "main"]);
    write_file(repo.path(), "p", "2\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "M", "1700000200 +0000");

    let merge = Command::new(required_pinned_stock_git())
        .args(["merge", "--no-ff", "--no-commit", "side"])
        .current_dir(repo.path())
        .output()
        .expect("run conflicting pinned merge");
    assert_eq!(merge.status.code(), Some(1));
    write_file(repo.path(), "p", "3\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "X", "1700000300 +0000");
    repo
}

fn pinned_nested_path_replacement_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "dir/file.txt", "old\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "root", "1700000000 +0000");

    fs::remove_dir_all(repo.path().join("dir")).expect("remove nested tree");
    write_file(repo.path(), "dir", "replacement\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "tree-to-blob", "1700000100 +0000");

    fs::remove_file(repo.path().join("dir")).expect("remove replacement blob");
    write_file(repo.path(), "dir/file.txt", "new\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "blob-to-tree", "1700000200 +0000");
    repo
}

fn pinned_equivalent_merge_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "base", "1700000000 +0000");

    pinned_git_args(repo.path(), &["checkout", "-b", "left"]);
    write_file(repo.path(), "equivalent.txt", "same\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "left-equivalent", "1700000100 +0000");
    write_file(repo.path(), "left.txt", "left\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "left-unique", "1700000200 +0000");
    let left_tip = pinned_git_args(repo.path(), &["rev-parse", "left"])
        .trim()
        .to_owned();

    pinned_git_args(repo.path(), &["checkout", "main"]);
    pinned_git_args(repo.path(), &["checkout", "-b", "right"]);
    write_file(repo.path(), "equivalent.txt", "same\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "right-equivalent", "1700000100 +0000");
    write_file(repo.path(), "right.txt", "right\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "right-unique", "1700000200 +0000");

    pinned_git_args(repo.path(), &["checkout", "left"]);
    pinned_git_with_env(
        repo.path(),
        &["merge", "--no-ff", "-m", "merge", "right"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000300 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000300 +0000"),
        ],
    );
    let merge_tip = pinned_git_args(repo.path(), &["rev-parse", "HEAD"])
        .trim()
        .to_owned();
    pinned_git_args(
        repo.path(),
        &["update-ref", "refs/heads/left", left_tip.as_str()],
    );
    pinned_git_args(repo.path(), &["branch", "merge", merge_tip.as_str()]);
    pinned_git_args(repo.path(), &["checkout", "merge"]);
    pinned_git_args(
        repo.path(),
        &["tag", "-a", "left-equivalent-tag", "-m", "tag", "left~2"],
    );
    write_file(repo.path(), "tagged-blob.txt", "tagged\n");
    let blob = pinned_git_args(repo.path(), &["hash-object", "-w", "tagged-blob.txt"]);
    pinned_git_args(
        repo.path(),
        &["tag", "-a", "blob-tag", "-m", "blob", blob.trim()],
    );
    pinned_git_args(repo.path(), &["pack-refs", "--all", "--prune"]);
    repo
}

fn pinned_disconnected_exclude_first_parent_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "base", "1700000000 +0000");

    pinned_git_args(repo.path(), &["checkout", "--orphan", "side"]);
    pinned_git_args(repo.path(), &["rm", "-rf", "."]);
    write_file(repo.path(), "unrelated.txt", "unrelated\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "unrelated", "1700000100 +0000");

    pinned_git_args(repo.path(), &["checkout", "main"]);
    pinned_git_with_env(
        repo.path(),
        &[
            "merge",
            "--allow-unrelated-histories",
            "--no-ff",
            "-m",
            "merge",
            "side",
        ],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000200 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000200 +0000"),
        ],
    );
    pinned_git_args(repo.path(), &["checkout", "--orphan", "unrelated-negative"]);
    pinned_git_args(repo.path(), &["rm", "-rf", "."]);
    write_file(repo.path(), "unrelated-negative.txt", "negative\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "unrelated-negative", "1700000300 +0000");
    pinned_git_args(repo.path(), &["checkout", "main"]);
    pinned_git_args(repo.path(), &["pack-refs", "--all", "--prune"]);
    repo
}

fn pinned_commit_tree(
    repo: &std::path::Path,
    tree: &str,
    parents: &[&str],
    message: &str,
    date: &str,
) -> String {
    let mut command = Command::new(required_pinned_stock_git());
    command.arg("commit-tree").arg(tree);
    for parent in parents {
        command.args(["-p", parent]);
    }
    command
        .env("GIT_AUTHOR_NAME", "Bench")
        .env("GIT_AUTHOR_EMAIL", "bench@example.test")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_NAME", "Bench")
        .env("GIT_COMMITTER_EMAIL", "bench@example.test")
        .env("GIT_COMMITTER_DATE", date)
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn pinned commit-tree");
    child
        .stdin
        .take()
        .expect("commit-tree stdin")
        .write_all(format!("{message}\n").as_bytes())
        .expect("write commit-tree message");
    let output = child.wait_with_output().expect("wait pinned commit-tree");
    assert!(
        output.status.success(),
        "pinned commit-tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("commit-tree output")
        .trim()
        .to_owned()
}

fn pinned_criss_cross_merge_base_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "base", "1700000000 +0000");
    pinned_git_args(repo.path(), &["checkout", "-b", "left"]);
    write_file(repo.path(), "left.txt", "left\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "left", "1700000100 +0000");
    let left_base = pinned_git_args(repo.path(), &["rev-parse", "HEAD"])
        .trim()
        .to_owned();
    pinned_git_args(repo.path(), &["checkout", "main"]);
    pinned_git_args(repo.path(), &["checkout", "-b", "right"]);
    write_file(repo.path(), "right.txt", "right\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "right", "1700000100 +0000");
    let right_base = pinned_git_args(repo.path(), &["rev-parse", "HEAD"])
        .trim()
        .to_owned();
    let tree = pinned_git_args(repo.path(), &["rev-parse", "HEAD^{tree}"])
        .trim()
        .to_owned();
    let left_tip = pinned_commit_tree(
        repo.path(),
        &tree,
        &[left_base.as_str(), right_base.as_str()],
        "left merge",
        "1700000200 +0000",
    );
    let right_tip = pinned_commit_tree(
        repo.path(),
        &tree,
        &[right_base.as_str(), left_base.as_str()],
        "right merge",
        "1700000201 +0000",
    );
    pinned_git_args(
        repo.path(),
        &["update-ref", "refs/heads/left", left_tip.as_str()],
    );
    pinned_git_args(
        repo.path(),
        &["update-ref", "refs/heads/right", right_tip.as_str()],
    );
    pinned_git_args(repo.path(), &["checkout", "left"]);
    pinned_git_args(repo.path(), &["pack-refs", "--all", "--prune"]);
    repo
}

fn pinned_disconnected_equal_time_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "left"]);

    write_file(repo.path(), "left.txt", "left one\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "left-one", "1700000000 +0000");
    write_file(repo.path(), "left.txt", "left two\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "left-two", "1700000000 +0000");

    pinned_git_args(repo.path(), &["checkout", "--orphan", "right"]);
    pinned_git_args(repo.path(), &["rm", "-rf", "."]);
    write_file(repo.path(), "right.txt", "right one\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "right-one", "1700000000 +0000");
    write_file(repo.path(), "right.txt", "right two\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "right-two", "1700000000 +0000");
    pinned_git_args(repo.path(), &["checkout", "left"]);
    repo
}

fn pinned_four_way_equal_time_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "root.txt", "root\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "R", "1700000000 +0000");
    for (branch, file, subject) in [
        ("a", "a.txt", "A"),
        ("b", "b.txt", "B"),
        ("c", "c.txt", "C"),
        ("d", "d.txt", "D"),
    ] {
        pinned_git_args(repo.path(), &["checkout", "main"]);
        pinned_git_args(repo.path(), &["checkout", "-b", branch]);
        write_file(repo.path(), file, &format!("{subject}\n"));
        pinned_git_args(repo.path(), &["add", "-A"]);
        pinned_commit(repo.path(), subject, "1700000100 +0000");
    }
    pinned_git_args(repo.path(), &["checkout", "a"]);
    repo
}

fn pinned_path_bisection_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "target.txt", "root\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "R", "1700000000 +0000");

    pinned_git_args(repo.path(), &["checkout", "-b", "side"]);
    write_file(repo.path(), "target.txt", "side\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "S", "1700000100 +0000");

    pinned_git_args(repo.path(), &["checkout", "main"]);
    write_file(repo.path(), "main-noise.txt", "noise\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "M1", "1700000150 +0000");
    pinned_git_with_env(
        repo.path(),
        &["merge", "--no-ff", "-m", "M", "side"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000200 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000200 +0000"),
        ],
    );
    repo
}

fn pinned_sparse_path_bisection_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "root.txt", "root\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "R", "1700000000 +0000");

    write_file(repo.path(), "main-noise.txt", "main\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "C", "1700000100 +0000");
    let main_branch = pinned_git_args(repo.path(), &["branch", "--show-current"]);

    pinned_git_args(repo.path(), &["checkout", "-q", "-b", "side"]);
    write_file(repo.path(), "side-noise.txt", "side\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "S", "1700000150 +0000");

    pinned_git_args(repo.path(), &["checkout", "-q", main_branch.as_str()]);
    pinned_git_with_env(
        repo.path(),
        &["merge", "--no-ff", "-m", "M", "side"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000200 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000200 +0000"),
        ],
    );
    write_file(repo.path(), "target.txt", "tip\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "T", "1700000300 +0000");
    repo
}

fn pinned_sparse_external_parent_bisection_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "target.txt", "base\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "B", "1700000000 +0000");

    pinned_git_args(repo.path(), &["checkout", "-b", "u"]);
    write_file(repo.path(), "target.txt", "u\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "U", "1700000100 +0000");
    let u = pinned_git_args(repo.path(), &["rev-parse", "refs/heads/u"])
        .trim()
        .to_owned();

    pinned_git_args(repo.path(), &["checkout", "main"]);
    pinned_git_args(repo.path(), &["checkout", "-b", "v"]);
    write_file(repo.path(), "v-noise.txt", "v\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "V", "1700000200 +0000");
    let v = pinned_git_args(repo.path(), &["rev-parse", "refs/heads/v"])
        .trim()
        .to_owned();
    let v_tree = pinned_git_args(repo.path(), &["rev-parse", "refs/heads/v^{tree}"])
        .trim()
        .to_owned();
    let merge = pinned_commit_tree(
        repo.path(),
        &v_tree,
        &[u.as_str(), v.as_str()],
        "M",
        "1700000300 +0000",
    );
    pinned_git_args(repo.path(), &["update-ref", "refs/heads/main", &merge]);
    repo
}

fn pinned_bisection_asymmetric_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "root.txt", "root\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "R", "1700000000 +0000");

    pinned_git_args(repo.path(), &["checkout", "-b", "left"]);
    write_file(repo.path(), "left.txt", "left\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "A1", "1700000100 +0000");

    pinned_git_args(repo.path(), &["checkout", "main"]);
    pinned_git_args(repo.path(), &["checkout", "-b", "right"]);
    write_file(repo.path(), "right.txt", "right one\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "B1", "1700000100 +0000");
    write_file(repo.path(), "right.txt", "right two\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "B2", "1700000200 +0000");
    repo
}

fn pinned_treesame_bisection_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "root.txt", "root\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "R", "1700000000 +0000");

    pinned_git_args(repo.path(), &["checkout", "-b", "side"]);
    write_file(repo.path(), "shared.txt", "shared\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "S", "1700000100 +0000");

    pinned_git_args(repo.path(), &["checkout", "main"]);
    write_file(repo.path(), "shared.txt", "shared\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "M1", "1700000150 +0000");
    pinned_git_with_env(
        repo.path(),
        &["merge", "--no-ff", "-m", "M", "side"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000200 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000200 +0000"),
        ],
    );
    write_file(repo.path(), "after.txt", "after\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "E", "1700000300 +0000");
    repo
}

fn pinned_date_order_topology_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);

    write_file(repo.path(), "root.txt", "root\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "root", "1700000000 +0000");
    write_file(repo.path(), "common.txt", "common\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "common", "1700000500 +0000");

    pinned_git_args(repo.path(), &["checkout", "-b", "left"]);
    write_file(repo.path(), "left.txt", "left\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "left", "1700000100 +0000");

    pinned_git_args(repo.path(), &["checkout", "main"]);
    pinned_git_args(repo.path(), &["checkout", "-b", "right"]);
    write_file(repo.path(), "right.txt", "right\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "right", "1700000300 +0000");

    pinned_git_args(repo.path(), &["checkout", "left"]);
    pinned_git_with_env(
        repo.path(),
        &["merge", "--no-ff", "-m", "merge", "right"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000400 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000400 +0000"),
        ],
    );
    repo
}

fn pinned_hidden_intermediate_history_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);

    write_file(repo.path(), "root.txt", "root\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_git_with_env(
        repo.path(),
        &["commit", "-m", "root"],
        &[
            ("GIT_AUTHOR_NAME", "Root"),
            ("GIT_AUTHOR_EMAIL", "root@example.test"),
            ("GIT_AUTHOR_DATE", "1700000300 +0000"),
            ("GIT_COMMITTER_NAME", "Root"),
            ("GIT_COMMITTER_EMAIL", "root@example.test"),
            ("GIT_COMMITTER_DATE", "1700000300 +0000"),
        ],
    );

    write_file(repo.path(), "middle.txt", "middle\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_git_with_env(
        repo.path(),
        &["commit", "-m", "middle"],
        &[
            ("GIT_AUTHOR_NAME", "Middle"),
            ("GIT_AUTHOR_EMAIL", "middle@example.test"),
            ("GIT_AUTHOR_DATE", "1700000200 +0000"),
            ("GIT_COMMITTER_NAME", "Middle"),
            ("GIT_COMMITTER_EMAIL", "middle@example.test"),
            ("GIT_COMMITTER_DATE", "1700000200 +0000"),
        ],
    );

    write_file(repo.path(), "tip.txt", "tip\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_git_with_env(
        repo.path(),
        &["commit", "-m", "tip"],
        &[
            ("GIT_AUTHOR_NAME", "Tip"),
            ("GIT_AUTHOR_EMAIL", "tip@example.test"),
            ("GIT_AUTHOR_DATE", "1700000100 +0000"),
            ("GIT_COMMITTER_NAME", "Tip"),
            ("GIT_COMMITTER_EMAIL", "tip@example.test"),
            ("GIT_COMMITTER_DATE", "1700000100 +0000"),
        ],
    );
    repo
}

fn pinned_rev_list_dash_path_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    for (index, date) in [
        ("one", "1700000000 +0000"),
        ("two", "1700000100 +0000"),
        ("three", "1700000200 +0000"),
    ] {
        write_file(repo.path(), "--not-a-revision", &format!("{index}\n"));
        pinned_git_args(repo.path(), &["add", "--", "--not-a-revision"]);
        pinned_commit(repo.path(), index, date);
    }
    repo
}

fn pinned_orphan_annotated_tag_fixture(sha256: bool) -> TempDir {
    let repo = pinned_history_fixture(sha256);
    pinned_git_args(repo.path(), &["checkout", "--orphan", "orphan"]);
    pinned_git_args(repo.path(), &["rm", "-rf", "."]);
    write_file(repo.path(), "orphan.txt", "orphan only\n");
    pinned_git_args(repo.path(), &["add", "orphan.txt"]);
    pinned_commit(repo.path(), "orphan", "1700000300 +0000");
    pinned_git_args(
        repo.path(),
        &["tag", "-a", "orphan-tag", "-m", "orphan tag", "HEAD"],
    );
    pinned_git_args(repo.path(), &["checkout", "main"]);
    pinned_git_args(repo.path(), &["branch", "-D", "orphan"]);
    pinned_git_args(repo.path(), &["pack-refs", "--all", "--prune"]);
    repo
}

fn pinned_annotated_tag_polarity_fixture(sha256: bool) -> TempDir {
    let repo = pinned_orphan_annotated_tag_fixture(sha256);
    pinned_git_args(repo.path(), &["branch", "orphan-root", "orphan-tag"]);
    pinned_git_args(repo.path(), &["pack-refs", "--all", "--prune"]);
    repo
}

fn pinned_tag_root_error_fixture(sha256: bool, malformed: bool) -> TempDir {
    let repo = pinned_history_fixture(sha256);
    let algorithm = if sha256 {
        GitHashAlgorithm::Sha256
    } else {
        GitHashAlgorithm::Sha1
    };
    let missing_target = "a".repeat(algorithm.digest_len() * 2);
    let content = if malformed {
        b"not a valid tag object\n".to_vec()
    } else {
        format!(
            "object {missing_target}\ntype commit\ntag missing\ntagger Bench <bench@example.test> 1700000500 +0000\n\nmissing target\n"
        )
        .into_bytes()
    };
    let tag_id = write_loose_object_for_algorithm(repo.path(), algorithm, "tag", &content);
    let tag_ref_dir = repo.path().join(".git/refs/tags");
    fs::create_dir_all(&tag_ref_dir).expect("create broken tag ref directory");
    fs::write(tag_ref_dir.join("broken"), format!("{tag_id}\n")).expect("write broken tag ref");
    repo
}

fn pinned_mismatched_tag_fixture(sha256: bool, target_is_blob: bool) -> TempDir {
    let repo = pinned_history_fixture(sha256);
    let algorithm = if sha256 {
        GitHashAlgorithm::Sha256
    } else {
        GitHashAlgorithm::Sha1
    };
    let target = if target_is_blob {
        write_loose_object_for_algorithm(repo.path(), algorithm, "blob", b"mismatched target")
    } else {
        String::from_utf8(raw_pinned_output(repo.path(), &["rev-parse", "HEAD"]).stdout)
            .expect("HEAD object id")
            .trim()
            .to_owned()
    };
    let declared_type = if target_is_blob { "commit" } else { "blob" };
    let content = format!(
        "object {target}\ntype {declared_type}\ntag mismatched\ntagger Bench <bench@example.test> 1700000500 +0000\n\nmismatched\n"
    );
    let tag = write_loose_object_for_algorithm(repo.path(), algorithm, "tag", content.as_bytes());
    let tags = repo.path().join(".git/refs/tags");
    fs::write(tags.join("mismatched"), format!("{tag}\n")).expect("write mismatched tag ref");
    repo
}

fn pinned_terminal_noncommit_tag_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);
    write_file(repo.path(), "root.txt", "root\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "root", "1700000000 +0000");

    let algorithm = if sha256 {
        GitHashAlgorithm::Sha256
    } else {
        GitHashAlgorithm::Sha1
    };
    let blob = write_loose_object_for_algorithm(repo.path(), algorithm, "blob", b"terminal\n");
    let mut tree_content = b"100644 terminal.txt\0".to_vec();
    tree_content.extend_from_slice(&decode_hex_object_id(&blob));
    let tree = write_loose_object_for_algorithm(repo.path(), algorithm, "tree", &tree_content);
    let tags = repo.path().join(".git/refs/tags");
    for (name, target, target_type) in [("blob-tag", blob, "blob"), ("tree-tag", tree, "tree")] {
        let content = format!(
            "object {target}\ntype {target_type}\ntag {name}\ntagger Bench <bench@example.test> 1700000500 +0000\n\n{name}\n"
        );
        let tag =
            write_loose_object_for_algorithm(repo.path(), algorithm, "tag", content.as_bytes());
        fs::write(tags.join(name), format!("{tag}\n")).expect("write terminal tag ref");
    }
    repo
}

fn pinned_dangling_ref_fixture(sha256: bool) -> TempDir {
    let repo = pinned_history_fixture(sha256);
    let algorithm = if sha256 {
        GitHashAlgorithm::Sha256
    } else {
        GitHashAlgorithm::Sha1
    };
    let missing = "b".repeat(algorithm.digest_len() * 2);
    let heads = repo.path().join(".git/refs/heads");
    fs::write(heads.join("dangling"), format!("{missing}\n")).expect("write dangling ref");
    repo
}

fn pinned_deep_nested_tag_fixture(sha256: bool) -> TempDir {
    let repo = pinned_history_fixture(sha256);
    let algorithm = if sha256 {
        GitHashAlgorithm::Sha256
    } else {
        GitHashAlgorithm::Sha1
    };
    let head = String::from_utf8(raw_pinned_output(repo.path(), &["rev-parse", "HEAD"]).stdout)
        .expect("HEAD object id")
        .trim()
        .to_owned();
    let mut target = head;
    for depth in 0..12 {
        let target_type = if depth == 0 { "commit" } else { "tag" };
        let content = format!(
            "object {target}\ntype {target_type}\ntag nested-{depth}\ntagger Bench <bench@example.test> 1700000500 +0000\n\nnested\n"
        );
        target =
            write_loose_object_for_algorithm(repo.path(), algorithm, "tag", content.as_bytes());
    }
    let tags = repo.path().join(".git/refs/tags");
    fs::write(tags.join("deep"), format!("{target}\n")).expect("write deep tag ref");
    repo
}

fn pinned_linear_reflog_fixture() -> TempDir {
    let repo = pinned_sha1_repo();
    configure_pinned_identity(repo.path());
    pinned_git_args(repo.path(), &["checkout", "-b", "main"]);

    write_file(repo.path(), "base.txt", "base\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "base", "1700000000 +0000");

    write_file(repo.path(), "next.txt", "next\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "next", "1700000100 +0000");

    repo
}

fn pinned_reftable_history_fixture() -> TempDir {
    pinned_reftable_history_fixture_for(false)
}

fn pinned_reftable_sha256_history_fixture() -> TempDir {
    pinned_reftable_history_fixture_for(true)
}

fn pinned_reftable_history_fixture_for(sha256: bool) -> TempDir {
    let repo = TempDir::new().expect("temp reftable repo");
    let init_args = if sha256 {
        [
            "init",
            "--quiet",
            "--object-format=sha256",
            "--ref-format=reftable",
        ]
        .as_slice()
    } else {
        ["init", "--quiet", "--ref-format=reftable"].as_slice()
    };
    let output = Command::new(required_pinned_stock_git())
        .args(init_args)
        .current_dir(repo.path())
        .output()
        .expect("run pinned reftable init");
    assert!(
        output.status.success(),
        "pinned reftable init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    pinned_git_args(repo.path(), &["symbolic-ref", "HEAD", "refs/heads/master"]);
    configure_pinned_identity(repo.path());
    write_file(repo.path(), "reftable.txt", "one\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "reftable-one", "1700000000 +0000");
    write_file(repo.path(), "reftable.txt", "two\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "reftable-two", "1700000100 +0000");
    repo
}

fn pinned_reftable_warning_fixture_for(sha256: bool) -> TempDir {
    let repo = pinned_reftable_history_fixture_for(sha256);
    write_file(repo.path(), "reftable.txt", "three\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "reftable-three", "1700000200 +0000");
    repo
}

fn collision_pair(repo: &std::path::Path, algorithm: GitHashAlgorithm) -> (String, String) {
    let mut seen = BTreeMap::<String, (String, Vec<u8>)>::new();
    for nonce in 0..100_000_u32 {
        let content = format!("abbreviation-collision-{nonce}\n").into_bytes();
        let id = loose_object_id_for_algorithm(algorithm, "blob", &content);
        let prefix = id[..4].to_owned();
        if let Some((previous_id, previous_content)) = seen.get(&prefix) {
            write_loose_object_for_algorithm(repo, algorithm, "blob", previous_content);
            write_loose_object_for_algorithm(repo, algorithm, "blob", &content);
            return (previous_id.clone(), id);
        }
        seen.insert(prefix, (id, content));
    }
    panic!("failed to create deterministic object abbreviation collision");
}

fn raw_pinned_output(repo: &std::path::Path, args: &[&str]) -> RawCommandOutput {
    command_raw_output(
        required_pinned_stock_git()
            .to_str()
            .expect("pinned Git path is UTF-8"),
        repo,
        args,
        "pinned Git",
    )
}

fn raw_zmin_output(repo: &std::path::Path, args: &[&str]) -> RawCommandOutput {
    command_raw_output(zmin_bin(), repo, args, "zmin")
}

fn pinned_object_kind(repo: &std::path::Path, id: &str) -> String {
    let output = Command::new(required_pinned_stock_git())
        .args(["cat-file", "-t"])
        .arg(id)
        .current_dir(repo)
        .output()
        .expect("run pinned cat-file kind");
    assert!(output.status.success(), "pinned cat-file failed for {id}");
    String::from_utf8(output.stdout)
        .expect("pinned object kind is UTF-8")
        .trim()
        .to_owned()
}

fn pinned_object_kind_counts(repo: &std::path::Path, output: &[u8]) -> [usize; 3] {
    let mut counts = [0usize; 3];
    for line in output.split(|byte| *byte == b'\n') {
        let Some(id) = line.split(|byte| *byte == b' ' || *byte == b'\t').next() else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        match pinned_object_kind(repo, std::str::from_utf8(id).expect("object id is UTF-8"))
            .as_str()
        {
            "commit" => counts[0] += 1,
            "tree" => counts[1] += 1,
            "blob" => counts[2] += 1,
            kind => panic!("unexpected pinned object kind {kind}"),
        }
    }
    counts
}

fn raw_history_output_with_env(
    command: &str,
    repo: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> RawCommandOutput {
    let mut command = Command::new(command);
    command.args(args).current_dir(repo);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.output().expect("run history command");
    RawCommandOutput {
        status: output.status.code().expect("history command exit code"),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

fn assert_history_tuple(repo: &std::path::Path, args: &[&str]) {
    let zmin = raw_zmin_output(repo, args);
    assert_eq!(
        zmin,
        raw_pinned_output(repo, args),
        "history tuple mismatch for {args:?}"
    );
}

fn assert_reflog_selector_tuple(repo: &std::path::Path, ref_name: &str, selector: &str) {
    let objectish = format!("{ref_name}@{{{selector}}}");
    let args = ["rev-parse", "--verify", objectish.as_str()];
    assert_eq!(
        raw_zmin_output(repo, &args),
        raw_pinned_output(repo, &args),
        "reflog selector tuple mismatch for {objectish}"
    );
}

fn rewrite_main_reflog_field_at_timestamp(
    repo: &std::path::Path,
    timestamp: &str,
    field: usize,
    replacement: &str,
) {
    assert!(field < 2, "only reflog object-id fields are rewritable");
    let path = repo.join(".git/logs/refs/heads/main");
    let original = fs::read_to_string(&path).expect("read main reflog");
    let mut changed = false;
    let mut rewritten = String::with_capacity(original.len());
    for record in original.split_inclusive('\n') {
        let has_newline = record.ends_with('\n');
        let body = record.strip_suffix('\n').unwrap_or(record);
        let mut fields = body.splitn(3, ' ').collect::<Vec<_>>();
        if fields.len() == 3 && fields[2].split_whitespace().nth(2) == Some(timestamp) {
            fields[field] = replacement;
            rewritten.push_str(fields.join(" ").as_str());
            changed = true;
        } else {
            rewritten.push_str(body);
        }
        if has_newline {
            rewritten.push('\n');
        }
    }
    assert!(changed, "reflog timestamp not found: {timestamp}");
    fs::write(path, rewritten).expect("rewrite main reflog");
}

fn zero_object_id_hex(sha256: bool) -> String {
    "0".repeat(if sha256 { 64 } else { 40 })
}

fn assert_known_ambiguous_prefix_gap(repo: &std::path::Path, prefix: &str) {
    let stock = raw_pinned_output(repo, &["rev-parse", prefix]);
    let zmin = raw_zmin_output(repo, &["rev-parse", prefix]);
    assert_eq!(
        zmin.status, stock.status,
        "ambiguous status drift for {prefix}"
    );
    assert_eq!(stock.stdout, format!("{prefix}\n").into_bytes());
    assert!(
        stock
            .stderr
            .windows(b"short object ID ".len())
            .any(|window| window == b"short object ID "),
        "pinned Git must report ambiguous candidates for {prefix}: {:?}",
        String::from_utf8_lossy(&stock.stderr)
    );
    assert!(
        zmin.stdout.is_empty()
            && zmin
                .stderr
                .windows(b"unknown revision or path".len())
                .any(|window| window == b"unknown revision or path"),
        "current Zmin ambiguity diagnostic changed unexpectedly: {:?}",
        String::from_utf8_lossy(&zmin.stderr)
    );
}

fn assert_known_missing_prefix_gap(repo: &std::path::Path, prefix: &str) {
    let stock = raw_pinned_output(repo, &["rev-parse", prefix]);
    let zmin = raw_zmin_output(repo, &["rev-parse", prefix]);
    assert_eq!(
        zmin.status, stock.status,
        "missing status drift for {prefix}"
    );
    assert_eq!(zmin.stderr, stock.stderr);
    assert_eq!(stock.stdout, format!("{prefix}\n").into_bytes());
    assert!(zmin.stdout.is_empty());
}

fn assert_known_reflog_ordinal_range_gap(repo: &std::path::Path, selector: &str) {
    let objectish = format!("HEAD@{{{selector}}}");
    let args = ["log", "--reflog", "--format=%H", objectish.as_str()];
    let stock = raw_pinned_output(repo, &args);
    let zmin = raw_zmin_output(repo, &args);
    assert_eq!(zmin.status, stock.status);
    assert!(stock.stdout.is_empty());
    assert!(
        stock
            .stderr
            .windows(b"only has ".len())
            .any(|window| { window == b"only has " })
    );
    assert!(zmin.stdout.is_empty());
    assert!(
        zmin.stderr
            .windows(b"unknown revision or path".len())
            .any(|window| window == b"unknown revision or path")
    );
}

fn assert_pseudoref_tuple(repo: &std::path::Path, name: &str) {
    let stock = raw_pinned_output(repo, &["rev-parse", name]);
    let zmin = raw_zmin_output(repo, &["rev-parse", name]);
    if stock.status == 0 {
        assert_eq!(zmin, stock, "pseudoref tuple mismatch for {name}");
        return;
    }
    // Keep any pinned-Git fixture-level failure visible without turning it
    // into an active-algorithm failure. Git echoes an unresolved token while
    // Zmin's existing generic formatter does not; this is queued separately.
    assert_eq!(
        zmin.status, stock.status,
        "pseudoref status drift for {name}"
    );
    assert_eq!(zmin.stderr, stock.stderr);
    assert_eq!(stock.stdout, format!("{name}\n").into_bytes());
    assert!(zmin.stdout.is_empty());
}

fn unique_prefix(repo: &std::path::Path, candidates: &[String], length: usize) -> String {
    for candidate in candidates {
        let prefix = &candidate[..length];
        let output = raw_pinned_output(repo, &["rev-parse", prefix]);
        if output.status == 0 && output.stdout == format!("{candidate}\n").into_bytes() {
            return prefix.to_owned();
        }
    }
    panic!("fixture has no unique {length}-character object prefix");
}

fn missing_prefix(repo: &std::path::Path) -> String {
    for candidate in ["ffff", "eeee", "dead", "beef", "cafe", "face"] {
        if raw_pinned_output(repo, &["rev-parse", candidate]).status != 0 {
            return candidate.to_owned();
        }
    }
    panic!("fixture has no missing object prefix");
}

fn commit_collision_pair(repo: &std::path::Path, tree: &str) -> (String, String) {
    let mut seen = BTreeMap::<String, (String, Vec<u8>)>::new();
    for nonce in 0..200_000_u32 {
        let content = format!(
            "tree {tree}\nauthor Collision <collision@example.test> 1700000000 +0000\ncommitter Collision <collision@example.test> 1700000000 +0000\n\ncollision-{nonce}\n"
        )
        .into_bytes();
        let id = loose_object_id("commit", &content);
        let prefix = id[..7].to_owned();
        if let Some((previous_id, previous_content)) = seen.get(&prefix) {
            write_loose_object(repo, "commit", previous_content);
            write_loose_object(repo, "commit", &content);
            return (previous_id.clone(), id);
        }
        seen.insert(prefix, (id, content));
    }
    panic!("failed to create deterministic commit abbreviation collision");
}

#[test]
fn show_option_errors_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["commit", "--allow-empty", "-m", "base"]);

    for args in [
        ["show", "--frobnicate"].as_slice(),
        ["show", "-Q"].as_slice(),
        ["show", "--format"].as_slice(),
        ["show", "--format", "--frobnicate"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), repo.path(), args, "zmin"),
            command_any_output("git", repo.path(), args, "git"),
            "show option error mismatch for {args:?}"
        );
    }
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

fn git_commit_with_split_dates(
    cwd: &std::path::Path,
    author_name: &str,
    author_email: &str,
    author_date: &str,
    committer_name: &str,
    committer_email: &str,
    committer_date: &str,
    message: &str,
) {
    let output = Command::new(stock_git_bin())
        .args(["-c", "commit.gpgsign=false", "commit", "-m", message])
        .env("GIT_AUTHOR_NAME", author_name)
        .env("GIT_AUTHOR_EMAIL", author_email)
        .env("GIT_AUTHOR_DATE", author_date)
        .env("GIT_COMMITTER_NAME", committer_name)
        .env("GIT_COMMITTER_EMAIL", committer_email)
        .env("GIT_COMMITTER_DATE", committer_date)
        .current_dir(cwd)
        .output()
        .expect("git commit with split dates");
    assert!(
        output.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_commit_with_date(
    cwd: &std::path::Path,
    path: &str,
    content: &str,
    date: &str,
    message: &str,
) {
    write_file(cwd, path, content);
    git(cwd, ["add", "-A"]);
    git_commit_with_author(cwd, "A", "a@example.test", date, message);
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

fn blame_whitespace_rewrite_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    write_commit_with_date(
        repo.path(),
        "a.txt",
        "\nUse commands as `zmin <command>`.\n\nThis preview is not 100% Git-compatible yet. Zmin has handlers for all `151`\nGit `2.47.1` command names, but command routing is only the entry point. A\ncommand is complete only after its documented options, option values, option\ncombinations, repository states, transports and platform cases match stock Git.\n\nReal compatibility is counted by behavior variants:\n",
        "1700000000 +0000",
        "base",
    );
    write_commit_with_date(
        repo.path(),
        "a.txt",
        "\nUse commands as `zmin <command>`.\n\nZmin is not 100% Git-compatible yet. It has handlers for all `151` Git `2.47.1`\ncommand names, but a handler only proves that the command can be routed.\n\nReal compatibility is measured at behavior-row level:\n\n`command + option + value + option combination + repository state + transport + platform`\n",
        "1700000100 +0000",
        "rewrite",
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
    if stock_git_version_at_least(2, 54) {
        let prefix = vec!["whatchanged", "--i-still-use-this"];
        let mut plain = prefix.clone();
        plain.extend(["--max-count", "1"]);
        let mut stat = prefix.clone();
        stat.extend(["--stat", "--max-count", "1"]);
        let mut oneline = prefix;
        oneline.extend(["--oneline", "--max-count", "1"]);
        return vec![plain, stat, oneline];
    }
    vec![
        vec!["whatchanged", "--max-count", "1"],
        vec!["whatchanged", "--stat", "--max-count", "1"],
        vec!["whatchanged", "--oneline", "--max-count", "1"],
    ]
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

fn pack_as_from_promisor(repo: &std::path::Path, object_id: &str) {
    let stock = stock_git_bin();
    pack_as_from_promisor_with_git(
        repo,
        object_id,
        stock.to_str().expect("stock Git path UTF-8"),
    );
}

fn pack_as_from_promisor_with_git(repo: &std::path::Path, object_id: &str, git_bin: &str) {
    let mut pack_objects = Command::new(git_bin)
        .args(["pack-objects", ".git/objects/pack/pack"])
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pack-objects");
    pack_objects
        .stdin
        .as_mut()
        .expect("pack-objects stdin")
        .write_all(format!("{object_id}\n").as_bytes())
        .expect("write object id");
    let output = pack_objects.wait_with_output().expect("wait pack-objects");
    assert!(
        output.status.success(),
        "pack-objects failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pack_hash = String::from_utf8(output.stdout)
        .expect("pack hash utf8")
        .trim()
        .to_owned();
    assert!(!pack_hash.is_empty(), "missing pack hash");
    fs::write(
        repo.join(format!(".git/objects/pack/pack-{pack_hash}.promisor")),
        b"",
    )
    .expect("write promisor marker");
}

fn delete_loose_object(repo: &std::path::Path, object_id: &str) {
    let path = repo
        .join(".git/objects")
        .join(&object_id[..2])
        .join(&object_id[2..]);
    fs::remove_file(path).expect("delete loose object");
}

fn promise_and_delete(repo: &std::path::Path, object_name: &str) {
    let object_id = git(repo, ["rev-parse", object_name]);
    git(
        repo,
        ["tag", "-a", "-m", "message", "my_annotated_tag", &object_id],
    );
    let tag_id = git(repo, ["rev-parse", "my_annotated_tag"]);
    pack_as_from_promisor(repo, &tag_id);
    git(repo, ["tag", "-d", "my_annotated_tag"]);
    delete_loose_object(repo, &object_id);
}

fn bare_filtered_rename_partial_clone_fixture(sha256: bool) -> (TempDir, std::path::PathBuf) {
    let source = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(source.path());
    pinned_git_args(source.path(), &["config", "uploadpack.allowFilter", "true"]);
    pinned_git_args(
        source.path(),
        &["config", "uploadpack.allowAnySHA1InWant", "true"],
    );
    write_file(source.path(), "old-file.txt", "content\n");
    write_file(source.path(), "wild*file.txt", "literal wildcard path\n");
    pinned_git_args(source.path(), &["add", "-A"]);
    pinned_commit(source.path(), "create-a-file", "1700000000 +0000");
    pinned_git_args(source.path(), &["mv", "old-file.txt", "new-file.txt"]);
    pinned_commit(source.path(), "rename-the-file", "1700000100 +0000");

    let partial_root = TempDir::new().expect("partial tempdir");
    let partial_git = partial_root.path().join("partial.git");
    let partial_url = format!("file://{}", source.path().display());
    let output = Command::new(required_pinned_stock_git())
        .args([
            "clone",
            "--filter=blob:none",
            "--bare",
            &partial_url,
            partial_git.to_str().expect("partial path utf8"),
        ])
        .output()
        .expect("clone filtered bare repo");
    assert!(
        output.status.success(),
        "git clone failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (partial_root, partial_git)
}

fn pinned_follow_directory_rename_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    write_file(repo.path(), "old-dir/file.txt", "directory content\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "create-dir", "1700000000 +0000");
    pinned_git_args(repo.path(), &["mv", "old-dir", "new-dir"]);
    pinned_commit(repo.path(), "rename-dir", "1700000100 +0000");
    repo
}

fn pinned_follow_merge_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    write_file(repo.path(), "old-file.txt", "content\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "create-a-file", "1700000000 +0000");
    let main_branch = pinned_git_args(repo.path(), &["branch", "--show-current"]);
    pinned_git_args(repo.path(), &["checkout", "-q", "-b", "side"]);
    write_file(repo.path(), "side.txt", "side\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "side-change", "1700000100 +0000");
    pinned_git_args(repo.path(), &["checkout", "-q", main_branch.as_str()]);
    pinned_git_args(repo.path(), &["mv", "old-file.txt", "new-file.txt"]);
    pinned_commit(repo.path(), "rename-the-file", "1700000200 +0000");
    pinned_git_with_env(
        repo.path(),
        &["merge", "--no-ff", "-m", "merge", "side"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000300 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000300 +0000"),
        ],
    );
    repo
}

fn pinned_follow_linear_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    write_file(repo.path(), "old-file.txt", "content\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "create-a-file", "1700000000 +0000");
    pinned_git_args(repo.path(), &["mv", "old-file.txt", "new-file.txt"]);
    pinned_commit(repo.path(), "rename-the-file", "1700000100 +0000");
    repo
}

fn pinned_follow_linear_creation_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    write_file(repo.path(), "new-file.txt", "one\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "create-file", "1700000000 +0000");
    write_file(repo.path(), "new-file.txt", "two\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "edit1", "1700000100 +0000");
    write_file(repo.path(), "new-file.txt", "three\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "edit2", "1700000200 +0000");
    repo
}

fn pinned_follow_rename_then_edit_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    write_file(repo.path(), "old-file.txt", "one\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "create-file", "1700000000 +0000");
    pinned_git_args(repo.path(), &["mv", "old-file.txt", "new-file.txt"]);
    pinned_commit(repo.path(), "rename-the-file", "1700000100 +0000");
    write_file(repo.path(), "new-file.txt", "two\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "edit-new-file", "1700000200 +0000");
    repo
}

fn pinned_follow_rename_delete_readd_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    write_file(repo.path(), "old-file.txt", "one\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "create-file", "1700000000 +0000");
    pinned_git_args(repo.path(), &["mv", "old-file.txt", "new-file.txt"]);
    pinned_commit(repo.path(), "rename-the-file", "1700000100 +0000");
    pinned_git_args(repo.path(), &["rm", "new-file.txt"]);
    pinned_commit(repo.path(), "delete-new-file", "1700000200 +0000");
    write_file(repo.path(), "new-file.txt", "one\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "readd-new-file", "1700000300 +0000");
    repo
}

fn pinned_follow_merge_second_parent_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    write_file(repo.path(), "old-file.txt", "content\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "create-a-file", "1700000000 +0000");
    let main_branch = pinned_git_args(repo.path(), &["branch", "--show-current"]);
    pinned_git_args(repo.path(), &["checkout", "-q", "-b", "side"]);
    pinned_git_args(repo.path(), &["mv", "old-file.txt", "new-file.txt"]);
    pinned_commit(repo.path(), "rename-the-file", "1700000100 +0000");
    pinned_git_args(repo.path(), &["checkout", "-q", main_branch.as_str()]);
    write_file(repo.path(), "main.txt", "main\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "main-change", "1700000200 +0000");
    pinned_git_with_env(
        repo.path(),
        &["merge", "--no-ff", "-m", "merge", "side"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000300 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000300 +0000"),
        ],
    );
    repo
}

fn pinned_follow_merge_changed_both_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    write_file(repo.path(), "old-file.txt", "content\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "base", "1700000000 +0000");
    let main_branch = pinned_git_args(repo.path(), &["branch", "--show-current"]);

    pinned_git_args(repo.path(), &["checkout", "-q", "-b", "p1"]);
    pinned_git_args(repo.path(), &["mv", "old-file.txt", "new-file.txt"]);
    pinned_commit(repo.path(), "p1", "1700000100 +0000");
    pinned_git_args(repo.path(), &["checkout", "-q", main_branch.as_str()]);
    pinned_git_args(repo.path(), &["checkout", "-q", "-b", "p2"]);
    pinned_git_args(repo.path(), &["mv", "old-file.txt", "new-file.txt"]);
    pinned_commit(repo.path(), "p2", "1700000100 +0000");
    pinned_git_args(repo.path(), &["checkout", "-q", "p1"]);
    pinned_git_with_env(
        repo.path(),
        &["merge", "--no-ff", "-m", "merge", "p2"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000200 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000200 +0000"),
        ],
    );
    repo
}

fn pinned_follow_merge_delete_readd_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        pinned_sha1_repo()
    };
    configure_pinned_identity(repo.path());
    write_file(repo.path(), "old-file.txt", "content\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_commit(repo.path(), "base", "1700000000 +0000");
    let main_branch = pinned_git_args(repo.path(), &["branch", "--show-current"]);

    pinned_git_args(repo.path(), &["checkout", "-q", "-b", "p1"]);
    pinned_git_args(repo.path(), &["mv", "old-file.txt", "new-file.txt"]);
    pinned_commit(repo.path(), "p1", "1700000100 +0000");
    let p1 = pinned_git_args(repo.path(), &["rev-parse", "HEAD"]);

    pinned_git_args(repo.path(), &["checkout", "-q", main_branch.as_str()]);
    pinned_git_args(repo.path(), &["checkout", "-q", "-b", "p2"]);
    pinned_git_args(repo.path(), &["rm", "old-file.txt"]);
    pinned_commit(repo.path(), "p2", "1700000100 +0000");
    let p2 = pinned_git_args(repo.path(), &["rev-parse", "HEAD"]);

    pinned_git_args(repo.path(), &["checkout", "-q", "p1"]);
    let tree = pinned_git_args(repo.path(), &["write-tree"]);
    let merge = pinned_commit_tree(
        repo.path(),
        tree.trim(),
        &[p1.trim(), p2.trim()],
        "merge",
        "1700000200 +0000",
    );
    pinned_git_args(
        repo.path(),
        &["update-ref", "refs/heads/p1", merge.as_str()],
    );
    pinned_git_args(repo.path(), &["checkout", "-q", "p1"]);
    repo
}

#[test]
fn whatchanged_default_invocation_matches_stock_git_version_contract() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "a\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["whatchanged"], "zmin"),
        command_any_output("git", repo.path(), &["whatchanged"], "git")
    );
}

#[test]
fn whatchanged_i_still_use_this_matches_stock_git_invalid_input_contract() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "a\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["whatchanged", "--i-still-use-this"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["whatchanged", "--i-still-use-this"],
            "git"
        )
    );
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
        [
            "shortlog",
            "--date=short",
            "--group=format:%ad",
            "-sn",
            "HEAD",
        ]
        .as_slice(),
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
        [
            "shortlog",
            "--date=bogus",
            "--group=format:%ad",
            "-sn",
            "HEAD",
        ]
        .as_slice(),
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
        [
            "shortlog",
            "--grep=banana",
            "--grep=carrot",
            "--all-match",
            "HEAD",
        ]
        .as_slice(),
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

    for args in [["shortlog", "--grep-reflog=one", "HEAD"].as_slice()] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn shortlog_history_selector_and_filter_batch_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "a@example.test",
        "1700000000 +0000",
        "one",
    );
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Bob",
        "b@example.test",
        "1700000600 +0000",
        "two",
    );
    git(repo.path(), ["branch", "side", "HEAD~1"]);
    git(repo.path(), ["tag", "v1", "HEAD~1"]);
    write_file(repo.path(), "a.txt", "three\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "a@example.test",
        "1700001200 +0000",
        "three",
    );

    for args in [
        ["shortlog", "--after=1700000300", "HEAD"].as_slice(),
        ["shortlog", "--before=1700000900", "HEAD"].as_slice(),
        ["shortlog", "--author=Alice", "HEAD"].as_slice(),
        ["shortlog", "--all"].as_slice(),
        ["shortlog", "--branches", "--summary"].as_slice(),
        ["shortlog", "--tags", "--summary"].as_slice(),
        ["shortlog", "--max-count=2", "HEAD"].as_slice(),
        ["shortlog", "--grep=o.e", "--basic-regexp", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn rev_list_objects_and_shortlog_order_filters_match_pinned_stock() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        for args in [
            ["rev-list", "--objects", "--topo-order", "HEAD"].as_slice(),
            ["rev-list", "--objects", "--date-order", "HEAD"].as_slice(),
            [
                "rev-list",
                "--objects",
                "--author-date-order",
                "--reverse",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "--topo-order",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--filter=blob:none",
                "--date-order",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--grep=merge",
                "--max-count=1",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--date-order",
                "--skip=1",
                "--max-count=2",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--reverse",
                "--skip=1",
                "--max-count=2",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--max-count=2",
                "--skip=1",
                "--reverse",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "--reverse",
                "--skip=1",
                "--max-count=2",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "--max-count=2",
                "--skip=1",
                "--reverse",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--simplify-by-decoration",
                "--all",
                "--max-count=1",
            ]
            .as_slice(),
            ["rev-list", "--reverse", "--skip=1", "--max-count=2", "HEAD"].as_slice(),
            ["rev-list", "--author=Bench", "--max-count=2", "HEAD"].as_slice(),
            [
                "rev-list",
                "--simplify-by-decoration",
                "--all",
                "--max-count=1",
            ]
            .as_slice(),
            [
                "rev-list",
                "--simplify-by-decoration",
                "--all",
                "--skip=1",
                "--max-count=1",
            ]
            .as_slice(),
            ["shortlog", "--summary", "--reverse", "HEAD"].as_slice(),
            ["shortlog", "--summary", "--topo-order", "HEAD"].as_slice(),
            [
                "shortlog",
                "--summary",
                "--date-order",
                "--max-count=2",
                "HEAD",
            ]
            .as_slice(),
            [
                "shortlog",
                "--summary",
                "--author-date-order",
                "--skip=1",
                "--max-count=1",
                "HEAD",
            ]
            .as_slice(),
            [
                "shortlog",
                "--summary",
                "--reverse",
                "--skip=1",
                "--max-count=1",
                "HEAD",
            ]
            .as_slice(),
            [
                "shortlog",
                "--summary",
                "--simplify-by-decoration",
                "--all",
                "--max-count=1",
            ]
            .as_slice(),
            [
                "shortlog",
                "--summary",
                "--simplify-by-decoration",
                "--all",
                "--skip=1",
                "--max-count=1",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn shortlog_subject_schedule_and_pagination_match_pinned_stock() {
    for sha256 in [false, true] {
        let repo = pinned_disconnected_equal_time_fixture(sha256);
        for args in [
            ["shortlog", "--format=%s", "--all"].as_slice(),
            ["shortlog", "--format=%s", "--topo-order", "--all"].as_slice(),
            ["shortlog", "--format=%s", "--date-order", "--all"].as_slice(),
            ["shortlog", "--format=%s", "--author-date-order", "--all"].as_slice(),
            ["shortlog", "--format=%s", "--reverse", "--all"].as_slice(),
            [
                "shortlog",
                "--format=%s",
                "--reverse",
                "--skip=1",
                "--max-count=2",
                "--all",
            ]
            .as_slice(),
            [
                "shortlog",
                "--format=%s",
                "--max-count=2",
                "--skip=1",
                "--reverse",
                "--all",
            ]
            .as_slice(),
            [
                "shortlog",
                "--format=%s",
                "--grep=two",
                "--reverse",
                "--all",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
        for order in ["--topo-order", "--date-order", "--author-date-order"] {
            for args in [
                [
                    "rev-list",
                    "--objects",
                    order,
                    "--reverse",
                    "--skip=1",
                    "--max-count=2",
                    "--all",
                ]
                .as_slice(),
                [
                    "rev-list",
                    "--objects",
                    order,
                    "--max-count=2",
                    "--skip=1",
                    "--reverse",
                    "--all",
                ]
                .as_slice(),
                [
                    "rev-list",
                    "--objects",
                    "--no-object-names",
                    order,
                    "--reverse",
                    "--skip=1",
                    "--max-count=2",
                    "--all",
                ]
                .as_slice(),
                [
                    "rev-list",
                    "--objects",
                    "--no-object-names",
                    order,
                    "--max-count=2",
                    "--skip=1",
                    "--reverse",
                    "--all",
                ]
                .as_slice(),
            ] {
                assert_history_tuple(repo.path(), args);
            }
        }
        for args in [
            [
                "rev-list",
                "--objects",
                "--reverse",
                "--skip=1",
                "--max-count=2",
                "--all",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--max-count=2",
                "--skip=1",
                "--reverse",
                "--all",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "--reverse",
                "--skip=1",
                "--max-count=2",
                "--all",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "--max-count=2",
                "--skip=1",
                "--reverse",
                "--all",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn rev_list_lane_selection_and_object_pagination_match_pinned_stock() {
    for sha256 in [false, true] {
        let repo = pinned_disconnected_equal_time_fixture(sha256);
        for args in [
            ["rev-list", "left...right"].as_slice(),
            ["rev-list", "--left-only", "left...right"].as_slice(),
            ["rev-list", "--right-only", "left...right"].as_slice(),
            ["rev-list", "--left-right", "left...right"].as_slice(),
            ["rev-list", "--cherry", "left...right"].as_slice(),
            ["rev-list", "--cherry-mark", "left...right"].as_slice(),
            ["rev-list", "--cherry-pick", "left...right"].as_slice(),
            ["rev-list", "--left-right", "--cherry-pick", "left...right"].as_slice(),
            ["rev-list", "--left-right", "--cherry-mark", "left...right"].as_slice(),
            ["rev-list", "--boundary", "left...right"].as_slice(),
            [
                "rev-list",
                "--objects",
                "--left-right",
                "--cherry-pick",
                "left...right",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "--left-right",
                "--cherry-pick",
                "left...right",
            ]
            .as_slice(),
            ["rev-list", "--objects", "--first-parent", "left...right"].as_slice(),
            [
                "rev-list",
                "--objects",
                "--boundary",
                "--cherry-pick",
                "left...right",
            ]
            .as_slice(),
            [
                "log",
                "--format=%H",
                "--boundary",
                "--cherry-pick",
                "left...right",
            ]
            .as_slice(),
            [
                "shortlog",
                "--format=%H",
                "--boundary",
                "--cherry-pick",
                "left...right",
            ]
            .as_slice(),
            ["shortlog", "--format=%s", "--left-only", "left...right"].as_slice(),
            ["shortlog", "--format=%s", "--right-only", "left...right"].as_slice(),
            ["shortlog", "--format=%s", "--cherry", "left...right"].as_slice(),
            ["shortlog", "--format=%s", "--cherry-mark", "left...right"].as_slice(),
            ["shortlog", "--format=%s", "--cherry-pick", "left...right"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn history_selection_conflicts_match_pinned_stock_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_disconnected_equal_time_fixture(sha256);
        for args in [
            ["rev-list", "--left-only", "--right-only", "left...right"].as_slice(),
            ["rev-list", "--right-only", "--left-only", "left...right"].as_slice(),
            ["log", "--left-only", "--right-only", "left...right"].as_slice(),
            ["log", "--right-only", "--left-only", "left...right"].as_slice(),
            ["shortlog", "--left-only", "--right-only", "left...right"].as_slice(),
            ["shortlog", "--right-only", "--left-only", "left...right"].as_slice(),
            ["rev-list", "--cherry-pick", "--cherry-mark", "left...right"].as_slice(),
            ["rev-list", "--cherry-mark", "--cherry-pick", "left...right"].as_slice(),
            ["log", "--cherry-pick", "--cherry-mark", "left...right"].as_slice(),
            ["log", "--cherry-mark", "--cherry-pick", "left...right"].as_slice(),
            ["shortlog", "--cherry-pick", "--cherry-mark", "left...right"].as_slice(),
            ["shortlog", "--cherry-mark", "--cherry-pick", "left...right"].as_slice(),
            ["rev-list", "--cherry", "--cherry-pick", "left...right"].as_slice(),
            ["rev-list", "--cherry-pick", "--cherry", "left...right"].as_slice(),
            ["log", "--cherry", "--cherry-pick", "left...right"].as_slice(),
            ["log", "--cherry-pick", "--cherry", "left...right"].as_slice(),
            ["shortlog", "--cherry", "--cherry-pick", "left...right"].as_slice(),
            ["shortlog", "--cherry-pick", "--cherry", "left...right"].as_slice(),
            ["rev-list", "--cherry", "--left-only", "left...right"].as_slice(),
            ["rev-list", "--left-only", "--cherry", "left...right"].as_slice(),
            ["log", "--cherry", "--left-only", "left...right"].as_slice(),
            ["shortlog", "--cherry", "--left-only", "left...right"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn first_parent_and_exclusion_parent_policy_match_pinned_stock_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        for args in [
            ["rev-list", "--first-parent", "--all"].as_slice(),
            ["rev-list", "--objects", "--first-parent", "--all"].as_slice(),
            ["log", "--first-parent", "--all", "--format=%s"].as_slice(),
            ["shortlog", "--first-parent", "--all", "--format=%s"].as_slice(),
            ["rev-list", "--exclude-first-parent-only", "HEAD", "^HEAD~1"].as_slice(),
            [
                "rev-list",
                "--objects",
                "--exclude-first-parent-only",
                "HEAD",
                "^HEAD~1",
            ]
            .as_slice(),
            [
                "log",
                "--exclude-first-parent-only",
                "HEAD",
                "^HEAD~1",
                "--format=%s",
            ]
            .as_slice(),
            [
                "shortlog",
                "--exclude-first-parent-only",
                "HEAD",
                "^HEAD~1",
                "--format=%s",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn equivalent_merge_selection_and_tag_objects_match_pinned_stock_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_equivalent_merge_fixture(sha256);
        for args in [
            ["rev-list", "--left-right", "--cherry-mark", "left...right"].as_slice(),
            ["rev-list", "--left-right", "--cherry-pick", "left...right"].as_slice(),
            ["rev-list", "--cherry", "merge...right"].as_slice(),
            ["rev-list", "--cherry-pick", "--cherry", "merge...right"].as_slice(),
            [
                "rev-list",
                "--first-parent",
                "--left-right",
                "merge...right",
            ]
            .as_slice(),
            ["rev-list", "--first-parent", "--cherry", "merge...right"].as_slice(),
            [
                "rev-list",
                "--first-parent",
                "--cherry-mark",
                "merge...right",
            ]
            .as_slice(),
            [
                "rev-list",
                "--first-parent",
                "--cherry-pick",
                "merge...right",
            ]
            .as_slice(),
            ["rev-list", "--left-right", "--cherry-mark", "merge...right"].as_slice(),
            ["rev-list", "--left-right", "--cherry-pick", "merge...right"].as_slice(),
            ["rev-list", "--left-right", "--header", "merge...right"].as_slice(),
            ["rev-list", "--left-right", "--format=%s", "merge...right"].as_slice(),
            ["rev-list", "--left-right", "--parents", "merge...right"].as_slice(),
            ["rev-list", "--left-right", "--timestamp", "merge...right"].as_slice(),
            ["rev-list", "--left-right", "--children", "merge...right"].as_slice(),
            ["rev-list", "--boundary", "--date-order", "merge...right"].as_slice(),
            [
                "rev-list",
                "--boundary",
                "--author-date-order",
                "merge...right",
            ]
            .as_slice(),
            ["rev-list", "--boundary", "--topo-order", "merge...right"].as_slice(),
            [
                "rev-list",
                "--objects",
                "--left-right",
                "--cherry",
                "merge...right",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--first-parent",
                "--left-right",
                "merge...right",
            ]
            .as_slice(),
            [
                "log",
                "--first-parent",
                "--left-right",
                "--cherry-mark",
                "--format=%s",
                "merge...right",
            ]
            .as_slice(),
            [
                "shortlog",
                "--first-parent",
                "--left-right",
                "--cherry-mark",
                "--format=%s",
                "merge...right",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--left-right",
                "--cherry-pick",
                "left...right",
            ]
            .as_slice(),
            ["rev-list", "--first-parent", "HEAD"].as_slice(),
            ["rev-list", "--objects", "--first-parent", "HEAD"].as_slice(),
            ["log", "--first-parent", "--format=%s", "HEAD"].as_slice(),
            ["shortlog", "--first-parent", "--format=%s", "HEAD"].as_slice(),
            ["rev-list", "--objects", "--tags"].as_slice(),
            ["rev-list", "--objects", "--tags", "--no-object-names"].as_slice(),
            [
                "rev-list",
                "--objects",
                "--boundary",
                "--cherry-pick",
                "--skip=1",
                "--max-count=2",
                "--reverse",
                "left...right",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--boundary",
                "--skip=1",
                "--max-count=2",
                "--reverse",
                "left...right",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--boundary",
                "--max-count=2",
                "--skip=1",
                "--reverse",
                "left...right",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "--boundary",
                "--skip=1",
                "--max-count=2",
                "--reverse",
                "left...right",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn merge_base_and_exclude_first_parent_match_pinned_stock_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_equivalent_merge_fixture(sha256);
        for args in [
            ["merge-base", "left", "right"].as_slice(),
            ["merge-base", "--all", "left", "right"].as_slice(),
            ["rev-list", "--exclude-first-parent-only", "right", "^merge"].as_slice(),
            ["rev-list", "right", "^merge"].as_slice(),
            ["rev-list", "right", "--exclude-first-parent-only", "^merge"].as_slice(),
            ["rev-list", "right", "^merge", "--exclude-first-parent-only"].as_slice(),
            ["rev-list", "--objects", "right", "^merge"].as_slice(),
            [
                "rev-list",
                "--objects",
                "--exclude-first-parent-only",
                "right",
                "^merge",
            ]
            .as_slice(),
            ["log", "--format=%s", "right", "^merge"].as_slice(),
            [
                "log",
                "--exclude-first-parent-only",
                "--format=%s",
                "right",
                "^merge",
            ]
            .as_slice(),
            ["shortlog", "--format=%s", "right", "^merge"].as_slice(),
            [
                "shortlog",
                "--exclude-first-parent-only",
                "--format=%s",
                "right",
                "^merge",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn merge_base_all_criss_cross_returns_all_best_bases_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_criss_cross_merge_base_fixture(sha256);
        let stock_output = Command::new(required_pinned_stock_git())
            .args(["merge-base", "--all", "left", "right"])
            .current_dir(repo.path())
            .output()
            .expect("run pinned merge-base");
        let stock = RawCommandOutput {
            status: stock_output.status.code().expect("pinned exit code"),
            stdout: stock_output.stdout,
            stderr: stock_output.stderr,
        };
        assert_eq!(stock.status, 0);
        assert_eq!(stock.stderr, b"");
        assert_eq!(stock.stdout.split(|byte| *byte == b'\n').count() - 1, 2);
        assert_history_tuple(repo.path(), &["merge-base", "--all", "left", "right"]);
    }
}

#[test]
fn exclude_first_parent_only_preserves_disconnected_second_parent_objects() {
    for sha256 in [false, true] {
        let repo = pinned_disconnected_exclude_first_parent_fixture(sha256);
        for args in [
            [
                "rev-list",
                "--objects",
                "--exclude-first-parent-only",
                "side",
                "^HEAD",
            ]
            .as_slice(),
            ["rev-list", "--objects", "--first-parent", "HEAD", "^HEAD~1"].as_slice(),
            [
                "rev-list",
                "--objects",
                "--first-parent",
                "HEAD",
                "^unrelated-negative",
            ]
            .as_slice(),
            ["rev-list", "--objects", "side", "^HEAD"].as_slice(),
            ["rev-list", "--exclude-first-parent-only", "side", "^HEAD"].as_slice(),
            ["rev-list", "side", "^HEAD"].as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "--exclude-first-parent-only",
                "side",
                "^HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "side",
                "^HEAD",
            ]
            .as_slice(),
            [
                "log",
                "--exclude-first-parent-only",
                "--format=%s",
                "side",
                "^HEAD",
            ]
            .as_slice(),
            ["log", "--format=%s", "side", "^HEAD"].as_slice(),
            [
                "shortlog",
                "--exclude-first-parent-only",
                "--format=%s",
                "side",
                "^HEAD",
            ]
            .as_slice(),
            ["shortlog", "--format=%s", "side", "^HEAD"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn date_order_respects_topology_before_timestamp_match_pinned_stock() {
    for sha256 in [false, true] {
        let repo = pinned_date_order_topology_fixture(sha256);
        for args in [
            ["rev-list", "--date-order", "--format=%s", "HEAD"].as_slice(),
            ["rev-list", "--objects", "--date-order", "HEAD"].as_slice(),
            ["log", "--date-order", "--format=%s", "HEAD"].as_slice(),
            [
                "log",
                "--date-order",
                "--reverse",
                "--skip=1",
                "--max-count=3",
                "--format=%s",
                "HEAD",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn hidden_intermediate_ancestry_and_order_match_pinned_stock_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_hidden_intermediate_history_fixture(sha256);
        for args in [
            [
                "rev-list",
                "--objects",
                "--ancestry-path",
                "--grep=tip",
                "HEAD~2..HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--ancestry-path",
                "--author=Tip",
                "HEAD~2..HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--date-order",
                "--grep=tip\\|root",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--author-date-order",
                "--author=Tip\\|Root",
                "HEAD",
            ]
            .as_slice(),
            ["rev-list", "--date-order", "--grep=tip\\|root", "HEAD"].as_slice(),
            [
                "rev-list",
                "--author-date-order",
                "--author=Tip\\|Root",
                "HEAD",
            ]
            .as_slice(),
            [
                "log",
                "--date-order",
                "--format=%s",
                "--grep=tip\\|root",
                "HEAD",
            ]
            .as_slice(),
            [
                "shortlog",
                "--date-order",
                "--format=%s",
                "--grep=tip\\|root",
                "HEAD",
            ]
            .as_slice(),
            [
                "shortlog",
                "--author-date-order",
                "--format=%s",
                "--author=Tip\\|Root",
                "HEAD",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn object_second_parent_exclusion_and_count_match_pinned_stock_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        for args in [
            [
                "rev-list",
                "--objects",
                "--exclude-first-parent-only",
                "HEAD",
                "^HEAD^2",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "--exclude-first-parent-only",
                "HEAD",
                "^HEAD^2",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--count",
                "--exclude-first-parent-only",
                "HEAD",
                "^HEAD^2",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--count",
                "--first-parent",
                "HEAD",
                "^HEAD~1",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--count",
                "--first-parent",
                "--exclude-first-parent-only",
                "HEAD",
                "^HEAD~1",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn ordered_history_selector_events_match_pinned_stock_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        for args in [
            ["log", "--max-count", "1", "HEAD"].as_slice(),
            ["rev-list", "--max-count", "1", "HEAD"].as_slice(),
            [
                "log",
                "--glob=refs/heads/*",
                "--glob=refs/tags/*",
                "--format=%H",
            ]
            .as_slice(),
            [
                "log",
                "--exclude=refs/tags/*",
                "--glob=refs/heads/*",
                "--format=%H",
            ]
            .as_slice(),
            [
                "shortlog",
                "--glob=refs/heads/*",
                "--glob=refs/tags/*",
                "--summary",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
        pinned_git_args(repo.path(), &["checkout", "--detach", "HEAD"]);
        for args in [
            ["shortlog", "--all", "--summary"].as_slice(),
            ["rev-list", "--all", "--format=%H"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn history_parser_and_selector_boundaries_match_pinned_stock() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        pinned_git_args(repo.path(), &["branch", "topic/nested", "HEAD~1"]);
        pinned_git_args(repo.path(), &["tag", "topic/nested-tag", "HEAD~2"]);
        pinned_git_args(
            repo.path(),
            &["update-ref", "refs/remotes/origin/topic/nested", "HEAD~2"],
        );
        for args in [
            ["log", "--max-count", "--format=%H", "HEAD"].as_slice(),
            ["rev-list", "--max-count", "--format=%H", "HEAD"].as_slice(),
            ["log", "--max-count=0", "HEAD"].as_slice(),
            ["log", "--pretty", "HEAD"].as_slice(),
            ["log", "--pretty=", "HEAD"].as_slice(),
            ["log", "--pretty=medium", "HEAD"].as_slice(),
            ["rev-list", "--pretty", "HEAD"].as_slice(),
            ["rev-list", "--pretty=", "HEAD"].as_slice(),
            ["log", "HEAD", "--not", "HEAD"].as_slice(),
            ["log", "--max-count=0", "HEAD", "--", "-ps"].as_slice(),
            ["shortlog", "-w20,4,6", "HEAD"].as_slice(),
            ["shortlog", "-w", "20,4,6"].as_slice(),
            ["shortlog", "--max-count=bad"].as_slice(),
            ["shortlog", "--max-count=0", "--all", "--summary"].as_slice(),
            ["rev-list", "HEAD", "--not", "--not", "HEAD"].as_slice(),
            ["rev-list", "--branches=topic", "--format=%H"].as_slice(),
            ["rev-list", "--branches=topic/*", "--format=%H"].as_slice(),
            ["rev-list", "--branches=", "--format=%H"].as_slice(),
            ["rev-list", "--glob=", "--format=%H"].as_slice(),
            ["rev-list", "--tags=topic", "--format=%H"].as_slice(),
            ["rev-list", "--tags=topic/*", "--format=%H"].as_slice(),
            ["rev-list", "--remotes=topic", "--format=%H"].as_slice(),
            ["rev-list", "--remotes=topic/*", "--format=%H"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn shortlog_option_surface_batch_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Alice",
        "a@example.test",
        "1700000000 +0000",
        "one",
    );
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_commit_with_author(
        repo.path(),
        "Bob",
        "b@example.test",
        "1700000600 +0000",
        "two",
    );

    for args in [
        ["shortlog", "--do-walk", "HEAD"].as_slice(),
        ["shortlog", "--topo-order", "HEAD"].as_slice(),
        ["shortlog", "--date-order", "HEAD"].as_slice(),
        ["shortlog", "--author-date-order", "HEAD"].as_slice(),
        ["shortlog", "--left-right", "HEAD"].as_slice(),
        ["shortlog", "--right-only", "HEAD"].as_slice(),
        ["shortlog", "--cherry-pick", "HEAD"].as_slice(),
        ["shortlog", "--cherry-mark", "HEAD"].as_slice(),
        ["shortlog", "--boundary", "HEAD"].as_slice(),
        ["shortlog", "--children", "HEAD"].as_slice(),
        ["shortlog", "--parents", "HEAD"].as_slice(),
        ["shortlog", "--objects", "HEAD"].as_slice(),
        ["shortlog", "--graph", "HEAD"].as_slice(),
        ["shortlog", "--show-signature", "HEAD"].as_slice(),
        ["shortlog", "--abbrev-commit", "HEAD"].as_slice(),
        ["shortlog", "--oneline", "HEAD"].as_slice(),
        ["shortlog", "--pretty=oneline", "HEAD"].as_slice(),
        ["shortlog", "--encoding=UTF-8", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    for args in [
        ["shortlog", "--object-names", "HEAD"].as_slice(),
        ["shortlog", "--no-object-names", "HEAD"].as_slice(),
        ["shortlog", "--mailmap", "HEAD"].as_slice(),
        ["shortlog", "--source", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn shortlog_shared_history_schema_batch_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_commit_with_date(repo.path(), "a.txt", "one\n", "1700000000 +0000", "one");
    write_commit_with_date(repo.path(), "a.txt", "two\n", "1700001000 +0000", "two");
    write_commit_with_date(repo.path(), "a.txt", "three\n", "1700002000 +0000", "three");

    for args in [
        ["shortlog", "-sne", "--since=1700000300", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--until=1700000900", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--max-age=1700000300", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--min-age=1700000900", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--skip=1", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--first-parent", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--no-walk", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--max-parents=1", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--min-parents=0", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--merges", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--no-max-parents", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--no-min-parents", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--reverse", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn shortlog_proof_only_option_surface_batch_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_commit_with_date(repo.path(), "a.txt", "one\n", "1700000000 +0000", "one");
    write_commit_with_date(repo.path(), "a.txt", "two\n", "1700000600 +0000", "two");
    write_commit_with_date(repo.path(), "a.txt", "three\n", "1700001200 +0000", "three");

    for args in [
        ["shortlog", "-sne", "--ignore-missing", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--indexed-objects", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--objects-edge", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--objects-edge-aggressive", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--quiet", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--standard-notes", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--no-standard-notes", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--notes", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--no-notes", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--show-notes", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--show-notes-by-default", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--no-abbrev-commit", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--no-expand-tabs", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--show-pulls", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--simplify-merges", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--sparse", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--unpacked", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--remotes", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--remove-empty", "HEAD"].as_slice(),
        [
            "shortlog",
            "-sne",
            "--relative-date",
            "--group=format:%ad",
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
        ["shortlog", "-sne", "--header", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--progress", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--no-filter", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--missing", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--use-bitmap-index", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--timestamp", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn shortlog_proof_only_history_tail_batch_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_commit_with_date(repo.path(), "a.txt", "one\n", "1700000000 +0000", "one");
    write_commit_with_date(repo.path(), "a.txt", "two\n", "1700000600 +0000", "two");
    write_commit_with_date(repo.path(), "a.txt", "three\n", "1700001200 +0000", "three");

    for args in [
        ["shortlog", "-sne", "--alternate-refs", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--bisect", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--cherry", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--count", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--dense", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--full-history", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--glob=main", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--in-commit-order", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--expand-tabs", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--show-linear-break", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    for args in [
        ["shortlog", "-sne", "--bisect-all", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--bisect-vars", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--commit-header", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--disk-usage", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--single-worktree", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--filter=blob:none", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--filter-print-omitted", "HEAD"].as_slice(),
        ["shortlog", "-sne", "--filter-provided-objects", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn shortlog_remaining_documented_tail_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_commit_with_date(repo.path(), "a.txt", "one\n", "1700000000 +0000", "one");
    write_commit_with_date(repo.path(), "a.txt", "two\n", "1700000600 +0000", "two");
    git(repo.path(), ["branch", "side", "HEAD~1"]);
    git(repo.path(), ["tag", "v1", "HEAD~1"]);

    for args in [
        ["shortlog", "--ancestry-path", "HEAD~1..HEAD"].as_slice(),
        ["shortlog", "--exclude=main", "--all"].as_slice(),
        ["shortlog", "--exclude-first-parent-only", "--all"].as_slice(),
        ["shortlog", "--exclude-hidden=fetch", "--all"].as_slice(),
        ["shortlog", "--left-only", "HEAD...side"].as_slice(),
        ["shortlog", "--not", "HEAD"].as_slice(),
        ["shortlog", "--simplify-by-decoration", "--all"].as_slice(),
        ["shortlog", "--since-as-filter=1700000300", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    for args in [
        ["shortlog", "--exclude-promisor-objects", "HEAD"].as_slice(),
        ["shortlog", "--no-commit-header", "HEAD"].as_slice(),
        ["shortlog", "--merge", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn exclude_hidden_selectors_match_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        let hidden_tree = pinned_git_args(repo.path(), &["rev-parse", "HEAD^{tree}"])
            .trim()
            .to_owned();
        let hidden_commit = pinned_git_args(
            repo.path(),
            &["commit-tree", hidden_tree.as_str(), "-m", "hidden"],
        )
        .trim()
        .to_owned();
        pinned_git_args(
            repo.path(),
            &["update-ref", "refs/hidden/secret", hidden_commit.as_str()],
        );
        pinned_git_args(repo.path(), &["update-ref", "refs/heads/public", "HEAD"]);
        pinned_git_args(
            repo.path(),
            &[
                "update-ref",
                "refs/heads/public-only",
                hidden_commit.as_str(),
            ],
        );
        pinned_git_args(repo.path(), &["config", "transfer.hideRefs", "refs/hidden"]);
        pinned_git_args(repo.path(), &["config", "fetch.hideRefs", "refs/tags"]);

        for args in [
            ["rev-list", "--format=%s", "--exclude-hidden=fetch", "--all"].as_slice(),
            [
                "rev-list",
                "--format=%s",
                "--exclude-hidden=fetch",
                "--glob=refs/heads/*",
            ]
            .as_slice(),
            [
                "rev-list",
                "--format=%s",
                "--exclude=refs/heads/public-only",
                "--exclude-hidden=fetch",
                "--all",
            ]
            .as_slice(),
            [
                "rev-list",
                "--format=%s",
                "--exclude=refs/heads/public",
                "--exclude-hidden=fetch",
                "--all",
            ]
            .as_slice(),
            [
                "rev-list",
                "--format=%s",
                "--exclude-hidden=fetch",
                "--all",
                "--exclude-hidden=fetch",
                "--all",
            ]
            .as_slice(),
            ["log", "--format=%H", "--exclude-hidden=fetch", "--all"].as_slice(),
            ["shortlog", "-sne", "--exclude-hidden=fetch", "--all"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }

        for args in [
            ["rev-list", "--exclude-hidden=fetch", "--branches"].as_slice(),
            [
                "rev-list",
                "--exclude-hidden=fetch",
                "--exclude-hidden=receive",
                "--all",
            ]
            .as_slice(),
            ["rev-list", "--exclude-hidden=invalid", "--all"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn exclude_hidden_namespace_patterns_match_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        let tree = pinned_git_args(repo.path(), &["rev-parse", "HEAD^{tree}"])
            .trim()
            .to_owned();
        let namespaced_commit = pinned_git_args(
            repo.path(),
            &["commit-tree", tree.as_str(), "-m", "namespaced"],
        )
        .trim()
        .to_owned();
        pinned_git_args(
            repo.path(),
            &[
                "update-ref",
                "refs/namespaces/foo/refs/heads/nsbranch",
                namespaced_commit.as_str(),
            ],
        );

        let args = [
            "-c",
            "core.abbrev=7",
            "rev-list",
            "--format=%H",
            "--exclude-hidden=fetch",
            "--all",
        ];
        let mut stock_outputs = Vec::new();
        for pattern in [
            "refs/heads/nsbranch",
            "refs/namespaces/foo/refs/heads/nsbranch",
            "^refs/namespaces/foo/refs/heads/nsbranch",
        ] {
            pinned_git_args(
                repo.path(),
                &["config", "--replace-all", "transfer.hideRefs", pattern],
            );
            let stock = raw_history_output_with_env(
                required_pinned_stock_git()
                    .to_str()
                    .expect("pinned Git path is UTF-8"),
                repo.path(),
                &args,
                &[("GIT_NAMESPACE", "foo")],
            );
            let zmin = raw_history_output_with_env(
                zmin_bin(),
                repo.path(),
                &args,
                &[("GIT_NAMESPACE", "foo")],
            );
            assert_eq!(zmin, stock, "namespace hidden tuple mismatch for {pattern}");
            stock_outputs.push(stock.stdout);
        }
        assert_ne!(stock_outputs[0], stock_outputs[1]);
        assert_eq!(stock_outputs[0], stock_outputs[2]);
    }
}

#[test]
fn exclude_hidden_nested_namespace_patterns_match_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        let tree = pinned_git_args(repo.path(), &["rev-parse", "HEAD^{tree}"])
            .trim()
            .to_owned();
        let nested_commit = pinned_git_args(
            repo.path(),
            &["commit-tree", tree.as_str(), "-m", "nested namespace"],
        )
        .trim()
        .to_owned();
        let outside_commit = pinned_git_args(
            repo.path(),
            &["commit-tree", tree.as_str(), "-m", "outside namespace"],
        )
        .trim()
        .to_owned();

        let nested_ref = repo
            .path()
            .join(".git/refs/namespaces/foo/refs/namespaces/bar/refs/heads/nested");
        fs::create_dir_all(nested_ref.parent().expect("nested ref parent"))
            .expect("create nested namespace refs");
        fs::write(&nested_ref, format!("{nested_commit}\n")).expect("write nested ref");
        let outside_ref = repo
            .path()
            .join(".git/refs/namespaces/other/refs/heads/outside");
        fs::create_dir_all(outside_ref.parent().expect("outside ref parent"))
            .expect("create outside namespace refs");
        fs::write(&outside_ref, format!("{outside_commit}\n")).expect("write outside ref");

        let args = [
            "-c",
            "core.abbrev=7",
            "rev-list",
            "--format=%H",
            "--exclude-hidden=fetch",
            "--all",
        ];
        let mut outputs = Vec::new();
        for pattern in [
            "refs/heads/nested",
            "refs/namespaces/foo/refs/namespaces/bar/refs/heads/nested",
            "^refs/namespaces/foo/refs/namespaces/bar/refs/heads/nested",
            "refs/heads/outside",
        ] {
            pinned_git_args(
                repo.path(),
                &["config", "--replace-all", "transfer.hideRefs", pattern],
            );
            let stock = raw_history_output_with_env(
                required_pinned_stock_git()
                    .to_str()
                    .expect("pinned Git path is UTF-8"),
                repo.path(),
                &args,
                &[("GIT_NAMESPACE", "foo/bar")],
            );
            let zmin = raw_history_output_with_env(
                zmin_bin(),
                repo.path(),
                &args,
                &[("GIT_NAMESPACE", "foo/bar")],
            );
            assert_eq!(zmin, stock, "nested namespace tuple mismatch for {pattern}");
            outputs.push(stock.stdout);
        }
        assert_eq!(outputs[0], outputs[2]);
        assert_eq!(outputs[1], outputs[3]);
        assert_ne!(outputs[0], outputs[1]);
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
        [
            "log",
            "--grep=banana",
            "--invert-grep",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        [
            "log",
            "--grep=banana",
            "--grep=carrot",
            "--all-match",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        [
            "log",
            "--grep=banana",
            "--grep=carrot",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        ["log", "--grep=BA[N]ANA", "-E", "--format=%s", "HEAD"].as_slice(),
        [
            "log",
            "--grep=BA[N]ANA",
            "--extended-regexp",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        [
            "log",
            "--grep=BA[N]ANA",
            "--basic-regexp",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
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
fn rev_list_grep_family_matches_stock_git() {
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
        ["rev-list", "--grep=banana", "HEAD"].as_slice(),
        ["rev-list", "--grep=banana", "-i", "HEAD"].as_slice(),
        ["rev-list", "--grep=banana", "--regexp-ignore-case", "HEAD"].as_slice(),
        ["rev-list", "--grep=banana", "--invert-grep", "HEAD"].as_slice(),
        [
            "rev-list",
            "--grep=banana",
            "--grep=carrot",
            "--all-match",
            "HEAD",
        ]
        .as_slice(),
        ["rev-list", "--grep=banana", "--grep=carrot", "HEAD"].as_slice(),
        ["rev-list", "--grep=BA[N]ANA", "-E", "HEAD"].as_slice(),
        ["rev-list", "--grep=BA[N]ANA", "--extended-regexp", "HEAD"].as_slice(),
        ["rev-list", "--grep=BA[N]ANA", "--basic-regexp", "HEAD"].as_slice(),
        ["rev-list", "--grep=banana", "-F", "HEAD"].as_slice(),
        ["rev-list", "--grep=banana", "--fixed-strings", "HEAD"].as_slice(),
        [
            "rev-list",
            "--grep=BA[N]ANA",
            "--extended-regexp",
            "--fixed-strings",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--grep=BA[N]ANA",
            "--fixed-strings",
            "--extended-regexp",
            "HEAD",
        ]
        .as_slice(),
        ["rev-list", "--grep=ba.+na", "-P", "HEAD"].as_slice(),
        ["rev-list", "--grep=ba.+na", "--perl-regexp", "HEAD"].as_slice(),
        ["rev-list", "--grep=ba.+na", "-P", "-F", "HEAD"].as_slice(),
        ["rev-list", "--grep=ba.+na", "-F", "-P", "HEAD"].as_slice(),
        [
            "rev-list",
            "--grep=ba.+na",
            "--perl-regexp",
            "--fixed-strings",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
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
fn rev_list_reflog_and_first_parent_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);

    write_file(repo.path(), "a.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "topic"]);
    write_file(repo.path(), "topic.txt", "topic\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "topic"]);

    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "main.txt", "main\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);
    git(
        repo.path(),
        ["merge", "--no-ff", "topic", "-m", "merge topic"],
    );

    for args in [
        ["rev-list", "--first-parent", "HEAD"].as_slice(),
        [
            "rev-list",
            "--walk-reflogs",
            "--grep-reflog=commit",
            "--format=%H",
            "HEAD",
        ]
        .as_slice(),
        ["rev-list", "-g", "--max-count=2", "--format=%H", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    assert_eq!(
        run_zmin_failure_output(repo.path(), &["rev-list", "--grep-reflog=commit", "HEAD"]),
        git_failure_output(repo.path(), &["rev-list", "--grep-reflog=commit", "HEAD"])
    );
}

#[test]
fn rev_list_reflog_and_first_parent_expansion_lanes_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);

    write_file(repo.path(), "a.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "topic"]);
    write_file(repo.path(), "topic.txt", "topic\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "topic"]);

    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "main.txt", "main\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);
    git(
        repo.path(),
        ["merge", "--no-ff", "topic", "-m", "merge topic"],
    );

    for args in [
        ["rev-list", "--first-parent", "HEAD~2..HEAD"].as_slice(),
        ["rev-list", "--first-parent", "HEAD", "^HEAD~2"].as_slice(),
        [
            "rev-list",
            "--walk-reflogs",
            "--grep-reflog=commit",
            "--grep-reflog=checkout",
            "--format=%H",
            "HEAD",
        ]
        .as_slice(),
        ["rev-list", "-g", "--format=%gd|%gs|%gn|%ge", "-2", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    for args in [
        ["rev-list", "-g", "--reverse", "HEAD"].as_slice(),
        ["rev-list", "-g", "HEAD", "^HEAD~1"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn rev_list_ref_selection_family_matches_stock_git() {
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
    git(repo.path(), ["tag", "v1", "HEAD"]);

    for args in [
        ["rev-list", "--branches", "--format=%H"].as_slice(),
        ["rev-list", "--branches=fea*", "--format=%H"].as_slice(),
        ["rev-list", "--remotes", "--format=%H"].as_slice(),
        ["rev-list", "--remotes=origin/*", "--format=%H"].as_slice(),
        ["rev-list", "--tags", "--format=%H"].as_slice(),
        ["rev-list", "--tags=v*", "--format=%H"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
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
        ["log", "--after=2023-11-14T22:20:00Z", "--format=%s", "HEAD"].as_slice(),
        ["log", "--until=2023-11-14T22:20:00Z", "--format=%s", "HEAD"].as_slice(),
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
fn rev_list_identity_time_and_parent_filters_match_stock_git() {
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
        ["rev-list", "--author=Alice", "HEAD"].as_slice(),
        ["rev-list", "--committer=Eve", "HEAD"].as_slice(),
        ["rev-list", "--since=2023-11-14T22:20:00Z", "HEAD"].as_slice(),
        ["rev-list", "--after=2023-11-14T22:20:00Z", "HEAD"].as_slice(),
        ["rev-list", "--until=2023-11-14T22:20:00Z", "HEAD"].as_slice(),
        ["rev-list", "--before=2023-11-14T22:20:00Z", "HEAD"].as_slice(),
        ["rev-list", "--merges", "HEAD"].as_slice(),
        ["rev-list", "--no-merges", "HEAD"].as_slice(),
        ["rev-list", "--max-parents=1", "HEAD"].as_slice(),
        ["rev-list", "--no-max-parents", "HEAD"].as_slice(),
        ["rev-list", "--min-parents=2", "HEAD"].as_slice(),
        ["rev-list", "--no-min-parents", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn rev_list_notes_abbrev_and_text_rendering_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());

    write_file(repo.path(), "body.txt", "body\n");
    git(repo.path(), ["add", "-A"]);
    let message_path = repo.path().join("message.txt");
    fs::write(&message_path, "subject\n\nline\twith\ttabs\n").expect("write message");
    git(repo.path(), ["commit", "-F", "message.txt"]);
    git(repo.path(), ["notes", "add", "-m", "note body"]);

    for args in [
        ["rev-list", "--no-notes", "-1", "HEAD"].as_slice(),
        ["rev-list", "--abbrev-commit", "-1", "HEAD"].as_slice(),
        ["rev-list", "--oneline", "--no-abbrev-commit", "-1", "HEAD"].as_slice(),
        ["rev-list", "--pretty=medium", "-1", "HEAD"].as_slice(),
        ["rev-list", "--pretty=medium", "--expand-tabs", "-1", "HEAD"].as_slice(),
        [
            "rev-list",
            "--pretty=medium",
            "--no-expand-tabs",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--pretty=medium",
            "--encoding=UTF-8",
            "-1",
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

    assert_eq!(
        run_zmin_failure_output(
            repo.path(),
            &["rev-list", "--format=%N", "--notes", "-1", "HEAD"]
        ),
        git_failure_output(
            repo.path(),
            &["rev-list", "--format=%N", "--notes", "-1", "HEAD"]
        )
    );
}

#[test]
fn rev_list_date_and_format_modes_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);

    for args in [
        ["rev-list", "--date=iso", "--format=%ad|%cd", "-1", "HEAD"].as_slice(),
        [
            "rev-list",
            "--pretty=format:%ad|%cd",
            "--date=iso",
            "-1",
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
fn rev_list_reflog_relative_date_and_notes_aliases_match_stock_git() {
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

    for args in [
        ["rev-list", "--quiet", "-1", "HEAD"].as_slice(),
        ["rev-list", "--quiet", "--format=%H", "-1", "HEAD"].as_slice(),
        [
            "rev-list",
            "--relative-date",
            "--format=%ad|%cd",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--date=iso",
            "--relative-date",
            "--format=%ad|%cd",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--relative-date",
            "--date=iso",
            "--format=%ad|%cd",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--quiet",
            "--date=relative",
            "--format=%ad|%cd",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        ["rev-list", "--reflog", "HEAD", "--format=%H"].as_slice(),
        ["rev-list", "--quiet", "--reflog", "HEAD", "--format=%H"].as_slice(),
        [
            "rev-list",
            "--reflog",
            "HEAD",
            "--date=relative",
            "--pretty=format:%gd|%ad|%cd",
        ]
        .as_slice(),
        [
            "rev-list",
            "--reflog",
            "HEAD",
            "--relative-date",
            "--pretty=format:%gd|%ad|%cd",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    git(repo.path(), ["notes", "add", "-m", "note body"]);

    for args in [
        [
            "rev-list",
            "--no-standard-notes",
            "--pretty=format:%N",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--standard-notes",
            "--pretty=format:%N",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--standard-notes",
            "--no-standard-notes",
            "--pretty=format:%N",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--show-notes-by-default",
            "--standard-notes",
            "--pretty=format:%N",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--show-notes-by-default",
            "--no-standard-notes",
            "--standard-notes",
            "--pretty=format:%N",
            "-1",
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
        [
            "rev-list",
            "--show-notes",
            "--pretty=format:%N",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--show-notes-by-default",
            "--pretty=format:%N",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--standard-notes",
            "--show-notes",
            "--pretty=format:%N",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--show-notes-by-default",
            "--no-standard-notes",
            "--pretty=format:%N",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--show-notes",
            "--no-standard-notes",
            "--pretty=format:%N",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--show-notes",
            "--standard-notes",
            "--pretty=format:%N",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "rev-list",
            "--standard-notes",
            "--show-notes",
            "--no-standard-notes",
            "--pretty=format:%N",
            "-1",
            "HEAD",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
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
        ["-c", "core.abbrev=8", "log", "--oneline", "-1"].as_slice(),
        ["-c", "core.abbrev=no", "log", "--oneline", "-1"].as_slice(),
        [
            "-c",
            "core.abbrev=8",
            "show",
            "--oneline",
            "--no-patch",
            "HEAD",
        ]
        .as_slice(),
        ["-c", "core.abbrev=8", "rev-list", "--oneline", "-1", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn history_core_abbrev_errors_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["commit", "--allow-empty", "-m", "base"]);
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");

    for args in [
        ["-c", "core.abbrev=3", "log", "--oneline", "-1"].as_slice(),
        ["-c", "core.abbrev=bogus", "log", "--oneline", "-1"].as_slice(),
        ["-c", "core.abbrev=full", "log", "--oneline", "-1"].as_slice(),
        [
            "-c",
            "core.abbrev=bogus",
            "-c",
            "core.abbrev=7",
            "log",
            "--oneline",
            "-1",
        ]
        .as_slice(),
        [
            "-c",
            "core.abbrev=7",
            "-c",
            "core.abbrev=bogus",
            "log",
            "--oneline",
            "-1",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_raw_output(zmin_bin(), repo.path(), args, "zmin"),
            command_raw_output(stock, repo.path(), args, "stock Git"),
            "args: {args:?}"
        );
    }
}

#[test]
fn sha256_log_abbrev_matches_pinned_git() {
    let repo = pinned_git_init_sha256();
    pinned_git_args(repo.path(), &["config", "user.name", "Bench"]);
    pinned_git_args(repo.path(), &["config", "user.email", "bench@example.test"]);
    write_file(repo.path(), "sha256.txt", "sha256\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_git_with_env(
        repo.path(),
        &["commit", "-m", "sha256 commit"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
    );
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    for (args, expected_width) in [
        (
            [
                "-c",
                "core.abbrev=12",
                "log",
                "--oneline",
                "--no-decorate",
                "-1",
            ]
            .as_slice(),
            12,
        ),
        (
            [
                "-c",
                "core.abbrev=no",
                "log",
                "--oneline",
                "--no-decorate",
                "-1",
            ]
            .as_slice(),
            64,
        ),
    ] {
        let zmin = command_raw_output(zmin_bin(), repo.path(), args, "zmin");
        let stock_result = command_raw_output(stock, repo.path(), args, "stock Git");
        assert_eq!(zmin, stock_result, "SHA-256 log tuple: {args:?}");
        let object_name = stock_result
            .stdout
            .split(|byte| byte.is_ascii_whitespace())
            .next()
            .expect("SHA-256 log object name");
        assert_eq!(
            object_name.len(),
            expected_width,
            "SHA-256 object name width"
        );
    }

    pinned_git_args(repo.path(), &["tag", "-a", "v1.0.0", "-m", "version"]);
    for args in [
        ["describe", "--always"].as_slice(),
        ["describe", "--abbrev=12"].as_slice(),
        ["describe", "--abbrev=0"].as_slice(),
    ] {
        assert_eq!(
            command_raw_output(zmin_bin(), repo.path(), args, "zmin"),
            command_raw_output(stock, repo.path(), args, "stock Git"),
            "SHA-256 describe tuple: {args:?}"
        );
    }
}

#[test]
fn sha256_annotated_tag_show_decodes_with_repository_algorithm() {
    let repo = pinned_git_init_sha256();
    pinned_git_args(repo.path(), &["config", "user.name", "Bench"]);
    pinned_git_args(repo.path(), &["config", "user.email", "bench@example.test"]);
    write_file(repo.path(), "sha256-tag.txt", "annotated tag\n");
    pinned_git_args(repo.path(), &["add", "sha256-tag.txt"]);
    pinned_git_with_env(
        repo.path(),
        &["commit", "-m", "sha256 tag target"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
    );
    pinned_git_args(
        repo.path(),
        &["tag", "-a", "sha256-tag", "-m", "annotated SHA-256 tag"],
    );
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    for args in [["show", "--no-patch", "sha256-tag"].as_slice()] {
        assert_eq!(
            command_raw_output(zmin_bin(), repo.path(), args, "zmin"),
            command_raw_output(stock, repo.path(), args, "stock Git"),
            "SHA-256 annotated tag show tuple: {args:?}"
        );
    }
}

#[test]
fn packed_refs_show_and_rev_list_match_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        let full = pinned_git_args(repo.path(), &["rev-parse", "refs/heads/main"]);
        let short = full[..12].to_owned();
        let stock = required_pinned_stock_git();
        let stock = stock.to_str().expect("pinned Git path is UTF-8");
        let cases = [
            vec!["rev-parse", "--verify", "HEAD"],
            vec!["rev-parse", "--verify", "refs/heads/main"],
            vec!["rev-parse", "--verify", "refs/heads/loose"],
            vec!["rev-parse", "--verify", "annotated"],
            vec!["show", "--no-patch", "HEAD"],
            vec!["show", "--no-patch", "refs/heads/main"],
            vec!["show", "--no-patch", "refs/heads/loose"],
            vec!["show", "--no-patch", "annotated"],
            vec!["show", "--no-patch", "lightweight"],
            vec!["show", "--no-patch", full.as_str()],
            vec!["show", "--no-patch", short.as_str()],
            vec!["show", "-s", "--format=%H", "HEAD"],
            vec![
                "show",
                "--no-patch",
                "--format=%H",
                "--decorate=full",
                "HEAD",
            ],
            vec!["rev-list", "-1", "HEAD"],
            vec!["rev-list", "-1", "refs/heads/main"],
            vec!["rev-list", "-1", "refs/heads/loose"],
            vec!["rev-list", "-1", "annotated"],
            vec!["rev-list", "-1", full.as_str()],
            vec!["rev-list", "-1", short.as_str()],
            vec!["rev-list", "-1", "--format=%H", "HEAD"],
            vec!["rev-list", "--all"],
            vec!["rev-list", "--format=%H", "--all"],
            vec!["log", "-1", "--format=%H", "--decorate=full", "HEAD"],
        ];
        for args in cases {
            let zmin = command_raw_output(zmin_bin(), repo.path(), &args, "zmin");
            let stock_result = command_raw_output(stock, repo.path(), &args, "stock Git");
            assert_eq!(
                zmin, stock_result,
                "packed-ref tuple: sha256={sha256} {args:?}"
            );
        }
    }
}

#[test]
fn sha256_filtered_rev_list_objects_matches_pinned_git() {
    let repo = pinned_history_fixture(true);
    let args = ["rev-list", "--objects", "--filter=blob:none", "HEAD"];
    assert_eq!(
        raw_zmin_output(repo.path(), &args),
        raw_pinned_output(repo.path(), &args),
        "SHA-256 filtered rev-list --objects tuple"
    );
}

#[test]
fn rev_list_filter_option_contract_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        for args in [
            ["rev-list", "--filter=blob:none", "HEAD"].as_slice(),
            ["rev-list", "--filter-provided-objects", "HEAD"].as_slice(),
            ["rev-list", "--filter-provided-objects", "--objects", "HEAD"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn sha256_promisor_pack_index_uses_repository_algorithm() {
    let repo = pinned_history_fixture(true);
    let head = pinned_git_args(repo.path(), &["rev-parse", "HEAD"]);
    let stock = required_pinned_stock_git();
    pack_as_from_promisor_with_git(
        repo.path(),
        &head,
        stock.to_str().expect("pinned Git path UTF-8"),
    );
    pinned_git_args(
        repo.path(),
        &["config", "core.repositoryformatversion", "1"],
    );
    pinned_git_args(
        repo.path(),
        &["config", "extensions.partialclone", "pinned-local-promisor"],
    );
    let args = [
        "rev-list",
        "--exclude-promisor-objects",
        "--objects",
        "HEAD",
    ];
    assert_eq!(
        raw_zmin_output(repo.path(), &args),
        raw_pinned_output(repo.path(), &args),
        "SHA-256 promisor pack index tuple"
    );
}

#[test]
fn sha256_show_and_reflog_abbrev_match_pinned_git() {
    let repo = pinned_git_init_sha256();
    pinned_git_args(repo.path(), &["config", "user.name", "Bench"]);
    pinned_git_args(repo.path(), &["config", "user.email", "bench@example.test"]);
    write_file(repo.path(), "sha256.txt", "one\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_git_with_env(
        repo.path(),
        &["commit", "-m", "sha256 one"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
    );
    write_file(repo.path(), "sha256.txt", "two\n");
    pinned_git_args(repo.path(), &["add", "-A"]);
    pinned_git_with_env(
        repo.path(),
        &["commit", "-m", "sha256 two"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000100 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000100 +0000"),
        ],
    );
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    for (args, expected_width) in [
        (
            [
                "show",
                "--oneline",
                "--no-patch",
                "--no-abbrev-commit",
                "HEAD",
            ]
            .as_slice(),
            64,
        ),
        (
            [
                "-c",
                "core.abbrev=12",
                "show",
                "--oneline",
                "--no-patch",
                "--abbrev-commit",
                "HEAD",
            ]
            .as_slice(),
            12,
        ),
        (
            [
                "log",
                "--reflog",
                "--oneline",
                "--no-decorate",
                "--no-abbrev-commit",
                "-1",
                "HEAD",
            ]
            .as_slice(),
            64,
        ),
        (
            ["log", "--reflog", "--format=%H", "HEAD@{1}"].as_slice(),
            64,
        ),
        (
            [
                "log",
                "--reflog",
                "--format=%H",
                "HEAD@{2023-11-14 22:15:00 +0000}",
            ]
            .as_slice(),
            64,
        ),
        (
            [
                "-c",
                "core.abbrev=12",
                "log",
                "--reflog",
                "--oneline",
                "--no-decorate",
                "--abbrev-commit",
                "-1",
                "HEAD",
            ]
            .as_slice(),
            12,
        ),
        (
            ["reflog", "show", "--no-abbrev-commit", "HEAD"].as_slice(),
            64,
        ),
    ] {
        let zmin = command_raw_output(zmin_bin(), repo.path(), args, "zmin");
        let stock_result = command_raw_output(stock, repo.path(), args, "stock Git");
        assert_eq!(zmin, stock_result, "SHA-256 show/reflog tuple: {args:?}");
        let object_name = stock_result
            .stdout
            .split(|byte| byte.is_ascii_whitespace())
            .next()
            .expect("SHA-256 show/reflog object name");
        assert_eq!(object_name.len(), expected_width, "SHA-256 object width");
    }
}

#[test]
fn reflog_numeric_threshold_and_date_scan_match_pinned_git() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        for selector in [
            "0",
            "1",
            "100000000",
            "1700000200",
            "2023-11-14 22:15:00 +0000",
            "2023.11.14 22:15:00 +0000",
        ] {
            let objectish = format!("HEAD@{{{selector}}}");
            let args = ["rev-parse", "--verify", objectish.as_str()];
            assert_eq!(
                command_raw_output(zmin_bin(), repo.path(), &args, "zmin"),
                command_raw_output(
                    required_pinned_stock_git()
                        .to_str()
                        .expect("pinned Git path is UTF-8"),
                    repo.path(),
                    &args,
                    "stock Git",
                ),
                "reflog selector tuple: sha256={sha256} selector={selector}"
            );
        }
        assert_known_reflog_ordinal_range_gap(&repo.path(), "99999999");
    }
}

#[test]
fn reflog_missing_empty_and_malformed_logs_match_pinned_git() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        let log_path = repo.path().join(".git/logs/refs/heads/main");
        let original = fs::read(&log_path).expect("read branch reflog");

        fs::remove_file(&log_path).expect("remove branch reflog");
        for selector in ["0", "1", "100000000", "1700000200"] {
            assert_reflog_selector_tuple(repo.path(), "main", selector);
        }

        fs::write(&log_path, Vec::<u8>::new()).expect("write empty branch reflog");
        for selector in ["0", "1", "100000000", "1700000200"] {
            assert_reflog_selector_tuple(repo.path(), "main", selector);
        }

        fs::write(&log_path, b"not a reflog record\n\xff\n")
            .expect("write malformed branch reflog");
        for selector in ["0", "1", "100000000", "1700000200"] {
            assert_reflog_selector_tuple(repo.path(), "main", selector);
        }

        fs::write(&log_path, original).expect("restore branch reflog");
    }
}

#[test]
fn reftable_reflog_selectors_match_pinned_git() {
    let repo = pinned_reftable_history_fixture();
    for selector in [
        "0",
        "1",
        "1600000000",
        "1700000000",
        "1700000050",
        "1700000100",
        "1800000000",
    ] {
        assert_reflog_selector_tuple(repo.path(), "refs/heads/master", selector);
    }
}

#[test]
fn reftable_reflog_tombstone_matches_pinned_git() {
    for sha256 in [false, true] {
        let repo = if sha256 {
            pinned_reftable_sha256_history_fixture()
        } else {
            pinned_reftable_history_fixture()
        };
        pinned_git_args(
            repo.path(),
            ["reflog", "delete", "--rewrite", "refs/heads/master@{1}"].as_slice(),
        );
        for selector in ["0", "1", "1700000000", "1700000050", "1800000000"] {
            assert_reflog_selector_tuple(repo.path(), "refs/heads/master", selector);
        }
    }
}

#[test]
fn reflog_gap_warning_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        let gap_old = pinned_git_args(repo.path(), ["rev-parse", "HEAD~2"].as_slice());
        rewrite_main_reflog_field_at_timestamp(repo.path(), "1700000300", 0, &gap_old);
        assert_reflog_selector_tuple(repo.path(), "main", "1700000200");
    }
}

#[test]
fn reflog_null_successor_old_id_suppresses_gap_warning() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        rewrite_main_reflog_field_at_timestamp(
            repo.path(),
            "1700000300",
            0,
            &zero_object_id_hex(sha256),
        );
        assert_reflog_selector_tuple(repo.path(), "main", "1700000200");
    }
}

#[test]
fn reflog_diverged_current_warning_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        let alternate = pinned_git_args(repo.path(), ["rev-parse", "HEAD~1"].as_slice());
        rewrite_main_reflog_field_at_timestamp(repo.path(), "1700000300", 1, &alternate);
        assert_reflog_selector_tuple(repo.path(), "main", "1800000000");
    }
}

#[test]
fn reftable_reflog_gap_warning_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_reftable_warning_fixture_for(sha256);
        pinned_git_args(repo.path(), &["reflog", "delete", "refs/heads/master@{1}"]);
        let args = ["rev-parse", "--verify", "refs/heads/master@{1700000050}"];
        let stock = raw_pinned_output(repo.path(), &args);
        let zmin = raw_zmin_output(repo.path(), &args);
        assert_eq!(zmin, stock, "reftable gap tuple: sha256={sha256}");
        assert!(
            stock
                .stderr
                .windows(b"has gap after".len())
                .any(|window| window == b"has gap after"),
            "pinned reftable gap warning missing: {:?}",
            String::from_utf8_lossy(&stock.stderr)
        );
    }
}

#[test]
fn reftable_reflog_reset_suppresses_null_old_gap_warning_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_reftable_history_fixture_for(sha256);
        let current = pinned_git_args(repo.path(), &["rev-parse", "refs/heads/master"]);
        pinned_git_args(repo.path(), &["update-ref", "-d", "refs/heads/master"]);
        pinned_git_with_env(
            repo.path(),
            &["update-ref", "refs/heads/master", &current],
            &[("GIT_COMMITTER_DATE", "1700000200 +0000")],
        );
        let args = ["rev-parse", "--verify", "refs/heads/master@{1700000050}"];
        let stock = raw_pinned_output(repo.path(), &args);
        let zmin = raw_zmin_output(repo.path(), &args);
        assert_eq!(zmin, stock, "reftable reset tuple: sha256={sha256}");
        assert!(
            !stock
                .stderr
                .windows(b"has gap after".len())
                .any(|window| { window == b"has gap after" })
        );
    }
}

#[test]
fn reftable_reflog_diverged_current_warning_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_reftable_warning_fixture_for(sha256);
        pinned_git_args(repo.path(), &["reflog", "delete", "refs/heads/master@{0}"]);
        let args = ["rev-parse", "--verify", "refs/heads/master@{1800000000}"];
        let stock = raw_pinned_output(repo.path(), &args);
        let zmin = raw_zmin_output(repo.path(), &args);
        assert_eq!(zmin, stock, "reftable diverged tuple: sha256={sha256}");
        assert!(
            stock
                .stderr
                .windows(b"unexpectedly ended".len())
                .any(|window| window == b"unexpectedly ended"),
            "pinned reftable diverged warning missing: {:?}",
            String::from_utf8_lossy(&stock.stderr)
        );
    }
}

#[test]
fn log_oneline_extends_seed_width_for_colliding_commit_ids() {
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "collision.txt", "collision\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    let tree = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);
    let (left, right) = commit_collision_pair(repo.path(), &tree);
    assert_eq!(
        &left[..7],
        &right[..7],
        "fixture must collide at seed width"
    );
    git(
        repo.path(),
        ["update-ref", "refs/heads/collision-left", &left],
    );
    git(
        repo.path(),
        ["update-ref", "refs/heads/collision-right", &right],
    );
    for value in ["7", "12", "4", "no"] {
        let config = format!("core.abbrev={value}");
        let args = [
            "-c",
            config.as_str(),
            "log",
            "--all",
            "--oneline",
            "--no-decorate",
        ];
        let zmin = command_raw_output(zmin_bin(), repo.path(), &args, "zmin");
        let stock = command_raw_output(stock, repo.path(), &args, "stock Git");
        assert_eq!(zmin, stock, "core.abbrev={value}");
        let minimum = value.parse::<usize>().unwrap_or(40);
        let stdout = String::from_utf8(stock.stdout.clone()).expect("stock log stdout");
        let ids = stdout
            .lines()
            .map(|line| line.split_whitespace().next().expect("commit id"));
        for id in ids {
            assert!(id.len() >= minimum, "core.abbrev={value}: {id}");
            if value == "no" {
                assert_eq!(id.len(), 40, "core.abbrev=no");
            }
        }
    }
    for args in [["log", "--all", "--format=%h", "--no-decorate"].as_slice()] {
        assert_eq!(
            command_raw_output(zmin_bin(), repo.path(), args, "zmin formatted history"),
            command_raw_output(stock, repo.path(), args, "stock formatted history"),
            "formatted history: {args:?}"
        );
    }
}

#[test]
fn merge_parent_renderers_use_per_object_abbreviation_widths() {
    let stock = required_pinned_stock_git();
    let stock = stock.to_str().expect("pinned Git path is UTF-8");
    let repo = TempDir::new().expect("temporary merge repository");
    pinned_git_args(repo.path(), &["init", "-q"]);
    let tree = write_loose_object(repo.path(), "tree", &[]);
    let (left, right) = commit_collision_pair(repo.path(), &tree);
    assert_eq!(
        &left[..7],
        &right[..7],
        "parents must collide at seed width"
    );
    let merge_content = format!(
        "tree {tree}\nparent {left}\nparent {right}\nauthor Merge <merge@example.test> 1700000001 +0000\ncommitter Merge <merge@example.test> 1700000001 +0000\n\nmerge\n"
    );
    let merge = write_loose_object(repo.path(), "commit", merge_content.as_bytes());
    pinned_git_args(
        repo.path(),
        &["update-ref", "refs/heads/main", merge.as_str()],
    );

    for args in [
        [
            "-c",
            "core.abbrev=4",
            "log",
            "--oneline",
            "--parents",
            "--no-decorate",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "-c",
            "core.abbrev=4",
            "show",
            "--no-patch",
            "--pretty=medium",
            "--abbrev=4",
            "HEAD",
        ]
        .as_slice(),
        [
            "-c",
            "core.abbrev=4",
            "rev-list",
            "--oneline",
            "--parents",
            "-1",
            "HEAD",
        ]
        .as_slice(),
        [
            "-c",
            "core.abbrev=4",
            "show",
            "--no-patch",
            "--separate-merges",
            "--pretty=medium",
            "--abbrev=4",
            "HEAD",
        ]
        .as_slice(),
    ] {
        let zmin = command_raw_output(zmin_bin(), repo.path(), args, "zmin");
        let expected = command_raw_output(stock, repo.path(), args, "stock Git");
        assert_eq!(zmin, expected, "merge parent tuple: {args:?}");
    }

    let oneline = command_raw_output(
        stock,
        repo.path(),
        &[
            "-c",
            "core.abbrev=4",
            "log",
            "--oneline",
            "--parents",
            "--no-decorate",
            "-1",
            "HEAD",
        ],
        "stock Git",
    );
    let first_line = oneline
        .stdout
        .split(|byte| *byte == b'\n')
        .next()
        .expect("merge log line");
    let widths = first_line
        .split(|byte| byte.is_ascii_whitespace())
        .filter(|field| !field.is_empty())
        .take(3)
        .map(<[u8]>::len)
        .collect::<Vec<_>>();
    assert_eq!(widths, [4, 8, 8], "commit and colliding parent widths");
}

#[test]
fn pretty_parent_placeholder_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        let cases = [
            ["log", "-1", "--format=%p", "HEAD~2"].as_slice(),
            ["log", "-1", "--format=%p", "HEAD~1"].as_slice(),
            ["log", "-1", "--format=%p", "HEAD"].as_slice(),
            ["show", "--no-patch", "--format=%h|%p|%P", "HEAD"].as_slice(),
            [
                "show",
                "--no-patch",
                "--format=%h|%p|%P",
                "--abbrev=4",
                "HEAD",
            ]
            .as_slice(),
            [
                "show",
                "--no-patch",
                "--format=%h|%p|%P",
                "--abbrev=12",
                "HEAD",
            ]
            .as_slice(),
            [
                "show",
                "--no-patch",
                "--format=%h|%p|%P",
                "--abbrev",
                "HEAD",
            ]
            .as_slice(),
            [
                "show",
                "--no-patch",
                "--format=%h|%p|%P",
                "--no-abbrev",
                "HEAD",
            ]
            .as_slice(),
            [
                "show",
                "--no-patch",
                "--format=%h|%p|%P",
                "--no-abbrev-commit",
                "HEAD",
            ]
            .as_slice(),
            [
                "-c",
                "core.abbrev=12",
                "log",
                "-1",
                "--format=%h|%p|%P",
                "--no-abbrev-commit",
                "HEAD",
            ]
            .as_slice(),
            [
                "-c",
                "core.abbrev=12",
                "log",
                "--oneline",
                "--parents",
                "--no-decorate",
                "--no-abbrev-commit",
                "-1",
                "HEAD",
            ]
            .as_slice(),
            ["log", "-1", "--pretty=medium", "--no-abbrev-commit", "HEAD"].as_slice(),
            [
                "show",
                "-s",
                "--pretty=medium",
                "--no-abbrev-commit",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects-edge",
                "-z",
                "HEAD",
                "^HEAD~1",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ];
        for args in cases {
            assert_eq!(
                raw_zmin_output(repo.path(), args),
                raw_pinned_output(repo.path(), args),
                "pretty parent tuple: sha256={sha256}, args={args:?}"
            );
        }

        for config in ["core.abbrev=4", "core.abbrev=12"] {
            let args = ["-c", config, "log", "-1", "--format=%h|%p|%P", "HEAD"];
            assert_eq!(
                raw_zmin_output(repo.path(), &args),
                raw_pinned_output(repo.path(), &args),
                "pretty parent config tuple: sha256={sha256}, config={config}"
            );
        }

        if !sha256 {
            for args in [
                ["rev-list", "--format=%h|%p|%P", "-1", "HEAD"].as_slice(),
                [
                    "-c",
                    "core.abbrev=12",
                    "rev-list",
                    "-1",
                    "--format=%h|%p|%P",
                    "--no-abbrev-commit",
                    "HEAD",
                ]
                .as_slice(),
                [
                    "-c",
                    "core.abbrev=12",
                    "rev-list",
                    "--oneline",
                    "--parents",
                    "--no-abbrev-commit",
                    "-1",
                    "HEAD",
                ]
                .as_slice(),
            ] {
                assert_eq!(
                    raw_zmin_output(repo.path(), args),
                    raw_pinned_output(repo.path(), args),
                    "pretty parent rev-list tuple: {args:?}"
                );
            }
        }
    }
}

#[test]
fn show_empty_user_formats_match_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        let cases = [
            ["-c", "core.abbrev=7", "show", "--format=", "HEAD~2"].as_slice(),
            ["-c", "core.abbrev=7", "show", "--format=", "HEAD~1"].as_slice(),
            ["-c", "core.abbrev=7", "show", "--format=", "HEAD"].as_slice(),
            ["show", "--format=", "--no-patch", "HEAD~1"].as_slice(),
            [
                "-c",
                "core.abbrev=7",
                "show",
                "--format=",
                "HEAD~1",
                "HEAD~2",
            ]
            .as_slice(),
            ["show", "--format=", "--no-patch", "HEAD~1", "HEAD~2"].as_slice(),
            ["-c", "core.abbrev=7", "show", "--pretty=format:", "HEAD~1"].as_slice(),
            [
                "-c",
                "core.abbrev=7",
                "show",
                "--pretty=format:",
                "HEAD~1",
                "HEAD~2",
            ]
            .as_slice(),
            ["-c", "core.abbrev=7", "show", "--pretty=tformat:", "HEAD~1"].as_slice(),
            [
                "-c",
                "core.abbrev=7",
                "show",
                "--pretty=tformat:",
                "HEAD~1",
                "HEAD~2",
            ]
            .as_slice(),
            ["show", "--pretty=format:", "--no-patch", "HEAD~1", "HEAD~2"].as_slice(),
            ["show", "--format=%n", "--no-patch", "HEAD~1"].as_slice(),
        ];
        for args in cases {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn log_and_rev_list_empty_user_formats_match_pinned_git() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        let log_cases = [
            ["log", "--format=", "HEAD~2"].as_slice(),
            ["log", "--pretty=", "HEAD"].as_slice(),
            ["log", "--pretty=format:", "HEAD~1", "HEAD"].as_slice(),
            ["log", "--pretty=tformat:", "HEAD~2", "HEAD"].as_slice(),
            ["log", "-z", "--format=", "HEAD~1", "HEAD"].as_slice(),
            ["log", "-z", "--pretty=format:", "HEAD~1", "HEAD"].as_slice(),
            ["log", "-z", "--pretty=tformat:", "HEAD~1", "HEAD"].as_slice(),
        ];
        for args in log_cases {
            assert_history_tuple(repo.path(), args);
        }

        if !sha256 {
            let rev_list_cases = [
                ["rev-list", "--format=", "HEAD~2"].as_slice(),
                ["rev-list", "--pretty=", "HEAD"].as_slice(),
                ["rev-list", "--pretty=format:", "HEAD~1", "HEAD"].as_slice(),
                ["rev-list", "--pretty=tformat:", "HEAD~2", "HEAD"].as_slice(),
            ];
            for args in rev_list_cases {
                assert_history_tuple(repo.path(), args);
            }
        }
    }
}

#[test]
fn bare_pretty_is_medium_but_explicit_empty_stays_empty() {
    // SHA-256 revision resolution remains a separately queued gap; this
    // parser-boundary matrix uses the supported SHA-1 history surface.
    let repo = pinned_history_fixture(false);
    for args in [
        ["show", "-s", "--pretty", "HEAD"].as_slice(),
        ["show", "-s", "--pretty=", "HEAD"].as_slice(),
        ["show", "-s", "--pretty=medium", "HEAD"].as_slice(),
        ["log", "-1", "--pretty", "HEAD^"].as_slice(),
        ["log", "-1", "--pretty=", "HEAD^"].as_slice(),
        ["log", "-1", "--pretty=medium", "HEAD^"].as_slice(),
        ["rev-list", "-1", "--pretty", "HEAD^"].as_slice(),
        ["rev-list", "-1", "--pretty=", "HEAD^"].as_slice(),
        ["rev-list", "-1", "--pretty=medium", "HEAD^"].as_slice(),
    ] {
        assert_history_tuple(repo.path(), args);
    }
}

#[test]
fn diff_tree_pretty_value_is_optional_but_equals_binds_the_format() {
    let repo = pinned_history_fixture(false);
    for args in [
        ["diff-tree", "--root", "HEAD"].as_slice(),
        ["diff-tree", "--root", "--pretty", "HEAD"].as_slice(),
        ["diff-tree", "--root", "--pretty=", "HEAD"].as_slice(),
        ["diff-tree", "--root", "--pretty=medium", "HEAD"].as_slice(),
    ] {
        assert_history_tuple(repo.path(), args);
    }
}

#[test]
fn rev_list_walk_reflogs_bare_pretty_matches_medium_with_lf_records() {
    let repo = pinned_linear_reflog_fixture();
    for args in [
        ["rev-list", "--walk-reflogs", "-n", "2", "--pretty", "HEAD"].as_slice(),
        [
            "rev-list",
            "--walk-reflogs",
            "-n",
            "2",
            "--pretty=medium",
            "HEAD",
        ]
        .as_slice(),
    ] {
        assert_history_tuple(repo.path(), args);
    }
}

#[test]
fn separate_merge_empty_formats_match_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        let cases = [
            [
                "-c",
                "core.abbrev=7",
                "log",
                "-1",
                "--patch",
                "--diff-merges=separate",
                "--format=",
                "HEAD",
            ]
            .as_slice(),
            [
                "-c",
                "core.abbrev=7",
                "log",
                "-1",
                "--patch",
                "--diff-merges=separate",
                "--pretty=",
                "HEAD",
            ]
            .as_slice(),
            [
                "-c",
                "core.abbrev=7",
                "log",
                "-1",
                "--patch",
                "--diff-merges=separate",
                "--pretty=format:",
                "HEAD",
            ]
            .as_slice(),
            [
                "-c",
                "core.abbrev=7",
                "log",
                "-1",
                "--patch",
                "--diff-merges=separate",
                "--pretty=tformat:",
                "HEAD",
            ]
            .as_slice(),
            [
                "-c",
                "core.abbrev=7",
                "log",
                "-z",
                "--patch",
                "--diff-merges=separate",
                "--format=",
                "HEAD~1",
                "HEAD",
            ]
            .as_slice(),
            [
                "-c",
                "core.abbrev=7",
                "log",
                "-z",
                "--patch",
                "--diff-merges=separate",
                "--pretty=tformat:",
                "HEAD~1",
                "HEAD",
            ]
            .as_slice(),
            [
                "-c",
                "core.abbrev=7",
                "log",
                "-z",
                "--patch",
                "--diff-merges=separate",
                "--pretty=format:",
                "HEAD~1",
                "HEAD",
            ]
            .as_slice(),
        ];
        for args in cases {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn history_output_selection_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        let cases = [
            ["log", "-1", "--format=%H", "--no-patch", "HEAD"].as_slice(),
            ["log", "-1", "--format=%H", "-s", "HEAD"].as_slice(),
            [
                "log",
                "-1",
                "--format=%H",
                "--patch",
                "--no-patch",
                "HEAD~1",
            ]
            .as_slice(),
            [
                "log",
                "-1",
                "--format=%H",
                "--no-patch",
                "--patch",
                "HEAD~1",
            ]
            .as_slice(),
            ["log", "-1", "--format=%H", "-ps", "HEAD~1"].as_slice(),
            ["log", "-1", "--format=%H", "-sp", "HEAD~1"].as_slice(),
            ["log", "-1", "--format=%H", "HEAD~1", "--", "-ps"].as_slice(),
            ["log", "-1", "--format=%H", "--diff-merges=separate", "HEAD"].as_slice(),
            [
                "log",
                "-1",
                "--format=%H",
                "--diff-merges=separate",
                "--no-patch",
                "HEAD",
            ]
            .as_slice(),
            [
                "log",
                "-1",
                "--format=%H",
                "--no-patch",
                "--diff-merges=separate",
                "HEAD",
            ]
            .as_slice(),
            [
                "log",
                "-1",
                "--format=%H",
                "--diff-merges=first-parent",
                "HEAD",
            ]
            .as_slice(),
            [
                "log",
                "-1",
                "--format=%H",
                "--diff-merges=first-parent",
                "--no-patch",
                "HEAD",
            ]
            .as_slice(),
            [
                "log",
                "-1",
                "--format=%H",
                "--no-patch",
                "--diff-merges=first-parent",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "-1",
                "--format=%H",
                "--no-commit-header",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--no-commit-header",
                "--format=%H",
                "HEAD",
                "HEAD~1",
            ]
            .as_slice(),
        ];
        for args in cases {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn show_patch_option_order_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        for args in [
            ["show", "--format=%H", "--patch", "--no-patch", "HEAD~1"].as_slice(),
            ["show", "--format=%H", "--no-patch", "--patch", "HEAD~1"].as_slice(),
            ["show", "--format=%H", "-s", "-p", "HEAD~1"].as_slice(),
            ["show", "--format=%H", "-p", "-s", "HEAD~1"].as_slice(),
            ["show", "--format=%H", "-ps", "HEAD~1"].as_slice(),
            ["show", "--format=%H", "-sp", "HEAD~1"].as_slice(),
            ["show", "--format=%H", "HEAD~1", "--", "-ps"].as_slice(),
            ["show", "--format=%H", "HEAD~1"].as_slice(),
            ["show", "--root", "--format=%H", "HEAD~2"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn rev_list_builtin_formats_and_reflogs_match_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        for format in ["medium", "short", "full", "fuller"] {
            let pretty = format!("--pretty={format}");
            for args in [
                ["rev-list", "-2", pretty.as_str(), "HEAD"].as_slice(),
                [
                    "rev-list",
                    "-2",
                    "--no-commit-header",
                    pretty.as_str(),
                    "HEAD",
                ]
                .as_slice(),
                ["rev-list", "--walk-reflogs", "-2", pretty.as_str(), "HEAD"].as_slice(),
                [
                    "rev-list",
                    "--walk-reflogs",
                    "-2",
                    "--no-commit-header",
                    pretty.as_str(),
                    "HEAD",
                ]
                .as_slice(),
            ] {
                assert_history_tuple(repo.path(), args);
            }
        }
    }
}

#[test]
fn log_reflog_relative_date_and_notes_aliases_match_stock_git() {
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

    for args in [
        ["log", "--quiet", "-1"].as_slice(),
        ["log", "--quiet", "-1", "--format=%H"].as_slice(),
        ["log", "--relative-date", "-1", "--format=%ad|%cd"].as_slice(),
        [
            "log",
            "--date=iso",
            "--relative-date",
            "-1",
            "--format=%ad|%cd",
        ]
        .as_slice(),
        [
            "log",
            "--relative-date",
            "--date=iso",
            "-1",
            "--format=%ad|%cd",
        ]
        .as_slice(),
        [
            "log",
            "--quiet",
            "--date=relative",
            "-1",
            "--format=%ad|%cd",
        ]
        .as_slice(),
        ["log", "--reflog", "HEAD", "--format=%H"].as_slice(),
        ["log", "--quiet", "--reflog", "HEAD", "--format=%H"].as_slice(),
        [
            "log",
            "--reflog",
            "HEAD",
            "--date=relative",
            "--pretty=format:%gd|%ad|%cd",
        ]
        .as_slice(),
        [
            "log",
            "--reflog",
            "HEAD",
            "--relative-date",
            "--pretty=format:%gd|%ad|%cd",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    git(repo.path(), ["notes", "add", "-m", "note body"]);

    for args in [
        ["log", "--show-notes", "-1", "--format=%N"].as_slice(),
        ["log", "--show-notes-by-default", "-1", "--format=%N"].as_slice(),
        ["log", "--no-standard-notes", "-1", "--format=%N"].as_slice(),
        ["log", "--standard-notes", "-1", "--format=%N"].as_slice(),
        [
            "log",
            "--standard-notes",
            "--no-standard-notes",
            "-1",
            "--format=%N",
        ]
        .as_slice(),
        [
            "log",
            "--show-notes",
            "--no-standard-notes",
            "-1",
            "--format=%N",
        ]
        .as_slice(),
        [
            "log",
            "--show-notes-by-default",
            "--no-standard-notes",
            "-1",
            "--format=%N",
        ]
        .as_slice(),
        [
            "log",
            "--standard-notes",
            "--show-notes",
            "-1",
            "--format=%N",
        ]
        .as_slice(),
        [
            "log",
            "--show-notes-by-default",
            "--standard-notes",
            "-1",
            "--format=%N",
        ]
        .as_slice(),
        [
            "log",
            "--standard-notes",
            "--show-notes-by-default",
            "-1",
            "--format=%N",
        ]
        .as_slice(),
        [
            "log",
            "--show-notes",
            "--standard-notes",
            "-1",
            "--format=%N",
        ]
        .as_slice(),
        [
            "log",
            "--standard-notes",
            "--show-notes",
            "--no-standard-notes",
            "-1",
            "--format=%N",
        ]
        .as_slice(),
        [
            "log",
            "--show-notes-by-default",
            "--no-standard-notes",
            "--standard-notes",
            "-1",
            "--format=%N",
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
        ["show", "--show-signature", "HEAD"].as_slice(),
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
fn log_and_show_text_rendering_options_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());

    write_file(repo.path(), "a.txt", "body\n");
    git(repo.path(), ["add", "-A"]);
    let message_path = repo.path().join("message.txt");
    fs::write(&message_path, "subject\n\nline\twith\ttabs\n").expect("write message");
    git(repo.path(), ["commit", "-F", "message.txt"]);

    for args in [
        ["log", "--no-walk", "HEAD"].as_slice(),
        ["log", "--no-walk", "--expand-tabs", "HEAD"].as_slice(),
        ["log", "--no-walk", "--no-expand-tabs", "HEAD"].as_slice(),
        ["log", "--no-walk", "--encoding=UTF-8", "HEAD"].as_slice(),
        ["show", "--no-patch", "HEAD"].as_slice(),
        ["show", "--no-patch", "--expand-tabs", "HEAD"].as_slice(),
        ["show", "--no-patch", "--no-expand-tabs", "HEAD"].as_slice(),
        ["show", "--no-patch", "--encoding=UTF-8", "HEAD"].as_slice(),
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
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
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
fn blame_ignore_whitespace_rewrite_matches_stock_git() {
    let git_repo = blame_whitespace_rewrite_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    for args in [
        ["blame", "--porcelain", "-l", "-w", "HEAD", "--", "a.txt"].as_slice(),
        ["blame", "--incremental", "-l", "-w", "HEAD", "--", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
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
            &[
                "annotate",
                "--contents",
                &zmin_contents,
                "HEAD",
                "--",
                "a.txt"
            ],
        ),
        git_args(
            git_repo.path(),
            &[
                "annotate",
                "--contents",
                &git_contents,
                "HEAD",
                "--",
                "a.txt"
            ],
        )
    );

    let opt_git_repo = git_init();
    configure_identity(opt_git_repo.path());
    write_file(opt_git_repo.path(), "a.txt", "one\ntwo\n");
    git(opt_git_repo.path(), ["add", "-A"]);
    git_with_env(opt_git_repo.path(), ["commit", "-m", "init"]);
    let opt_zmin_repo = clone_repo_fixture(opt_git_repo.path());
    let opt_head = command_output("git", opt_git_repo.path(), &["rev-parse", "HEAD"], "git").1;
    write_file(
        opt_git_repo.path(),
        "ignore-revs.txt",
        &format!("{opt_head}\n"),
    );
    write_file(
        opt_zmin_repo.path(),
        "ignore-revs.txt",
        &format!("{opt_head}\n"),
    );
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
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        for args in [
            ["log", "-1", "--format=%s", "-m"].as_slice(),
            ["log", "-1", "--format=%s", "-m", "--no-patch"].as_slice(),
            ["log", "-1", "--format=%s", "--no-patch", "-m"].as_slice(),
            ["log", "-1", "--format=%s", "-m", "--patch"].as_slice(),
            ["log", "-1", "--format=%s", "-m", "--stat"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
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
        [
            "log",
            "-z",
            "--date=iso-strict",
            "--format=%H%x00%ad%x00%cd",
            "-1",
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
fn show_rename_detection_ide_option_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "old.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["mv", "old.txt", "new.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "rename"]);

    for args in [
        ["show", "--format=%H", "--name-status", "-M", "HEAD"].as_slice(),
        ["show", "--format=%H", "--raw", "-M", "HEAD"].as_slice(),
        [
            "show",
            "--format=%H%x00%s",
            "--name-status",
            "-z",
            "-M",
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
fn log_object_and_selector_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "a.txt", "two\n");
    write_file(repo.path(), "b.txt", "new\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    git(repo.path(), ["tag", "v1", "HEAD"]);

    for args in [
        ["log", "--tags", "--format=%H"].as_slice(),
        ["log", "--tags=v*", "--format=%H"].as_slice(),
        ["log", "HEAD", "--not", "HEAD~1", "--format=%H"].as_slice(),
        ["log", "--children", "--format=%H", "HEAD"].as_slice(),
        ["log", "--objects", "HEAD"].as_slice(),
        ["log", "--filter=blob:none", "--objects", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    for args in [
        ["log", "--filter=blob:none", "HEAD"].as_slice(),
        [
            "log",
            "--filter=blob:none",
            "--filter-provided-objects",
            "HEAD",
        ]
        .as_slice(),
        [
            "log",
            "--filter=blob:none",
            "--filter-provided-objects",
            "--objects",
            "HEAD",
        ]
        .as_slice(),
        ["log", "--no-object-names", "HEAD"].as_slice(),
        ["log", "--objects", "--no-object-names", "HEAD"].as_slice(),
        ["log", "--object-names", "HEAD"].as_slice(),
        ["log", "--timestamp", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
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
fn log_and_rev_list_left_right_cherry_boundary_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);

    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    git(repo.path(), ["checkout", "-b", "left"]);
    write_file(repo.path(), "same.txt", "same\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "leftsame"]);

    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "same.txt", "same\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "rightsame"]);
    write_file(repo.path(), "right.txt", "right\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "rightonly"]);

    for args in [
        ["rev-list", "--cherry", "left...main"].as_slice(),
        ["rev-list", "--left-right", "left...main"].as_slice(),
        ["rev-list", "--left-right", "--cherry-pick", "left...main"].as_slice(),
        ["rev-list", "--left-right", "--cherry-mark", "left...main"].as_slice(),
        ["rev-list", "--left-right", "--boundary", "left...main"].as_slice(),
        ["log", "--left-right", "--oneline", "left...main"].as_slice(),
        [
            "log",
            "--left-right",
            "--cherry-pick",
            "--oneline",
            "left...main",
        ]
        .as_slice(),
        [
            "log",
            "--left-right",
            "--cherry-mark",
            "--oneline",
            "left...main",
        ]
        .as_slice(),
        [
            "log",
            "--left-right",
            "--boundary",
            "--oneline",
            "left...main",
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
fn log_and_rev_list_traversal_order_family_matches_stock_git() {
    fn build_repo(repo: &std::path::Path) {
        configure_identity(repo);
        git(repo, ["checkout", "-b", "main"]);

        write_file(repo, "base.txt", "base\n");
        git(repo, ["add", "-A"]);
        git_commit_with_split_dates(
            repo,
            "Base",
            "base@example.test",
            "2024-01-01T00:00:00+0000",
            "Base",
            "base@example.test",
            "2024-01-01T00:00:00+0000",
            "base",
        );

        git(repo, ["checkout", "-b", "side"]);
        write_file(repo, "side.txt", "side\n");
        git(repo, ["add", "-A"]);
        git_commit_with_split_dates(
            repo,
            "Side",
            "side@example.test",
            "2024-01-02T00:00:00+0000",
            "Side",
            "side@example.test",
            "2024-01-04T00:00:00+0000",
            "side",
        );

        git(repo, ["checkout", "main"]);
        write_file(repo, "main.txt", "main\n");
        git(repo, ["add", "-A"]);
        git_commit_with_split_dates(
            repo,
            "Main",
            "main@example.test",
            "2024-01-05T00:00:00+0000",
            "Main",
            "main@example.test",
            "2024-01-03T00:00:00+0000",
            "main",
        );

        let output = Command::new(stock_git_bin())
            .args(["merge", "--no-ff", "side", "-m", "merge"])
            .env("GIT_AUTHOR_NAME", "Merge")
            .env("GIT_AUTHOR_EMAIL", "merge@example.test")
            .env("GIT_AUTHOR_DATE", "2024-01-06T00:00:00+0000")
            .env("GIT_COMMITTER_NAME", "Merge")
            .env("GIT_COMMITTER_EMAIL", "merge@example.test")
            .env("GIT_COMMITTER_DATE", "2024-01-02T12:00:00+0000")
            .current_dir(repo)
            .output()
            .expect("git merge");
        assert!(
            output.status.success(),
            "git merge failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let git_repo = git_init();
    let zmin_repo = git_init();
    build_repo(git_repo.path());
    build_repo(zmin_repo.path());

    for args in [
        ["rev-list", "--topo-order", "HEAD"].as_slice(),
        ["rev-list", "--date-order", "HEAD"].as_slice(),
        ["rev-list", "--author-date-order", "HEAD"].as_slice(),
        ["log", "--topo-order", "--format=%s", "HEAD"].as_slice(),
        ["log", "--date-order", "--format=%s", "HEAD"].as_slice(),
        ["log", "--author-date-order", "--format=%s", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn log_and_rev_list_history_simplification_acceptance_family_matches_stock_git() {
    fn build_repo(repo: &std::path::Path) {
        configure_identity(repo);
        git(repo, ["checkout", "-b", "main"]);

        write_file(repo, "base.txt", "base\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "base"]);

        git(repo, ["checkout", "-b", "side"]);
        write_file(repo, "side.txt", "side1\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "side1"]);
        write_file(repo, "side.txt", "side1\nside2\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "side2"]);

        git(repo, ["checkout", "main"]);
        write_file(repo, "main.txt", "main1\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "main1"]);
        write_file(repo, "main.txt", "main1\nmain2\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "main2"]);

        let output = Command::new(stock_git_bin())
            .args(["merge", "--no-ff", "side", "-m", "merge"])
            .env("GIT_AUTHOR_NAME", "Merge")
            .env("GIT_AUTHOR_EMAIL", "merge@example.test")
            .env("GIT_AUTHOR_DATE", "2024-01-06T00:00:00+0000")
            .env("GIT_COMMITTER_NAME", "Merge")
            .env("GIT_COMMITTER_EMAIL", "merge@example.test")
            .env("GIT_COMMITTER_DATE", "2024-01-06T00:00:00+0000")
            .current_dir(repo)
            .output()
            .expect("git merge");
        assert!(
            output.status.success(),
            "git merge failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let git_repo = git_init();
    let zmin_repo = git_init();
    build_repo(git_repo.path());
    build_repo(zmin_repo.path());

    for args in [
        ["rev-list", "--full-history", "HEAD"].as_slice(),
        ["rev-list", "--dense", "HEAD"].as_slice(),
        ["rev-list", "--sparse", "HEAD"].as_slice(),
        ["rev-list", "--show-pulls", "HEAD"].as_slice(),
        ["rev-list", "--ancestry-path", "HEAD~2..HEAD"].as_slice(),
        ["rev-list", "--ancestry-path", "side~1..HEAD"].as_slice(),
        ["log", "--full-history", "--format=%s", "HEAD"].as_slice(),
        ["log", "--dense", "--format=%s", "HEAD"].as_slice(),
        ["log", "--sparse", "--format=%s", "HEAD"].as_slice(),
        ["log", "--show-pulls", "--format=%s", "HEAD"].as_slice(),
        ["log", "--ancestry-path", "--format=%s", "HEAD~2..HEAD"].as_slice(),
        ["log", "--ancestry-path", "--format=%s", "side~1..HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn log_and_rev_list_path_history_simplification_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_path_history_fixture(sha256);
        let zmin_repo = pinned_path_history_fixture(sha256);
        let cases = [
            ["log", "--format=%s", "HEAD", "--", "path.txt"].as_slice(),
            [
                "log",
                "--full-history",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "log",
                "--full-history",
                "--dense",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            ["log", "--dense", "--format=%s", "HEAD", "--", "path.txt"].as_slice(),
            [
                "log",
                "--full-history",
                "--sparse",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            ["log", "--sparse", "--format=%s", "HEAD", "--", "path.txt"].as_slice(),
            [
                "log",
                "--full-history",
                "--parents",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            ["rev-list", "--format=%s", "HEAD", "--", "path.txt"].as_slice(),
            [
                "rev-list",
                "--full-history",
                "--format=%s",
                "--parents",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--full-history",
                "--sparse",
                "--parents",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--full-history",
                "--dense",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            ["rev-list", "--dense", "HEAD", "--", "path.txt"].as_slice(),
            ["rev-list", "--sparse", "HEAD", "--", "path.txt"].as_slice(),
        ];
        for args in cases {
            assert_eq!(
                raw_zmin_output(zmin_repo.path(), args),
                raw_pinned_output(stock_repo.path(), args),
                "path-history tuple mismatch for sha256={sha256}, args={args:?}"
            );
        }
    }
}

fn pinned_commit_id_for_subject(repo: &std::path::Path, subject: &str) -> String {
    let output = raw_pinned_output(repo, &["log", "--all", "--format=%H:%s"]);
    assert_eq!(output.status, 0, "pinned subject lookup failed: {subject}");
    String::from_utf8(output.stdout)
        .expect("pinned subject lookup UTF-8")
        .lines()
        .find_map(|line| {
            let (id, found_subject) = line.split_once(':')?;
            (found_subject == subject).then(|| id.to_owned())
        })
        .unwrap_or_else(|| panic!("pinned fixture has no subject {subject:?}"))
}

fn assert_pinned_child_edges(repo: &std::path::Path, output: &RawCommandOutput) {
    let parent_output = raw_pinned_output(repo, &["rev-list", "--all", "--parents"]);
    assert_eq!(parent_output.status, 0, "pinned parent lookup failed");
    let mut parents_by_id = BTreeMap::<String, Vec<String>>::new();
    for line in String::from_utf8(parent_output.stdout)
        .expect("pinned parent output UTF-8")
        .lines()
    {
        let ids = line.split_whitespace().collect::<Vec<_>>();
        if let Some((id, parents)) = ids.split_first() {
            parents_by_id.insert(
                (*id).to_owned(),
                parents.iter().map(|parent| (*parent).to_owned()).collect(),
            );
        }
    }
    for line in String::from_utf8(output.stdout.clone())
        .expect("child output UTF-8")
        .lines()
    {
        let ids = line.split_whitespace().collect::<Vec<_>>();
        let Some((parent, children)) = ids.split_first() else {
            continue;
        };
        for child in children {
            let child_parents = parents_by_id
                .get(*child)
                .unwrap_or_else(|| panic!("child decoration names unknown commit {child}"));
            assert!(
                child_parents.iter().any(|candidate| candidate == parent),
                "child decoration {parent} -> {child} is not a raw parent edge"
            );
        }
    }
}

fn assert_bisect_all_rows(
    repo: &std::path::Path,
    output: &RawCommandOutput,
    require_child_edge: bool,
) {
    let parent_output = raw_pinned_output(repo, &["rev-list", "--all", "--parents"]);
    assert_eq!(parent_output.status, 0, "pinned parent lookup failed");
    let mut parents_by_id = BTreeMap::<String, Vec<String>>::new();
    for line in String::from_utf8(parent_output.stdout)
        .expect("pinned parent output UTF-8")
        .lines()
    {
        let mut ids = line.split_whitespace();
        let Some(id) = ids.next() else {
            continue;
        };
        parents_by_id.insert(id.to_owned(), ids.map(str::to_owned).collect());
    }
    let mut seen_ids = BTreeSet::new();
    let mut previous: Option<(usize, Vec<u8>)> = None;
    let mut row_count = 0usize;
    let mut child_edge_count = 0usize;
    for line in output.stdout.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let dist_offset = line
            .windows(b"dist=".len())
            .position(|window| window == b"dist=")
            .unwrap_or_else(|| panic!("bisect-all row has no numeric distance: {line:?}"));
        let prefix = &line[..dist_offset];
        let distance = std::str::from_utf8(&line[dist_offset + b"dist=".len()..])
            .expect("bisect distance UTF-8")
            .trim_end_matches(')')
            .parse::<usize>()
            .expect("bisect distance numeric");
        let tokens = prefix.split(|byte| *byte == b' ').collect::<Vec<_>>();
        let Some(id) = tokens.first().copied().filter(|id| !id.is_empty()) else {
            panic!("bisect-all row has no object id: {line:?}");
        };
        assert!(id.len() == 40 || id.len() == 64, "invalid bisect object id");
        assert!(id.iter().all(u8::is_ascii_hexdigit), "non-hex bisect id");
        assert!(seen_ids.insert(id), "duplicate bisect-all object id");
        if let Some((previous_distance, previous_id)) = previous.as_ref() {
            assert!(
                distance < *previous_distance
                    || (distance == *previous_distance && id >= previous_id.as_slice()),
                "bisect-all rows are not sorted by distance/raw id"
            );
        }
        previous = Some((distance, id.to_vec()));
        for child in tokens.iter().skip(1).copied() {
            if child.len() != id.len() || !child.iter().all(u8::is_ascii_hexdigit) {
                continue;
            }
            let child_text = std::str::from_utf8(child).expect("child id UTF-8");
            let parent_text = std::str::from_utf8(id).expect("parent id UTF-8");
            assert!(
                parents_by_id
                    .get(child_text)
                    .is_some_and(|parents| parents.iter().any(|parent| parent == parent_text)),
                "bisect child decoration {parent_text} -> {child_text} is not a raw parent edge"
            );
            child_edge_count += 1;
        }
        row_count += 1;
    }
    assert!(
        row_count > 1,
        "bisect-all must expose multiple numeric rows"
    );
    if require_child_edge {
        assert!(
            child_edge_count > 0,
            "bisect child fixture emitted no child edge"
        );
    }
}

fn assert_bisect_vars_numeric(output: &RawCommandOutput) {
    assert_eq!(output.status, 0);
    let mut numeric_fields = 0usize;
    let mut revision_id = false;
    for line in output.stdout.split(|byte| *byte == b'\n') {
        let mut fields = line.splitn(2, |byte| *byte == b'=');
        let Some(key) = fields.next() else {
            continue;
        };
        let Some(value) = fields.next() else {
            continue;
        };
        if key == b"bisect_rev" {
            let value = value
                .strip_prefix(b"'")
                .and_then(|value| value.strip_suffix(b"'"))
                .unwrap_or(value);
            assert!(value.len() == 40 || value.len() == 64);
            assert!(value.iter().all(u8::is_ascii_hexdigit));
            revision_id = true;
        } else if key.starts_with(b"bisect_") {
            value
                .iter()
                .copied()
                .all(|byte| byte.is_ascii_digit())
                .then_some(())
                .unwrap_or_else(|| panic!("non-numeric bisect variable: {line:?}"));
            numeric_fields += 1;
        }
    }
    assert!(revision_id, "bisect vars omitted bisect_rev");
    assert_eq!(numeric_fields, 5, "bisect vars omitted numeric fields");
}

fn bisect_all_row_ids(output: &RawCommandOutput) -> BTreeSet<String> {
    output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let id = line.split(|byte| *byte == b' ').next()?;
            Some(String::from_utf8(id.to_vec()).expect("bisect id UTF-8"))
        })
        .collect()
}

fn bisect_vars_revision(output: &RawCommandOutput) -> String {
    output
        .stdout
        .split(|byte| *byte == b'\n')
        .find_map(|line| {
            let value = line.strip_prefix(b"bisect_rev='")?.strip_suffix(b"'")?;
            Some(String::from_utf8(value.to_vec()).expect("bisect revision UTF-8"))
        })
        .expect("bisect vars omitted bisect_rev")
}

fn assert_pinned_object_child_edges(
    repo: &std::path::Path,
    output: &RawCommandOutput,
    require_outside_page_child: bool,
) {
    let parent_output = raw_pinned_output(repo, &["rev-list", "--all", "--parents"]);
    assert_eq!(parent_output.status, 0, "pinned parent lookup failed");
    let mut parents_by_id = BTreeMap::<String, Vec<String>>::new();
    for line in String::from_utf8(parent_output.stdout)
        .expect("pinned parent output UTF-8")
        .lines()
    {
        let ids = line.split_whitespace().collect::<Vec<_>>();
        if let Some((id, parents)) = ids.split_first() {
            parents_by_id.insert(
                (*id).to_owned(),
                parents.iter().map(|parent| (*parent).to_owned()).collect(),
            );
        }
    }
    let mut commit_rows = 0usize;
    let mut child_edges = 0usize;
    let displayed_commits = String::from_utf8(output.stdout.clone())
        .expect("object child output UTF-8")
        .lines()
        .filter_map(|line| {
            let id = line.split_whitespace().next()?;
            (pinned_object_kind(repo, id) == "commit").then(|| id.to_owned())
        })
        .collect::<std::collections::HashSet<_>>();
    let mut outside_page_child = false;
    for line in String::from_utf8(output.stdout.clone())
        .expect("object child output UTF-8")
        .lines()
    {
        let ids = line.split_whitespace().collect::<Vec<_>>();
        let Some((id, children)) = ids.split_first() else {
            continue;
        };
        if pinned_object_kind(repo, id) != "commit" {
            continue;
        }
        commit_rows += 1;
        for child in children {
            let child_parents = parents_by_id
                .get(*child)
                .unwrap_or_else(|| panic!("object child decoration names unknown commit {child}"));
            assert!(
                child_parents.iter().any(|candidate| candidate == id),
                "object child decoration {id} -> {child} is not a raw parent edge"
            );
            child_edges += 1;
            outside_page_child |= !displayed_commits.contains(*child);
        }
    }
    assert!(commit_rows > 0, "objects+children emitted no commit rows");
    assert!(child_edges > 0, "objects+children emitted no child edges");
    if require_outside_page_child {
        assert!(
            outside_page_child,
            "objects+children lost child outside page"
        );
    }
}

fn assert_pinned_root_child_order(
    stock_output: &RawCommandOutput,
    zmin_output: &RawCommandOutput,
    root_id: &str,
) {
    fn child_tokens(output: &RawCommandOutput, root_id: &str) -> Vec<String> {
        String::from_utf8(output.stdout.clone())
            .expect("child output UTF-8")
            .lines()
            .find_map(|line| {
                let mut fields = line.split_whitespace();
                (fields.next()? == root_id).then(|| fields.map(str::to_owned).collect())
            })
            .unwrap_or_else(|| panic!("child output has no root row {root_id}"))
    }

    let stock_children = child_tokens(stock_output, root_id);
    assert!(
        stock_children.len() >= 2,
        "pinned root row did not expose the combined child-order case"
    );
    assert_eq!(
        child_tokens(zmin_output, root_id),
        stock_children,
        "root child insertion order differs from pinned prepared walk"
    );
}

#[test]
fn path_rev_list_objects_and_shortlog_match_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_path_history_fixture(sha256);
        let zmin_repo = pinned_path_history_fixture(sha256);
        let cases = [
            ["rev-list", "--objects", "HEAD", "--", "path.txt"].as_slice(),
            [
                "rev-list",
                "--objects",
                "--full-history",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--full-history",
                "--dense",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--sparse",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--full-history",
                "--sparse",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--parents",
                "--count",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--children",
                "--count",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "--full-history",
                "--sparse",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--full-history",
                "--parents",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--full-history",
                "--sparse",
                "--parents",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            ["rev-list", "--objects", "--count", "HEAD", "--", "path.txt"].as_slice(),
            ["rev-list", "--count", "HEAD", "--", "path.txt"].as_slice(),
            ["rev-list", "--children", "HEAD", "--", "path.txt"].as_slice(),
            [
                "rev-list",
                "--children",
                "--full-history",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--dense",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--sparse",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--topo-order",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--date-order",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--reverse",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--full-history",
                "--dense",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--full-history",
                "--sparse",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--full-history",
                "--count",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--dense",
                "--count",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--sparse",
                "--count",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            ["rev-list", "--parents", "--count", "HEAD", "--", "path.txt"].as_slice(),
            [
                "rev-list",
                "--children",
                "--count",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--filter=blob:none",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--missing=print",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--missing=allow-promisor",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--exclude-promisor-objects",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--skip=1",
                "--max-count=2",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--max-count=2",
                "--skip=1",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            ["rev-list", "--objects-edge", "HEAD", "--", "path.txt"].as_slice(),
            ["shortlog", "--group=committer", "HEAD", "--", "path.txt"].as_slice(),
            ["shortlog", "HEAD", "--", "path.txt"].as_slice(),
            ["shortlog", "-s", "HEAD", "--", "path.txt"].as_slice(),
            ["shortlog", "--full-history", "HEAD", "--", "path.txt"].as_slice(),
            [
                "shortlog",
                "--full-history",
                "--sparse",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            ["shortlog", "--first-parent", "HEAD", "--", "path.txt"].as_slice(),
            ["shortlog", "--reverse", "HEAD", "--", "path.txt"].as_slice(),
        ];
        for args in cases {
            let zmin = raw_zmin_output(zmin_repo.path(), args);
            let stock = raw_pinned_output(stock_repo.path(), args);
            assert_eq!(
                zmin, stock,
                "path object/shortlog tuple mismatch for sha256={sha256}, args={args:?}"
            );
        }
        for (args, expected_counts) in [
            (
                ["rev-list", "--objects", "HEAD", "--", "path.txt"].as_slice(),
                [4, 4, 3],
            ),
            (
                [
                    "rev-list",
                    "--objects",
                    "--full-history",
                    "HEAD",
                    "--",
                    "path.txt",
                ]
                .as_slice(),
                [5, 5, 3],
            ),
            (
                [
                    "rev-list",
                    "--objects",
                    "--sparse",
                    "HEAD",
                    "--",
                    "path.txt",
                ]
                .as_slice(),
                [8, 8, 3],
            ),
            (
                [
                    "rev-list",
                    "--objects",
                    "--full-history",
                    "--sparse",
                    "HEAD",
                    "--",
                    "path.txt",
                ]
                .as_slice(),
                [9, 9, 3],
            ),
        ] {
            let output = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(output.status, 0, "path object command failed: {args:?}");
            assert!(
                !output.stdout.is_empty(),
                "path object output was empty: {args:?}"
            );
            let text = String::from_utf8(output.stdout.clone()).expect("path object output UTF-8");
            assert!(
                text.contains("path.txt"),
                "path object output lost path.txt: {args:?}"
            );
            assert!(
                !text.contains("noise"),
                "path object output leaked noise: {args:?}"
            );
            assert_eq!(
                pinned_object_kind_counts(zmin_repo.path(), &output.stdout),
                expected_counts,
                "path object kind counts changed: {args:?}"
            );
        }
        for args in [
            [
                "rev-list",
                "--objects",
                "--parents",
                "--count",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--children",
                "--count",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ] {
            let output = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(
                output.status, 0,
                "path object count command failed: {args:?}"
            );
            let count = String::from_utf8(output.stdout)
                .expect("path object count output UTF-8")
                .trim()
                .parse::<usize>()
                .expect("path object count is numeric");
            assert!(count > 0, "path object count was zero: {args:?}");
        }
        for args in [
            ["rev-list", "--count", "HEAD", "--", "path.txt"].as_slice(),
            ["rev-list", "--parents", "--count", "HEAD", "--", "path.txt"].as_slice(),
            [
                "rev-list",
                "--children",
                "--count",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ] {
            let output = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(
                output.status, 0,
                "path commit count command failed: {args:?}"
            );
            let count = String::from_utf8(output.stdout)
                .expect("path commit count output UTF-8")
                .trim()
                .parse::<usize>()
                .expect("path commit count is numeric");
            assert!(count > 0, "path commit count was zero: {args:?}");
        }
        for args in [
            [
                "rev-list",
                "--children",
                "--full-history",
                "--count",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--dense",
                "--count",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--sparse",
                "--count",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ] {
            let output = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(
                output,
                raw_pinned_output(stock_repo.path(), args),
                "path child count tuple mismatch for sha256={sha256}, args={args:?}"
            );
            assert_eq!(
                output.status, 0,
                "path child count command failed: {args:?}"
            );
            let count = String::from_utf8(output.stdout)
                .expect("path child count output UTF-8")
                .trim()
                .parse::<usize>()
                .expect("path child count is numeric");
            assert!(count > 0, "path child count was zero: {args:?}");
        }
        for args in [
            ["rev-list", "--children", "HEAD", "--", "path.txt"].as_slice(),
            [
                "rev-list",
                "--children",
                "--full-history",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--dense",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--sparse",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ] {
            let output = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(
                output,
                raw_pinned_output(stock_repo.path(), args),
                "path child tuple mismatch for sha256={sha256}, args={args:?}"
            );
            assert_eq!(output.status, 0, "path child command failed: {args:?}");
            assert!(
                !output.stdout.is_empty(),
                "path child output was empty: {args:?}"
            );
            assert!(
                output
                    .stdout
                    .split(|byte| *byte == b'\n')
                    .filter(|line| !line.is_empty())
                    .any(|line| line.split(|byte| *byte == b' ').count() > 1),
                "path child output had no child column: {args:?}"
            );
        }
        let m1_id = pinned_commit_id_for_subject(stock_repo.path(), "M1");
        for args in [
            ["rev-list", "--children", "HEAD", "--", "path.txt"].as_slice(),
            [
                "rev-list",
                "--children",
                "--sparse",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--full-history",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--full-history",
                "--sparse",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--dense",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--topo-order",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--date-order",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--reverse",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ] {
            let output = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(
                output,
                raw_pinned_output(stock_repo.path(), args),
                "path child matrix tuple mismatch for sha256={sha256}, args={args:?}"
            );
            assert_eq!(output.status, 0, "path child matrix failed: {args:?}");
            assert!(
                !output.stdout.is_empty(),
                "path child matrix was empty: {args:?}"
            );
            assert_pinned_child_edges(stock_repo.path(), &output);
            if args.contains(&"--full-history") {
                assert!(
                    output
                        .stdout
                        .windows(m1_id.len())
                        .any(|window| window == m1_id.as_bytes()),
                    "full-history child universe lost M1: {args:?}"
                );
            } else {
                assert!(
                    !output
                        .stdout
                        .windows(m1_id.len())
                        .any(|window| window == m1_id.as_bytes()),
                    "simplified child universe leaked M1: {args:?}"
                );
            }
        }
        let root_id = pinned_commit_id_for_subject(stock_repo.path(), "R");
        for args in [
            [
                "rev-list",
                "--children",
                "--full-history",
                "--sparse",
                "--reverse",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--full-history",
                "--sparse",
                "--topo-order",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--full-history",
                "--sparse",
                "--date-order",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--full-history",
                "--sparse",
                "--date-order",
                "--reverse",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ] {
            let stock_output = raw_pinned_output(stock_repo.path(), args);
            let zmin_output = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(
                zmin_output, stock_output,
                "combined child-order tuple mismatch for sha256={sha256}, args={args:?}"
            );
            assert_eq!(
                zmin_output.status, 0,
                "combined child-order command failed: {args:?}"
            );
            assert_pinned_child_edges(stock_repo.path(), &zmin_output);
            assert_pinned_root_child_order(&stock_output, &zmin_output, &root_id);
        }
        let no_path_args = ["rev-list", "--children", "HEAD"];
        let no_path_output = raw_zmin_output(zmin_repo.path(), &no_path_args);
        assert_eq!(
            no_path_output,
            raw_pinned_output(stock_repo.path(), &no_path_args),
            "no-path child tuple mismatch for sha256={sha256}"
        );
        assert!(
            no_path_output
                .stdout
                .windows(m1_id.len())
                .any(|window| window == m1_id.as_bytes()),
            "no-path child output lost M1 merge edge"
        );
        for args in [
            [
                "rev-list",
                "--children",
                "--full-history",
                "--sparse",
                "--skip=1",
                "--max-count=2",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--full-history",
                "--sparse",
                "--max-count=2",
                "--skip=1",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ] {
            let output = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(
                output,
                raw_pinned_output(stock_repo.path(), args),
                "paged child tuple mismatch for sha256={sha256}, args={args:?}"
            );
            assert_eq!(output.status, 0, "paged child command failed: {args:?}");
            let mut displayed = std::collections::HashSet::new();
            let mut decorated_children = Vec::new();
            for line in output
                .stdout
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
            {
                let mut fields = line.split(|byte| *byte == b' ');
                let Some(parent) = fields.next() else {
                    continue;
                };
                displayed.insert(parent.to_vec());
                decorated_children.extend(fields.map(|child| child.to_vec()));
            }
            assert!(
                decorated_children
                    .iter()
                    .any(|child| !displayed.contains(child)),
                "paged child output did not retain a child outside the displayed page: {args:?}"
            );
        }
        for args in [
            [
                "rev-list",
                "--parents",
                "--children",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--simplify-merges",
                "--children",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ] {
            let output = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(
                output,
                raw_pinned_output(stock_repo.path(), args),
                "children diagnostic mismatch for sha256={sha256}, args={args:?}"
            );
            assert_eq!(output.status, 128, "children diagnostic status: {args:?}");
            assert!(
                output.stdout.is_empty(),
                "children diagnostic emitted stdout"
            );
            assert!(!output.stderr.is_empty(), "children diagnostic lost stderr");
        }
        let filtered = raw_zmin_output(
            zmin_repo.path(),
            &[
                "rev-list",
                "--objects",
                "--filter=blob:none",
                "HEAD",
                "--",
                "path.txt",
            ],
        );
        assert_eq!(filtered.status, 0);
        assert_eq!(
            pinned_object_kind_counts(zmin_repo.path(), &filtered.stdout)[2],
            0
        );
        for args in [
            [
                "rev-list",
                "--objects",
                "--skip=1",
                "--max-count=2",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--max-count=2",
                "--skip=1",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ] {
            let output = raw_zmin_output(zmin_repo.path(), args);
            assert!(!output.stdout.is_empty(), "tail output was empty: {args:?}");
        }
        let shortlog_group = raw_zmin_output(
            zmin_repo.path(),
            &["shortlog", "--group=committer", "HEAD", "--", "path.txt"],
        );
        assert_eq!(shortlog_group.status, 0);
        assert!(
            shortlog_group
                .stdout
                .windows(b"Bench".len())
                .any(|window| window == b"Bench")
        );
        let no_names = raw_zmin_output(
            zmin_repo.path(),
            &[
                "rev-list",
                "--objects",
                "--no-object-names",
                "--full-history",
                "--sparse",
                "HEAD",
                "--",
                "path.txt",
            ],
        );
        assert_eq!(no_names.status, 0);
        assert!(!no_names.stdout.is_empty());
        assert!(no_names.stdout.split(|byte| *byte == b'\n').all(|line| {
            line.is_empty()
                || (line.len() == if sha256 { 64 } else { 40 }
                    && line.iter().all(u8::is_ascii_hexdigit))
        }));
    }
}

#[test]
fn pathless_rev_list_children_match_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_path_history_fixture(sha256);
        let zmin_repo = pinned_path_history_fixture(sha256);
        let matrix = [
            ["rev-list", "--children", "HEAD"].as_slice(),
            ["rev-list", "--children", "--topo-order", "HEAD"].as_slice(),
            ["rev-list", "--children", "--date-order", "HEAD"].as_slice(),
            ["rev-list", "--children", "--reverse", "HEAD"].as_slice(),
            [
                "rev-list",
                "--children",
                "--topo-order",
                "--reverse",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--date-order",
                "--reverse",
                "HEAD",
            ]
            .as_slice(),
            ["rev-list", "--children", "--first-parent", "--all"].as_slice(),
            ["rev-list", "--children", "--first-parent", "HEAD"].as_slice(),
            [
                "rev-list",
                "--children",
                "--first-parent",
                "--reverse",
                "HEAD",
            ]
            .as_slice(),
        ];
        for args in matrix {
            let stock = raw_pinned_output(stock_repo.path(), args);
            let zmin = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(
                zmin, stock,
                "pathless child tuple mismatch for sha256={sha256}, args={args:?}"
            );
            assert_eq!(zmin.status, 0, "pathless child command failed: {args:?}");
            assert!(
                !zmin.stdout.is_empty(),
                "pathless child output was empty: {args:?}"
            );
            assert_pinned_child_edges(stock_repo.path(), &zmin);
        }

        for args in [
            [
                "rev-list",
                "--children",
                "--skip=1",
                "--max-count=2",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--max-count=2",
                "--skip=1",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--reverse",
                "--skip=1",
                "--max-count=2",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--reverse",
                "--max-count=2",
                "--skip=1",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--first-parent",
                "--skip=1",
                "--max-count=2",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--first-parent",
                "--reverse",
                "--skip=1",
                "--max-count=2",
                "HEAD",
            ]
            .as_slice(),
        ] {
            let stock = raw_pinned_output(stock_repo.path(), args);
            let zmin = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(
                zmin, stock,
                "pathless paged child tuple mismatch for sha256={sha256}, args={args:?}"
            );
            assert_eq!(
                zmin.status, 0,
                "pathless paged child command failed: {args:?}"
            );
            let lines = zmin
                .stdout
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            assert_eq!(lines.len(), 2, "pathless page size mismatch: {args:?}");
            let displayed = lines
                .iter()
                .filter_map(|line| line.split(|byte| *byte == b' ').next())
                .collect::<std::collections::HashSet<_>>();
            let child_count = lines
                .iter()
                .flat_map(|line| line.split(|byte| *byte == b' ').skip(1))
                .count();
            assert!(
                child_count > 0,
                "pathless page lost all child columns: {args:?}"
            );
            assert!(
                lines
                    .iter()
                    .flat_map(|line| line.split(|byte| *byte == b' ').skip(1))
                    .any(|child| !displayed.contains(child)),
                "pathless page lost child outside displayed page: {args:?}"
            );
            assert_pinned_child_edges(stock_repo.path(), &zmin);
        }
    }
}

#[test]
fn pathless_children_author_date_split_matches_pinned_git() {
    for sha256 in [false, true] {
        let stock_repo = pinned_author_date_children_fixture(sha256);
        let zmin_repo = pinned_author_date_children_fixture(sha256);
        for args in [
            ["rev-list", "--children", "--author-date-order", "--all"].as_slice(),
            [
                "rev-list",
                "--children",
                "--author-date-order",
                "--grep=side",
                "--all",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--author-date-order",
                "--reverse",
                "--all",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--author-date-order",
                "--skip=1",
                "--max-count=1",
                "--all",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--author-date-order",
                "--reverse",
                "--skip=1",
                "--max-count=1",
                "--all",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--author-date-order",
                "--reverse",
                "--max-count=1",
                "--skip=1",
                "--all",
            ]
            .as_slice(),
        ] {
            let stock = raw_pinned_output(stock_repo.path(), args);
            let zmin = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(zmin, stock, "author-date child mismatch: {args:?}");
            assert_eq!(zmin.status, 0, "author-date child failed: {args:?}");
            assert!(!zmin.stdout.is_empty(), "author-date child empty: {args:?}");
            assert_pinned_child_edges(stock_repo.path(), &zmin);
        }
    }
}

#[test]
fn pathless_children_grep_keeps_full_child_universe() {
    for sha256 in [false, true] {
        let stock_repo = pinned_linear_children_grep_fixture(sha256);
        let zmin_repo = pinned_linear_children_grep_fixture(sha256);
        for args in [
            ["rev-list", "--children", "--grep=B", "HEAD"].as_slice(),
            [
                "rev-list",
                "--children",
                "--grep=B",
                "--invert-grep",
                "HEAD",
            ]
            .as_slice(),
        ] {
            let stock = raw_pinned_output(stock_repo.path(), args);
            let zmin = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(zmin, stock, "grep child tuple mismatch: {args:?}");
            assert_eq!(zmin.status, 0, "grep child command failed: {args:?}");
            let lines = zmin
                .stdout
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty());
            if args.iter().any(|arg| *arg == "--invert-grep") {
                assert_eq!(
                    lines.count(),
                    3,
                    "invert-grep display cardinality: {args:?}"
                );
            } else {
                assert_eq!(lines.count(), 1, "grep display cardinality: {args:?}");
            }
            assert_pinned_child_edges(stock_repo.path(), &zmin);
        }
    }
}

#[test]
fn pathless_children_boundary_uses_post_page_handles() {
    for sha256 in [false, true] {
        let stock_repo = pinned_path_history_fixture(sha256);
        let zmin_repo = pinned_path_history_fixture(sha256);
        for args in [
            [
                "rev-list",
                "--children",
                "--boundary",
                "--skip=1",
                "--max-count=2",
                "HEAD",
                "^HEAD~5",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--boundary",
                "--reverse",
                "--skip=1",
                "--max-count=2",
                "HEAD",
                "^HEAD~5",
            ]
            .as_slice(),
        ] {
            let stock = raw_pinned_output(stock_repo.path(), args);
            let zmin = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(zmin, stock, "boundary child tuple mismatch: {args:?}");
            assert_eq!(zmin.status, 0, "boundary child command failed: {args:?}");
            assert!(!zmin.stdout.is_empty(), "boundary child output empty");
            assert!(
                zmin.stdout
                    .split(|byte| *byte == b'\n')
                    .any(|line| line.first() == Some(&b'-')),
                "boundary child output lost boundary row: {args:?}"
            );
        }
    }
}

#[test]
fn pathless_children_object_and_no_walk_routes_match_pinned_git() {
    for sha256 in [false, true] {
        let stock_repo = pinned_path_history_fixture(sha256);
        let zmin_repo = pinned_path_history_fixture(sha256);
        for args in [
            ["rev-list", "--objects", "--children", "HEAD"].as_slice(),
            [
                "rev-list",
                "--objects",
                "--children",
                "--reverse",
                "--skip=1",
                "--max-count=2",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--children",
                "--max-count=2",
                "--skip=1",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--children",
                "--first-parent",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--children",
                "--first-parent",
                "--reverse",
                "--skip=1",
                "--max-count=2",
                "HEAD",
            ]
            .as_slice(),
        ] {
            let stock = raw_pinned_output(stock_repo.path(), args);
            let zmin = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(zmin, stock, "objects+children mismatch: {args:?}");
            assert_eq!(zmin.status, 0, "objects+children failed: {args:?}");
            assert!(!zmin.stdout.is_empty(), "objects+children empty: {args:?}");
            let counts = pinned_object_kind_counts(zmin_repo.path(), &zmin.stdout);
            assert!(counts[0] > 0 && counts[1] > 0 && counts[2] > 0);
            assert_pinned_object_child_edges(
                zmin_repo.path(),
                &zmin,
                args.iter().any(|arg| *arg == "--skip=1"),
            );
        }

        for args in [
            ["rev-list", "--children", "--no-walk", "HEAD"].as_slice(),
            [
                "rev-list",
                "--children",
                "--no-walk=unsorted",
                "HEAD",
                "HEAD~1",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--no-walk=unsorted",
                "--reverse",
                "HEAD",
                "HEAD~1",
            ]
            .as_slice(),
            ["rev-list", "--children", "--no-walk", "HEAD", "HEAD~1"].as_slice(),
            [
                "rev-list",
                "--children",
                "--no-walk=sorted",
                "--reverse",
                "HEAD",
                "HEAD~1",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--no-walk",
                "--max-count=2",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--max-count=2",
                "--no-walk",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--no-walk",
                "--skip=1",
                "--max-count=1",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--skip=1",
                "--no-walk",
                "--max-count=1",
                "HEAD",
            ]
            .as_slice(),
            ["rev-list", "--children", "--no-walk", "--skip=1", "HEAD"].as_slice(),
            ["rev-list", "--children", "--skip=1", "--no-walk", "HEAD"].as_slice(),
        ] {
            let stock = raw_pinned_output(stock_repo.path(), args);
            let zmin = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(zmin, stock, "no-walk children mismatch: {args:?}");
            assert_eq!(zmin.status, 0, "no-walk children failed: {args:?}");
            if !args
                .iter()
                .any(|arg| *arg == "--max-count=2" || *arg == "--skip=1")
            {
                for line in zmin.stdout.split(|byte| *byte == b'\n') {
                    if !line.is_empty() {
                        assert_eq!(line.split(|byte| *byte == b' ').count(), 1);
                    }
                }
            }
        }
    }
}

#[test]
fn pathless_children_render_surface_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_path_history_fixture(sha256);
        let zmin_repo = pinned_path_history_fixture(sha256);
        let matrix = [
            ["rev-list", "--children", "--format=%s", "HEAD"].as_slice(),
            ["rev-list", "--children", "--pretty=oneline", "HEAD"].as_slice(),
            ["rev-list", "--children", "--header", "HEAD"].as_slice(),
            ["rev-list", "--children", "--quiet", "HEAD"].as_slice(),
            ["rev-list", "--children", "--count", "HEAD"].as_slice(),
            ["rev-list", "--children", "--disk-usage", "HEAD"].as_slice(),
            ["rev-list", "--children", "--disk-usage=human", "HEAD"].as_slice(),
            ["rev-list", "--children", "--bisect", "HEAD", "side"].as_slice(),
            ["rev-list", "--children", "--bisect-all", "HEAD", "side"].as_slice(),
            ["rev-list", "--bisect-all", "HEAD", "side"].as_slice(),
            ["rev-list", "--children", "--bisect-vars", "HEAD", "side"].as_slice(),
            ["rev-list", "--bisect-vars", "HEAD", "side"].as_slice(),
            [
                "rev-list",
                "--objects",
                "--children",
                "--no-object-names",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--filter=blob:none",
                "--children",
                "--no-object-names",
                "HEAD",
            ]
            .as_slice(),
            ["rev-list", "--children", "--no-walk", "--all", "HEAD"].as_slice(),
            [
                "rev-list",
                "--children",
                "--no-walk",
                "--all",
                "--reverse",
                "HEAD",
            ]
            .as_slice(),
            ["rev-list", "--children", "--simplify-by-decoration", "HEAD"].as_slice(),
            ["rev-list", "--children", "--disk-usage=wat", "HEAD"].as_slice(),
        ];
        for args in matrix {
            let stock = raw_pinned_output(stock_repo.path(), args);
            let zmin = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(zmin, stock, "render-surface tuple mismatch: {args:?}");
            if args.iter().any(|arg| *arg == "--quiet") {
                assert!(zmin.stdout.is_empty(), "quiet emitted bytes: {args:?}");
            }
            if args.iter().any(|arg| *arg == "--count") {
                let count = zmin.stdout.strip_suffix(&[b'\n']).unwrap_or(&zmin.stdout);
                assert!(!count.is_empty() && count.iter().all(u8::is_ascii_digit));
            }
            if args.iter().any(|arg| *arg == "--disk-usage=wat") {
                assert_eq!(zmin.status, 128);
                assert!(zmin.stdout.is_empty());
                assert!(
                    zmin.stderr
                        .windows(b"invalid value for".len())
                        .any(|window| window == b"invalid value for")
                );
            }
            if args.iter().any(|arg| *arg == "--simplify-by-decoration") {
                assert_eq!(zmin.status, 128);
                assert!(zmin.stdout.is_empty());
                assert!(zmin.stderr.windows(9).any(|window| window == b"--parents"));
            }
        }
    }
}

#[test]
fn rev_list_disk_usage_modes_match_packed_and_loose_stock_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_disk_reachable_loose_fixture(sha256);
        let loose_id = loose_object_id_for_algorithm(
            if sha256 {
                GitHashAlgorithm::Sha256
            } else {
                GitHashAlgorithm::Sha1
            },
            "blob",
            b"loose object\n",
        );
        assert!(
            repo.path()
                .join(".git/objects")
                .join(&loose_id[..2])
                .join(&loose_id[2..])
                .is_file(),
            "fixture must retain a loose object"
        );
        assert!(
            fs::read_dir(repo.path().join(".git/objects/pack"))
                .expect("read fixture pack directory")
                .next()
                .is_some(),
            "fixture must contain packed objects"
        );
        for rev in ["HEAD", "HEAD^{tree}", "HEAD:reachable-loose.txt"] {
            let id = pinned_git_args(repo.path(), &["rev-parse", rev])
                .trim()
                .to_owned();
            assert!(
                repo.path()
                    .join(".git/objects")
                    .join(&id[..2])
                    .join(&id[2..])
                    .is_file(),
                "reachable {rev} must remain loose: {id}"
            );
        }

        for args in [
            ["rev-list", "--disk-usage", "HEAD"].as_slice(),
            ["rev-list", "--quiet", "--disk-usage", "HEAD"].as_slice(),
            ["rev-list", "--count", "--disk-usage", "HEAD"].as_slice(),
            ["rev-list", "--objects", "--disk-usage", "HEAD"].as_slice(),
            ["rev-list", "--objects", "--count", "--disk-usage", "HEAD"].as_slice(),
            [
                "rev-list",
                "--objects",
                "--disk-usage",
                "--filter=blob:none",
                "HEAD",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--disk-usage",
                "--filter=blob:limit=0",
                "HEAD",
            ]
            .as_slice(),
            ["rev-list", "--all", "--disk-usage"].as_slice(),
            ["rev-list", "--no-walk", "--disk-usage", "HEAD"].as_slice(),
            ["rev-list", "--no-walk", "--all", "--disk-usage"].as_slice(),
            ["rev-list", "--disk-usage=human", "HEAD"].as_slice(),
            ["rev-list", "--disk-usage=bytes", "HEAD"].as_slice(),
            ["rev-list", "--disk-usage=", "HEAD"].as_slice(),
            ["rev-list", "--disk-usage=false", "HEAD"].as_slice(),
            ["rev-list", "--disk-usage=true", "HEAD"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
            let output = raw_zmin_output(repo.path(), args);
            if output.status == 0 {
                assert!(!output.stdout.is_empty(), "disk output is empty: {args:?}");
                if args.iter().any(|arg| *arg == "--disk-usage")
                    && !args.iter().any(|arg| *arg == "--disk-usage=human")
                {
                    let decimal = String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .parse::<u64>();
                    assert!(decimal.is_ok(), "disk total is not decimal: {args:?}");
                }
                if args.iter().any(|arg| *arg == "--disk-usage=human") {
                    let human = String::from_utf8_lossy(&output.stdout);
                    assert!(
                        human.contains("byte")
                            || human.contains("KiB")
                            || human.contains("MiB")
                            || human.contains("GiB"),
                        "human disk total has no Git unit: {human:?}"
                    );
                }
                if args.iter().any(|arg| *arg == "--count") {
                    assert!(
                        output
                            .stdout
                            .split(|byte| *byte == b'\n')
                            .filter(|line| !line.is_empty())
                            .count()
                            >= 2,
                        "count+disk must print both totals: {args:?}"
                    );
                }
            } else {
                assert_eq!(output.status, 128, "invalid disk mode status: {args:?}");
                assert!(output.stdout.is_empty());
                assert!(
                    output
                        .stderr
                        .windows(b"invalid value for".len())
                        .any(|window| { window == b"invalid value for" })
                );
            }
        }
        let objects = raw_pinned_output(
            repo.path(),
            &["rev-list", "--objects", "--disk-usage", "HEAD"],
        );
        let filtered = raw_pinned_output(
            repo.path(),
            &[
                "rev-list",
                "--objects",
                "--disk-usage",
                "--filter=blob:none",
                "HEAD",
            ],
        );
        assert_eq!(objects.status, 0);
        assert_eq!(filtered.status, 0);
        assert_ne!(
            objects.stdout, filtered.stdout,
            "object filter must change disk total"
        );
    }
}

#[test]
fn rev_list_bisection_weights_match_asymmetric_and_treesame_stock_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let asymmetric_stock = pinned_bisection_asymmetric_fixture(sha256);
        let asymmetric_zmin = pinned_bisection_asymmetric_fixture(sha256);
        let b2 = pinned_git_args(asymmetric_stock.path(), &["rev-parse", "refs/heads/right"])
            .trim()
            .to_owned();
        for args in [
            [
                "rev-list",
                "--bisect",
                "refs/heads/left",
                "refs/heads/right",
                "^refs/heads/main",
            ]
            .as_slice(),
            [
                "rev-list",
                "--reverse",
                "--bisect",
                "refs/heads/left",
                "refs/heads/right",
                "^refs/heads/main",
            ]
            .as_slice(),
            [
                "rev-list",
                "--bisect-vars",
                "refs/heads/left",
                "refs/heads/right",
                "^refs/heads/main",
            ]
            .as_slice(),
            [
                "rev-list",
                "--bisect-all",
                "refs/heads/left",
                "refs/heads/right",
                "^refs/heads/main",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--bisect-all",
                "refs/heads/left",
                "refs/heads/right",
                "^refs/heads/main",
            ]
            .as_slice(),
        ] {
            let stock = raw_pinned_output(asymmetric_stock.path(), args);
            let zmin = raw_zmin_output(asymmetric_zmin.path(), args);
            assert_eq!(zmin, stock, "asymmetric bisection mismatch: {args:?}");
            assert_eq!(stock.status, 0);
            assert!(!stock.stdout.is_empty());
            if args.iter().any(|arg| *arg == "--bisect-all") {
                assert_bisect_all_rows(
                    asymmetric_stock.path(),
                    &stock,
                    args.iter().any(|arg| *arg == "--children"),
                );
            }
            if args.iter().any(|arg| *arg == "--bisect-vars") {
                assert_bisect_vars_numeric(&stock);
            }
            if args.iter().any(|arg| *arg == "--bisect")
                && !args.iter().any(|arg| *arg == "--bisect-all")
            {
                assert!(
                    stock.stdout.starts_with(b2.as_bytes()),
                    "pinned asymmetric bisection must choose B2: {:?}",
                    String::from_utf8_lossy(&stock.stdout)
                );
            }
        }

        let treesame_stock = pinned_treesame_bisection_fixture(sha256);
        let treesame_zmin = pinned_treesame_bisection_fixture(sha256);
        for args in [
            [
                "rev-list",
                "--bisect",
                "refs/heads/main",
                "refs/heads/side",
                "^refs/heads/main~3",
            ]
            .as_slice(),
            [
                "rev-list",
                "--bisect-all",
                "refs/heads/main",
                "refs/heads/side",
                "^refs/heads/main~3",
            ]
            .as_slice(),
            [
                "rev-list",
                "--bisect-vars",
                "refs/heads/main",
                "refs/heads/side",
                "^refs/heads/main~3",
            ]
            .as_slice(),
            [
                "rev-list",
                "--children",
                "--bisect-all",
                "refs/heads/main",
                "refs/heads/side",
                "^refs/heads/main~3",
            ]
            .as_slice(),
            [
                "rev-list",
                "--first-parent",
                "--bisect",
                "refs/heads/main",
                "refs/heads/side",
                "^refs/heads/main~3",
            ]
            .as_slice(),
            [
                "rev-list",
                "--first-parent",
                "--bisect-all",
                "refs/heads/main",
                "refs/heads/side",
                "^refs/heads/main~3",
            ]
            .as_slice(),
            [
                "rev-list",
                "--first-parent",
                "--bisect-vars",
                "refs/heads/main",
                "refs/heads/side",
                "^refs/heads/main~3",
            ]
            .as_slice(),
        ] {
            let stock = raw_pinned_output(treesame_stock.path(), args);
            let zmin = raw_zmin_output(treesame_zmin.path(), args);
            assert_eq!(zmin, stock, "TREESAME bisection mismatch: {args:?}");
            assert_eq!(stock.status, 0);
            assert!(!stock.stdout.is_empty());
            if args.iter().any(|arg| *arg == "--bisect-all") {
                assert_bisect_all_rows(
                    treesame_stock.path(),
                    &stock,
                    args.iter().any(|arg| *arg == "--children"),
                );
            }
            if args.iter().any(|arg| *arg == "--bisect-vars") {
                assert_bisect_vars_numeric(&stock);
            }
        }
    }
}

#[test]
fn rev_list_bisection_preserves_prepared_tie_order_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_four_way_equal_time_fixture(sha256);
        let zmin_repo = pinned_four_way_equal_time_fixture(sha256);
        let ids = ["a", "b", "c", "d"]
            .into_iter()
            .map(|branch| {
                let reference = format!("refs/heads/{branch}");
                (
                    branch,
                    pinned_git_args(stock_repo.path(), &["rev-parse", reference.as_str()])
                        .trim()
                        .to_owned(),
                )
            })
            .collect::<HashMap<_, _>>();
        for (args, expected_branch) in [
            (
                [
                    "rev-list",
                    "--bisect",
                    "refs/heads/a",
                    "refs/heads/b",
                    "refs/heads/c",
                    "refs/heads/d",
                    "^refs/heads/main",
                ]
                .as_slice(),
                "d",
            ),
            (
                [
                    "rev-list",
                    "--reverse",
                    "--bisect",
                    "refs/heads/a",
                    "refs/heads/b",
                    "refs/heads/c",
                    "refs/heads/d",
                    "^refs/heads/main",
                ]
                .as_slice(),
                "d",
            ),
            (
                [
                    "rev-list",
                    "--bisect",
                    "refs/heads/d",
                    "refs/heads/c",
                    "refs/heads/b",
                    "refs/heads/a",
                    "^refs/heads/main",
                ]
                .as_slice(),
                "a",
            ),
            (
                [
                    "rev-list",
                    "--bisect",
                    "refs/heads/b",
                    "refs/heads/d",
                    "refs/heads/a",
                    "refs/heads/c",
                    "^refs/heads/main",
                ]
                .as_slice(),
                "c",
            ),
        ] {
            let stock = raw_pinned_output(stock_repo.path(), args);
            let zmin = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(zmin, stock, "four-way bisection mismatch: {args:?}");
            assert_eq!(stock.status, 0);
            let first_line = stock
                .stdout
                .split(|byte| *byte == b'\n')
                .next()
                .unwrap_or_default();
            assert_eq!(first_line, ids[expected_branch].as_bytes());
        }
    }
}

#[test]
fn rev_list_path_bisection_uses_path_treesame_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_path_bisection_fixture(sha256);
        let zmin_repo = pinned_path_bisection_fixture(sha256);
        let side = pinned_git_args(stock_repo.path(), &["rev-parse", "refs/heads/side"])
            .trim()
            .to_owned();
        let merge = pinned_git_args(stock_repo.path(), &["rev-parse", "HEAD"])
            .trim()
            .to_owned();
        let root = pinned_git_args(stock_repo.path(), &["rev-parse", "HEAD^1^"])
            .trim()
            .to_owned();
        for args in [
            [
                "rev-list",
                "--full-history",
                "--bisect",
                "HEAD",
                "--",
                "target.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--full-history",
                "--bisect-all",
                "HEAD",
                "--",
                "target.txt",
            ]
            .as_slice(),
        ] {
            let stock = raw_pinned_output(stock_repo.path(), args);
            let zmin = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(zmin, stock, "path bisection mismatch: {args:?}");
            assert_eq!(stock.status, 0);
            assert!(!stock.stdout.is_empty());
        }
        let bisect = raw_pinned_output(
            stock_repo.path(),
            &[
                "rev-list",
                "--full-history",
                "--bisect",
                "HEAD",
                "--",
                "target.txt",
            ],
        );
        assert_eq!(
            bisect.stdout.split(|byte| *byte == b'\n').next(),
            Some(side.as_bytes())
        );
        let bisect_all = raw_pinned_output(
            stock_repo.path(),
            &[
                "rev-list",
                "--full-history",
                "--bisect-all",
                "HEAD",
                "--",
                "target.txt",
            ],
        );
        for id in [&side, &merge, &root] {
            assert!(
                bisect_all
                    .stdout
                    .windows(id.len())
                    .any(|window| window == id.as_bytes())
            );
        }
        assert!(
            bisect_all
                .stdout
                .windows(b"dist=0".len())
                .any(|window| window == b"dist=0")
        );
        assert!(
            bisect_all
                .stdout
                .windows(b"dist=1".len())
                .any(|window| window == b"dist=1")
        );

        let sparse_stock = pinned_sparse_path_bisection_fixture(sha256);
        let sparse_zmin = pinned_sparse_path_bisection_fixture(sha256);
        for args in [
            [
                "rev-list",
                "--sparse",
                "--bisect",
                "HEAD",
                "--",
                "target.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--full-history",
                "--sparse",
                "--bisect-all",
                "HEAD",
                "--",
                "target.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--full-history",
                "--sparse",
                "--bisect-vars",
                "HEAD",
                "--",
                "target.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--first-parent",
                "--sparse",
                "--bisect",
                "HEAD",
                "--",
                "target.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--first-parent",
                "--full-history",
                "--sparse",
                "--bisect-all",
                "HEAD",
                "--",
                "target.txt",
            ]
            .as_slice(),
        ] {
            let stock = raw_pinned_output(sparse_stock.path(), args);
            let zmin = raw_zmin_output(sparse_zmin.path(), args);
            assert_eq!(zmin, stock, "sparse path bisection mismatch: {args:?}");
            assert_eq!(stock.status, 0);
            assert!(!stock.stdout.is_empty());
            if args.iter().any(|arg| *arg == "--bisect-all") {
                assert_bisect_all_rows(
                    sparse_stock.path(),
                    &stock,
                    args.iter().any(|arg| *arg == "--children"),
                );
            }
            if args.iter().any(|arg| *arg == "--bisect-vars") {
                assert_bisect_vars_numeric(&stock);
            }
        }

        for args in [
            [
                "rev-list",
                "--full-history",
                "--bisect",
                "HEAD",
                "^HEAD^1",
                "--",
                "target.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--first-parent",
                "--full-history",
                "--bisect",
                "HEAD",
                "^HEAD^1",
                "--",
                "target.txt",
            ]
            .as_slice(),
        ] {
            let stock = raw_pinned_output(stock_repo.path(), args);
            let zmin = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(
                zmin, stock,
                "excluded-parent path bisection mismatch: {args:?}"
            );
        }
    }
}

#[test]
fn sparse_bisection_uses_irrelevant_external_parent_comparisons_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_sparse_external_parent_bisection_fixture(sha256);
        let zmin_repo = pinned_sparse_external_parent_bisection_fixture(sha256);
        let merge = pinned_git_args(stock_repo.path(), &["rev-parse", "refs/heads/main"])
            .trim()
            .to_owned();
        for args in [
            [
                "rev-list",
                "--sparse",
                "--bisect",
                "refs/heads/main",
                "^refs/heads/u",
                "^refs/heads/v",
                "--",
                "target.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--full-history",
                "--sparse",
                "--bisect-all",
                "refs/heads/main",
                "^refs/heads/u",
                "^refs/heads/v",
                "--",
                "target.txt",
            ]
            .as_slice(),
        ] {
            let stock = raw_pinned_output(stock_repo.path(), args);
            let zmin = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(zmin, stock, "external-parent bisection mismatch: {args:?}");
            assert_eq!(stock.status, 0);
            assert!(stock.stdout.starts_with(merge.as_bytes()));
            if args.iter().any(|arg| *arg == "--bisect-all") {
                let output = String::from_utf8(stock.stdout.clone())
                    .expect("external-parent bisect-all output UTF-8");
                let line = output
                    .lines()
                    .find(|line| !line.is_empty())
                    .expect("external-parent bisect-all row");
                let distance = line
                    .split_once("dist=")
                    .and_then(|(_, value)| value.strip_suffix(')'))
                    .expect("external-parent bisect-all dist= decoration")
                    .parse::<usize>()
                    .expect("external-parent bisect-all numeric distance");
                assert_eq!(distance, 0);
            }
        }
    }
}

#[test]
fn path_objects_reused_subtree_prefix_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_reused_subtree_path_fixture(sha256);
        let zmin_repo = pinned_reused_subtree_path_fixture(sha256);
        for args in [
            ["rev-list", "--objects", "HEAD", "--", "second/shared.txt"].as_slice(),
            [
                "rev-list",
                "--objects",
                "HEAD",
                "--",
                "first/shared.txt",
                "second/shared.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "HEAD",
                "--",
                "second/shared.txt",
            ]
            .as_slice(),
        ] {
            assert_eq!(
                raw_zmin_output(zmin_repo.path(), args),
                raw_pinned_output(stock_repo.path(), args),
                "reused subtree tuple mismatch for sha256={sha256}, args={args:?}"
            );
        }
        let output = raw_zmin_output(
            zmin_repo.path(),
            &[
                "rev-list",
                "--objects",
                "HEAD",
                "--",
                "first/shared.txt",
                "second/shared.txt",
            ],
        );
        assert_eq!(output.status, 0);
        let text = String::from_utf8(output.stdout).expect("reused subtree output UTF-8");
        let ids = text
            .lines()
            .filter_map(|line| line.split_ascii_whitespace().next())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids.len(), text.lines().count());
        for path in ["first/shared.txt", "second/shared.txt"] {
            let separate = raw_zmin_output(
                zmin_repo.path(),
                &["rev-list", "--objects", "HEAD", "--", path],
            );
            assert_eq!(separate.status, 0);
            assert!(
                !separate.stdout.is_empty(),
                "shared prefix was not traversed: {path}"
            );
        }
    }
}

#[test]
fn path_objects_edge_boundaries_match_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_path_object_edge_fixture(sha256);
        let zmin_repo = pinned_path_object_edge_fixture(sha256);
        let cases = [
            [
                "rev-list",
                "--objects-edge",
                "--parents",
                "HEAD",
                "^HEAD~1",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects-edge-aggressive",
                "--parents",
                "HEAD",
                "^HEAD~1",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects-edge",
                "HEAD",
                "^HEAD~1",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects-edge-aggressive",
                "HEAD",
                "^other",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects-edge-aggressive",
                "HEAD",
                "^HEAD~1",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ];
        for args in cases {
            let stock = raw_pinned_output(stock_repo.path(), args);
            let zmin = raw_zmin_output(zmin_repo.path(), args);
            assert_eq!(
                zmin, stock,
                "path edge mismatch for sha256={sha256}, args={args:?}"
            );
            assert_eq!(zmin.status, 0);
            assert!(
                zmin.stdout
                    .split(|byte| *byte == b'\n')
                    .any(|line| line.starts_with(b"-")),
                "path edge output lacked a boundary: sha256={sha256}, args={args:?}"
            );
        }
    }
}

#[test]
fn all_treesame_merge_path_history_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_all_treesame_merge_fixture(sha256);
        let zmin_repo = pinned_all_treesame_merge_fixture(sha256);
        let cases = [
            ["log", "--sparse", "--format=%s", "HEAD", "--", "path.txt"].as_slice(),
            [
                "log",
                "--full-history",
                "--parents",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "log",
                "--full-history",
                "--sparse",
                "--parents",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--sparse",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--full-history",
                "--parents",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--full-history",
                "--sparse",
                "--parents",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ];
        for args in cases {
            assert_eq!(
                raw_zmin_output(zmin_repo.path(), args),
                raw_pinned_output(stock_repo.path(), args),
                "all-TREESAME tuple mismatch for sha256={sha256}, args={args:?}"
            );
        }
    }
}

#[test]
fn path_bisection_default_and_full_history_have_distinct_treesame_candidates_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_all_treesame_merge_fixture(sha256);
        let zmin_repo = pinned_all_treesame_merge_fixture(sha256);
        let default_all_args = [
            "rev-list",
            "--sparse",
            "--bisect-all",
            "HEAD",
            "--",
            "path.txt",
        ];
        let full_all_args = [
            "rev-list",
            "--full-history",
            "--sparse",
            "--bisect-all",
            "HEAD",
            "--",
            "path.txt",
        ];
        let default_all = raw_pinned_output(stock_repo.path(), &default_all_args);
        let full_all = raw_pinned_output(stock_repo.path(), &full_all_args);
        assert_eq!(
            raw_zmin_output(zmin_repo.path(), &default_all_args),
            default_all
        );
        assert_eq!(raw_zmin_output(zmin_repo.path(), &full_all_args), full_all);
        let default_ids = bisect_all_row_ids(&default_all);
        let full_ids = bisect_all_row_ids(&full_all);
        assert!(!default_ids.is_empty());
        assert!(full_ids.len() > default_ids.len());
        assert!(full_ids.difference(&default_ids).next().is_some());

        for (extra, candidate_ids) in [("--bisect", &default_ids), ("--bisect-vars", &default_ids)]
        {
            let args = ["rev-list", "--sparse", extra, "HEAD", "--", "path.txt"];
            let stock = raw_pinned_output(stock_repo.path(), &args);
            let zmin = raw_zmin_output(zmin_repo.path(), &args);
            assert_eq!(zmin, stock, "default path bisection mismatch: {args:?}");
            let id = if extra == "--bisect" {
                String::from_utf8(
                    stock
                        .stdout
                        .split(|byte| *byte == b'\n')
                        .next()
                        .unwrap_or_default()
                        .to_vec(),
                )
                .expect("bisect id UTF-8")
            } else {
                bisect_vars_revision(&stock)
            };
            assert!(candidate_ids.contains(&id));
        }

        for (extra, candidate_ids) in [("--bisect", &full_ids), ("--bisect-vars", &full_ids)] {
            let args = [
                "rev-list",
                "--full-history",
                "--sparse",
                extra,
                "HEAD",
                "--",
                "path.txt",
            ];
            let stock = raw_pinned_output(stock_repo.path(), &args);
            let zmin = raw_zmin_output(zmin_repo.path(), &args);
            assert_eq!(zmin, stock, "full path bisection mismatch: {args:?}");
            let id = if extra == "--bisect" {
                String::from_utf8(
                    stock
                        .stdout
                        .split(|byte| *byte == b'\n')
                        .next()
                        .unwrap_or_default()
                        .to_vec(),
                )
                .expect("bisect id UTF-8")
            } else {
                bisect_vars_revision(&stock)
            };
            assert!(candidate_ids.contains(&id));
        }
    }
}

#[test]
fn first_parent_path_history_retains_original_merge_parents_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_all_treesame_merge_fixture(sha256);
        let zmin_repo = pinned_all_treesame_merge_fixture(sha256);
        for args in [
            [
                "log",
                "--first-parent",
                "--sparse",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "log",
                "--first-parent",
                "--full-history",
                "--parents",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--first-parent",
                "--sparse",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--first-parent",
                "--full-history",
                "--parents",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ] {
            assert_eq!(
                raw_zmin_output(zmin_repo.path(), args),
                raw_pinned_output(stock_repo.path(), args),
                "first-parent path tuple mismatch for sha256={sha256}, args={args:?}"
            );
        }
    }
}

#[test]
fn first_parent_path_history_retains_both_parents_when_first_parent_changes_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_first_parent_changed_merge_fixture(sha256);
        let zmin_repo = pinned_first_parent_changed_merge_fixture(sha256);
        for args in [
            ["log", "--first-parent", "--format=%H:%P", "HEAD", "--", "p"].as_slice(),
            [
                "rev-list",
                "--first-parent",
                "--sparse",
                "--parents",
                "--format=%s",
                "HEAD",
                "--",
                "p",
            ]
            .as_slice(),
        ] {
            assert_eq!(
                raw_zmin_output(zmin_repo.path(), args),
                raw_pinned_output(stock_repo.path(), args),
                "first-parent changed merge tuple mismatch for sha256={sha256}, args={args:?}"
            );
        }
    }
}

#[test]
fn sparse_path_history_parent_rewrite_policy_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_all_treesame_merge_fixture(sha256);
        let zmin_repo = pinned_all_treesame_merge_fixture(sha256);
        for command in ["log", "rev-list"] {
            for mode in [
                &[][..],
                &["--first-parent"][..],
                &["--first-parent", "--full-history"][..],
            ] {
                let mut args = vec![command];
                args.extend_from_slice(mode);
                args.extend_from_slice(&[
                    "--sparse",
                    "--parents",
                    "--format=%s",
                    "HEAD",
                    "--",
                    "path.txt",
                ]);
                assert_eq!(
                    raw_zmin_output(zmin_repo.path(), &args),
                    raw_pinned_output(stock_repo.path(), &args),
                    "sparse parent rewrite tuple mismatch for sha256={sha256}, command={command}, mode={mode:?}"
                );
            }
        }
    }
}

#[test]
fn path_history_parent_placeholders_match_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_all_treesame_merge_fixture(sha256);
        let zmin_repo = pinned_all_treesame_merge_fixture(sha256);
        for command in ["log", "rev-list"] {
            for format in ["%H:%P", "%H:%p"] {
                for parents in [false, true] {
                    let format_arg = format!("--format={format}");
                    let mut args = vec![command];
                    if parents {
                        args.push("--parents");
                    }
                    args.extend_from_slice(&[format_arg.as_str(), "HEAD", "--", "path.txt"]);
                    assert_eq!(
                        raw_zmin_output(zmin_repo.path(), &args),
                        raw_pinned_output(stock_repo.path(), &args),
                        "parent placeholder tuple mismatch for sha256={sha256}, command={command}, format={format}, parents={parents}"
                    );
                }
            }
        }
    }
}

#[test]
fn sparse_path_history_seeds_explicit_treesame_roots_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_sparse_explicit_root_fixture(sha256);
        let zmin_repo = pinned_sparse_explicit_root_fixture(sha256);
        for args in [
            ["log", "--sparse", "--format=%s", "HEAD", "--", "path.txt"].as_slice(),
            [
                "rev-list",
                "--sparse",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "log",
                "--sparse",
                "--format=%s",
                "HEAD",
                "--",
                ":(literal)path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--sparse",
                "--format=%s",
                "HEAD",
                "--",
                ":(literal)path.txt",
            ]
            .as_slice(),
        ] {
            assert_eq!(
                raw_zmin_output(zmin_repo.path(), args),
                raw_pinned_output(stock_repo.path(), args),
                "sparse root tuple mismatch for sha256={sha256}, args={args:?}"
            );
        }
    }
}

#[test]
fn path_history_ordering_and_pagination_match_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_path_history_fixture(sha256);
        let zmin_repo = pinned_path_history_fixture(sha256);
        for args in [
            [
                "log",
                "--date-order",
                "--skip=1",
                "--max-count=3",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "log",
                "--topo-order",
                "--skip=1",
                "--max-count=3",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "log",
                "--author-date-order",
                "--skip=1",
                "--max-count=3",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--date-order",
                "--skip=1",
                "--max-count=3",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--topo-order",
                "--skip=1",
                "--max-count=3",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--author-date-order",
                "--skip=1",
                "--max-count=3",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ] {
            assert_eq!(
                raw_zmin_output(zmin_repo.path(), args),
                raw_pinned_output(stock_repo.path(), args),
                "path ordering tuple mismatch for sha256={sha256}, args={args:?}"
            );
        }
    }
}

#[test]
fn path_history_pickaxe_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_path_history_fixture(sha256);
        let zmin_repo = pinned_path_history_fixture(sha256);
        for args in [
            ["log", "-Sside-one", "--format=%s", "HEAD", "--", "path.txt"].as_slice(),
            ["log", "-Gside-one", "--format=%s", "HEAD", "--", "path.txt"].as_slice(),
            [
                "log",
                "-Sside-one",
                "--pickaxe-all",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ]
            .as_slice(),
        ] {
            assert_eq!(
                raw_zmin_output(zmin_repo.path(), args),
                raw_pinned_output(stock_repo.path(), args),
                "path pickaxe tuple mismatch for sha256={sha256}, args={args:?}"
            );
        }
    }
}

#[test]
fn path_rev_list_predicates_filter_before_pagination_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_path_history_fixture(sha256);
        let zmin_repo = pinned_path_history_fixture(sha256);
        for command in ["log", "rev-list"] {
            let args = [
                command,
                "--date-order",
                "--since=1699999999",
                "--until=1700000800",
                "--author=Bench",
                "--committer=Bench",
                "--grep=R",
                "--max-parents=1",
                "--skip=1",
                "--max-count=1",
                "--format=%s",
                "HEAD",
                "--",
                "path.txt",
            ];
            let stock = raw_pinned_output(stock_repo.path(), &args);
            assert!(
                !stock.stdout.is_empty(),
                "predicate fixture unexpectedly produced no stock output for sha256={sha256}, command={command}"
            );
            assert_eq!(
                raw_zmin_output(zmin_repo.path(), &args),
                stock,
                "path predicate/pagination tuple mismatch for sha256={sha256}, command={command}"
            );
        }
    }
}

#[test]
fn path_pickaxe_tree_blob_replacement_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_nested_path_replacement_fixture(sha256);
        let zmin_repo = pinned_nested_path_replacement_fixture(sha256);
        for args in [
            ["log", "-Sreplacement", "--format=%s", "HEAD", "--", "dir"].as_slice(),
            ["log", "-Greplacement", "--format=%s", "HEAD", "--", "dir"].as_slice(),
        ] {
            let stock = raw_pinned_output(stock_repo.path(), args);
            assert!(
                !stock.stdout.is_empty(),
                "replacement pickaxe fixture unexpectedly produced no stock output for sha256={sha256}, args={args:?}"
            );
            assert_eq!(
                raw_zmin_output(zmin_repo.path(), args),
                stock,
                "replacement pickaxe tuple mismatch for sha256={sha256}, args={args:?}"
            );
        }
    }
}

#[test]
fn nested_tree_blob_replacement_path_history_matches_pinned_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let stock_repo = pinned_nested_path_replacement_fixture(sha256);
        let zmin_repo = pinned_nested_path_replacement_fixture(sha256);
        for args in [
            ["log", "--format=%s", "HEAD", "--", "dir/file.txt"].as_slice(),
            [
                "log",
                "--full-history",
                "--format=%s",
                "HEAD",
                "--",
                "dir/file.txt",
            ]
            .as_slice(),
            [
                "rev-list",
                "--full-history",
                "--format=%s",
                "HEAD",
                "--",
                "dir/file.txt",
            ]
            .as_slice(),
        ] {
            assert_eq!(
                raw_zmin_output(zmin_repo.path(), args),
                raw_pinned_output(stock_repo.path(), args),
                "nested tree replacement mismatch for sha256={sha256}, args={args:?}"
            );
        }
    }
}

#[test]
fn log_and_rev_list_simplify_merges_and_decoration_match_stock_git() {
    fn build_repo(repo: &std::path::Path) {
        configure_identity(repo);
        git(repo, ["checkout", "-b", "main"]);

        write_file(repo, "base.txt", "base\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "base"]);

        git(repo, ["checkout", "-b", "topic"]);
        write_file(repo, "topic.txt", "topic1\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "topic1"]);
        write_file(repo, "topic.txt", "topic1\ntopic2\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "topic2"]);

        git(repo, ["checkout", "main"]);
        write_file(repo, "main.txt", "main1\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "main1"]);

        git(repo, ["tag", "anchor", "HEAD~1"]);
        git(repo, ["branch", "topic-anchor", "topic~1"]);

        let output = Command::new(stock_git_bin())
            .args(["merge", "--no-ff", "topic", "-m", "merge"])
            .env("GIT_AUTHOR_NAME", "Merge")
            .env("GIT_AUTHOR_EMAIL", "merge@example.test")
            .env("GIT_AUTHOR_DATE", "2024-01-04 00:00:00 +0000")
            .env("GIT_COMMITTER_NAME", "Merge")
            .env("GIT_COMMITTER_EMAIL", "merge@example.test")
            .env("GIT_COMMITTER_DATE", "2024-01-04 00:00:00 +0000")
            .current_dir(repo)
            .output()
            .expect("git merge");
        assert!(
            output.status.success(),
            "git merge failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        git(repo, ["tag", "merged", "HEAD"]);
    }

    let git_repo = git_init();
    let zmin_repo = git_init();
    build_repo(git_repo.path());
    build_repo(zmin_repo.path());

    for args in [
        ["rev-list", "--simplify-merges", "HEAD"].as_slice(),
        ["rev-list", "--simplify-by-decoration", "HEAD"].as_slice(),
        [
            "rev-list",
            "--simplify-merges",
            "--simplify-by-decoration",
            "HEAD",
        ]
        .as_slice(),
        ["log", "--simplify-merges", "--format=%s", "HEAD"].as_slice(),
        ["log", "--simplify-by-decoration", "--format=%s", "HEAD"].as_slice(),
        [
            "log",
            "--simplify-merges",
            "--simplify-by-decoration",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn simplify_by_decoration_all_peels_packed_tags_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        for args in [
            ["log", "--simplify-by-decoration", "--all", "--format=%s"].as_slice(),
            [
                "rev-list",
                "--simplify-by-decoration",
                "--all",
                "--format=%s",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--simplify-by-decoration",
                "--all",
                "--skip=1",
                "--max-count=2",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--simplify-by-decoration",
                "--all",
                "--max-count=2",
                "--skip=1",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "--simplify-by-decoration",
                "--all",
                "--skip=1",
                "--max-count=2",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "--simplify-by-decoration",
                "--all",
                "--max-count=2",
                "--skip=1",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--simplify-by-decoration",
                "--all",
                "--reverse",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--simplify-by-decoration",
                "--all",
                "--reverse",
                "--skip=1",
                "--max-count=2",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--simplify-by-decoration",
                "--all",
                "--max-count=2",
                "--skip=1",
                "--reverse",
            ]
            .as_slice(),
            ["log", "--all", "--format=%s"].as_slice(),
            ["rev-list", "--all", "--format=%s"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn simplify_by_decoration_all_includes_packed_orphan_annotated_tag_targets() {
    for sha256 in [false, true] {
        let repo = pinned_orphan_annotated_tag_fixture(sha256);
        for args in [
            ["log", "--simplify-by-decoration", "--all", "--format=%H"].as_slice(),
            [
                "rev-list",
                "--simplify-by-decoration",
                "--all",
                "--format=%H",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn rev_list_negative_annotated_tag_root_matches_stock_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_annotated_tag_polarity_fixture(sha256);
        for args in [
            ["rev-list", "HEAD", "--not", "--tags", "--format=%H"].as_slice(),
            ["rev-list", "--all", "--not", "--tags", "--format=%H"].as_slice(),
            ["rev-list", "--not", "--tags", "--all", "--format=%H"].as_slice(),
            [
                "rev-list",
                "refs/heads/orphan-root",
                "^refs/tags/orphan-tag",
                "--format=%H",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn rev_list_selector_order_exclude_scope_and_dangling_refs_match_stock() {
    for sha256 in [false, true] {
        let repo = pinned_dangling_ref_fixture(sha256);
        for args in [
            ["rev-list", "--all", "--format=%H"].as_slice(),
            ["rev-list", "--branches=dangling*", "--format=%H"].as_slice(),
            ["rev-list", "--glob=refs/heads/dangling*", "--format=%H"].as_slice(),
            ["rev-list", "--glob=refs/heads/topic", "--format=%H"].as_slice(),
            ["rev-list", "--glob=refs/heads/*", "--format=%H"].as_slice(),
            [
                "rev-list",
                "HEAD",
                "--not",
                "--branches=dangling*",
                "--format=%H",
            ]
            .as_slice(),
            [
                "rev-list",
                "--exclude=refs/tags/*",
                "--all",
                "--exclude=refs/heads/main",
                "--branches",
                "--format=%H",
            ]
            .as_slice(),
            ["rev-list", "--exclude=side", "--branches", "--format=%H"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn rev_list_interspersed_options_and_dashdash_match_stock() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        for args in [
            ["rev-list", "HEAD", "-n1", "--format=%H"].as_slice(),
            ["rev-list", "HEAD", "--max-count=1", "--format=%H"].as_slice(),
            ["rev-list", "HEAD", "--max-count", "1", "--format=%H"].as_slice(),
            ["rev-list", "HEAD", "--all", "--format=%H"].as_slice(),
            [
                "rev-list",
                "--all",
                "HEAD",
                "--not",
                "--tags",
                "--format=%H",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
        let short_a = raw_zmin_output(repo.path(), &["rev-list", "-a"]);
        assert_ne!(short_a.status, 0, "short -a must not alias --all");
        assert!(short_a.stdout.is_empty(), "short -a must not emit all refs");
        let dash_path_repo = pinned_rev_list_dash_path_fixture(sha256);
        assert_history_tuple(
            dash_path_repo.path(),
            &["rev-list", "--format=%H", "HEAD", "--", "--not-a-revision"],
        );
    }
}

#[test]
fn rev_list_deep_nested_annotated_tag_matches_stock_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_deep_nested_tag_fixture(sha256);
        for args in [
            ["rev-list", "--tags", "--format=%H"].as_slice(),
            ["rev-list", "--all", "--format=%H"].as_slice(),
            ["rev-list", "deep", "--objects"].as_slice(),
            ["rev-list", "deep", "--objects", "--no-object-names"].as_slice(),
            ["rev-list", "deep", "--objects", "--count"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn rev_list_declared_tag_type_mismatch_matches_stock_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        for target_is_blob in [false, true] {
            let repo = pinned_mismatched_tag_fixture(sha256, target_is_blob);
            for args in [
                ["rev-list", "--tags", "--format=%H"].as_slice(),
                ["rev-list", "--all", "--format=%H"].as_slice(),
                ["rev-list", "mismatched", "--format=%H"].as_slice(),
                ["rev-list", "mismatched", "--objects"].as_slice(),
                ["log", "mismatched", "--format=%H"].as_slice(),
            ] {
                assert_history_tuple(repo.path(), args);
            }
        }
    }
}

#[test]
fn rev_list_boundary_object_pagination_matches_pinned_stock() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        for args in [
            ["rev-list", "--objects", "--boundary", "HEAD...side"].as_slice(),
            [
                "rev-list",
                "--objects",
                "--boundary",
                "--skip=1",
                "--max-count=2",
                "--reverse",
                "HEAD...side",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "--boundary",
                "HEAD...side",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--boundary",
                "--cherry-pick",
                "--reverse",
                "--skip=1",
                "--max-count=2",
                "HEAD...side",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--boundary",
                "--cherry-pick",
                "--skip=1",
                "--max-count=2",
                "--reverse",
                "HEAD...side",
            ]
            .as_slice(),
            [
                "rev-list",
                "--objects",
                "--no-object-names",
                "--boundary",
                "--cherry-pick",
                "--reverse",
                "--skip=1",
                "--max-count=2",
                "HEAD...side",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn rev_list_terminal_noncommit_tags_match_stock_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        let repo = pinned_terminal_noncommit_tag_fixture(sha256);
        for args in [
            ["rev-list", "--tags", "--objects"].as_slice(),
            ["rev-list", "--tags", "--objects", "--no-object-names"].as_slice(),
            ["rev-list", "--tags", "--objects", "--filter=blob:none"].as_slice(),
            [
                "rev-list",
                "--tags",
                "--objects",
                "--no-object-names",
                "--filter=blob:none",
            ]
            .as_slice(),
            [
                "rev-list",
                "--tags",
                "--objects",
                "--no-object-names",
                "--filter=blob:none",
                "--filter-provided-objects",
            ]
            .as_slice(),
            [
                "rev-list",
                "--tags",
                "--objects",
                "--count",
                "--filter=blob:none",
            ]
            .as_slice(),
            [
                "rev-list",
                "--tags",
                "--objects",
                "--count",
                "--filter=blob:none",
                "--filter-provided-objects",
            ]
            .as_slice(),
            ["rev-list", "--tags", "--objects", "--count"].as_slice(),
            [
                "rev-list",
                "--tags",
                "--objects",
                "--count",
                "--filter=blob:limit=0",
            ]
            .as_slice(),
            [
                "rev-list",
                "--tags",
                "--objects",
                "--count",
                "--filter=blob:limit=0",
                "--filter-provided-objects",
            ]
            .as_slice(),
            [
                "rev-list",
                "--all",
                "--objects",
                "--count",
                "--filter=blob:limit=0",
            ]
            .as_slice(),
            [
                "rev-list",
                "--all",
                "--objects",
                "--count",
                "--filter=blob:limit=0",
                "--filter-provided-objects",
            ]
            .as_slice(),
            [
                "rev-list",
                "blob-tag",
                "--objects",
                "--count",
                "--filter=blob:limit=0",
            ]
            .as_slice(),
            [
                "rev-list",
                "blob-tag",
                "--objects",
                "--count",
                "--filter=blob:none",
            ]
            .as_slice(),
            [
                "rev-list",
                "blob-tag",
                "--objects",
                "--count",
                "--filter=blob:none",
                "--filter-provided-objects",
            ]
            .as_slice(),
            [
                "rev-list",
                "blob-tag",
                "--objects",
                "--count",
                "--filter=blob:limit=0",
                "--filter-provided-objects",
            ]
            .as_slice(),
            [
                "rev-list",
                "tree-tag",
                "--objects",
                "--count",
                "--filter=blob:limit=0",
            ]
            .as_slice(),
            [
                "rev-list",
                "tree-tag",
                "--objects",
                "--count",
                "--filter=blob:none",
            ]
            .as_slice(),
            [
                "rev-list",
                "tree-tag",
                "--objects",
                "--count",
                "--filter=blob:none",
                "--filter-provided-objects",
            ]
            .as_slice(),
            [
                "rev-list",
                "tree-tag",
                "--objects",
                "--count",
                "--filter=blob:limit=0",
                "--filter-provided-objects",
            ]
            .as_slice(),
            ["rev-list", "blob-tag", "--objects"].as_slice(),
            ["rev-list", "tree-tag", "--objects"].as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
    }
}

#[test]
fn rev_list_object_filter_kind_and_malformed_specs_match_pinned_stock() {
    for sha256 in [false, true] {
        let repo = pinned_terminal_noncommit_tag_fixture(sha256);
        for args in [
            [
                "rev-list",
                "--tags",
                "--objects",
                "--filter=object:type=blob",
            ]
            .as_slice(),
            [
                "rev-list",
                "--tags",
                "--objects",
                "--no-object-names",
                "--filter=object:type=blob",
            ]
            .as_slice(),
            [
                "rev-list",
                "--tags",
                "--objects",
                "--count",
                "--filter=object:type=blob",
            ]
            .as_slice(),
            [
                "rev-list",
                "--tags",
                "--objects",
                "--filter=object:type=commit",
            ]
            .as_slice(),
            [
                "rev-list",
                "--tags",
                "--objects",
                "--filter=object:type=tag",
            ]
            .as_slice(),
            [
                "rev-list",
                "--tags",
                "--objects",
                "--filter=object:type=tree",
            ]
            .as_slice(),
            [
                "rev-list",
                "blob-tag",
                "--objects",
                "--filter=object:type=blob",
            ]
            .as_slice(),
            [
                "rev-list",
                "blob-tag",
                "--objects",
                "--filter=object:type=blob",
                "--filter-provided-objects",
            ]
            .as_slice(),
            [
                "rev-list",
                "blob-tag",
                "--objects",
                "--no-object-names",
                "--filter=object:type=blob",
            ]
            .as_slice(),
            [
                "rev-list",
                "blob-tag",
                "--objects",
                "--count",
                "--filter=object:type=blob",
            ]
            .as_slice(),
            ["rev-list", "blob-tag", "--objects", "--filter=object:type="].as_slice(),
            [
                "rev-list",
                "blob-tag",
                "--objects",
                "--filter=object:type=blob,commit",
            ]
            .as_slice(),
            [
                "rev-list",
                "--tags",
                "--objects",
                "--filter=object:type=bad",
            ]
            .as_slice(),
        ] {
            assert_history_tuple(repo.path(), args);
        }
        let history = pinned_history_fixture(sha256);
        for args in [
            ["rev-list", "--objects", "--filter=blob:limit=x", "HEAD"].as_slice(),
            ["rev-list", "--objects", "--filter=blob:limit=", "HEAD"].as_slice(),
            ["rev-list", "--objects", "--filter=blob:limit=-1", "HEAD"].as_slice(),
            ["rev-list", "--objects", "--filter=not-a-filter", "HEAD"].as_slice(),
        ] {
            assert_history_tuple(history.path(), args);
        }
    }
}

#[test]
fn rev_list_tag_root_errors_match_stock_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        for malformed in [false, true] {
            let repo = pinned_tag_root_error_fixture(sha256, malformed);
            for args in [
                ["rev-list", "--tags", "--format=%H"].as_slice(),
                ["rev-list", "--all", "--format=%H"].as_slice(),
                ["rev-list", "broken", "--format=%H"].as_slice(),
                ["log", "broken", "--format=%H"].as_slice(),
                ["rev-list", "broken", "--objects"].as_slice(),
            ] {
                assert_history_tuple(repo.path(), args);
            }
        }
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
fn log_no_walk_value_forms_with_stdin_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "a.txt", "one\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "one"]);
        write_file(repo, "a.txt", "two\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "two"]);
    }

    let git_head = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let git_parent = git(git_repo.path(), ["rev-parse", "HEAD~1"]);
    let zmin_head = git(zmin_repo.path(), ["rev-parse", "HEAD"]);
    let zmin_parent = git(zmin_repo.path(), ["rev-parse", "HEAD~1"]);

    for option in ["--no-walk=unsorted", "--no-walk=sorted"] {
        let args = ["log", option, "--format=%s", "--stdin"];
        assert_eq!(
            command_any_output_with_stdin(
                zmin_bin(),
                zmin_repo.path(),
                &args,
                &format!("{zmin_head}\n{zmin_parent}\n"),
                "zmin",
            ),
            command_any_output_with_stdin(
                "git",
                git_repo.path(),
                &args,
                &format!("{git_head}\n{git_parent}\n"),
                "git",
            ),
            "args: {args:?}"
        );
    }
}

#[test]
fn log_and_rev_list_shared_history_schema_batch_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        write_commit_with_date(repo, "a.txt", "one\n", "1700000000 +0000", "one");
        write_commit_with_date(repo, "a.txt", "two\n", "1700001000 +0000", "two");
        write_commit_with_date(repo, "a.txt", "three\n", "1700002000 +0000", "three");
    }

    for args in [
        ["log", "--do-walk", "--format=%s", "HEAD"].as_slice(),
        ["log", "--max-age=1700000500", "--format=%s", "HEAD"].as_slice(),
        ["log", "--min-age=1700001500", "--format=%s", "HEAD"].as_slice(),
        ["log", "--skip=1", "--format=%s", "HEAD"].as_slice(),
        ["log", "--do-walk", "--skip=1", "--format=%s", "HEAD"].as_slice(),
        ["rev-list", "--do-walk", "HEAD"].as_slice(),
        ["rev-list", "--max-age=1700000500", "HEAD"].as_slice(),
        ["rev-list", "--min-age=1700001500", "HEAD"].as_slice(),
        ["rev-list", "--skip=1", "HEAD"].as_slice(),
        ["rev-list", "--timestamp", "HEAD"].as_slice(),
        ["rev-list", "--object-names", "--objects", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn log_remaining_documented_tail_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_commit_with_date(repo.path(), "a.txt", "one\n", "1700000000 +0000", "one");
    write_commit_with_date(repo.path(), "a.txt", "two\n", "1700000600 +0000", "two");
    git(repo.path(), ["branch", "side", "HEAD~1"]);
    git(repo.path(), ["tag", "v1", "HEAD~1"]);

    for args in [
        ["log", "--alternate-refs", "--format=%s", "HEAD"].as_slice(),
        ["log", "--bisect", "--format=%s", "HEAD"].as_slice(),
        ["log", "--cherry", "--format=%s", "HEAD"].as_slice(),
        ["log", "--exclude=main", "--all", "--format=%s"].as_slice(),
        ["log", "--exclude-first-parent-only", "--all", "--format=%s"].as_slice(),
        ["log", "--exclude-hidden=fetch", "--all", "--format=%s"].as_slice(),
        ["log", "--glob=main", "--format=%s", "HEAD"].as_slice(),
        ["log", "--ignore-missing", "--format=%s", "HEAD"].as_slice(),
        ["log", "--in-commit-order", "--format=%s", "HEAD"].as_slice(),
        ["log", "--indexed-objects", "--format=%s", "HEAD"].as_slice(),
        ["log", "--left-only", "--format=%s", "HEAD...side"].as_slice(),
        ["log", "--no-filter", "--format=%s", "HEAD"].as_slice(),
        ["log", "--remove-empty", "--format=%s", "HEAD"].as_slice(),
        ["log", "--right-only", "--format=%s", "HEAD...side"].as_slice(),
        ["log", "--show-linear-break", "--format=%s", "HEAD"].as_slice(),
        ["log", "--since-as-filter=1700000300", "--format=%s", "HEAD"].as_slice(),
        ["log", "--single-worktree", "--format=%s", "HEAD"].as_slice(),
        ["log", "--stdin", "--format=%s", "HEAD"].as_slice(),
        ["log", "--unpacked", "--format=%s", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    for args in [
        ["log", "--bisect-all", "--format=%s", "HEAD"].as_slice(),
        ["log", "--bisect-vars", "--format=%s", "HEAD"].as_slice(),
        ["log", "--commit-header", "HEAD"].as_slice(),
        ["log", "--exclude-promisor-objects", "HEAD"].as_slice(),
        ["log", "--filter-print-omitted", "HEAD"].as_slice(),
        ["log", "--header", "HEAD"].as_slice(),
        ["log", "--merge", "HEAD"].as_slice(),
        ["log", "--missing", "HEAD"].as_slice(),
        ["log", "--no-commit-header", "HEAD"].as_slice(),
        ["log", "--progress", "HEAD"].as_slice(),
        ["log", "--use-bitmap-index", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn log_line_range_matches_stock_git_for_top_of_file_root_lane() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_commit_with_date(
        repo.path(),
        "a.txt",
        "one\ntwo\n",
        "1700000000 +0000",
        "base",
    );

    assert_eq!(
        run_zmin_args(repo.path(), &["log", "-L", "1,1:a.txt", "HEAD", "--"]),
        git_args(repo.path(), &["log", "-L", "1,1:a.txt", "HEAD", "--"])
    );
}

#[test]
fn log_pathspec_separator_preserves_a_literal_dashdash_path() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_commit_with_date(repo.path(), "--", "one\n", "1700000000 +0000", "dashdash");

    let args = ["log", "--format=%s", "HEAD", "--", "--"];
    assert_eq!(
        run_zmin_args(repo.path(), &args),
        git_args(repo.path(), &args)
    );
}

#[test]
fn log_output_surface_tail_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_commit_with_date(repo.path(), "a.txt", "one\n", "1700000000 +0000", "one");
    write_commit_with_date(repo.path(), "a.txt", "two\n", "1700000600 +0000", "two");

    for args in [
        [
            "log",
            "--decorate-refs=refs/heads/main",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        [
            "log",
            "--decorate-refs-exclude=refs/tags/*",
            "--format=%s",
            "HEAD",
        ]
        .as_slice(),
        ["log", "--full-diff", "--format=%s", "HEAD"].as_slice(),
        ["log", "--mailmap", "--format=%s", "HEAD"].as_slice(),
        ["log", "--no-decorate", "--format=%s", "HEAD"].as_slice(),
        ["log", "--no-mailmap", "--format=%s", "HEAD"].as_slice(),
        ["log", "--no-use-mailmap", "--format=%s", "HEAD"].as_slice(),
        ["log", "--objects-edge", "--format=%s", "HEAD"].as_slice(),
        ["log", "--objects-edge-aggressive", "--format=%s", "HEAD"].as_slice(),
        ["log", "--source", "--format=%s", "HEAD"].as_slice(),
        ["log", "--use-mailmap", "--format=%s", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    assert_eq!(
        run_zmin_failure_output(repo.path(), &["log", "--disk-usage", "HEAD"]),
        git_failure_output(repo.path(), &["log", "--disk-usage", "HEAD"])
    );
}

#[test]
fn log_documented_unsigned_tail_batch_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "a.txt", "one\ntwo\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);

    for args in [
        ["log", "--graph", "--format=%s", "HEAD"].as_slice(),
        ["log", "--log-size", "--format=%s", "HEAD"].as_slice(),
        ["log", "--show-signature", "--format=%s", "HEAD"].as_slice(),
        ["log", "--follow", "--format=%s", "a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn rev_list_documented_tail_batch_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["checkout", "-b", "main"]);
    write_commit_with_date(repo.path(), "a.txt", "one\n", "1700000000 +0000", "one");
    write_commit_with_date(repo.path(), "a.txt", "two\n", "1700000600 +0000", "two");
    git(repo.path(), ["branch", "side", "HEAD~1"]);
    git(repo.path(), ["tag", "v1", "HEAD~1"]);

    for args in [
        ["rev-list", "--alternate-refs", "HEAD"].as_slice(),
        ["rev-list", "--bisect", "HEAD"].as_slice(),
        ["rev-list", "--bisect-all", "HEAD"].as_slice(),
        ["rev-list", "--bisect-vars", "HEAD"].as_slice(),
        ["rev-list", "--commit-header", "HEAD"].as_slice(),
        ["rev-list", "--disk-usage", "HEAD"].as_slice(),
        ["rev-list", "--exclude=main", "--all"].as_slice(),
        ["rev-list", "--exclude-first-parent-only", "--all"].as_slice(),
        ["rev-list", "--exclude-hidden=fetch", "--all"].as_slice(),
        ["rev-list", "--exclude-promisor-objects", "HEAD"].as_slice(),
        ["rev-list", "--filter-print-omitted", "HEAD"].as_slice(),
        ["rev-list", "--glob=main", "HEAD"].as_slice(),
        ["rev-list", "--graph", "HEAD"].as_slice(),
        ["rev-list", "--header", "HEAD"].as_slice(),
        ["rev-list", "--ignore-missing", "HEAD"].as_slice(),
        ["rev-list", "--in-commit-order", "HEAD"].as_slice(),
        ["rev-list", "--indexed-objects", "HEAD"].as_slice(),
        ["rev-list", "--left-only", "HEAD...side"].as_slice(),
        ["rev-list", "--no-commit-header", "HEAD"].as_slice(),
        ["rev-list", "--no-filter", "HEAD"].as_slice(),
        ["rev-list", "--no-walk", "HEAD"].as_slice(),
        ["rev-list", "--objects-edge", "HEAD"].as_slice(),
        ["rev-list", "--objects-edge-aggressive", "HEAD"].as_slice(),
        ["rev-list", "--remove-empty", "HEAD"].as_slice(),
        ["rev-list", "--right-only", "HEAD...side"].as_slice(),
        ["rev-list", "--show-linear-break", "HEAD"].as_slice(),
        ["rev-list", "--show-signature", "HEAD"].as_slice(),
        ["rev-list", "--since-as-filter=1700000300", "HEAD"].as_slice(),
        ["rev-list", "--single-worktree", "HEAD"].as_slice(),
        ["rev-list", "--stdin", "HEAD"].as_slice(),
        ["rev-list", "--unpacked", "HEAD"].as_slice(),
        ["rev-list", "--use-bitmap-index", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    for args in [
        ["rev-list", "--merge", "HEAD"].as_slice(),
        ["rev-list", "--missing", "HEAD"].as_slice(),
        ["rev-list", "--progress", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn rev_list_missing_print_for_tree_root_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"one\n").expect("write tracked file");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    let tree = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);
    let blob = git(repo.path(), ["rev-parse", "HEAD:a.txt"]);
    let blob_path = repo
        .path()
        .join(".git/objects")
        .join(&blob[..2])
        .join(&blob[2..]);
    fs::remove_file(&blob_path).expect("remove blob object");
    assert!(!blob_path.exists(), "missing path blob was not removed");

    assert_eq!(
        run_zmin_args(
            repo.path(),
            &[
                "rev-list",
                "--objects",
                "--missing=print",
                &tree,
                "--",
                "a.txt",
            ]
        ),
        git_args(
            repo.path(),
            &[
                "rev-list",
                "--objects",
                "--missing=print",
                &tree,
                "--",
                "a.txt",
            ]
        )
    );
}

fn pinned_missing_tree_path_fixture(sha256: bool, nested: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        git_init()
    };
    configure_identity(repo.path());
    let path = if nested { "dir/a.txt" } else { "a.txt" };
    write_file(repo.path(), path, "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let treeish = if nested { "HEAD:dir" } else { "HEAD^{tree}" };
    let tree = git(repo.path(), ["rev-parse", treeish]);
    delete_loose_object(repo.path(), &tree);
    assert!(
        !repo
            .path()
            .join(".git/objects")
            .join(&tree[..2])
            .join(&tree[2..])
            .exists()
    );
    repo
}

#[test]
fn rev_list_missing_path_trees_match_stock_git_for_sha1_and_sha256() {
    for sha256 in [false, true] {
        for (nested, path) in [(false, "a.txt"), (true, "dir/a.txt")] {
            let stock_repo = pinned_missing_tree_path_fixture(sha256, nested);
            let zmin_repo = pinned_missing_tree_path_fixture(sha256, nested);
            for missing_mode in ["--missing=allow-any", "--missing=print"] {
                let args = ["rev-list", "--objects", missing_mode, "HEAD", "--", path];
                let zmin = command_any_output(
                    zmin_bin(),
                    zmin_repo.path(),
                    &args,
                    "zmin path missing tree",
                );
                let stock = command_any_output(
                    stock_git_bin().to_str().expect("stock git path utf8"),
                    stock_repo.path(),
                    &args,
                    "git path missing tree",
                );
                assert_eq!(zmin, stock, "path missing tuple mismatch: sha256={sha256}");
                assert_eq!(stock.0, 128, "missing path tree unexpectedly succeeded");
                assert!(stock.1.is_empty());
                assert!(stock.2.contains("fatal: unable to read tree ("));
            }

            let args = ["rev-list", "--objects", "--missing=print", "HEAD"];
            let zmin = command_any_output(
                zmin_bin(),
                zmin_repo.path(),
                &args,
                "zmin missing tree print",
            );
            let stock = command_any_output(
                stock_git_bin().to_str().expect("stock git path utf8"),
                stock_repo.path(),
                &args,
                "git missing tree print",
            );
            assert_eq!(zmin, stock, "missing print tuple mismatch: sha256={sha256}");
            assert_eq!(stock.0, 0);
            assert!(stock.1.lines().any(|line| line.starts_with('?')));
            assert!(stock.2.is_empty());
        }
    }
}

#[test]
fn rev_list_exclude_promisor_objects_stops_at_missing_promised_commit() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "foo"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "bar"]);

    let promised_parent = git(repo.path(), ["rev-parse", "HEAD~1"]);
    pack_as_from_promisor(repo.path(), &promised_parent);
    delete_loose_object(repo.path(), &promised_parent);
    assert!(
        !repo
            .path()
            .join(".git/objects")
            .join(&promised_parent[..2])
            .join(&promised_parent[2..])
            .exists(),
        "promised parent remained loose"
    );
    git(repo.path(), ["config", "core.repositoryformatversion", "1"]);
    git(
        repo.path(),
        ["config", "extensions.partialclone", "arbitrary string"],
    );

    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &[
                "rev-list",
                "--exclude-promisor-objects",
                "--objects",
                "HEAD",
                "--",
                "a.txt",
            ],
            "zmin rev-list exclude-promisor-objects",
        ),
        command_any_output(
            stock_git_bin().to_str().expect("stock git path utf8"),
            repo.path(),
            &[
                "rev-list",
                "--exclude-promisor-objects",
                "--objects",
                "HEAD",
                "--",
                "a.txt",
            ],
            "git rev-list exclude-promisor-objects",
        )
    );
}

#[test]
fn rev_list_missing_promised_trees_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "foo"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "bar"]);
    write_file(repo.path(), "a.txt", "three\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "baz"]);

    let promised_tree_one = git(repo.path(), ["rev-parse", "HEAD~1^{tree}"]);
    let promised_tree_two = git(repo.path(), ["rev-parse", "HEAD~2^{tree}"]);
    promise_and_delete(repo.path(), "HEAD~1^{tree}");
    promise_and_delete(repo.path(), "HEAD~2^{tree}");
    for promised_tree in [&promised_tree_one, &promised_tree_two] {
        assert!(
            !repo
                .path()
                .join(".git/objects")
                .join(&promised_tree[..2])
                .join(&promised_tree[2..])
                .exists(),
            "promised tree remained loose: {promised_tree}"
        );
    }
    git(repo.path(), ["config", "core.repositoryformatversion", "1"]);
    git(
        repo.path(),
        ["config", "extensions.partialclone", "arbitrary string"],
    );

    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &[
                "rev-list",
                "--missing=allow-promisor",
                "--objects",
                "HEAD",
                "--",
                "a.txt",
            ],
            "zmin rev-list missing allow-promisor",
        ),
        command_any_output(
            stock_git_bin().to_str().expect("stock git path utf8"),
            repo.path(),
            &[
                "rev-list",
                "--missing=allow-promisor",
                "--objects",
                "HEAD",
                "--",
                "a.txt",
            ],
            "git rev-list missing allow-promisor",
        )
    );

    promise_and_delete(repo.path(), "HEAD^{tree}");

    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &[
                "rev-list",
                "--exclude-promisor-objects",
                "--objects",
                "HEAD",
                "--",
                "a.txt",
            ],
            "zmin rev-list exclude promised trees",
        ),
        command_any_output(
            stock_git_bin().to_str().expect("stock git path utf8"),
            repo.path(),
            &[
                "rev-list",
                "--exclude-promisor-objects",
                "--objects",
                "HEAD",
                "--",
                "a.txt",
            ],
            "git rev-list exclude promised trees",
        )
    );

    let conflict_args = [
        "rev-list",
        "--exclude-promisor-objects",
        "--missing=allow-promisor",
        "HEAD",
    ];
    assert_eq!(
        command_raw_output(
            zmin_bin(),
            repo.path(),
            &conflict_args,
            "zmin rev-list promisor/missing conflict",
        ),
        command_raw_output(
            required_pinned_stock_git()
                .to_str()
                .expect("pinned Git path utf8"),
            repo.path(),
            &conflict_args,
            "stock rev-list promisor/missing conflict",
        )
    );
}

#[test]
fn bare_filtered_partial_clone_history_commands_match_stock_git() {
    let (_temp, repo) = bare_filtered_rename_partial_clone_fixture(false);
    let repo_path = repo.to_str().expect("partial repo path utf8").to_owned();

    for args in [
        ["-C", repo_path.as_str(), "rev-list", "HEAD"].as_slice(),
        ["-C", repo_path.as_str(), "rev-list", "--no-walk", "HEAD"].as_slice(),
        [
            "-C",
            repo_path.as_str(),
            "rev-list",
            "--objects",
            "--missing=print",
            "HEAD",
        ]
        .as_slice(),
        [
            "-C",
            repo_path.as_str(),
            "log",
            "--no-walk",
            "--oneline",
            "HEAD",
        ]
        .as_slice(),
        [
            "-C",
            repo_path.as_str(),
            "show",
            "--no-patch",
            "--oneline",
            "HEAD",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_any_output(
                zmin_bin(),
                std::path::Path::new("."),
                args,
                "zmin bare partial history",
            ),
            command_any_output(
                stock_git_bin().to_str().expect("stock git path utf8"),
                std::path::Path::new("."),
                args,
                "git bare partial history",
            ),
            "args: {args:?}"
        );
    }
}

#[test]
fn rev_list_direct_promised_objects_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "foo"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "bar"]);
    write_file(repo.path(), "a.txt", "three\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "baz"]);

    let commit = git(repo.path(), ["rev-parse", "HEAD~2"]);
    let tree = git(repo.path(), ["rev-parse", "HEAD~1^{tree}"]);
    let blob = git(repo.path(), ["hash-object", "a.txt"]);

    promise_and_delete(repo.path(), &commit);
    promise_and_delete(repo.path(), &tree);
    promise_and_delete(repo.path(), &blob);
    git(repo.path(), ["config", "core.repositoryformatversion", "1"]);
    git(
        repo.path(),
        ["config", "extensions.partialclone", "arbitrary string"],
    );

    for args in [
        [
            "rev-list",
            "--objects",
            "--exclude-promisor-objects",
            &commit,
        ]
        .as_slice(),
        [
            "rev-list",
            "--objects-edge-aggressive",
            "--exclude-promisor-objects",
            &commit,
        ]
        .as_slice(),
        [
            "rev-list",
            "--ignore-missing",
            "--objects",
            "--exclude-promisor-objects",
            &commit,
        ]
        .as_slice(),
        [
            "rev-list",
            "--ignore-missing",
            "--objects-edge-aggressive",
            "--exclude-promisor-objects",
            &commit,
        ]
        .as_slice(),
        ["rev-list", "--objects", "--exclude-promisor-objects", &tree].as_slice(),
        [
            "rev-list",
            "--objects-edge-aggressive",
            "--exclude-promisor-objects",
            &tree,
        ]
        .as_slice(),
        [
            "rev-list",
            "--ignore-missing",
            "--objects",
            "--exclude-promisor-objects",
            &tree,
        ]
        .as_slice(),
        [
            "rev-list",
            "--ignore-missing",
            "--objects-edge-aggressive",
            "--exclude-promisor-objects",
            &tree,
        ]
        .as_slice(),
        ["rev-list", "--objects", "--exclude-promisor-objects", &blob].as_slice(),
        [
            "rev-list",
            "--objects-edge-aggressive",
            "--exclude-promisor-objects",
            &blob,
        ]
        .as_slice(),
        [
            "rev-list",
            "--ignore-missing",
            "--objects",
            "--exclude-promisor-objects",
            &blob,
        ]
        .as_slice(),
        [
            "rev-list",
            "--ignore-missing",
            "--objects-edge-aggressive",
            "--exclude-promisor-objects",
            &blob,
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                args,
                "zmin rev-list direct promised object",
            ),
            command_any_output(
                stock_git_bin().to_str().expect("stock git path utf8"),
                repo.path(),
                args,
                "git rev-list direct promised object",
            ),
            "args: {args:?}"
        );
    }
}

#[test]
fn rev_list_missing_print_for_direct_promised_commit_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "foo"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "bar"]);

    let commit = git(repo.path(), ["rev-parse", "HEAD~1"]);
    promise_and_delete(repo.path(), &commit);
    git(repo.path(), ["config", "core.repositoryformatversion", "1"]);
    git(
        repo.path(),
        ["config", "extensions.partialclone", "arbitrary string"],
    );

    let args = ["rev-list", "--objects", "--missing=print", commit.as_str()];
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &args,
            "zmin rev-list missing print direct promised commit",
        ),
        command_any_output(
            stock_git_bin().to_str().expect("stock git path utf8"),
            repo.path(),
            &args,
            "git rev-list missing print direct promised commit",
        )
    );
}

#[test]
fn bare_filtered_partial_clone_log_follow_path_matches_stock_git() {
    let pinned = required_pinned_stock_git();
    let pinned = pinned.to_str().expect("pinned Git path utf8");
    for sha256 in [false, true] {
        let (_stock_temp, stock_repo) = bare_filtered_rename_partial_clone_fixture(sha256);
        let (_zmin_temp, zmin_repo) = bare_filtered_rename_partial_clone_fixture(sha256);
        let stock_repo_path = stock_repo
            .to_str()
            .expect("stock partial repo path utf8")
            .to_owned();
        let zmin_repo_path = zmin_repo
            .to_str()
            .expect("Zmin partial repo path utf8")
            .to_owned();
        for repo in [stock_repo.as_path(), zmin_repo.as_path()] {
            assert_eq!(
                pinned_git_args(repo, &["config", "remote.origin.promisor"]),
                "true"
            );
            assert_eq!(
                pinned_git_args(repo, &["config", "remote.origin.partialclonefilter"]),
                "blob:none"
            );
        }
        let stock_old_blob =
            pinned_git_args(stock_repo.as_path(), &["rev-parse", "HEAD~1:old-file.txt"]);
        let zmin_old_blob =
            pinned_git_args(zmin_repo.as_path(), &["rev-parse", "HEAD~1:old-file.txt"]);
        assert_eq!(stock_old_blob, zmin_old_blob);
        let stock_renamed_blob =
            pinned_git_args(stock_repo.as_path(), &["rev-parse", "HEAD:new-file.txt"]);
        let zmin_renamed_blob =
            pinned_git_args(zmin_repo.as_path(), &["rev-parse", "HEAD:new-file.txt"]);
        assert_eq!(stock_old_blob, stock_renamed_blob);
        assert_eq!(zmin_old_blob, zmin_renamed_blob);
        let stock_before = raw_pinned_output(
            stock_repo.as_path(),
            &["rev-list", "--objects", "--missing=print", "--all"],
        );
        let zmin_before = command_raw_output(
            zmin_bin(),
            std::path::Path::new("."),
            &[
                "-C",
                zmin_repo_path.as_str(),
                "rev-list",
                "--objects",
                "--missing=print",
                "--all",
            ],
            "zmin pre-follow missing probe",
        );
        for (label, output, old_blob) in [
            ("stock", &stock_before, &stock_old_blob),
            ("Zmin", &zmin_before, &zmin_old_blob),
        ] {
            assert_eq!(
                output.status, 0,
                "{label} pre-follow missing status SHA-256={sha256}"
            );
            let promised_line = format!("?{old_blob}");
            assert!(
                output
                    .stdout
                    .split(|byte| *byte == b'\n')
                    .any(|line| line == promised_line.as_bytes()),
                "{label} pre-follow probe lacks promised blob {old_blob} SHA-256={sha256}: {}",
                String::from_utf8_lossy(&output.stdout)
            );
        }

        let follow_extra_args = vec!["--follow", "--format=%s", "--", "new-file.txt"];
        let mut stock_follow_args = vec!["-C", stock_repo_path.as_str(), "log"];
        stock_follow_args.extend(follow_extra_args.iter().copied());
        let mut zmin_follow_args = vec!["-C", zmin_repo_path.as_str(), "log"];
        zmin_follow_args.extend(follow_extra_args.iter().copied());
        let stock_follow = command_raw_output(
            pinned,
            std::path::Path::new("."),
            &stock_follow_args,
            "pinned follow",
        );
        let zmin_follow = command_raw_output(
            zmin_bin(),
            std::path::Path::new("."),
            &zmin_follow_args,
            "zmin follow",
        );
        assert_eq!(zmin_follow, stock_follow, "follow SHA-256={sha256}");
        assert_eq!(zmin_follow.status, 0, "follow status SHA-256={sha256}");
        assert_eq!(
            zmin_follow.stdout, b"rename-the-file\ncreate-a-file\n",
            "follow stdout SHA-256={sha256}"
        );
        assert!(
            zmin_follow.stderr.is_empty(),
            "follow stderr SHA-256={sha256}"
        );
        let stock_after = raw_pinned_output(
            stock_repo.as_path(),
            &["rev-list", "--objects", "--missing=print", "--all"],
        );
        let zmin_after = command_raw_output(
            zmin_bin(),
            std::path::Path::new("."),
            &[
                "-C",
                zmin_repo_path.as_str(),
                "rev-list",
                "--objects",
                "--missing=print",
                "--all",
            ],
            "zmin post-follow missing probe",
        );
        assert_eq!(
            zmin_after, stock_after,
            "post-follow missing probe SHA-256={sha256}"
        );
        for (label, output, old_blob) in [
            ("stock", &stock_after, &stock_old_blob),
            ("Zmin", &zmin_after, &zmin_old_blob),
        ] {
            assert_eq!(
                output.status, 0,
                "{label} post-follow missing status SHA-256={sha256}"
            );
            let promised_line = format!("?{old_blob}");
            assert!(
                output
                    .stdout
                    .split(|byte| *byte == b'\n')
                    .any(|line| line == promised_line.as_bytes()),
                "{label} follow fetched promised blob {old_blob} SHA-256={sha256}: {}",
                String::from_utf8_lossy(&output.stdout)
            );
        }

        for (label, extra_args, expected) in [
            (
                "fixed",
                vec!["--format=%s", "--", "new-file.txt"],
                b"rename-the-file\n".as_slice(),
            ),
            (
                "no-follow",
                vec!["--no-follow", "--format=%s", "--", "new-file.txt"],
                b"rename-the-file\n".as_slice(),
            ),
        ] {
            let mut args = vec!["-C", zmin_repo_path.as_str(), "log"];
            args.extend(extra_args);
            let zmin = command_raw_output(zmin_bin(), std::path::Path::new("."), &args, label);
            let stock = command_raw_output(pinned, std::path::Path::new("."), &args, "pinned Git");
            assert_eq!(zmin, stock, "{label} SHA-256={sha256}");
            assert_eq!(zmin.status, 0, "{label} status SHA-256={sha256}");
            assert_eq!(zmin.stdout, expected, "{label} stdout SHA-256={sha256}");
            assert!(zmin.stderr.is_empty(), "{label} stderr SHA-256={sha256}");
        }

        for (label, pathspecs) in [
            ("follow-two-paths", vec!["new-file.txt", "old-file.txt"]),
            ("follow-no-path", Vec::new()),
            ("follow-icase", vec![":(icase)new-file.txt"]),
            ("follow-glob", vec![":(glob)new-file.txt"]),
            ("follow-literal", vec![":(literal)new-file.txt"]),
            ("follow-top-literal", vec![":(top,literal)new-file.txt"]),
            ("follow-literal-top", vec![":(literal,top)new-file.txt"]),
            ("follow-top", vec![":(top)new-file.txt"]),
            ("follow-root", vec![":/new-file.txt"]),
            ("follow-top-icase", vec![":(top,icase)new-file.txt"]),
            (
                "follow-top-glob-icase",
                vec![":(top,glob,icase)new-file.txt"],
            ),
            (
                "follow-top-icase-glob",
                vec![":(top,icase,glob)new-file.txt"],
            ),
            ("follow-exclude-only", vec![":(exclude)new-file.txt"]),
            (
                "follow-positive-exclude",
                vec!["new-file.txt", ":(exclude)old-file.txt"],
            ),
            (
                "follow-exclude-positive",
                vec![":(exclude)old-file.txt", "new-file.txt"],
            ),
            ("follow-exclude-glob", vec![":(exclude,glob)new-file.txt"]),
            ("follow-caret-only", vec![":^new-file.txt"]),
            (
                "follow-positive-caret",
                vec!["new-file.txt", ":^old-file.txt"],
            ),
            (
                "follow-caret-positive",
                vec![":^old-file.txt", "new-file.txt"],
            ),
        ] {
            let mut args = vec![
                "-C",
                zmin_repo_path.as_str(),
                "log",
                "--follow",
                "--format=%s",
            ];
            if !pathspecs.is_empty() {
                args.push("--");
                args.extend(pathspecs);
            }
            let zmin = command_raw_output(zmin_bin(), std::path::Path::new("."), &args, label);
            let stock = command_raw_output(pinned, std::path::Path::new("."), &args, "pinned Git");
            assert_eq!(zmin, stock, "{label} SHA-256={sha256}");
            if matches!(
                label,
                "follow-literal"
                    | "follow-top-literal"
                    | "follow-literal-top"
                    | "follow-top"
                    | "follow-root"
            ) {
                assert_eq!(zmin.status, 0, "{label} status SHA-256={sha256}");
                assert_eq!(
                    zmin.stdout, b"rename-the-file\ncreate-a-file\n",
                    "{label} stdout SHA-256={sha256}"
                );
                assert!(zmin.stderr.is_empty(), "{label} stderr SHA-256={sha256}");
            } else {
                assert_ne!(
                    zmin.status, 0,
                    "{label} must reject unsupported follow input"
                );
                assert!(zmin.stdout.is_empty(), "{label} stdout SHA-256={sha256}");
                assert!(!zmin.stderr.is_empty(), "{label} stderr SHA-256={sha256}");
                if matches!(label, "follow-top-glob-icase" | "follow-top-icase-glob") {
                    assert!(
                        zmin.stderr
                            .windows(b"'glob'".len())
                            .any(|window| window == b"'glob'"),
                        "missing glob token in {label} stderr SHA-256={sha256}: {}",
                        String::from_utf8_lossy(&zmin.stderr)
                    );
                    assert!(
                        zmin.stderr
                            .windows(b"'icase'".len())
                            .any(|window| window == b"'icase'"),
                        "missing icase token in {label} stderr SHA-256={sha256}: {}",
                        String::from_utf8_lossy(&zmin.stderr)
                    );
                }
                if matches!(
                    label,
                    "follow-exclude-only"
                        | "follow-positive-exclude"
                        | "follow-exclude-positive"
                        | "follow-exclude-glob"
                        | "follow-caret-only"
                        | "follow-positive-caret"
                        | "follow-caret-positive"
                ) {
                    assert_eq!(
                        zmin.stderr, b"fatal: --follow requires exactly one pathspec\n",
                        "exclude validation diagnostic SHA-256={sha256}"
                    );
                }
            }
        }

        let wildcard_args = [
            "-C",
            zmin_repo_path.as_str(),
            "log",
            "--follow",
            "--format=%s",
            "--",
            ":(literal)wild*file.txt",
        ];
        let wildcard_zmin = command_raw_output(
            zmin_bin(),
            std::path::Path::new("."),
            &wildcard_args,
            "zmin wildcard",
        );
        let wildcard_stock = command_raw_output(
            pinned,
            std::path::Path::new("."),
            &wildcard_args,
            "pinned wildcard",
        );
        assert_eq!(
            wildcard_zmin, wildcard_stock,
            "literal wildcard SHA-256={sha256}"
        );
        assert_eq!(
            wildcard_zmin.status, 0,
            "literal wildcard status SHA-256={sha256}"
        );
        assert_eq!(
            wildcard_zmin.stdout, b"create-a-file\n",
            "literal wildcard stdout SHA-256={sha256}"
        );
        assert!(
            wildcard_zmin.stderr.is_empty(),
            "literal wildcard stderr SHA-256={sha256}"
        );

        for mode in [
            "--patch",
            "--stat",
            "--name-status",
            "--raw",
            "-Scontent",
            "-Gcontent",
        ] {
            let args = [
                "-C",
                zmin_repo_path.as_str(),
                "log",
                "--follow",
                "--format=%s",
                mode,
                "--",
                "new-file.txt",
            ];
            let output = command_raw_output(
                zmin_bin(),
                std::path::Path::new("."),
                &args,
                "unsupported follow",
            );
            assert_eq!(
                output.status, 128,
                "unsupported follow status {mode} SHA-256={sha256}"
            );
            assert!(
                output.stdout.is_empty(),
                "unsupported follow stdout {mode} SHA-256={sha256}"
            );
            assert_eq!(
                output.stderr, b"fatal: --follow is not supported with this output mode\n",
                "unsupported follow stderr {mode} SHA-256={sha256}"
            );
        }
    }
}

#[test]
fn follow_directory_rename_is_not_file_lineage_sha1_and_sha256() {
    let pinned = required_pinned_stock_git();
    let pinned = pinned.to_str().expect("pinned Git path utf8");
    for sha256 in [false, true] {
        let repo = pinned_follow_directory_rename_fixture(sha256);
        for path in ["new-dir", "new-dir/file.txt"] {
            let args = [
                "-C",
                repo.path().to_str().expect("repo path utf8"),
                "log",
                "--follow",
                "--format=%s",
                "--",
                path,
            ];
            let zmin = command_raw_output(
                zmin_bin(),
                std::path::Path::new("."),
                &args,
                "zmin directory follow",
            );
            let stock = command_raw_output(
                pinned,
                std::path::Path::new("."),
                &args,
                "pinned directory follow",
            );
            assert_eq!(zmin, stock, "directory path {path} SHA-256={sha256}");
            assert_eq!(
                zmin.status, 0,
                "directory path status {path} SHA-256={sha256}"
            );
            if path == "new-dir" {
                assert_eq!(
                    zmin.stdout, b"rename-dir\n",
                    "directory rename must not follow SHA-256={sha256}"
                );
            } else {
                assert_eq!(
                    zmin.stdout, b"rename-dir\ncreate-dir\n",
                    "file path must follow SHA-256={sha256}"
                );
            }
            assert!(
                zmin.stderr.is_empty(),
                "directory path stderr {path} SHA-256={sha256}"
            );
        }
    }
}

fn assert_follow_merge_subjects(
    repo: &TempDir,
    pinned: &str,
    sha256: bool,
    extra_args: &[&str],
    expected: &[u8],
    label: &str,
) {
    let mut args = vec![
        "-C",
        repo.path().to_str().expect("follow merge repo path utf8"),
        "log",
        "--follow",
        "--format=%s",
    ];
    args.extend_from_slice(extra_args);
    args.extend(["--", "new-file.txt"]);
    let stock = command_raw_output(
        pinned,
        std::path::Path::new("."),
        &args,
        "pinned merge follow subjects",
    );
    let zmin = command_raw_output(
        zmin_bin(),
        std::path::Path::new("."),
        &args,
        "zmin merge follow subjects",
    );
    assert_eq!(zmin, stock, "{label} SHA-256={sha256} args={extra_args:?}");
    assert_eq!(zmin.status, 0, "{label} status SHA-256={sha256}");
    assert_eq!(zmin.stdout, expected, "{label} subjects SHA-256={sha256}");
    assert!(zmin.stderr.is_empty(), "{label} stderr SHA-256={sha256}");
}

fn assert_follow_merge_raw_tuple(
    repo: &TempDir,
    pinned: &str,
    sha256: bool,
    extra_args: &[&str],
    label: &str,
) {
    let mut args = vec![
        "-C",
        repo.path().to_str().expect("follow merge repo path utf8"),
        "log",
        "--follow",
        "--format=%H:%P:%s",
    ];
    args.extend_from_slice(extra_args);
    args.extend(["--", "new-file.txt"]);
    let stock = command_raw_output(
        pinned,
        std::path::Path::new("."),
        &args,
        "pinned merge follow tuple",
    );
    let zmin = command_raw_output(
        zmin_bin(),
        std::path::Path::new("."),
        &args,
        "zmin merge follow tuple",
    );
    assert_eq!(zmin, stock, "{label} SHA-256={sha256} args={extra_args:?}");
    assert_eq!(zmin.status, 0, "{label} tuple status SHA-256={sha256}");
    assert!(
        zmin.stderr.is_empty(),
        "{label} tuple stderr SHA-256={sha256}"
    );
}

fn assert_follow_merge_reverse_matches_stock(
    repo: &TempDir,
    pinned: &str,
    sha256: bool,
    label: &str,
) {
    let args = [
        "-C",
        repo.path().to_str().expect("follow merge repo path utf8"),
        "log",
        "--follow",
        "--reverse",
        "--format=%H:%P:%s",
        "--",
        "new-file.txt",
    ];
    let stock = command_raw_output(
        pinned,
        std::path::Path::new("."),
        &args,
        "pinned merge follow reverse",
    );
    let zmin = command_raw_output(
        zmin_bin(),
        std::path::Path::new("."),
        &args,
        "zmin merge follow reverse",
    );
    assert_eq!(zmin, stock, "{label} SHA-256={sha256}");
    assert_eq!(zmin.status, 0, "{label} status SHA-256={sha256}");
    assert!(!zmin.stdout.is_empty(), "{label} output SHA-256={sha256}");
    assert!(zmin.stderr.is_empty(), "{label} stderr SHA-256={sha256}");
}

fn assert_follow_merge_preserves_original_parents(repo: &TempDir, pinned: &str, sha256: bool) {
    let repo_path = repo.path().to_str().expect("follow merge repo path utf8");
    let merge = command_raw_output(
        pinned,
        std::path::Path::new("."),
        &["-C", repo_path, "rev-parse", "HEAD"],
        "pinned follow merge id",
    );
    let parent1 = command_raw_output(
        pinned,
        std::path::Path::new("."),
        &["-C", repo_path, "rev-parse", "HEAD^1"],
        "pinned follow merge first parent",
    );
    let parent2 = command_raw_output(
        pinned,
        std::path::Path::new("."),
        &["-C", repo_path, "rev-parse", "HEAD^2"],
        "pinned follow merge second parent",
    );
    let merge = String::from_utf8(merge.stdout)
        .expect("merge id utf8")
        .trim()
        .to_owned();
    let parent1 = String::from_utf8(parent1.stdout)
        .expect("first parent utf8")
        .trim()
        .to_owned();
    let parent2 = String::from_utf8(parent2.stdout)
        .expect("second parent utf8")
        .trim()
        .to_owned();
    let args = [
        "-C",
        repo_path,
        "log",
        "--follow",
        "--first-parent",
        "--format=%H:%P:%s",
        "--",
        "new-file.txt",
    ];
    let stock = command_raw_output(
        pinned,
        std::path::Path::new("."),
        &args,
        "pinned follow merge original parents",
    );
    let zmin = command_raw_output(
        zmin_bin(),
        std::path::Path::new("."),
        &args,
        "zmin follow merge original parents",
    );
    assert_eq!(
        zmin, stock,
        "follow merge original parents SHA-256={sha256}"
    );
    let expected_prefix = format!("{merge}:{parent1} {parent2}:");
    assert!(
        stock
            .stdout
            .split(|byte| *byte == b'\n')
            .any(|line| line.starts_with(expected_prefix.as_bytes())),
        "shown merge must retain both original parents SHA-256={sha256}: {}",
        String::from_utf8_lossy(&stock.stdout)
    );
}

fn assert_no_follow_merge_subjects(
    repo: &TempDir,
    pinned: &str,
    sha256: bool,
    extra_args: &[&str],
    expected: &[u8],
    label: &str,
) {
    let mut args = vec![
        "-C",
        repo.path()
            .to_str()
            .expect("no-follow merge repo path utf8"),
        "log",
        "--format=%s",
    ];
    args.extend_from_slice(extra_args);
    args.extend(["--", "new-file.txt"]);
    let stock = command_raw_output(
        pinned,
        std::path::Path::new("."),
        &args,
        "pinned no-follow merge",
    );
    let zmin = command_raw_output(
        zmin_bin(),
        std::path::Path::new("."),
        &args,
        "zmin no-follow merge",
    );
    assert_eq!(zmin, stock, "{label} SHA-256={sha256}");
    assert_eq!(zmin.status, 0, "{label} status SHA-256={sha256}");
    assert_eq!(zmin.stdout, expected, "{label} subjects SHA-256={sha256}");
    assert!(zmin.stderr.is_empty(), "{label} stderr SHA-256={sha256}");
}

fn assert_no_follow_merge_raw_tuple(
    repo: &TempDir,
    pinned: &str,
    sha256: bool,
    extra_args: &[&str],
    label: &str,
) {
    let mut args = vec![
        "-C",
        repo.path()
            .to_str()
            .expect("no-follow merge tuple repo path utf8"),
        "log",
        "--format=%H:%P:%s",
    ];
    args.extend_from_slice(extra_args);
    args.extend(["--", "new-file.txt"]);
    let stock = command_raw_output(
        pinned,
        std::path::Path::new("."),
        &args,
        "pinned no-follow merge tuple",
    );
    let zmin = command_raw_output(
        zmin_bin(),
        std::path::Path::new("."),
        &args,
        "zmin no-follow merge tuple",
    );
    assert_eq!(zmin, stock, "{label} SHA-256={sha256}");
    assert_eq!(zmin.status, 0, "{label} status SHA-256={sha256}");
    assert!(
        zmin.stdout.ends_with(b"\n") && !zmin.stdout.is_empty(),
        "{label} output SHA-256={sha256}"
    );
    assert!(zmin.stderr.is_empty(), "{label} stderr SHA-256={sha256}");
}

#[test]
fn follow_merge_history_uses_edge_specific_paths_sha1_and_sha256() {
    let pinned = required_pinned_stock_git();
    let pinned = pinned.to_str().expect("pinned Git path utf8");
    for sha256 in [false, true] {
        let f1 = pinned_follow_merge_fixture(sha256);
        assert_follow_merge_subjects(
            &f1,
            pinned,
            sha256,
            &[],
            b"rename-the-file\ncreate-a-file\n",
            "F1 default follow",
        );
        assert_follow_merge_subjects(
            &f1,
            pinned,
            sha256,
            &["--first-parent"],
            b"rename-the-file\ncreate-a-file\n",
            "F1 first-parent follow",
        );
        assert_follow_merge_subjects(
            &f1,
            pinned,
            sha256,
            &["--full-history"],
            b"rename-the-file\ncreate-a-file\n",
            "F1 full-history follow",
        );
        assert_follow_merge_subjects(
            &f1,
            pinned,
            sha256,
            &["--full-history", "--first-parent"],
            b"rename-the-file\ncreate-a-file\n",
            "F1 full-history first-parent follow",
        );
        assert_follow_merge_raw_tuple(&f1, pinned, sha256, &[], "F1 default tuple");
        assert_follow_merge_raw_tuple(&f1, pinned, sha256, &["--parents"], "F1 parents tuple");
        for extra_args in [
            &["--first-parent"][..],
            &["--full-history"][..],
            &["--full-history", "--first-parent"][..],
            &["--parents", "--first-parent"][..],
            &["--parents", "--full-history"][..],
            &["--parents", "--full-history", "--first-parent"][..],
        ] {
            assert_follow_merge_raw_tuple(&f1, pinned, sha256, extra_args, "F1 mode tuple");
        }
        for extra_args in [&["--topo-order"][..], &["--date-order"][..]] {
            assert_follow_merge_raw_tuple(&f1, pinned, sha256, extra_args, "F1 order tuple");
        }
        assert_follow_merge_reverse_matches_stock(&f1, pinned, sha256, "F1 reverse follow");
        assert_no_follow_merge_subjects(
            &f1,
            pinned,
            sha256,
            &[],
            b"rename-the-file\n",
            "F1 no-follow default",
        );
        assert_no_follow_merge_subjects(
            &f1,
            pinned,
            sha256,
            &["--first-parent"],
            b"rename-the-file\n",
            "F1 no-follow first-parent",
        );
        assert_no_follow_merge_subjects(
            &f1,
            pinned,
            sha256,
            &["--full-history"],
            b"merge\nrename-the-file\n",
            "F1 no-follow full-history",
        );
        assert_no_follow_merge_subjects(
            &f1,
            pinned,
            sha256,
            &["--full-history", "--first-parent"],
            b"rename-the-file\n",
            "F1 no-follow full-history first-parent",
        );
        for extra_args in [
            &[][..],
            &["--first-parent"][..],
            &["--full-history"][..],
            &["--full-history", "--first-parent"][..],
        ] {
            assert_no_follow_merge_raw_tuple(
                &f1,
                pinned,
                sha256,
                extra_args,
                "F1 no-follow raw tuple",
            );
        }

        let f2 = pinned_follow_merge_second_parent_fixture(sha256);
        assert_follow_merge_subjects(
            &f2,
            pinned,
            sha256,
            &[],
            b"rename-the-file\ncreate-a-file\n",
            "F2 default follow",
        );
        assert_follow_merge_subjects(
            &f2,
            pinned,
            sha256,
            &["--first-parent"],
            b"merge\ncreate-a-file\n",
            "F2 first-parent follow",
        );
        assert_follow_merge_subjects(
            &f2,
            pinned,
            sha256,
            &["--full-history"],
            b"rename-the-file\ncreate-a-file\n",
            "F2 full-history follow",
        );
        assert_follow_merge_subjects(
            &f2,
            pinned,
            sha256,
            &["--full-history", "--first-parent"],
            b"merge\ncreate-a-file\n",
            "F2 full-history first-parent follow",
        );
        assert_follow_merge_raw_tuple(&f2, pinned, sha256, &[], "F2 default tuple");
        assert_follow_merge_raw_tuple(&f2, pinned, sha256, &["--parents"], "F2 parents tuple");
        for extra_args in [
            &["--full-history"][..],
            &["--full-history", "--first-parent"][..],
            &["--parents", "--full-history"][..],
            &["--parents", "--full-history", "--first-parent"][..],
        ] {
            assert_follow_merge_raw_tuple(&f2, pinned, sha256, extra_args, "F2 mode tuple");
        }
        assert_follow_merge_raw_tuple(
            &f2,
            pinned,
            sha256,
            &["--first-parent"],
            "F2 first-parent tuple",
        );
        assert_follow_merge_raw_tuple(
            &f2,
            pinned,
            sha256,
            &["--full-history", "--first-parent", "--parents"],
            "F2 full first-parent parents tuple",
        );
        for extra_args in [&["--topo-order"][..], &["--date-order"][..]] {
            assert_follow_merge_raw_tuple(&f2, pinned, sha256, extra_args, "F2 order tuple");
        }
        assert_follow_merge_reverse_matches_stock(&f2, pinned, sha256, "F2 reverse follow");
        assert_follow_merge_preserves_original_parents(&f2, pinned, sha256);
        assert_no_follow_merge_subjects(
            &f2,
            pinned,
            sha256,
            &[],
            b"rename-the-file\n",
            "F2 no-follow default",
        );
        assert_no_follow_merge_subjects(
            &f2,
            pinned,
            sha256,
            &["--first-parent"],
            b"merge\n",
            "F2 no-follow first-parent",
        );
        assert_no_follow_merge_subjects(
            &f2,
            pinned,
            sha256,
            &["--full-history"],
            b"merge\nrename-the-file\n",
            "F2 no-follow full-history",
        );
        assert_no_follow_merge_subjects(
            &f2,
            pinned,
            sha256,
            &["--full-history", "--first-parent"],
            b"merge\n",
            "F2 no-follow full-history first-parent",
        );
        for extra_args in [
            &[][..],
            &["--first-parent"][..],
            &["--full-history"][..],
            &["--full-history", "--first-parent"][..],
        ] {
            assert_no_follow_merge_raw_tuple(
                &f2,
                pinned,
                sha256,
                extra_args,
                "F2 no-follow raw tuple",
            );
        }
    }
}

fn assert_follow_merge_extended_matrix(repo: &TempDir, pinned: &str, sha256: bool, label: &str) {
    let mode_args: [(&str, &[&str]); 7] = [
        ("default", &[]),
        ("first-parent", &["--first-parent"]),
        ("full-history", &["--full-history"]),
        (
            "full-history-first-parent",
            &["--full-history", "--first-parent"],
        ),
        ("topo-order", &["--topo-order"]),
        ("date-order", &["--date-order"]),
        ("reverse", &["--reverse"]),
    ];
    for (mode, extra_args) in mode_args {
        let mut subject_args = vec![
            "-C",
            repo.path()
                .to_str()
                .expect("extended follow repo path utf8"),
            "log",
            "--follow",
            "--format=%s",
        ];
        subject_args.extend_from_slice(extra_args);
        subject_args.extend(["--", "new-file.txt"]);
        let stock = command_raw_output(
            pinned,
            std::path::Path::new("."),
            &subject_args,
            "pinned extended follow subjects",
        );
        let zmin = command_raw_output(
            zmin_bin(),
            std::path::Path::new("."),
            &subject_args,
            "zmin extended follow subjects",
        );
        assert_eq!(zmin, stock, "{label} {mode} SHA-256={sha256}");
        assert_eq!(zmin.status, 0, "{label} {mode} status SHA-256={sha256}");
        assert!(
            !zmin.stdout.is_empty(),
            "{label} {mode} output SHA-256={sha256}"
        );
        assert!(
            zmin.stdout.ends_with(b"\n")
                && zmin.stdout[..zmin.stdout.len() - 1]
                    .split(|byte| *byte == b'\n')
                    .all(|line| !line.is_empty()),
            "{label} {mode} subject structure SHA-256={sha256}"
        );
        assert!(
            zmin.stderr.is_empty(),
            "{label} {mode} stderr SHA-256={sha256}"
        );

        let mut tuple_args = vec![
            "-C",
            repo.path()
                .to_str()
                .expect("extended follow tuple repo path utf8"),
            "log",
            "--follow",
            "--format=%H:%P:%s",
        ];
        tuple_args.extend_from_slice(extra_args);
        tuple_args.extend(["--", "new-file.txt"]);
        let stock = command_raw_output(
            pinned,
            std::path::Path::new("."),
            &tuple_args,
            "pinned extended follow tuple",
        );
        let zmin = command_raw_output(
            zmin_bin(),
            std::path::Path::new("."),
            &tuple_args,
            "zmin extended follow tuple",
        );
        assert_eq!(zmin, stock, "{label} {mode} tuple SHA-256={sha256}");
        assert_eq!(
            zmin.status, 0,
            "{label} {mode} tuple status SHA-256={sha256}"
        );
        assert!(
            zmin.stderr.is_empty(),
            "{label} {mode} tuple stderr SHA-256={sha256}"
        );
    }

    for extra_args in [
        &[][..],
        &["--first-parent"][..],
        &["--full-history"][..],
        &["--full-history", "--first-parent"][..],
    ] {
        let mut args = vec![
            "-C",
            repo.path()
                .to_str()
                .expect("extended follow parents repo path utf8"),
            "log",
            "--follow",
            "--parents",
            "--format=%H:%P:%s",
        ];
        args.extend_from_slice(extra_args);
        args.extend(["--", "new-file.txt"]);
        let stock = command_raw_output(
            pinned,
            std::path::Path::new("."),
            &args,
            "pinned extended follow parents",
        );
        let zmin = command_raw_output(
            zmin_bin(),
            std::path::Path::new("."),
            &args,
            "zmin extended follow parents",
        );
        assert_eq!(zmin, stock, "{label} parents SHA-256={sha256}");
        assert_eq!(zmin.status, 0, "{label} parents status SHA-256={sha256}");
    }

    for extra_args in [&["--reverse"][..], &["--reverse", "--first-parent"][..]] {
        let mut args = vec![
            "-C",
            repo.path()
                .to_str()
                .expect("extended no-follow repo path utf8"),
            "log",
            "--format=%H:%P:%s",
        ];
        args.extend_from_slice(extra_args);
        args.extend(["--", "new-file.txt"]);
        let stock = command_raw_output(
            pinned,
            std::path::Path::new("."),
            &args,
            "pinned extended no-follow",
        );
        let zmin = command_raw_output(
            zmin_bin(),
            std::path::Path::new("."),
            &args,
            "zmin extended no-follow",
        );
        assert_eq!(zmin, stock, "{label} no-follow SHA-256={sha256}");
        assert_eq!(zmin.status, 0, "{label} no-follow status SHA-256={sha256}");
        assert!(
            !zmin.stdout.is_empty(),
            "{label} no-follow output SHA-256={sha256}"
        );
        assert!(
            zmin.stderr.is_empty(),
            "{label} no-follow stderr SHA-256={sha256}"
        );
    }
}

#[test]
fn follow_merge_changed_both_and_delete_readd_match_stock_sha1_and_sha256() {
    let pinned = required_pinned_stock_git();
    let pinned = pinned.to_str().expect("pinned Git path utf8");
    for sha256 in [false, true] {
        let changed_both = pinned_follow_merge_changed_both_fixture(sha256);
        assert_follow_merge_extended_matrix(&changed_both, pinned, sha256, "changed-both");

        let delete_readd = pinned_follow_merge_delete_readd_fixture(sha256);
        assert_follow_merge_extended_matrix(&delete_readd, pinned, sha256, "delete-readd");
    }
}

#[test]
fn follow_linear_reverse_matches_stock_git_sha1_and_sha256() {
    let pinned = required_pinned_stock_git();
    let pinned = pinned.to_str().expect("pinned Git path utf8");
    for sha256 in [false, true] {
        let repo = pinned_follow_linear_fixture(sha256);
        let args = [
            "-C",
            repo.path().to_str().expect("follow linear repo path utf8"),
            "log",
            "--follow",
            "--reverse",
            "--format=%s",
            "--",
            "new-file.txt",
        ];
        let stock = command_raw_output(
            pinned,
            std::path::Path::new("."),
            &args,
            "pinned linear reverse follow",
        );
        let zmin = command_raw_output(
            zmin_bin(),
            std::path::Path::new("."),
            &args,
            "zmin linear reverse follow",
        );
        assert_eq!(zmin, stock, "linear reverse follow SHA-256={sha256}");
        assert_eq!(zmin.status, 0, "linear reverse status SHA-256={sha256}");
        assert_eq!(zmin.stdout, b"rename-the-file\n");
        assert!(zmin.stderr.is_empty());

        let creation_repo = pinned_follow_linear_creation_fixture(sha256);
        for extra_args in [&["--reverse"][..], &["--reverse", "--first-parent"][..]] {
            let mut raw_args = vec![
                "-C",
                creation_repo
                    .path()
                    .to_str()
                    .expect("linear creation repo path utf8"),
                "log",
                "--follow",
            ];
            raw_args.extend_from_slice(extra_args);
            raw_args.extend(["--format=%H:%P:%s", "--", "new-file.txt"]);
            let stock = command_raw_output(
                pinned,
                std::path::Path::new("."),
                &raw_args,
                "pinned linear creation reverse tuple",
            );
            let zmin = command_raw_output(
                zmin_bin(),
                std::path::Path::new("."),
                &raw_args,
                "zmin linear creation reverse tuple",
            );
            assert_eq!(zmin, stock, "linear creation tuple SHA-256={sha256}");
            assert_eq!(zmin.status, 0, "linear creation status SHA-256={sha256}");
            assert!(
                String::from_utf8_lossy(&zmin.stdout).contains(":create-file\n")
                    && String::from_utf8_lossy(&zmin.stdout).contains(":edit1\n")
                    && String::from_utf8_lossy(&zmin.stdout).contains(":edit2\n"),
                "linear creation reverse rows SHA-256={sha256}: {}",
                String::from_utf8_lossy(&zmin.stdout)
            );
            assert!(zmin.stderr.is_empty());
        }
    }
}

#[test]
fn follow_reverse_rename_stops_before_post_rename_edit_sha1_and_sha256() {
    let pinned = required_pinned_stock_git();
    let pinned = pinned.to_str().expect("pinned Git path utf8");
    for sha256 in [false, true] {
        let repo = pinned_follow_rename_then_edit_fixture(sha256);
        let args = [
            "-C",
            repo.path().to_str().expect("rename edit repo path utf8"),
            "log",
            "--follow",
            "--reverse",
            "--format=%H:%P:%s",
            "--",
            "new-file.txt",
        ];
        let stock = command_raw_output(
            pinned,
            std::path::Path::new("."),
            &args,
            "pinned reverse rename edit tuple",
        );
        let zmin = command_raw_output(
            zmin_bin(),
            std::path::Path::new("."),
            &args,
            "zmin reverse rename edit tuple",
        );
        assert_eq!(zmin, stock, "reverse rename edit SHA-256={sha256}");
        assert_eq!(
            zmin.status, 0,
            "reverse rename edit status SHA-256={sha256}"
        );
        assert_eq!(
            zmin.stdout
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .count(),
            1,
            "reverse rename edit must stop at rename SHA-256={sha256}"
        );
        assert!(
            String::from_utf8_lossy(&zmin.stdout).contains(":rename-the-file\n"),
            "reverse rename row SHA-256={sha256}: {}",
            String::from_utf8_lossy(&zmin.stdout)
        );
        assert!(zmin.stderr.is_empty());
    }
}

#[test]
fn follow_reverse_rename_delete_readd_stops_at_rename_sha1_and_sha256() {
    let pinned = required_pinned_stock_git();
    let pinned = pinned.to_str().expect("pinned Git path utf8");
    for sha256 in [false, true] {
        let repo = pinned_follow_rename_delete_readd_fixture(sha256);
        let args = [
            "-C",
            repo.path()
                .to_str()
                .expect("rename delete readd repo path utf8"),
            "log",
            "--follow",
            "--reverse",
            "--format=%H:%P:%s",
            "--",
            "new-file.txt",
        ];
        let stock = command_raw_output(
            pinned,
            std::path::Path::new("."),
            &args,
            "pinned reverse rename delete readd tuple",
        );
        let zmin = command_raw_output(
            zmin_bin(),
            std::path::Path::new("."),
            &args,
            "zmin reverse rename delete readd tuple",
        );
        assert_eq!(zmin, stock, "reverse rename delete readd SHA-256={sha256}");
        assert_eq!(
            zmin.status, 0,
            "reverse rename delete readd status SHA-256={sha256}"
        );
        assert_eq!(
            zmin.stdout
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .count(),
            1,
            "reverse rename delete readd must stop at rename SHA-256={sha256}"
        );
        assert!(
            String::from_utf8_lossy(&zmin.stdout).contains(":rename-the-file\n"),
            "reverse rename delete readd row SHA-256={sha256}: {}",
            String::from_utf8_lossy(&zmin.stdout)
        );
        assert!(zmin.stderr.is_empty());
    }
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
fn log_metadata_fast_path_record_termination_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["commit", "--allow-empty", "-m", "one"]);
    git(repo.path(), ["commit", "--allow-empty", "-m", "two"]);

    for args in [
        ["log", "-1", "--format=%ad"].as_slice(),
        ["log", "-2", "--format=%H"].as_slice(),
        ["log", "-1", "--pretty=format:%ad"].as_slice(),
    ] {
        assert_eq!(
            command_stdout_bytes(zmin_bin(), repo.path(), args),
            command_stdout_bytes(
                stock_git_bin().to_str().expect("stock Git path"),
                repo.path(),
                args
            ),
            "raw stdout mismatch for {args:?}"
        );
    }
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

#[test]
fn active_algorithm_objectish_and_pseudoref_resolution_matches_pinned_git() {
    for sha256 in [false, true] {
        let repo = pinned_history_fixture(sha256);
        let algorithm = if sha256 {
            GitHashAlgorithm::Sha256
        } else {
            GitHashAlgorithm::Sha1
        };
        let head = pinned_git_args(repo.path(), &["rev-parse", "HEAD"]);
        let first_parent = pinned_git_args(repo.path(), &["rev-parse", "HEAD^1"]);
        let second_parent = pinned_git_args(repo.path(), &["rev-parse", "HEAD^2"]);
        let annotated = pinned_git_args(repo.path(), &["rev-parse", "annotated"]);
        let lightweight = pinned_git_args(repo.path(), &["rev-parse", "lightweight"]);
        let loose_blob = pinned_git_args(repo.path(), &["hash-object", "loose.txt"]);
        let candidates = vec![
            head.clone(),
            first_parent.clone(),
            second_parent.clone(),
            annotated.clone(),
            lightweight.clone(),
            loose_blob.clone(),
        ];
        let unique_four = unique_prefix(repo.path(), &candidates, 4);
        let unique_seven = unique_prefix(repo.path(), &candidates, 7);
        let unique_twelve = unique_prefix(repo.path(), &candidates, 12);
        let unique_forty = if sha256 {
            Some(unique_prefix(repo.path(), &candidates, 40))
        } else {
            None
        };
        let (ambiguous_id, _) = collision_pair(repo.path(), algorithm);
        let ambiguous_prefix = ambiguous_id[..4].to_owned();
        let missing = missing_prefix(repo.path());

        for objectish in [
            head.clone(),
            first_parent.clone(),
            second_parent.clone(),
            annotated.clone(),
            lightweight.clone(),
            loose_blob.clone(),
            unique_four,
            unique_seven,
            unique_twelve,
        ]
        .into_iter()
        .chain(unique_forty)
        {
            assert_history_tuple(repo.path(), &["rev-parse", objectish.as_str()]);
        }
        for objectish in [
            "HEAD^",
            "HEAD^2",
            "HEAD~2",
            "annotated^{}",
            "HEAD@{0}",
            "HEAD@{1}",
            "main",
            "side",
            "loose",
        ] {
            assert_history_tuple(repo.path(), &["rev-parse", objectish]);
        }
        // The resolver is active-algorithm correct, but its existing CLI error
        // formatter does not yet reproduce Git's candidate-hint diagnostic.
        // Keep this bounded evidence explicit while that separate P1 remains queued.
        assert_known_ambiguous_prefix_gap(repo.path(), ambiguous_prefix.as_str());
        // Git echoes a missing short object token on stdout; Zmin's existing
        // generic revision formatter does not. Keep this separate from the
        // active-algorithm resolver parity assertions as a queued P1.
        assert_known_missing_prefix_gap(repo.path(), missing.as_str());

        for args in [
            vec!["show", "--no-patch", "--format=%H|%P|%s", "HEAD"],
            vec!["show", "--no-patch", "--format=%H|%P|%s", "HEAD^2"],
            vec!["log", "--no-walk", "--format=%H|%P|%s", "HEAD"],
            vec!["log", "--no-walk", "--format=%H|%P|%s", "HEAD^2"],
        ] {
            assert_history_tuple(repo.path(), args.as_slice());
        }
        for args in [
            vec!["cat-file", "-t", head.as_str()],
            vec!["cat-file", "-p", loose_blob.as_str()],
        ] {
            assert_history_tuple(repo.path(), args.as_slice());
        }

        fs::write(repo.path().join(".git/ORIG_HEAD"), format!("{head}\n"))
            .expect("write valid ORIG_HEAD");
        fs::write(
            repo.path().join(".git/MERGE_HEAD"),
            format!("{second_parent}\n"),
        )
        .expect("write valid MERGE_HEAD");
        fs::write(
            repo.path().join(".git/FETCH_HEAD"),
            format!(
                "{second_parent}\tnot-for-merge branch 'side'\n{head}\t\tmerge branch 'main'\n"
            ),
        )
        .expect("write valid FETCH_HEAD");
        for pseudo in ["ORIG_HEAD", "MERGE_HEAD", "FETCH_HEAD"] {
            assert_pseudoref_tuple(repo.path(), pseudo);
        }

        let wrong_width = if sha256 {
            "a".repeat(40)
        } else {
            "a".repeat(64)
        };
        for pseudo in ["ORIG_HEAD", "MERGE_HEAD"] {
            fs::write(
                repo.path().join(format!(".git/{pseudo}")),
                format!("{wrong_width}\n"),
            )
            .expect("write malformed pseudo-ref");
            assert_known_missing_prefix_gap(repo.path(), pseudo);
        }
        fs::write(
            repo.path().join(".git/FETCH_HEAD"),
            b"not-an-object\tnot-for-merge\n",
        )
        .expect("write malformed FETCH_HEAD");
        assert_known_missing_prefix_gap(repo.path(), "FETCH_HEAD");
    }
}
