#![allow(dead_code)]

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::{fs, path::Path, path::PathBuf};

use tempfile::TempDir;

static REMOTE_HTTP_HELPER: OnceLock<PathBuf> = OnceLock::new();

pub fn skron_bin() -> &'static str {
    let _ = ensure_remote_http_helper();
    option_env!("CARGO_BIN_EXE_skron-git").unwrap_or(env!("CARGO_BIN_EXE_skron"))
}

fn ensure_remote_http_helper() -> &'static Path {
    REMOTE_HTTP_HELPER
        .get_or_init(|| {
            if let Ok(path) = std::env::var("SKRON_GIT_REMOTE_HTTP") {
                return PathBuf::from(path);
            }

            let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .expect("workspace root");
            let helper_name = if cfg!(windows) {
                "skron-git-remote-http.exe"
            } else {
                "skron-git-remote-http"
            };
            let helper = workspace_root
                .join("target")
                .join("debug")
                .join(helper_name);
            if helper.is_file() {
                return helper;
            }

            let status = Command::new("cargo")
                .args(["build", "-p", "skron-git-remote-http", "--quiet"])
                .current_dir(workspace_root)
                .status()
                .expect("build skron-git-remote-http");
            assert!(status.success(), "failed to build skron-git-remote-http");
            helper
        })
        .as_path()
}

pub fn git_init() -> TempDir {
    let repo = TempDir::new().expect("temp repo");
    git(repo.path(), ["init"]);
    repo
}

pub fn configure_identity(repo: &std::path::Path) {
    git(repo, ["config", "user.name", "Bench"]);
    git(repo, ["config", "user.email", "bench@example.test"]);
    git(repo, ["config", "commit.gpgsign", "false"]);
}

pub fn write_file(repo: &Path, path: &str, content: &str) {
    let path = repo.join(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, content).expect("write file");
}

pub fn run_skron<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> String {
    run_skron_args(cwd, &args)
}

pub fn run_skron_args(cwd: &std::path::Path, args: &[&str]) -> String {
    command_output(skron_bin(), cwd, args, "skron").1
}

pub fn run_skron_with_stdin<const N: usize>(
    cwd: &std::path::Path,
    args: [&str; N],
    stdin: &str,
) -> String {
    run_skron_with_stdin_args(cwd, &args, stdin)
}

pub fn run_skron_with_stdin_args(cwd: &std::path::Path, args: &[&str], stdin: &str) -> String {
    command_with_stdin(skron_bin(), cwd, args, stdin, "skron")
}

pub fn run_skron_with_stdin_bytes<const N: usize>(
    cwd: &std::path::Path,
    args: [&str; N],
    stdin: &[u8],
) -> String {
    command_with_stdin_bytes(skron_bin(), cwd, &args, stdin, "skron")
}

pub fn run_skron_with_env<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> String {
    command_output_with_env(
        skron_bin(),
        cwd,
        &args,
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
        "skron",
    )
    .1
}

pub fn git<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> String {
    git_args(cwd, &args)
}

pub fn git_args(cwd: &std::path::Path, args: &[&str]) -> String {
    command_output("git", cwd, args, "git").1
}

pub fn run_skron_status<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> i32 {
    run_skron_status_args(cwd, &args)
}

pub fn run_skron_status_args(cwd: &std::path::Path, args: &[&str]) -> i32 {
    command_status(skron_bin(), cwd, args, "skron")
}

pub fn git_status<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> i32 {
    git_status_args(cwd, &args)
}

pub fn git_status_args(cwd: &std::path::Path, args: &[&str]) -> i32 {
    command_status("git", cwd, args, "git")
}

pub fn git_with_env<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> String {
    command_output_with_env(
        "git",
        cwd,
        &args,
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
        "git",
    )
    .1
}

pub fn git_with_stdin<const N: usize>(
    cwd: &std::path::Path,
    args: [&str; N],
    stdin: &str,
) -> String {
    git_with_stdin_args(cwd, &args, stdin)
}

