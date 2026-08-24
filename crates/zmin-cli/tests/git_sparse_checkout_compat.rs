mod common;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use tempfile::TempDir;

use common::{
    clone_repo_fixture, command_any_output, configure_identity, git, git_failure_output, git_init,
    git_with_env, git_with_stdin, run_zmin, run_zmin_failure_output, run_zmin_with_stdin,
    visible_worktree_files, write_file, zmin_bin,
};

fn sparse_checkout_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "README.md", "readme\n");
    fs::create_dir_all(repo.path().join("docs")).expect("create docs");
    fs::create_dir_all(repo.path().join("src")).expect("create src");
    write_file(repo.path(), "docs/guide.md", "guide\n");
    write_file(repo.path(), "src/main.rs", "fn main() {}\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    repo
}

fn git_config_get(repo: &Path, key: &str) -> (i32, String, String) {
    let (status, stdout, stderr) = pinned_git_exact_output(repo, &["config", "--get", key]);
    (
        status,
        String::from_utf8(stdout).expect("pinned Git config stdout is UTF-8"),
        String::from_utf8(stderr).expect("pinned Git config stderr is UTF-8"),
    )
}

#[derive(Debug, PartialEq, Eq)]
struct SparseCheckoutCleanState {
    index: Vec<u8>,
    patterns: Vec<u8>,
    config: Vec<(i32, String, String)>,
    files: Vec<String>,
    staged: Vec<u8>,
}

