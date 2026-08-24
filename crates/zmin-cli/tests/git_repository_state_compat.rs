mod common;

use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::{
    assert_repository_state_matches, command_output_with_env, git, run_zmin, stock_git_bin,
    zmin_bin,
};
use tempfile::TempDir;

const FIXED_ENV: [(&str, &str); 6] = [
    ("GIT_AUTHOR_NAME", "Bench"),
    ("GIT_AUTHOR_EMAIL", "bench@example.test"),
    ("GIT_AUTHOR_DATE", "1700000000 +0000"),
    ("GIT_COMMITTER_NAME", "Bench"),
    ("GIT_COMMITTER_EMAIL", "bench@example.test"),
    ("GIT_COMMITTER_DATE", "1700000000 +0000"),
];

#[test]
fn git_seed_handoff_keeps_repository_state_identical() {
    let dir = TempDir::new().expect("temp dir");
    let seed = dir.path().join("git-seed");
    git(
        dir.path(),
        ["init", "-b", "main", seed.to_str().expect("seed path")],
    );
    configure_identity_with_git(&seed);
    seed_repository_with_git(&seed);

    git_with_fixed_env(
        dir.path(),
        ["clone", seed.to_str().expect("seed path"), "git-handoff"],
    );
    git_with_fixed_env(
        dir.path(),
        ["clone", seed.to_str().expect("seed path"), "zmin-handoff"],
    );

    let git_handoff = dir.path().join("git-handoff");
    let zmin_handoff = dir.path().join("zmin-handoff");
    configure_identity_with_git(&git_handoff);
    configure_identity_with_git(&zmin_handoff);
    apply_handoff_workflow_with_git(&git_handoff);
    apply_handoff_workflow_with_zmin(&zmin_handoff);

    assert_repository_state_matches(&zmin_handoff, &git_handoff);
}

#[test]
fn zmin_seed_handoff_keeps_repository_state_identical() {
    let dir = TempDir::new().expect("temp dir");
    let seed = dir.path().join("zmin-seed");
    run_zmin(
        dir.path(),
        [
            "init",
            "--initial-branch",
            "main",
            seed.to_str().expect("seed path"),
        ],
    );
    configure_identity_with_git(&seed);
    seed_repository_with_zmin(&seed);

    git_with_fixed_env(
        dir.path(),
        ["clone", seed.to_str().expect("seed path"), "git-handoff"],
    );
    git_with_fixed_env(
        dir.path(),
        ["clone", seed.to_str().expect("seed path"), "zmin-handoff"],
    );

    let git_handoff = dir.path().join("git-handoff");
    let zmin_handoff = dir.path().join("zmin-handoff");
    configure_identity_with_git(&git_handoff);
    configure_identity_with_git(&zmin_handoff);
    apply_handoff_workflow_with_git(&git_handoff);
    apply_handoff_workflow_with_zmin(&zmin_handoff);

    assert_repository_state_matches(&zmin_handoff, &git_handoff);
}