pub fn git_with_stdin_args(cwd: &std::path::Path, args: &[&str], stdin: &str) -> String {
    command_with_stdin("git", cwd, args, stdin, "git")
}

pub fn git_with_stdin_bytes<const N: usize>(
    cwd: &std::path::Path,
    args: [&str; N],
    stdin: &[u8],
) -> String {
    command_with_stdin_bytes("git", cwd, &args, stdin, "git")
}

pub fn run_skron_status_with_stdin<const N: usize>(
    cwd: &std::path::Path,
    args: [&str; N],
    stdin: &str,
) -> i32 {
    command_status_with_stdin(skron_bin(), cwd, &args, stdin, "skron")
}

pub fn git_status_with_stdin<const N: usize>(
    cwd: &std::path::Path,
    args: [&str; N],
    stdin: &str,
) -> i32 {
    command_status_with_stdin("git", cwd, &args, stdin, "git")
}

pub fn command_status_with_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> i32 {
    let mut child = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"))
        .status
        .code()
        .expect("process exit code")
}

pub fn command_status(command: &str, cwd: &std::path::Path, args: &[&str], label: &str) -> i32 {
    Command::new(command)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|err| panic!("run {label}: {err}"))
        .status
        .code()
        .expect("process exit code")
}

pub fn command_output(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    label: &str,
) -> (i32, String, String) {
    command_output_with_env(command, cwd, args, &[], label)
}

