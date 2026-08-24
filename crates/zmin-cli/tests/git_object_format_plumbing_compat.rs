mod common;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use tempfile::{Builder, TempDir};
use zmin_git_core::GitHashAlgorithm;

use common::{required_pinned_stock_git, zmin_bin};

const GLOBAL_CONFIG_NULL: &str = "/dev/null";
const CANONICAL_CA83_ROOT: &str = "/Users/dschewchenko/.codex/worktrees/ca83/skron-git";
const CANONICAL_MAIN_ROOT: &str = "/Users/dschewchenko/work/private/skron-git";

fn assert_expected_canonical_root() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .canonicalize()
        .expect("canonicalize workspace root");
    assert!(
        root == Path::new(CANONICAL_CA83_ROOT) || root == Path::new(CANONICAL_MAIN_ROOT),
        "unexpected canonical workspace root {}; expected {} or {}",
        root.display(),
        CANONICAL_CA83_ROOT,
        CANONICAL_MAIN_ROOT
    );
}

fn run(program: &Path, cwd: &Path, args: &[&str], stdin: Option<&[u8]>) -> Output {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", GLOBAL_CONFIG_NULL)
        .env("GIT_CONFIG_SYSTEM", GLOBAL_CONFIG_NULL)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Bench")
        .env("GIT_AUTHOR_EMAIL", "bench@example.test")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_NAME", "Bench")
        .env("GIT_COMMITTER_EMAIL", "bench@example.test")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000");
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn {}: {error}", program.display()));
    if let Some(stdin) = stdin {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("stdin pipe")
            .write_all(stdin)
            .expect("write command stdin");
    }
    child.wait_with_output().expect("wait for command")
}