#[test]
fn repository_context_matches_pinned_git_for_gitfile_env_and_linked_worktree() {
    let pinned_stock = std::env::var_os("ZMIN_STOCK_GIT")
        .expect("repository-context differential requires ZMIN_STOCK_GIT");
    let stock = stock_git_bin();
    assert_eq!(Path::new(&pinned_stock), stock);
    let version = Command::new(stock)
        .arg("--version")
        .output()
        .expect("pinned stock Git version");
    assert!(version.status.success());
    assert_eq!(version.stdout, b"git version 2.55.0\n");

    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    run_checked(
        stock,
        dir.path(),
        &["init", repo.to_str().unwrap()],
        &[],
        "stock init",
    );
    run_checked(
        stock,
        &repo,
        &["config", "user.name", "Bench"],
        &[],
        "stock config name",
    );
    run_checked(
        stock,
        &repo,
        &["config", "user.email", "bench@example.test"],
        &[],
        "stock config email",
    );
    fs::write(repo.join("README"), b"context\n").expect("seed file");
    run_checked(stock, &repo, &["add", "README"], &[], "stock add");
    run_checked(
        stock,
        &repo,
        &["commit", "-m", "context"],
        &FIXED_ENV,
        "stock commit",
    );

    let gitfile_root = dir.path().join("gitfile-root");
    let separate_git_dir = dir.path().join("separate.git");
    run_checked(
        stock,
        dir.path(),
        &[
            "clone",
            "--separate-git-dir",
            separate_git_dir.to_str().unwrap(),
            repo.to_str().unwrap(),
            gitfile_root.to_str().unwrap(),
        ],
        &[],
        "stock separate git dir",
    );
    assert_repo_context_equal(stock, Path::new(zmin_bin()), &gitfile_root, &[]);
    assert_repo_context_equal(
        stock,
        Path::new(zmin_bin()),
        &gitfile_root,
        &[
            ("GIT_DIR", OsStr::new(".git")),
            ("GIT_WORK_TREE", OsStr::new(".")),
        ],
    );
    assert_repo_context_with_args(
        stock,
        Path::new(zmin_bin()),
        &repo,
        &[],
        &[
            "--git-dir",
            ".git",
            "--work-tree",
            ".",
            "rev-parse",
            "--git-dir",
            "--show-toplevel",
            "--show-prefix",
        ],
    );

    let nested = repo.join("src");
    fs::create_dir_all(&nested).expect("nested cwd");
    assert_repo_context_equal(
        stock,
        Path::new(zmin_bin()),
        &nested,
        &[
            ("GIT_DIR", OsStr::new("../.git")),
            ("GIT_WORK_TREE", OsStr::new("..")),
        ],
    );
    let linked = dir.path().join("linked");
    run_checked(
        stock,
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "linked-context",
            linked.to_str().unwrap(),
        ],
        &[],
        "stock linked worktree",
    );
    assert_repo_context_equal(stock, Path::new(zmin_bin()), &linked, &[]);

    let bare_probe = repo.join(".git").join("objects");
    assert_repo_context_with_args(
        stock,
        Path::new(zmin_bin()),
        &bare_probe,
        &[],
        &["rev-parse", "--is-bare-repository"],
    );

    let configured_worktree = repo.join("configured-worktree");
    fs::create_dir_all(&configured_worktree).expect("configured worktree");
    run_checked(
        stock,
        &repo,
        &["config", "core.worktree", "../configured-worktree"],
        &[],
        "stock configure core.worktree",
    );
    assert_repo_context_equal(
        stock,
        Path::new(zmin_bin()),
        &configured_worktree,
        &[("GIT_DIR", OsStr::new("../.git"))],
    );
}