pub fn command_any_output(
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

pub fn command_any_output_with_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> (i32, String, String) {
    let mut child = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
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

pub fn command_output_with_env(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
    label: &str,
) -> (i32, String, String) {
    let mut command = command_with_test_envs(command, cwd, args, envs);
    let output = command
        .output()
        .unwrap_or_else(|err| panic!("run {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
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

pub fn run_skron_failure_output(cwd: &Path, args: &[&str]) -> (i32, String, String) {
    command_failure_output(skron_bin(), cwd, args, "skron")
}

pub fn git_failure_output(cwd: &Path, args: &[&str]) -> (i32, String, String) {
    command_failure_output("git", cwd, args, "git")
}

pub fn command_failure_output(
    command: &str,
    cwd: &Path,
    args: &[&str],
    label: &str,
) -> (i32, String, String) {
    command_failure_output_with_env(command, cwd, args, &[], label)
}

pub fn command_failure_output_with_env(
    command: &str,
    cwd: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
    label: &str,
) -> (i32, String, String) {
    let mut command = command_with_test_envs(command, cwd, args, envs);
    let output = command
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

fn command_with_test_envs(
    command: &str,
    cwd: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Command {
    let mut command = Command::new(command);
    command.args(args).current_dir(cwd);
    for (key, value) in envs {
        command.env(key, test_env_value(key, value));
    }
    command
}

#[cfg(windows)]
fn test_env_value(key: &str, value: &str) -> String {
    if matches!(
        key,
        "GIT_EDITOR" | "GIT_SEQUENCE_EDITOR" | "VISUAL" | "EDITOR"
    ) {
        return value.replace('\\', "/");
    }

    value.to_owned()
}

#[cfg(not(windows))]
fn test_env_value(_key: &str, value: &str) -> String {
    value.to_owned()
}

pub fn command_stdout_bytes(command: &str, cwd: &std::path::Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

pub fn command_stdout_bytes_with_stdin(
    command: &str,
    cwd: &Path,
    args: &[&str],
    stdin: &[u8],
) -> Vec<u8> {
    let mut child = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin)
        .unwrap_or_else(|err| panic!("write {command} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

pub fn clone_repo_fixture(source: &Path) -> TempDir {
    let repo = TempDir::new().expect("temp clone");
    git(
        repo.path(),
        ["clone", source.to_str().expect("source path"), "."],
    );
    repo
}

pub fn read_named_files(path: &std::path::Path) -> Vec<(String, String)> {
    let mut files = fs::read_dir(path)
        .expect("read output directory")
        .map(|entry| {
            let entry = entry.expect("read output entry");
            let name = entry
                .file_name()
                .into_string()
                .expect("output filename utf8");
            let contents = fs::read_to_string(entry.path()).expect("read output file");
            (name, contents)
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

pub fn visible_worktree_files(path: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_visible_worktree_files(path, path, &mut files);
    files.sort();
    files
}

pub fn assert_repository_state_matches(left: &Path, right: &Path) {
    assert_eq!(
        visible_worktree_file_contents(left),
        visible_worktree_file_contents(right),
        "worktree contents diverged"
    );
    assert_eq!(
        git_args(left, &["status", "--porcelain=v2", "--branch"]),
        git_args(right, &["status", "--porcelain=v2", "--branch"]),
        "status diverged"
    );
    assert_eq!(
        git_args(left, &["ls-files", "--stage"]),
        git_args(right, &["ls-files", "--stage"]),
        "index entries diverged"
    );
    assert_eq!(
        git_args(left, &["show-ref", "--head", "--dereference"]),
        git_args(right, &["show-ref", "--head", "--dereference"]),
        "refs diverged"
    );
    assert_eq!(
        git_args(
            left,
            &[
                "reflog",
                "show",
                "--all",
                "--date=raw",
                "--format=%H %gD %gs"
            ],
        ),
        git_args(
            right,
            &[
                "reflog",
                "show",
                "--all",
                "--date=raw",
                "--format=%H %gD %gs"
            ],
        ),
        "reflogs diverged"
    );
    assert_eq!(
        git_args(
            left,
            &[
                "cat-file",
                "--batch-all-objects",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)"
            ],
        ),
        git_args(
            right,
            &[
                "cat-file",
                "--batch-all-objects",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)"
            ],
        ),
        "object inventories diverged"
    );

    let left_git_dir = absolute_git_dir(left);
    let right_git_dir = absolute_git_dir(right);
    let left_layout = snapshot_git_layout(left, &left_git_dir);
    let right_layout = snapshot_git_layout(right, &right_git_dir);
    if left_layout != right_layout {
        let mismatch = left_layout
            .iter()
            .zip(right_layout.iter())
            .find(|(left, right)| left != right)
            .map(|(left, right)| format!("entry mismatch at {} != {}", left.path, right.path))
            .or_else(|| {
                (left_layout.len() != right_layout.len()).then(|| {
                    format!(
                        "entry count mismatch: left={} right={}",
                        left_layout.len(),
                        right_layout.len()
                    )
                })
            })
            .unwrap_or_else(|| "git dir layout diverged".to_owned());
        panic!("{mismatch}");
    }

    assert_eq!(
        git_status(left, ["fsck", "--strict"]),
        0,
        "left repo fsck failed"
    );
    assert_eq!(
        git_status(right, ["fsck", "--strict"]),
        0,
        "right repo fsck failed"
    );

    let left_commit_graph = left_git_dir.join("objects/info/commit-graph").is_file();
    let right_commit_graph = right_git_dir.join("objects/info/commit-graph").is_file();
    assert_eq!(
        left_commit_graph, right_commit_graph,
        "commit-graph presence diverged"
    );
    if left_commit_graph {
        assert_eq!(git_status(left, ["commit-graph", "verify"]), 0);
        assert_eq!(git_status(right, ["commit-graph", "verify"]), 0);
    }

    let left_midx = left_git_dir.join("objects/pack/multi-pack-index").is_file();
    let right_midx = right_git_dir
        .join("objects/pack/multi-pack-index")
        .is_file();
    assert_eq!(left_midx, right_midx, "multi-pack-index presence diverged");
    if left_midx {
        assert_eq!(git_status(left, ["multi-pack-index", "verify"]), 0);
        assert_eq!(git_status(right, ["multi-pack-index", "verify"]), 0);
    }
}

fn collect_visible_worktree_files(root: &Path, path: &Path, files: &mut Vec<String>) {
    for entry in fs::read_dir(path).expect("read worktree dir") {
        let entry = entry.expect("read worktree entry");
        if entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_visible_worktree_files(root, &path, files);
        } else if path.is_file() {
            files.push(
                path.strip_prefix(root)
                    .expect("strip root")
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

fn absolute_git_dir(repo: &Path) -> std::path::PathBuf {
    fs::canonicalize(git(repo, ["rev-parse", "--absolute-git-dir"])).expect("canonical git dir")
}

fn visible_worktree_file_contents(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    collect_visible_worktree_file_contents(path, path, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn collect_visible_worktree_file_contents(
    root: &Path,
    path: &Path,
    files: &mut Vec<(String, Vec<u8>)>,
) {
    for entry in fs::read_dir(path).expect("read worktree dir") {
        let entry = entry.expect("read worktree entry");
        if entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_visible_worktree_file_contents(root, &path, files);
        } else if path.is_file() {
            files.push((
                path.strip_prefix(root)
                    .expect("strip root")
                    .to_string_lossy()
                    .replace('\\', "/"),
                fs::read(&path).expect("read worktree file"),
            ));
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SnapshotEntry {
    path: String,
    kind: SnapshotKind,
}

#[derive(Debug, PartialEq, Eq)]
enum SnapshotKind {
    File(Vec<u8>),
    Symlink(String),
}

fn snapshot_git_layout(repo_root: &Path, git_dir: &Path) -> Vec<SnapshotEntry> {
    let mut entries = Vec::new();
    collect_git_layout(repo_root, git_dir, git_dir, &mut entries);
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    entries
}

fn collect_git_layout(
    repo_root: &Path,
    git_dir: &Path,
    path: &Path,
    entries: &mut Vec<SnapshotEntry>,
) {
    for entry in fs::read_dir(path).expect("read git layout dir") {
        let entry = entry.expect("read git layout entry");
        let path = entry.path();
        let relative = path
            .strip_prefix(git_dir)
            .expect("strip git dir")
            .to_string_lossy()
            .replace('\\', "/");
        if should_skip_git_layout_path(&relative) {
            continue;
        }

        let metadata = fs::symlink_metadata(&path).expect("symlink metadata");
        if metadata.is_dir() {
            collect_git_layout(repo_root, git_dir, &path, entries);
            continue;
        }

        let kind = if metadata.file_type().is_symlink() {
            SnapshotKind::Symlink(normalize_snapshot_text(
                &fs::read_link(&path)
                    .expect("read symlink")
                    .to_string_lossy(),
                repo_root,
                git_dir,
            ))
        } else {
            let bytes = fs::read(&path).expect("read git layout file");
            SnapshotKind::File(normalize_snapshot_bytes(&bytes, repo_root, git_dir))
        };
        entries.push(SnapshotEntry {
            path: relative,
            kind,
        });
    }
}

fn should_skip_git_layout_path(relative: &str) -> bool {
    if relative == "index" || relative.ends_with(".lock") {
        return true;
    }
    false
}

fn normalize_snapshot_bytes(bytes: &[u8], repo_root: &Path, git_dir: &Path) -> Vec<u8> {
    match String::from_utf8(bytes.to_vec()) {
        Ok(text) => normalize_snapshot_text(&text, repo_root, git_dir).into_bytes(),
        Err(_) => bytes.to_vec(),
    }
}

fn normalize_snapshot_text(text: &str, repo_root: &Path, git_dir: &Path) -> String {
    let repo_root = repo_root.to_string_lossy().replace('\\', "/");
    let git_dir = git_dir.to_string_lossy().replace('\\', "/");
    text.replace(&git_dir, "$GIT_DIR")
        .replace(&repo_root, "$WORKTREE")
}

fn command_with_stdin(
    command: &str,
    cwd: &Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let mut child = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_stdin_bytes(
    command: &str,
    cwd: &Path,
    args: &[&str],
    stdin: &[u8],
    label: &str,
) -> String {
    let mut child = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin)
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}
