mod common;

use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
};

use tempfile::{Builder, TempDir};

use common::{required_pinned_stock_git, zmin_bin};

const GLOBAL_CONFIG_NULL: &str = "/dev/null";
const CANONICAL_CA83_ROOT: &str = "/Users/dschewchenko/.codex/worktrees/ca83/skron-git";
const CANONICAL_MAIN_ROOT: &str = "/Users/dschewchenko/work/private/skron-git";

fn fixture_root() -> TempDir {
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
    Builder::new()
        .prefix("zmin-mktree-options-")
        .tempdir_in("/private/tmp")
        .expect("create external fixture root")
}

fn run(program: &Path, repo: &Path, command_name: &str, args: &[&str], input: &[u8]) -> Output {
    let mut command = Command::new(program);
    command
        .args(["-C", repo.to_str().expect("repo path UTF-8")])
        .arg(command_name)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", GLOBAL_CONFIG_NULL)
        .env("GIT_CONFIG_SYSTEM", GLOBAL_CONFIG_NULL)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn command");
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(input)
        .expect("write command input");
    child.wait_with_output().expect("wait for command")
}

fn assert_same_output(stock: &Path, zmin: &Path, repo: &Path, args: &[&str]) {
    let stock_output = run(stock, repo, "mktree", args, b"");
    let zmin_output = run(zmin, repo, "mktree", args, b"");
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
        "mktree option output mismatch for {args:?}"
    );
}

#[test]
fn mktree_post_end_of_options_tokens_match_pinned_git() {
    let stock = required_pinned_stock_git();
    let zmin = Path::new(zmin_bin());
    let root = fixture_root();
    let repo = root.path().join("repo");
    fs::create_dir_all(&repo).expect("create repository root");
    let init = run(&stock, &repo, "init", &["--quiet"], b"");
    assert!(init.status.success(), "git init failed: {init:?}");

    let cases = [
        ["--", "--batch-command"].as_slice(),
        ["--", "--no-batch-command"].as_slice(),
        ["--", "--batch-command=value"].as_slice(),
        ["--", "--no-batch-command=value"].as_slice(),
        ["--", "--missing"].as_slice(),
        ["--", "--no-missing"].as_slice(),
        ["--", "--batch"].as_slice(),
        ["--", "--no-batch"].as_slice(),
        ["--", "-x", "arbitrary", "--missing"].as_slice(),
        ["foo"].as_slice(),
        ["foo", "--", "bar"].as_slice(),
        ["--"].as_slice(),
        ["--batch-command"].as_slice(),
        ["--no-batch-command"].as_slice(),
        ["--batch-command=value"].as_slice(),
        ["--no-batch-command=value"].as_slice(),
        ["foo", "--batch-command"].as_slice(),
    ];
    for args in cases {
        assert_same_output(&stock, zmin, &repo, args);
    }
}