fn pinned_git_path() -> PathBuf {
    let path = std::env::var_os("ZMIN_STOCK_GIT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .expect("set ZMIN_STOCK_GIT to the pinned Git v2.55.0 comparator");
    assert!(
        path.is_absolute(),
        "ZMIN_STOCK_GIT must be an absolute path to the pinned comparator"
    );
    let output = Command::new(&path)
        .arg("--version")
        .output()
        .expect("run pinned Git --version");
    assert!(output.status.success(), "pinned Git --version failed");
    assert_eq!(
        std::str::from_utf8(&output.stdout)
            .expect("pinned Git version is UTF-8")
            .trim(),
        "git version 2.55.0",
        "ZMIN_STOCK_GIT must be the pinned Git v2.55.0 comparator"
    );
    path
}

fn require_pinned_git_v2_55() {
    let _ = pinned_git_path();
}

fn exact_command_output(command: &Path, cwd: &Path, args: &[&str]) -> (i32, Vec<u8>, Vec<u8>) {
    let output = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run exact command");
    (
        output.status.code().expect("process exit code"),
        output.stdout,
        output.stderr,
    )
}

fn pinned_git_exact_output(cwd: &Path, args: &[&str]) -> (i32, Vec<u8>, Vec<u8>) {
    exact_command_output(&pinned_git_path(), cwd, args)
}

fn zmin_exact_output(cwd: &Path, args: &[&str]) -> (i32, Vec<u8>, Vec<u8>) {
    exact_command_output(Path::new(zmin_bin()), cwd, args)
}

fn sparse_checkout_clean_state(repo: &Path) -> SparseCheckoutCleanState {
    let config = [
        "core.sparseCheckout",
        "core.sparseCheckoutCone",
        "index.sparse",
        "clean.requireForce",
    ]
    .into_iter()
    .map(|key| git_config_get(repo, key))
    .collect();
    SparseCheckoutCleanState {
        index: fs::read(repo.join(".git/index")).expect("read index"),
        patterns: fs::read(repo.join(".git/info/sparse-checkout")).expect("read patterns"),
        config,
        files: visible_worktree_files(repo),
        staged: pinned_git_exact_output(repo, &["ls-files", "--stage"]).1,
    }
}

fn assert_sparse_checkout_metadata_unchanged(
    before: &SparseCheckoutCleanState,
    after: &SparseCheckoutCleanState,
) {
    assert_eq!(before.index, after.index, "clean mutated the index");
    assert_eq!(
        before.patterns, after.patterns,
        "clean mutated sparse patterns"
    );
    assert_eq!(before.config, after.config, "clean mutated sparse config");
    assert_eq!(before.staged, after.staged, "clean mutated staged entries");
}

fn add_sparse_checkout_clean_candidates(repo: &Path) {
    write_file(repo, "src/untracked.txt", "untracked\n");
    write_file(repo, "src/ignored.txt", "ignored\n");
    fs::write(repo.join(".git/info/exclude"), "src/ignored.txt\n").expect("write excludes");
}

#[test]
fn sparse_checkout_clean_matches_pinned_git() {
    require_pinned_git_v2_55();
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    git(git_repo.path(), ["sparse-checkout", "set", "docs"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "set", "docs"]);
    add_sparse_checkout_clean_candidates(git_repo.path());
    add_sparse_checkout_clean_candidates(zmin_repo.path());

    let initial_git = sparse_checkout_clean_state(git_repo.path());
    let initial_zmin = sparse_checkout_clean_state(zmin_repo.path());

    for args in [
        ["sparse-checkout", "clean"].as_slice(),
        ["sparse-checkout", "clean", "--dry-run"].as_slice(),
        ["sparse-checkout", "clean", "-n", "-v"].as_slice(),
        ["sparse-checkout", "clean", "-n", "extra"].as_slice(),
        ["sparse-checkout", "clean", "-n", "--", "extra"].as_slice(),
        ["sparse-checkout", "clean", "--no-dry-run", "-n"].as_slice(),
        ["sparse-checkout", "clean", "-n", "-v", "--no-verbose"].as_slice(),
    ] {
        assert_eq!(
            zmin_exact_output(zmin_repo.path(), args),
            pinned_git_exact_output(git_repo.path(), args)
        );
        assert_sparse_checkout_metadata_unchanged(
            &initial_zmin,
            &sparse_checkout_clean_state(zmin_repo.path()),
        );
        assert_sparse_checkout_metadata_unchanged(
            &initial_git,
            &sparse_checkout_clean_state(git_repo.path()),
        );
    }

    assert_eq!(
        zmin_exact_output(zmin_repo.path(), &["sparse-checkout", "clean", "-f"]),
        pinned_git_exact_output(git_repo.path(), &["sparse-checkout", "clean", "-f"])
    );
    let zmin_after = sparse_checkout_clean_state(zmin_repo.path());
    let git_after = sparse_checkout_clean_state(git_repo.path());
    assert_sparse_checkout_metadata_unchanged(&initial_zmin, &zmin_after);
    assert_sparse_checkout_metadata_unchanged(&initial_git, &git_after);
    assert_eq!(zmin_after.files, git_after.files);
    assert!(!zmin_repo.path().join("src").exists());
    assert!(!git_repo.path().join("src").exists());
}

#[test]
fn sparse_checkout_clean_matches_without_sparse_index_and_without_force_config() {
    require_pinned_git_v2_55();
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    git(
        git_repo.path(),
        ["sparse-checkout", "set", "--no-sparse-index", "docs"],
    );
    run_zmin(
        zmin_repo.path(),
        ["sparse-checkout", "set", "--no-sparse-index", "docs"],
    );
    git(git_repo.path(), ["config", "clean.requireForce", "false"]);
    run_zmin(zmin_repo.path(), ["config", "clean.requireForce", "false"]);
    add_sparse_checkout_clean_candidates(git_repo.path());
    add_sparse_checkout_clean_candidates(zmin_repo.path());
    let initial_git = sparse_checkout_clean_state(git_repo.path());
    let initial_zmin = sparse_checkout_clean_state(zmin_repo.path());

    assert_eq!(
        zmin_exact_output(zmin_repo.path(), &["sparse-checkout", "clean"]),
        pinned_git_exact_output(git_repo.path(), &["sparse-checkout", "clean"])
    );
    let zmin_after = sparse_checkout_clean_state(zmin_repo.path());
    let git_after = sparse_checkout_clean_state(git_repo.path());
    assert_sparse_checkout_metadata_unchanged(&initial_zmin, &zmin_after);
    assert_sparse_checkout_metadata_unchanged(&initial_git, &git_after);
    assert_eq!(zmin_after.files, git_after.files);
    assert!(!zmin_repo.path().join("src").exists());
    assert!(!git_repo.path().join("src").exists());
}

#[test]
fn sparse_checkout_clean_preserves_vivified_tracked_paths() {
    require_pinned_git_v2_55();
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    git(git_repo.path(), ["sparse-checkout", "set", "docs"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "set", "docs"]);
    write_file(git_repo.path(), "src/main.rs", "modified\n");
    write_file(zmin_repo.path(), "src/main.rs", "modified\n");
    write_file(git_repo.path(), "src/untracked.txt", "untracked\n");
    write_file(zmin_repo.path(), "src/untracked.txt", "untracked\n");
    let initial_git = sparse_checkout_clean_state(git_repo.path());
    let initial_zmin = sparse_checkout_clean_state(zmin_repo.path());

    let git_output = pinned_git_exact_output(git_repo.path(), &["sparse-checkout", "clean", "-f"]);
    let zmin_output = zmin_exact_output(zmin_repo.path(), &["sparse-checkout", "clean", "-f"]);
    assert_eq!(zmin_output, git_output);
    assert!(git_repo.path().join("src/main.rs").exists());
    assert!(zmin_repo.path().join("src/main.rs").exists());
    let zmin_after = sparse_checkout_clean_state(zmin_repo.path());
    let git_after = sparse_checkout_clean_state(git_repo.path());
    assert_sparse_checkout_metadata_unchanged(&initial_zmin, &zmin_after);
    assert_sparse_checkout_metadata_unchanged(&initial_git, &git_after);
    assert_eq!(zmin_after.files, git_after.files);
}

#[cfg(target_os = "linux")]
fn raw_worktree_path(repo: &Path, path: &[u8]) -> std::path::PathBuf {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    repo.join(Path::new(OsStr::from_bytes(path)))
}

#[cfg(target_os = "linux")]
fn write_raw_worktree_file(repo: &Path, path: &[u8], contents: &[u8]) {
    let path = raw_worktree_path(repo, path);
    fs::create_dir_all(path.parent().expect("raw pathname has a parent"))
        .expect("create raw pathname parent");
    fs::write(path, contents).expect("write raw pathname");
}

#[cfg(target_os = "linux")]
#[test]
fn sparse_checkout_clean_prints_non_utf8_pathname_bytes_exactly() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    require_pinned_git_v2_55();
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    git(git_repo.path(), ["sparse-checkout", "set", "docs"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "set", "docs"]);

    let raw_path = b"src/\x80-name.txt";
    write_raw_worktree_file(git_repo.path(), raw_path, b"untracked\n");
    write_raw_worktree_file(zmin_repo.path(), raw_path, b"untracked\n");

    let args = ["sparse-checkout", "clean", "-f", "-v"];
    let git_output = pinned_git_exact_output(git_repo.path(), &args);
    let zmin_output = zmin_exact_output(zmin_repo.path(), &args);
    assert_eq!(zmin_output, git_output);
    assert!(
        git_output
            .1
            .windows(raw_path.len())
            .any(|window| window == raw_path),
        "pinned Git did not emit the expected raw pathname bytes"
    );
    assert!(
        !git_repo
            .path()
            .join(Path::new(OsStr::from_bytes(raw_path)))
            .exists()
    );
    assert!(
        !zmin_repo
            .path()
            .join(Path::new(OsStr::from_bytes(raw_path)))
            .exists()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn sparse_checkout_set_and_reapply_preserve_raw_collision_path() {
    require_pinned_git_v2_55();
    let git_repo = sparse_checkout_fixture_repo();
    write_file(git_repo.path(), "src/�.txt", "tracked replacement\n");
    git(git_repo.path(), ["add", "src/�.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "add replacement"]);
    let zmin_repo = clone_repo_fixture(git_repo.path());

    let raw_path = b"src/\x80.txt";
    write_raw_worktree_file(git_repo.path(), raw_path, b"raw untracked\n");
    write_raw_worktree_file(zmin_repo.path(), raw_path, b"raw untracked\n");

    for args in [
        ["sparse-checkout", "set", "docs"].as_slice(),
        ["sparse-checkout", "reapply"].as_slice(),
    ] {
        assert_eq!(
            zmin_exact_output(zmin_repo.path(), args),
            pinned_git_exact_output(git_repo.path(), args)
        );
        assert!(
            raw_worktree_path(git_repo.path(), raw_path).exists(),
            "pinned Git removed the raw untracked pathname"
        );
        assert!(
            raw_worktree_path(zmin_repo.path(), raw_path).exists(),
            "Zmin removed the raw untracked pathname"
        );
        assert!(
            !git_repo.path().join("src/�.txt").exists(),
            "pinned Git retained the excluded tracked pathname"
        );
        assert!(
            !zmin_repo.path().join("src/�.txt").exists(),
            "Zmin retained the excluded tracked pathname"
        );
    }
}

#[test]
fn sparse_checkout_clean_preconditions_and_usage_match_pinned_git() {
    require_pinned_git_v2_55();
    let non_sparse_git = sparse_checkout_fixture_repo();
    let non_sparse_zmin = clone_repo_fixture(non_sparse_git.path());
    let non_sparse_args = ["sparse-checkout", "clean", "--dry-run"];
    assert_eq!(
        zmin_exact_output(non_sparse_zmin.path(), &non_sparse_args),
        pinned_git_exact_output(non_sparse_git.path(), &non_sparse_args)
    );

    let non_cone_git = sparse_checkout_fixture_repo();
    let non_cone_zmin = clone_repo_fixture(non_cone_git.path());
    git(
        non_cone_git.path(),
        ["sparse-checkout", "set", "--no-cone", "docs"],
    );
    run_zmin(
        non_cone_zmin.path(),
        ["sparse-checkout", "set", "--no-cone", "docs"],
    );
    for args in [
        ["sparse-checkout", "clean", "--dry-run"].as_slice(),
        ["sparse-checkout", "clean", "-h"].as_slice(),
        ["sparse-checkout", "clean", "--bad"].as_slice(),
    ] {
        assert_eq!(
            zmin_exact_output(non_cone_zmin.path(), args),
            pinned_git_exact_output(non_cone_git.path(), args)
        );
    }

    let enabled_git = sparse_checkout_fixture_repo();
    let enabled_zmin = clone_repo_fixture(enabled_git.path());
    git(enabled_git.path(), ["sparse-checkout", "set", "docs"]);
    run_zmin(enabled_zmin.path(), ["sparse-checkout", "set", "docs"]);
    for args in [
        ["sparse-checkout", "clean", "-h"].as_slice(),
        ["sparse-checkout", "clean", "--bad"].as_slice(),
    ] {
        assert_eq!(
            zmin_exact_output(enabled_zmin.path(), args),
            pinned_git_exact_output(enabled_git.path(), args)
        );
    }
}

#[test]
fn sparse_checkout_set_list_disable_matches_stock_git_files() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    git(git_repo.path(), ["sparse-checkout", "set", "docs"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "set", "docs"]);
    assert_eq!(
        visible_worktree_files(zmin_repo.path()),
        visible_worktree_files(git_repo.path())
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        "docs"
    );

    git(git_repo.path(), ["sparse-checkout", "add", "src"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "add", "src"]);
    assert_eq!(
        visible_worktree_files(zmin_repo.path()),
        visible_worktree_files(git_repo.path())
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        "docs\nsrc"
    );

    fs::remove_file(zmin_repo.path().join("docs/guide.md")).expect("remove zmin sparse file");
    fs::remove_file(git_repo.path().join("docs/guide.md")).expect("remove git sparse file");
    git(git_repo.path(), ["sparse-checkout", "reapply"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "reapply"]);
    assert_eq!(
        visible_worktree_files(zmin_repo.path()),
        visible_worktree_files(git_repo.path())
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );

    git(git_repo.path(), ["sparse-checkout", "disable"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "disable"]);
    assert_eq!(
        visible_worktree_files(zmin_repo.path()),
        visible_worktree_files(git_repo.path())
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );
}

#[test]
fn sparse_checkout_pathspec_edge_cases_match_stock_git() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["sparse-checkout", "set", ".."]),
        git_failure_output(git_repo.path(), &["sparse-checkout", "set", ".."])
    );

    git(git_repo.path(), ["sparse-checkout", "set", "a\\b"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "set", "a\\b"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        git(git_repo.path(), ["sparse-checkout", "list"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["sparse-checkout", "add", ".."]),
        git_failure_output(git_repo.path(), &["sparse-checkout", "add", ".."])
    );
}

#[test]
fn sparse_checkout_skip_checks_option_matches_stock_git() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    let git_output = command_any_output(
        "git",
        git_repo.path(),
        &["sparse-checkout", "set", "--skip-checks", "docs"],
        "git",
    );
    let zmin_output = command_any_output(
        zmin_bin(),
        zmin_repo.path(),
        &["sparse-checkout", "set", "--skip-checks", "docs"],
        "zmin",
    );
    assert_eq!(zmin_output, git_output);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        git(git_repo.path(), ["sparse-checkout", "list"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );
    assert_eq!(
        visible_worktree_files(zmin_repo.path()),
        visible_worktree_files(git_repo.path())
    );
}

#[test]
fn sparse_checkout_non_sparse_failures_match_stock_git() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    for args in [
        ["sparse-checkout", "list"].as_slice(),
        ["sparse-checkout", "add", "docs"].as_slice(),
        ["sparse-checkout", "add", "--bad"].as_slice(),
        ["sparse-checkout", "reapply"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), args),
            git_failure_output(git_repo.path(), args)
        );
    }
}