#[test]
fn trace_setup_destinations_match_pinned_git() {
    let pinned_stock = std::env::var_os("ZMIN_STOCK_GIT")
        .expect("trace setup differential requires ZMIN_STOCK_GIT");
    let stock = stock_git_bin();
    assert_eq!(Path::new(&pinned_stock), stock);
    let version = Command::new(stock)
        .arg("--version")
        .output()
        .expect("pinned stock Git version");
    assert!(version.status.success());
    assert_eq!(version.stdout, b"git version 2.55.0\n");

    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    run_checked(
        stock,
        dir.path(),
        &["init", repo.to_str().expect("repo path")],
        &[],
        "stock init",
    );

    for (label, value) in [
        ("unset", None),
        ("zero", Some(OsStr::new("0"))),
        ("stderr-one", Some(OsStr::new("1"))),
        ("stderr-two", Some(OsStr::new("2"))),
    ] {
        let stock_output = trace_setup_output(stock, &repo, value);
        let zmin_output = trace_setup_output(Path::new(zmin_bin()), &repo, value);
        assert_eq!(
            stock_output.status.code(),
            zmin_output.status.code(),
            "{label} rc"
        );
        assert_eq!(stock_output.stdout, zmin_output.stdout, "{label} stdout");
        let setup_marker = b"setup: git_dir: ";
        let stock_has_setup = stock_output
            .stderr
            .windows(setup_marker.len())
            .any(|window| window == setup_marker);
        let zmin_has_setup = zmin_output
            .stderr
            .windows(setup_marker.len())
            .any(|window| window == setup_marker);
        assert_eq!(stock_has_setup, zmin_has_setup, "{label} trace destination");
        if label == "unset" || label == "zero" {
            assert!(!stock_has_setup, "{label} unexpectedly traced");
        } else {
            assert!(stock_has_setup, "{label} did not trace to stderr");
            assert!(zmin_has_setup, "{label} did not trace to stderr");
        }
    }

    let stock_trace = dir.path().join("stock.trace");
    let zmin_trace = dir.path().join("zmin.trace");
    let stock_output = trace_setup_output(stock, &repo, Some(stock_trace.as_os_str()));
    let zmin_output =
        trace_setup_output(Path::new(zmin_bin()), &repo, Some(zmin_trace.as_os_str()));
    assert_eq!(
        stock_output.status.code(),
        zmin_output.status.code(),
        "file rc"
    );
    assert_eq!(stock_output.stdout, zmin_output.stdout, "file stdout");
    assert!(
        stock_output.stderr.is_empty(),
        "stock file trace leaked to stderr"
    );
    assert!(
        zmin_output.stderr.is_empty(),
        "zmin file trace leaked to stderr"
    );
    assert_eq!(
        normalize_trace_setup(&fs::read(&stock_trace).expect("stock trace file")),
        normalize_trace_setup(&fs::read(&zmin_trace).expect("zmin trace file"))
    );

    let missing = dir.path().join("missing-parent").join("trace");
    let stock_output = trace_setup_output(stock, &repo, Some(missing.as_os_str()));
    let zmin_output = trace_setup_output(Path::new(zmin_bin()), &repo, Some(missing.as_os_str()));
    assert_eq!(
        stock_output.status.code(),
        zmin_output.status.code(),
        "missing file rc"
    );
    assert_eq!(
        stock_output.stdout, zmin_output.stdout,
        "missing file stdout"
    );
    assert_eq!(
        stock_output.stderr, zmin_output.stderr,
        "missing file warning"
    );

    let nested = repo.join("sub");
    fs::create_dir(&nested).expect("nested trace cwd");
    let stock_output = trace_setup_output(stock, &nested, Some(OsStr::new("1")));
    let zmin_output = trace_setup_output(Path::new(zmin_bin()), &nested, Some(OsStr::new("1")));
    let stock_trace = normalize_trace_setup(&stock_output.stderr);
    let zmin_trace = normalize_trace_setup(&zmin_output.stderr);
    assert_eq!(
        stock_output.status.code(),
        zmin_output.status.code(),
        "nested cwd rc"
    );
    assert_eq!(stock_output.stdout, zmin_output.stdout, "nested cwd stdout");
    assert_eq!(stock_trace, zmin_trace, "nested cwd trace");
    let expected_cwd = format!(
        "setup: cwd: {}\n",
        fs::canonicalize(&repo)
            .expect("canonical repository root")
            .display()
    );
    assert!(
        stock_trace
            .windows(expected_cwd.len())
            .any(|window| window == expected_cwd.as_bytes()),
        "trace did not retain nested cwd: {}",
        String::from_utf8_lossy(&stock_trace)
    );
    assert!(
        stock_trace
            .windows(b"setup: prefix: sub/\n".len())
            .any(|window| { window == b"setup: prefix: sub/\n" })
    );

    let envs = [
        ("GIT_DIR", OsStr::new("../.git")),
        ("GIT_WORK_TREE", OsStr::new("..")),
    ];
    let stock_output = trace_setup_output_with_env(stock, &nested, Some(OsStr::new("1")), &envs);
    let zmin_output =
        trace_setup_output_with_env(Path::new(zmin_bin()), &nested, Some(OsStr::new("1")), &envs);
    assert_eq!(
        normalize_trace_setup(&stock_output.stderr),
        normalize_trace_setup(&zmin_output.stderr),
        "nested env cwd trace"
    );
}

fn run_checked(program: &Path, cwd: &Path, args: &[&str], envs: &[(&str, &str)], label: &str) {
    let mut command = Command::new(program);
    command.current_dir(cwd).args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{label}: {error}"));
    assert!(
        output.status.success(),
        "{label}: rc={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_repo_context_equal(stock: &Path, zmin: &Path, cwd: &Path, envs: &[(&str, &OsStr)]) {
    assert_repo_context_with_args(
        stock,
        zmin,
        cwd,
        envs,
        &["rev-parse", "--git-dir", "--show-toplevel", "--show-prefix"],
    );
}

fn assert_repo_context_with_args(
    stock: &Path,
    zmin: &Path,
    cwd: &Path,
    envs: &[(&str, &OsStr)],
    args: &[&str],
) {
    let stock_output = repo_context_output(stock, cwd, envs, args);
    let zmin_output = repo_context_output(zmin, cwd, envs, args);
    assert_eq!(
        (
            stock_output.status.code(),
            stock_output.stdout,
            stock_output.stderr
        ),
        (
            zmin_output.status.code(),
            zmin_output.stdout,
            zmin_output.stderr
        ),
        "repository context mismatch in {} with env {:?}",
        cwd.display(),
        envs.iter()
            .map(|(key, value)| (*key, value))
            .collect::<Vec<_>>()
    );
}

fn repo_context_output(
    program: &Path,
    cwd: &Path,
    envs: &[(&str, &OsStr)],
    args: &[&str],
) -> Output {
    let mut command = Command::new(program);
    command
        .current_dir(cwd)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE");
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("repository context probe")
}

fn trace_setup_output(program: &Path, cwd: &Path, value: Option<&OsStr>) -> Output {
    trace_setup_output_with_env(program, cwd, value, &[])
}

fn trace_setup_output_with_env(
    program: &Path,
    cwd: &Path,
    value: Option<&OsStr>,
    envs: &[(&str, &OsStr)],
) -> Output {
    let mut command = Command::new(program);
    command
        .current_dir(cwd)
        .args(["symbolic-ref", "HEAD"])
        .env_remove("GIT_TRACE_SETUP");
    if let Some(value) = value {
        command.env("GIT_TRACE_SETUP", value);
    }
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("trace setup probe")
}

fn normalize_trace_setup(bytes: &[u8]) -> Vec<u8> {
    let marker = b"setup: ";
    let mut normalized = Vec::new();
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let Some(offset) = line
            .windows(marker.len())
            .position(|window| window == marker)
        else {
            normalized.extend_from_slice(line);
            continue;
        };
        normalized.extend_from_slice(&line[offset..]);
    }
    normalized
}