fn assert_success(output: Output, label: &str) -> String {
    assert!(
        output.status.success(),
        "{label} failed with {}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("command stdout utf8")
        .trim()
        .to_owned()
}

fn assert_failure(output: Output, label: &str) {
    assert!(
        !output.status.success(),
        "{label} unexpectedly succeeded\nstdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn init_repo(program: &Path, repo: &Path, sha256: bool, reftable: bool) {
    let mut args = vec!["init", "--quiet"];
    if sha256 {
        args.push("--object-format=sha256");
    }
    if reftable {
        args.push("--ref-format=reftable");
    }
    assert_success(run(program, repo, &args, None), "git init");
    assert_success(
        run(program, repo, &["config", "user.name", "Bench"], None),
        "git config user.name",
    );
    assert_success(
        run(
            program,
            repo,
            &["config", "user.email", "bench@example.test"],
            None,
        ),
        "git config user.email",
    );
}

fn loose_object_path(repo: &Path, id: &str) -> PathBuf {
    repo.join(".git/objects").join(&id[..2]).join(&id[2..])
}

fn fixture_root() -> TempDir {
    Builder::new()
        .prefix("zmin-object-format-plumbing-")
        .tempdir_in("/private/tmp")
        .expect("create unique external fixture root")
}

fn run_matrix(sha256: bool, reftable: bool, stock: &Path) {
    let root = fixture_root();
    let stock_repo = root.path().join("stock");
    let zmin_repo = root.path().join("zmin");
    fs::create_dir_all(&stock_repo).expect("create stock repo root");
    fs::create_dir_all(&zmin_repo).expect("create zmin repo root");
    init_repo(stock, &stock_repo, sha256, reftable);
    init_repo(stock, &zmin_repo, sha256, reftable);

    fs::write(stock_repo.join("tracked.txt"), b"base\n").expect("write stock base");
    fs::write(zmin_repo.join("tracked.txt"), b"base\n").expect("write zmin base");
    assert_success(
        run(stock, &stock_repo, &["add", "tracked.txt"], None),
        "stock add base",
    );
    assert_success(
        run(stock, &zmin_repo, &["add", "tracked.txt"], None),
        "stock add zmin base",
    );
    assert_success(
        run(stock, &stock_repo, &["commit", "-m", "base"], None),
        "stock base commit",
    );
    assert_success(
        run(stock, &zmin_repo, &["commit", "-m", "base"], None),
        "stock zmin base commit",
    );

    fs::write(stock_repo.join("tracked.txt"), b"next\n").expect("write stock next");
    fs::write(zmin_repo.join("tracked.txt"), b"next\n").expect("write zmin next");
    assert_success(
        run(stock, &stock_repo, &["add", "tracked.txt"], None),
        "stock add next",
    );
    assert_success(
        run(stock, &zmin_repo, &["add", "tracked.txt"], None),
        "stock add zmin next",
    );

    let stock_tree = assert_success(
        run(stock, &stock_repo, &["write-tree"], None),
        "stock write-tree",
    );
    let zmin = Path::new(zmin_bin());
    let zmin_tree = assert_success(
        run(zmin, &zmin_repo, &["write-tree"], None),
        "zmin write-tree",
    );
    let algorithm = if sha256 {
        GitHashAlgorithm::Sha256
    } else {
        GitHashAlgorithm::Sha1
    };
    assert_eq!(zmin_tree, stock_tree, "write-tree object mismatch");
    assert_eq!(zmin_tree.len(), algorithm.digest_len() * 2);

    let cache_path = zmin_repo.join(".git/zmin/write-tree-cache-v2");
    assert!(cache_path.is_file(), "v2 write-tree cache should exist");
    assert!(
        !zmin_repo.join(".git/zmin/write-tree-cache-v1").exists(),
        "SHA-1-only cache name must not be retained"
    );
    let cache_lines = fs::read_to_string(&cache_path)
        .expect("read write-tree cache")
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(cache_lines.len(), 2);
    assert_eq!(cache_lines[1], zmin_tree);
    assert_eq!(cache_lines[0].len(), algorithm.digest_len() * 2);

    assert_eq!(
        assert_success(
            run(zmin, &zmin_repo, &["write-tree"], None),
            "zmin cached write-tree"
        ),
        zmin_tree
    );

    let zmin_loose_tree = loose_object_path(&zmin_repo, &zmin_tree);
    fs::remove_file(&zmin_loose_tree).expect("remove cached tree object");
    assert_eq!(
        assert_success(
            run(zmin, &zmin_repo, &["write-tree"], None),
            "zmin missing-tree rebuild"
        ),
        zmin_tree
    );
    assert!(
        zmin_loose_tree.is_file(),
        "missing cached tree must be rebuilt"
    );

    let wrong_width = if sha256 { 40 } else { 64 };
    fs::write(
        &cache_path,
        format!("{}\n{}\n", "0".repeat(wrong_width), "0".repeat(wrong_width)),
    )
    .expect("write wrong-width cache");
    assert_eq!(
        assert_success(
            run(zmin, &zmin_repo, &["write-tree"], None),
            "zmin wrong-width cache"
        ),
        zmin_tree
    );
    let repaired_cache = fs::read_to_string(&cache_path).expect("read repaired cache");
    assert_eq!(
        repaired_cache.lines().next().unwrap().len(),
        algorithm.digest_len() * 2
    );

    fs::write(stock_repo.join("tracked.txt"), b"changed\n").expect("write stock changed");
    fs::write(zmin_repo.join("tracked.txt"), b"changed\n").expect("write zmin changed");
    assert_success(
        run(stock, &stock_repo, &["add", "tracked.txt"], None),
        "stock add changed",
    );
    assert_success(
        run(stock, &zmin_repo, &["add", "tracked.txt"], None),
        "stock add zmin changed",
    );
    let stock_changed_tree = assert_success(
        run(stock, &stock_repo, &["write-tree"], None),
        "stock invalidated write-tree",
    );
    let zmin_changed_tree = assert_success(
        run(zmin, &zmin_repo, &["write-tree"], None),
        "zmin invalidated write-tree",
    );
    assert_eq!(zmin_changed_tree, stock_changed_tree);
    assert_ne!(zmin_changed_tree, zmin_tree);

    let stock_commit = assert_success(
        run(
            stock,
            &stock_repo,
            &[
                "commit-tree",
                &stock_changed_tree,
                "-p",
                "HEAD",
                "-m",
                "child",
            ],
            None,
        ),
        "stock commit-tree with HEAD parent",
    );
    let zmin_commit = assert_success(
        run(
            zmin,
            &zmin_repo,
            &[
                "commit-tree",
                &zmin_changed_tree,
                "-p",
                "HEAD",
                "-m",
                "child",
            ],
            None,
        ),
        "zmin commit-tree with HEAD parent",
    );
    assert_eq!(zmin_commit, stock_commit);
    assert_eq!(zmin_commit.len(), algorithm.digest_len() * 2);

    let blob_input = b"real mktree blob\n";
    let stock_blob = assert_success(
        run(
            stock,
            &stock_repo,
            &["hash-object", "-w", "--stdin"],
            Some(blob_input),
        ),
        "stock hash-object blob",
    );
    let zmin_blob = assert_success(
        run(
            zmin,
            &zmin_repo,
            &["hash-object", "-w", "--stdin"],
            Some(blob_input),
        ),
        "zmin hash-object blob",
    );
    assert_eq!(zmin_blob, stock_blob);
    let mktree_input = format!("100644 blob {stock_blob}\tfile.txt\n");
    let stock_mktree = assert_success(
        run(
            stock,
            &stock_repo,
            &["mktree"],
            Some(mktree_input.as_bytes()),
        ),
        "stock mktree",
    );
    let zmin_mktree = assert_success(
        run(zmin, &zmin_repo, &["mktree"], Some(mktree_input.as_bytes())),
        "zmin mktree",
    );
    assert_eq!(zmin_mktree, stock_mktree);
    assert_eq!(zmin_mktree.len(), algorithm.digest_len() * 2);

    let missing_id = "f".repeat(algorithm.digest_len() * 2);
    let missing_input = format!("100644 blob {missing_id}\tmissing.txt\n");
    assert_failure(
        run(
            stock,
            &stock_repo,
            &["mktree"],
            Some(missing_input.as_bytes()),
        ),
        "stock missing-object mktree",
    );
    assert_failure(
        run(
            zmin,
            &zmin_repo,
            &["mktree"],
            Some(missing_input.as_bytes()),
        ),
        "zmin missing-object mktree",
    );

    let wrong_width_input = format!("100644 blob {}\twrong-width.txt\n", "0".repeat(wrong_width));
    assert_failure(
        run(
            stock,
            &stock_repo,
            &["mktree"],
            Some(wrong_width_input.as_bytes()),
        ),
        "stock wrong-width mktree",
    );
    assert_failure(
        run(
            zmin,
            &zmin_repo,
            &["mktree"],
            Some(wrong_width_input.as_bytes()),
        ),
        "zmin wrong-width mktree",
    );
}

#[test]
fn object_format_plumbing_matches_pinned_git_across_hashes_and_ref_backends() {
    assert_expected_canonical_root();
    let stock = required_pinned_stock_git();
    for sha256 in [false, true] {
        for reftable in [false, true] {
            run_matrix(sha256, reftable, &stock);
        }
    }
}