#[test]
fn sparse_checkout_unknown_subcommand_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["sparse-checkout", "bogus"]),
        git_failure_output(git_repo.path(), &["sparse-checkout", "bogus"]),
    );
}

#[test]
fn sparse_checkout_set_unknown_option_matches_stock_git_usage() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["sparse-checkout", "set", "--bad"]),
        git_failure_output(git_repo.path(), &["sparse-checkout", "set", "--bad"]),
    );
}

#[test]
fn sparse_checkout_init_unknown_option_matches_stock_git_usage() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["sparse-checkout", "init", "--bad"]),
        git_failure_output(git_repo.path(), &["sparse-checkout", "init", "--bad"]),
    );
}

#[test]
fn sparse_checkout_add_unknown_option_matches_stock_git_usage_when_enabled() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    git(git_repo.path(), ["sparse-checkout", "set", "docs"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "set", "docs"]);

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["sparse-checkout", "add", "--bad"]),
        git_failure_output(git_repo.path(), &["sparse-checkout", "add", "--bad"]),
    );
}

#[test]
fn sparse_checkout_add_stdin_ignores_positional_patterns_like_stock_git() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    git(git_repo.path(), ["sparse-checkout", "set", "docs"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "set", "docs"]);
    git_with_stdin(
        git_repo.path(),
        ["sparse-checkout", "add", "--stdin", "missing"],
        "src\n",
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        ["sparse-checkout", "add", "--stdin", "missing"],
        "src\n",
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        git(git_repo.path(), ["sparse-checkout", "list"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );
}

#[test]
fn sparse_checkout_stdin_and_config_options_match_stock_git() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    git_with_stdin(
        git_repo.path(),
        ["sparse-checkout", "set", "--stdin"],
        "docs\n",
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        ["sparse-checkout", "set", "--stdin"],
        "docs\n",
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        git(git_repo.path(), ["sparse-checkout", "list"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );

    git_with_stdin(
        git_repo.path(),
        ["sparse-checkout", "add", "--stdin"],
        "src\n",
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        ["sparse-checkout", "add", "--stdin"],
        "src\n",
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        git(git_repo.path(), ["sparse-checkout", "list"])
    );

    git_with_stdin(
        git_repo.path(),
        ["sparse-checkout", "set", "--stdin", "docs"],
        "src\n",
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        ["sparse-checkout", "set", "--stdin", "docs"],
        "src\n",
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        git(git_repo.path(), ["sparse-checkout", "list"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );

    git(
        git_repo.path(),
        ["sparse-checkout", "set", "--no-cone", "docs"],
    );
    run_zmin(
        zmin_repo.path(),
        ["sparse-checkout", "set", "--no-cone", "docs"],
    );
    assert_eq!(
        git_config_get(zmin_repo.path(), "core.sparseCheckoutCone"),
        git_config_get(git_repo.path(), "core.sparseCheckoutCone")
    );
    assert_eq!(
        git_config_get(zmin_repo.path(), "index.sparse"),
        git_config_get(git_repo.path(), "index.sparse")
    );

    let git_init_repo = git_init();
    let zmin_init_repo = git_init();
    git(
        git_init_repo.path(),
        ["sparse-checkout", "init", "--no-cone", "--no-sparse-index"],
    );
    run_zmin(
        zmin_init_repo.path(),
        ["sparse-checkout", "init", "--no-cone", "--no-sparse-index"],
    );
    for key in [
        "core.sparseCheckout",
        "core.sparseCheckoutCone",
        "index.sparse",
    ] {
        assert_eq!(
            git_config_get(zmin_init_repo.path(), key),
            git_config_get(git_init_repo.path(), key)
        );
    }
}

#[test]
fn sparse_checkout_sparse_index_options_match_stock_git() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    git(
        git_repo.path(),
        ["sparse-checkout", "set", "--sparse-index", "docs"],
    );
    run_zmin(
        zmin_repo.path(),
        ["sparse-checkout", "set", "--sparse-index", "docs"],
    );
    for key in [
        "core.sparseCheckout",
        "core.sparseCheckoutCone",
        "index.sparse",
    ] {
        assert_eq!(
            git_config_get(zmin_repo.path(), key),
            git_config_get(git_repo.path(), key)
        );
    }
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        git(git_repo.path(), ["sparse-checkout", "list"])
    );
    assert_eq!(
        visible_worktree_files(zmin_repo.path()),
        visible_worktree_files(git_repo.path())
    );

    let git_init_repo = git_init();
    let zmin_init_repo = git_init();
    git(
        git_init_repo.path(),
        ["sparse-checkout", "init", "--sparse-index", "--no-cone"],
    );
    run_zmin(
        zmin_init_repo.path(),
        ["sparse-checkout", "init", "--sparse-index", "--no-cone"],
    );
    for key in [
        "core.sparseCheckout",
        "core.sparseCheckoutCone",
        "index.sparse",
    ] {
        assert_eq!(
            git_config_get(zmin_init_repo.path(), key),
            git_config_get(git_init_repo.path(), key)
        );
    }
}