fn configure_identity_with_git(repo: &Path) {
    git(repo, ["config", "user.name", "Bench"]);
    git(repo, ["config", "user.email", "bench@example.test"]);
    git(repo, ["config", "commit.gpgsign", "false"]);
    git(repo, ["config", "tag.gpgsign", "false"]);
}

fn seed_repository_with_git(repo: &Path) {
    fs::write(repo.join("README.md"), b"seed\n").expect("write readme");
    fs::create_dir_all(repo.join("src")).expect("create src dir");
    fs::write(repo.join("src/lib.rs"), b"pub fn seed() {}\n").expect("write lib");
    git(repo, ["add", "-A"]);
    git_with_fixed_env(repo, ["commit", "-m", "seed"]);
    git(repo, ["tag", "seed-tag"]);
}

fn seed_repository_with_zmin(repo: &Path) {
    fs::write(repo.join("README.md"), b"seed\n").expect("write readme");
    fs::create_dir_all(repo.join("src")).expect("create src dir");
    fs::write(repo.join("src/lib.rs"), b"pub fn seed() {}\n").expect("write lib");
    run_zmin(repo, ["add", "-A"]);
    zmin_with_fixed_env(repo, ["commit", "-m", "seed"]);
    run_zmin(repo, ["tag", "seed-tag"]);
}

fn apply_handoff_workflow_with_git(repo: &Path) {
    git_with_fixed_env(repo, ["switch", "-c", "feature"]);
    fs::write(repo.join("README.md"), b"seed\nfeature\n").expect("update readme");
    fs::write(repo.join("src/lib.rs"), b"pub fn feature() {}\n").expect("update lib");
    fs::write(repo.join("notes.txt"), b"notes\n").expect("write notes");
    git(repo, ["add", "-A"]);
    git_with_fixed_env(repo, ["commit", "-m", "feature"]);
    git_with_fixed_env(repo, ["switch", "main"]);
    git_with_fixed_env(repo, ["merge", "--ff-only", "feature"]);
    git(repo, ["pack-refs", "--all"]);
    git(repo, ["repack", "-ad"]);
    git(repo, ["commit-graph", "write", "--reachable"]);
}

fn apply_handoff_workflow_with_zmin(repo: &Path) {
    zmin_with_fixed_env(repo, ["switch", "-c", "feature"]);
    fs::write(repo.join("README.md"), b"seed\nfeature\n").expect("update readme");
    fs::write(repo.join("src/lib.rs"), b"pub fn feature() {}\n").expect("update lib");
    fs::write(repo.join("notes.txt"), b"notes\n").expect("write notes");
    run_zmin(repo, ["add", "-A"]);
    zmin_with_fixed_env(repo, ["commit", "-m", "feature"]);
    zmin_with_fixed_env(repo, ["switch", "main"]);
    zmin_with_fixed_env(repo, ["merge", "--ff-only", "feature"]);
    run_zmin(repo, ["pack-refs", "--all"]);
    run_zmin(repo, ["repack", "-ad"]);
    run_zmin(repo, ["commit-graph", "write", "--reachable"]);
}

fn git_with_fixed_env<const N: usize>(repo: &Path, args: [&str; N]) {
    let _ = command_output_with_env("git", repo, &args, &FIXED_ENV, "git");
}

fn zmin_with_fixed_env<const N: usize>(repo: &Path, args: [&str; N]) {
    let _ = command_output_with_env(zmin_bin(), repo, &args, &FIXED_ENV, "zmin");
}
