mod common;

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fmt, fs,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use flate2::{Compression, write::ZlibEncoder};

#[cfg(unix)]
use std::os::unix::{ffi::OsStringExt, io::AsRawFd, process::CommandExt};

use tempfile::TempDir;
use zmin_git_core::{GitHashAlgorithm, GitObjectHash};

use common::{
    command_any_output as common_command_any_output, command_any_output_with_stdin,
    command_any_output_with_stdin_bytes,
    command_failure_output_with_env as common_command_failure_output_with_env,
    command_output_with_env as common_command_output_with_env, command_stdout_bytes, git_args,
    git_status, pinned_git_init_sha256, required_pinned_stock_git, run_zmin, run_zmin_args,
    run_zmin_failure_output, run_zmin_status, run_zmin_with_env, run_zmin_with_stdin,
    run_zmin_with_stdin_bytes, zmin_bin,
};

fn first_pack_index(repo: &std::path::Path) -> std::path::PathBuf {
    let mut paths = fs::read_dir(repo.join(".git/objects/pack"))
        .expect("read pack dir")
        .map(|entry| entry.expect("pack entry").path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("idx"))
        .collect::<Vec<_>>();
    paths.sort();
    paths.into_iter().next().expect("pack index")
}

fn pack_contains_object(repo: &std::path::Path, object_id: &str) -> bool {
    git(
        repo,
        [
            "verify-pack",
            "--verbose",
            first_pack_index(repo).to_str().expect("pack index path"),
        ],
    )
    .lines()
    .any(|line| line.split_whitespace().next() == Some(object_id))
}

fn rewrite_pack_index_version(path: &std::path::Path, version: u32) -> Vec<u8> {
    let mut bytes = fs::read(path).expect("read pack index");
    bytes[4..8].copy_from_slice(&version.to_be_bytes());
    let checksum_start = bytes.len() - GitHashAlgorithm::Sha1.digest_len();
    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update(&bytes[..checksum_start]);
    let checksum = hasher.finalize();
    bytes[checksum_start..].copy_from_slice(checksum.as_bytes());
    bytes
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

fn loose_object_path(repo: &std::path::Path, hex: &str) -> std::path::PathBuf {
    repo.join(".git/objects").join(&hex[..2]).join(&hex[2..])
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

fn write_loose_object(
    repo: &std::path::Path,
    algorithm: GitHashAlgorithm,
    kind: &[u8],
    content: &[u8],
) -> String {
    let mut object = format!(
        "{} {}\0",
        String::from_utf8(kind.to_vec()).unwrap(),
        content.len()
    )
    .into_bytes();
    object.extend_from_slice(content);
    let mut hasher = GitObjectHash::new(algorithm);
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

fn init_promisor_work_repo(remote: &std::path::Path, branch: &str) -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(
        repo.path(),
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path utf8"),
        ],
    );
    git(repo.path(), ["fetch", "origin"]);
    git(
        repo.path(),
        ["checkout", "-B", branch, &format!("origin/{branch}")],
    );
    git(repo.path(), ["config", "remote.origin.promisor", "true"]);
    repo
}

fn raw_command_output(
    program: impl AsRef<OsStr>,
    cwd: &Path,
    args: &[&str],
    label: &str,
) -> (i32, Vec<u8>, Vec<u8>) {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|error| panic!("run {label}: {error}"));
    (
        output.status.code().expect("process exit code"),
        output.stdout,
        output.stderr,
    )
}

fn raw_command_output_with_env(
    program: impl AsRef<OsStr>,
    cwd: &Path,
    args: &[&str],
    envs: &[(&str, &Path)],
    label: &str,
) -> (i32, Vec<u8>, Vec<u8>) {
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    for (name, value) in envs {
        command.env(name, value);
    }
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("run {label}: {error}"));
    (
        output.status.code().expect("process exit code"),
        output.stdout,
        output.stderr,
    )
}

fn raw_command_output_with_stdin(
    program: impl AsRef<OsStr>,
    cwd: &Path,
    args: &[&str],
    stdin: &[u8],
    label: &str,
) -> (i32, Vec<u8>, Vec<u8>) {
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn {label}: {error}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin)
        .unwrap_or_else(|error| panic!("write {label}: {error}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("wait {label}: {error}"));
    (
        output.status.code().expect("process exit code"),
        output.stdout,
        output.stderr,
    )
}

fn git<const N: usize>(cwd: &Path, args: [&str; N]) -> String {
    common_command_output_with_env(
        required_pinned_stock_git()
            .to_str()
            .expect("pinned Git path is UTF-8"),
        cwd,
        &args,
        &[],
        "pinned Git",
    )
    .1
}

fn git_init() -> TempDir {
    let repo = TempDir::new().expect("temp repo");
    git(repo.path(), ["init"]);
    repo
}

fn clone_repo_fixture(source: &Path) -> TempDir {
    let repo = TempDir::new().expect("temp clone");
    let output = raw_command_output(
        required_pinned_stock_git(),
        repo.path(),
        &["clone", source.to_str().expect("source path"), "."],
        "pinned Git clone fixture",
    );
    assert_eq!(output.0, 0, "pinned Git clone failed: {output:?}");
    repo
}

fn configure_identity(repo: &Path) {
    git(repo, ["config", "user.name", "Bench"]);
    git(repo, ["config", "user.email", "bench@example.test"]);
    git(repo, ["config", "commit.gpgsign", "false"]);
}

fn git_with_env<const N: usize>(cwd: &Path, args: [&str; N]) -> String {
    common_command_output_with_env(
        required_pinned_stock_git()
            .to_str()
            .expect("pinned Git path is UTF-8"),
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
        "pinned Git",
    )
    .1
}

fn git_with_stdin<const N: usize>(cwd: &Path, args: [&str; N], stdin: &str) -> String {
    command_any_output_with_stdin(
        required_pinned_stock_git()
            .to_str()
            .expect("pinned Git path is UTF-8"),
        cwd,
        &args,
        stdin,
        "pinned Git",
    )
    .1
}

fn git_with_stdin_bytes<const N: usize>(cwd: &Path, args: [&str; N], stdin: &[u8]) -> String {
    command_any_output_with_stdin_bytes(
        required_pinned_stock_git()
            .to_str()
            .expect("pinned Git path is UTF-8"),
        cwd,
        &args,
        stdin,
        "pinned Git",
    )
    .1
}

fn git_failure_output(cwd: &Path, args: &[&str]) -> (i32, String, String) {
    common_command_failure_output_with_env(
        required_pinned_stock_git()
            .to_str()
            .expect("pinned Git path is UTF-8"),
        cwd,
        args,
        &[],
        "pinned Git",
    )
}

fn command_any_output(
    command: &str,
    cwd: &Path,
    args: &[&str],
    label: &str,
) -> (i32, String, String) {
    if command == "git" {
        let output = raw_command_output(required_pinned_stock_git(), cwd, args, label);
        return (
            output.0,
            String::from_utf8(output.1)
                .expect("pinned Git stdout UTF-8")
                .trim_end_matches('\n')
                .to_owned(),
            String::from_utf8(output.2)
                .expect("pinned Git stderr UTF-8")
                .trim_end_matches('\n')
                .to_owned(),
        );
    }
    common_command_any_output(command, cwd, args, label)
}

fn command_output_with_env(
    command: &str,
    cwd: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    label: &str,
) -> (i32, String, String) {
    if command == "git" {
        return common_command_output_with_env(
            required_pinned_stock_git()
                .to_str()
                .expect("pinned Git path is UTF-8"),
            cwd,
            args,
            env,
            label,
        );
    }
    common_command_output_with_env(command, cwd, args, env, label)
}

fn command_failure_output_with_env(
    command: &str,
    cwd: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    label: &str,
) -> (i32, String, String) {
    if command == "git" {
        return common_command_failure_output_with_env(
            required_pinned_stock_git()
                .to_str()
                .expect("pinned Git path is UTF-8"),
            cwd,
            args,
            env,
            label,
        );
    }
    common_command_failure_output_with_env(command, cwd, args, env, label)
}

fn assert_cat_file_long_alias_matches_short(
    repo: &Path,
    long_option: &str,
    short_option: &str,
    object_id: &str,
    case_name: &str,
) {
    let long = raw_command_output(
        zmin_bin(),
        repo,
        &["cat-file", long_option, object_id],
        "zmin cat-file long alias",
    );
    let short = raw_command_output(
        zmin_bin(),
        repo,
        &["cat-file", short_option, object_id],
        "zmin cat-file short alias",
    );
    assert_eq!(
        long, short,
        "cat-file {long_option} must be byte-identical to {short_option} for {case_name}"
    );
}

fn show_ref_fixture(stock: &Path, sha256: bool) -> TempDir {
    let repo = TempDir::new().expect("show-ref fixture repo");
    let init_args = if sha256 {
        vec!["init", "-q", "--object-format=sha256"]
    } else {
        vec!["init", "-q"]
    };
    let output = raw_command_output(stock, repo.path(), &init_args, "pinned show-ref init");
    assert_eq!(output.0, 0, "pinned show-ref init failed: {output:?}");
    for (key, value) in [
        ("user.name", "Show Ref"),
        ("user.email", "show-ref@test"),
        ("core.abbrev", "12"),
    ] {
        let output = raw_command_output(
            stock,
            repo.path(),
            &["config", key, value],
            "pinned show-ref config",
        );
        assert_eq!(output.0, 0, "pinned show-ref config failed: {output:?}");
    }
    let algorithm = if sha256 {
        GitHashAlgorithm::Sha256
    } else {
        GitHashAlgorithm::Sha1
    };
    let tree = write_loose_object(repo.path(), algorithm, b"tree", b"");
    let commit = write_loose_object(
        repo.path(),
        algorithm,
        b"commit",
        format!(
            "tree {tree}\nauthor Show Ref <show-ref@test> 0 +0000\ncommitter Show Ref <show-ref@test> 0 +0000\n\ninitial\n"
        )
        .as_bytes(),
    );
    fs::create_dir_all(repo.path().join(".git/refs/heads")).expect("create show-ref heads");
    fs::write(
        repo.path().join(".git/refs/heads/master"),
        format!("{commit}\n"),
    )
    .expect("write show-ref master");
    fs::write(
        repo.path().join(".git/refs/heads/side"),
        format!("{commit}\n"),
    )
    .expect("write show-ref side");
    let tag = format!(
        "object {commit}\ntype commit\ntag v1\ntagger Show Ref <show-ref@test> 0 +0000\n\nannotated\n"
    );
    let tag_id = write_loose_object(repo.path(), algorithm, b"tag", tag.as_bytes());
    fs::create_dir_all(repo.path().join(".git/refs/tags")).expect("create show-ref tags");
    fs::write(repo.path().join(".git/refs/tags/v1"), format!("{tag_id}\n"))
        .expect("write deterministic annotated tag");
    let output = raw_command_output(
        stock,
        repo.path(),
        &["pack-refs", "--all"],
        "pinned show-ref pack refs",
    );
    assert_eq!(output.0, 0, "pinned show-ref pack refs failed: {output:?}");
    for (name, payload) in show_ref_collision_payloads(sha256) {
        let id = write_loose_object(repo.path(), algorithm, b"blob", payload);
        let path = repo.path().join(".git/refs/heads").join(name);
        fs::write(path, format!("{id}\n")).expect("write show-ref collision ref");
    }
    repo
}

fn try_show_ref_reftable_fixture(stock: &Path, sha256: bool) -> Option<TempDir> {
    let repo = TempDir::new().expect("show-ref reftable fixture repo");
    let init_output = if sha256 {
        raw_command_output(
            stock,
            repo.path(),
            &[
                "init",
                "-q",
                "--ref-format=reftable",
                "--object-format=sha256",
            ],
            "pinned show-ref reftable init",
        )
    } else {
        raw_command_output(
            stock,
            repo.path(),
            &["init", "-q", "--ref-format=reftable"],
            "pinned show-ref reftable init",
        )
    };
    if init_output.0 != 0 {
        return None;
    }
    for (key, value) in [
        ("user.name", "Show Ref Reftable"),
        ("user.email", "show-ref-reftable@test"),
        ("core.abbrev", "12"),
    ] {
        let output = raw_command_output(
            stock,
            repo.path(),
            &["config", key, value],
            "pinned show-ref reftable config",
        );
        assert_eq!(output.0, 0, "reftable config failed: {output:?}");
    }
    let object = write_reftable_object(stock, repo.path(), "blob", b"reftable\n");
    let output = raw_command_output(
        stock,
        repo.path(),
        &["update-ref", "refs/other/object", &object],
        "pinned show-ref reftable update-ref",
    );
    assert_eq!(output.0, 0, "reftable update-ref failed: {output:?}");
    Some(repo)
}

fn write_reftable_object(program: &Path, repo: &Path, kind: &str, content: &[u8]) -> String {
    let output = raw_command_output_with_stdin(
        program,
        repo,
        &["hash-object", "-w", "--stdin", "-t", kind],
        content,
        "write show-ref reftable object",
    );
    assert_eq!(output.0, 0, "reftable object write failed: {output:?}");
    String::from_utf8(output.1)
        .expect("reftable object id is utf8")
        .trim()
        .to_owned()
}

fn show_ref_collision_payloads(sha256: bool) -> [(&'static str, &'static [u8]); 2] {
    if sha256 {
        [
            ("collision-a", b"abbrev-sha256-collision-004695"),
            ("collision-b", b"abbrev-sha256-collision-020299"),
        ]
    } else {
        [
            ("collision-a", b"abbrev-sha1-collision-006687"),
            ("collision-b", b"abbrev-sha1-collision-040110"),
        ]
    }
}

fn assert_show_ref_tuple(
    stock: &Path,
    stock_repo: &Path,
    zmin_repo: &Path,
    args: &[&str],
    input: Option<&[u8]>,
) {
    let run = |program: &Path, repo: &Path, label: &str| {
        input.map_or_else(
            || raw_command_output(program, repo, args, label),
            |input| raw_command_output_with_stdin(program, repo, args, input, label),
        )
    };
    let stock_output = run(stock, stock_repo, "pinned show-ref");
    let zmin_output = run(Path::new(zmin_bin()), zmin_repo, "zmin show-ref");
    assert_eq!(
        zmin_output, stock_output,
        "show-ref tuple mismatch for {:?}",
        args
    );
}

fn assert_show_ref_same_repo_tuple(stock: &Path, repo: &Path, args: &[&str], input: Option<&[u8]>) {
    let run = |program: &Path, label: &str| {
        input.map_or_else(
            || raw_command_output(program, repo, args, label),
            |input| raw_command_output_with_stdin(program, repo, args, input, label),
        )
    };
    let stock_output = run(stock, "pinned show-ref same reftable repo");
    let zmin_output = run(Path::new(zmin_bin()), "zmin show-ref same reftable repo");
    assert_eq!(
        zmin_output, stock_output,
        "show-ref tuple mismatch for {:?} on shared reftable repo",
        args
    );
}

#[test]
fn show_ref_sha1_sha256_backend_and_input_contract_matches_stock_git() {
    let stock = pinned_stock_git_bin();
    for sha256 in [false, true] {
        let stock_repo = show_ref_fixture(&stock, sha256);
        let zmin_repo = show_ref_fixture(&stock, sha256);
        for args in [
            &["show-ref"][..],
            &["show-ref", "--hash"][..],
            &["show-ref", "--hash=7"][..],
            &["show-ref", "--hash=0"][..],
            &["show-ref", "--hash=7", "--no-abbrev"][..],
            &["show-ref", "--abbrev=0"][..],
            &["show-ref", "--abbrev=3"][..],
            &["show-ref", "--abbrev"][..],
            &["show-ref", "--no-abbrev"][..],
            &["show-ref", "--hash=7", "--abbrev=12"][..],
            &["show-ref", "--abbrev=12", "--hash=7"][..],
            &["show-ref", "--abbrev=12", "-s7"][..],
            &["show-ref", "--no-abbrev", "-s7"][..],
            &["show-ref", "-s7", "--no-abbrev"][..],
            &["show-ref", "--head", "--tags"][..],
            &["show-ref", "--heads"][..],
            &["show-ref", "--branches"][..],
            &["show-ref", "--tags"][..],
            &["show-ref", "--head"][..],
            &["show-ref", "master"][..],
            &["show-ref", "--dereference"][..],
            &["show-ref", "--verify", "refs/heads/master"][..],
            &["show-ref", "--verify", "--quiet", "refs/heads/missing"][..],
            &["show-ref", "--verify"][..],
            &["show-ref", "--verify", "--exists", "refs/heads/master"][..],
            &["show-ref", "--exists", "refs/heads/master"][..],
            &["show-ref", "--exists", "refs/heads/missing"][..],
            &["show-ref", "--exists", "refs/heads/collision-a"][..],
        ] {
            assert_show_ref_tuple(&stock, stock_repo.path(), zmin_repo.path(), args, None);
        }
        let invalid_input = b"refs/heads/master\n";
        for repo in [stock_repo.path(), zmin_repo.path()] {
            let output = raw_command_output(
                &stock,
                repo,
                &["config", "core.abbrev", "0"],
                "configure invalid show-ref core.abbrev",
            );
            assert_eq!(output.0, 0, "configure core.abbrev=0 failed: {output:?}");
        }
        for (args, input) in [
            (&["show-ref", "--exists", "refs/heads/master"][..], None),
            (
                &["show-ref", "--exclude-existing"][..],
                Some(invalid_input.as_slice()),
            ),
            (
                &["show-ref", "--verify", "--quiet", "refs/heads/missing"][..],
                None,
            ),
            (&["show-ref", "--verify"][..], None),
            (
                &["show-ref", "--verify", "--exists", "refs/heads/master"][..],
                None,
            ),
        ] {
            assert_show_ref_tuple(&stock, stock_repo.path(), zmin_repo.path(), args, input);
        }
        for repo in [stock_repo.path(), zmin_repo.path()] {
            let output = raw_command_output(
                &stock,
                repo,
                &["config", "core.abbrev", "12"],
                "restore show-ref core.abbrev",
            );
            assert_eq!(output.0, 0, "restore core.abbrev failed: {output:?}");
        }
        for repo in [stock_repo.path(), zmin_repo.path()] {
            let output = raw_command_output(
                &stock,
                repo,
                &["config", "core.abbrev", "no"],
                "configure show-ref no abbreviation",
            );
            assert_eq!(output.0, 0, "configure core.abbrev=no failed: {output:?}");
        }
        assert_show_ref_tuple(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            &["show-ref", "--abbrev"],
            None,
        );
        let bad_ref_width = if sha256 { 64 } else { 40 };
        for repo in [stock_repo.path(), zmin_repo.path()] {
            fs::write(
                repo.join(".git/refs/heads/bad"),
                format!("{:0width$}\n", 0, width = bad_ref_width),
            )
            .expect("write bad show-ref ref");
        }
        for args in [
            &["show-ref"][..],
            &["show-ref", "--verify", "refs/heads/bad"][..],
            &["show-ref", "--verify", "--quiet", "refs/heads/bad"][..],
        ] {
            assert_show_ref_tuple(&stock, stock_repo.path(), zmin_repo.path(), args, None);
        }
        for repo in [stock_repo.path(), zmin_repo.path()] {
            fs::write(
                repo.join(".git/refs/heads/zzzz-bad"),
                format!("{:0width$}\n", 0, width = bad_ref_width),
            )
            .expect("write late bad show-ref ref");
        }
        assert_show_ref_tuple(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            &["show-ref"],
            None,
        );
        assert_show_ref_tuple(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            &["show-ref", "--abbrev=12"],
            None,
        );
        for repo in [stock_repo.path(), zmin_repo.path()] {
            fs::write(
                repo.join(".git/refs/heads/dangling"),
                "ref: refs/heads/missing\n",
            )
            .expect("write dangling show-ref symbolic ref");
            fs::write(repo.join(".git/refs/heads/enotdir"), "not-a-directory\n")
                .expect("write show-ref ENOTDIR blocker");
            fs::create_dir_all(repo.join(".git/refs/heads/foo"))
                .expect("create non-empty show-ref directory");
            fs::write(repo.join(".git/refs/heads/foo/bar"), b"directory child\n")
                .expect("write non-empty show-ref directory child");
        }
        assert_show_ref_tuple(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            &["show-ref", "--exists", "refs/heads/dangling"],
            None,
        );
        assert_show_ref_tuple(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            &["show-ref", "--exists", "refs/heads/enotdir/child"],
            None,
        );
        let input = b"refs/heads/master\nrefs/heads/new\nrefs/heads/foo\nabc refs/heads/new^{}\nraw refs/heads/\x80\nbad refs/heads/\0\n";
        assert_show_ref_tuple(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            &["show-ref", "--exclude-existing"],
            Some(input),
        );
        assert_show_ref_tuple(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            &["show-ref", "--exclude-existing=refs/heads/"],
            Some(input),
        );
        let crlf_input = b"refs/heads/new\r\nrefs/heads/foo\r\nrefs/heads/final";
        assert_show_ref_tuple(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            &["show-ref", "--exclude-existing"],
            Some(crlf_input),
        );
        for input in [vec![b'x'; 1023], vec![b'x'; 1024]] {
            assert_show_ref_tuple(
                &stock,
                stock_repo.path(),
                zmin_repo.path(),
                &["show-ref", "--exclude-existing"],
                Some(&input),
            );
        }
        let boundary_input = vec![b'x'; 1024];
        assert_show_ref_tuple(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            &["show-ref", "--exclude-existing"],
            Some(&boundary_input),
        );
        for repo in [stock_repo.path(), zmin_repo.path()] {
            let id = write_loose_object(
                repo,
                if sha256 {
                    GitHashAlgorithm::Sha256
                } else {
                    GitHashAlgorithm::Sha1
                },
                b"raw-packed-object",
                b"raw-packed-object",
            );
            let mut packed = fs::OpenOptions::new()
                .append(true)
                .open(repo.join(".git/packed-refs"))
                .expect("open packed refs for raw name");
            write!(packed, "{id} refs/heads/").expect("write raw packed ref id");
            packed
                .write_all(&[0x80])
                .expect("write raw packed ref byte");
            packed.write_all(b"\n").expect("terminate raw packed ref");
        }
        let raw_packed_input = b"raw refs/heads/\x80\nraw refs/heads/\x81\n";
        assert_show_ref_tuple(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            &["show-ref", "--exclude-existing"],
            Some(raw_packed_input),
        );
        let stock_reftable = try_show_ref_reftable_fixture(&stock, sha256);
        if let Some(stock_reftable) = stock_reftable {
            for args in [
                &["show-ref", "--exists", "refs/other/object"][..],
                &["show-ref", "--exists", "refs/heads/missing"][..],
                &["show-ref"][..],
            ] {
                assert_show_ref_same_repo_tuple(&stock, stock_reftable.path(), args, None);
            }
        }
    }
}

fn pinned_stock_git_bin() -> PathBuf {
    let path = std::env::var_os("ZMIN_STOCK_GIT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!(
                "cat-file extension evidence requires ZMIN_STOCK_GIT to select pinned Git 2.55.0"
            )
        });
    assert!(
        path.is_absolute(),
        "ZMIN_STOCK_GIT must be an absolute path, got {}",
        path.display()
    );
    let metadata = fs::symlink_metadata(&path)
        .unwrap_or_else(|error| panic!("stat ZMIN_STOCK_GIT {}: {error}", path.display()));
    assert!(
        metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
        "ZMIN_STOCK_GIT must name a regular non-symlink file: {}",
        path.display()
    );
    let canonical = fs::canonicalize(&path)
        .unwrap_or_else(|error| panic!("canonicalize ZMIN_STOCK_GIT {}: {error}", path.display()));
    assert_eq!(
        canonical, path,
        "ZMIN_STOCK_GIT must already be the canonical non-symlink path"
    );
    let canonical_metadata = fs::symlink_metadata(&canonical).unwrap_or_else(|error| {
        panic!(
            "stat canonical ZMIN_STOCK_GIT {}: {error}",
            canonical.display()
        )
    });
    assert!(
        canonical_metadata.file_type().is_file() && !canonical_metadata.file_type().is_symlink(),
        "canonical ZMIN_STOCK_GIT must be a regular non-symlink file: {}",
        canonical.display()
    );
    let version = Command::new(&canonical)
        .arg("--version")
        .output()
        .unwrap_or_else(|error| panic!("run ZMIN_STOCK_GIT --version: {error}"));
    let reported = String::from_utf8_lossy(&version.stdout);
    let reported = reported.trim_end_matches(|character| character == '\r' || character == '\n');
    assert!(
        version.status.success() && reported == "git version 2.55.0",
        "ZMIN_STOCK_GIT must report exactly `git version 2.55.0`, got status {:?} and stdout {:?}",
        version.status.code(),
        reported
    );
    canonical
}

const HASH_OBJECT_TEST_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq, Eq)]
struct HashObjectCommandOutput {
    status: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Debug)]
struct HashObjectBatchCase {
    name: &'static str,
    args: Vec<&'static str>,
    input: Vec<u8>,
}

struct HashObjectByteReader {
    receiver: mpsc::Receiver<io::Result<Vec<u8>>>,
    handle: thread::JoinHandle<()>,
}

struct HashObjectLineReader {
    receiver: mpsc::Receiver<io::Result<Option<Vec<u8>>>>,
    handle: thread::JoinHandle<()>,
}

struct HashObjectBatchReaders {
    stdout: HashObjectByteReader,
    stderr: HashObjectByteReader,
}

struct HashObjectPersistentReaders {
    stdout: HashObjectLineReader,
    stderr: HashObjectByteReader,
}

fn spawn_hash_object_pipe_reader<R: Read + Send + 'static>(mut reader: R) -> HashObjectByteReader {
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        let mut output = Vec::new();
        let result = reader.read_to_end(&mut output).map(|_| output);
        let _ = sender.send(result);
    });
    HashObjectByteReader { receiver, handle }
}

fn spawn_hash_object_stdout_reader(stdout: ChildStdout) -> HashObjectLineReader {
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = Vec::new();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => {
                    let _ = sender.send(Ok(None));
                    break;
                }
                Ok(_) => {
                    if sender.send(Ok(Some(line))).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error));
                    break;
                }
            }
        }
    });
    HashObjectLineReader { receiver, handle }
}

impl HashObjectByteReader {
    fn drain_and_join(self) {
        let _ = self.receiver.recv_timeout(Duration::from_secs(1));
        let _ = self.handle.join();
    }
}

impl HashObjectLineReader {
    fn drain_and_join(self) {
        let _ = self.receiver.recv_timeout(Duration::from_secs(1));
        let _ = self.handle.join();
    }
}

impl HashObjectBatchReaders {
    fn drain_and_join(self) {
        self.stdout.drain_and_join();
        self.stderr.drain_and_join();
    }

    fn join(self) {
        let _ = self.stdout.handle.join();
        let _ = self.stderr.handle.join();
    }
}

impl HashObjectPersistentReaders {
    fn drain_and_join(self) {
        self.stdout.drain_and_join();
        self.stderr.drain_and_join();
    }

    fn join(self) {
        let _ = self.stdout.handle.join();
        let _ = self.stderr.handle.join();
    }
}

#[cfg(not(unix))]
struct HashObjectWriterTransfer {
    stdin: ChildStdin,
    result: io::Result<()>,
}

#[cfg(not(unix))]
struct HashObjectPendingWriter {
    receiver: mpsc::Receiver<HashObjectWriterTransfer>,
    handle: thread::JoinHandle<()>,
}

struct HashObjectInputWriter {
    stdin: Option<ChildStdin>,
    #[cfg(not(unix))]
    pending: Option<HashObjectPendingWriter>,
}

impl HashObjectInputWriter {
    fn new(stdin: ChildStdin) -> Self {
        Self {
            stdin: Some(stdin),
            #[cfg(not(unix))]
            pending: None,
        }
    }

    fn write_record(&mut self, input: &[u8], deadline: Instant) -> io::Result<()> {
        #[cfg(unix)]
        {
            return write_hash_object_record(
                self.stdin
                    .as_mut()
                    .expect("hash-object stdin already closed"),
                input,
                deadline,
            );
        }
        #[cfg(not(unix))]
        {
            let stdin = self.stdin.take().expect("hash-object stdin already closed");
            let input = input.to_vec();
            let (sender, receiver) = mpsc::channel();
            let handle = thread::spawn(move || {
                let mut stdin = stdin;
                let result = stdin.write_all(&input).and_then(|()| stdin.flush());
                let _ = sender.send(HashObjectWriterTransfer { stdin, result });
            });
            match receiver.recv_timeout(hash_object_remaining(deadline)) {
                Ok(transfer) => {
                    let _ = handle.join();
                    self.stdin = Some(transfer.stdin);
                    transfer.result
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.pending = Some(HashObjectPendingWriter { receiver, handle });
                    Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "hash-object stdin write exceeded deadline",
                    ))
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = handle.join();
                    Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "hash-object stdin writer disconnected",
                    ))
                }
            }
        }
    }

    fn join_after_kill(&mut self) {
        #[cfg(not(unix))]
        if let Some(pending) = self.pending.take() {
            let _ = pending.receiver.recv();
            let _ = pending.handle.join();
        }
    }
}

fn terminate_hash_object_process(child: &mut Child) {
    #[cfg(unix)]
    {
        let process_group = -(child.id() as libc::pid_t);
        // SAFETY: the child was started in its own process group by process_group(0).
        unsafe {
            libc::kill(process_group, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    let _ = child.wait();
}

fn hash_object_remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

#[derive(Debug)]
enum HashObjectPipeError {
    Read {
        label: &'static str,
        error: io::Error,
    },
    Deadline {
        label: &'static str,
    },
}

impl fmt::Display for HashObjectPipeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { label, error } => write!(formatter, "{label}: {error}"),
            Self::Deadline { label } => {
                write!(formatter, "{label} exceeded the 10 second deadline")
            }
        }
    }
}

fn receive_hash_object_pipe(
    reader: &HashObjectByteReader,
    deadline: Instant,
    label: &'static str,
) -> Result<Vec<u8>, HashObjectPipeError> {
    match reader
        .receiver
        .recv_timeout(hash_object_remaining(deadline))
    {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(HashObjectPipeError::Read { label, error }),
        Err(_) => Err(HashObjectPipeError::Deadline { label }),
    }
}

fn run_hash_object_batch(
    program: &Path,
    cwd: &Path,
    args: &[&str],
    input: &[u8],
) -> HashObjectCommandOutput {
    let deadline = Instant::now() + HASH_OBJECT_TEST_DEADLINE;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("spawn hash-object {}: {error}", program.display()));
    let mut writer =
        HashObjectInputWriter::new(child.stdin.take().expect("hash-object stdin pipe"));
    let readers = HashObjectBatchReaders {
        stdout: spawn_hash_object_pipe_reader(child.stdout.take().expect("hash-object stdout")),
        stderr: spawn_hash_object_pipe_reader(child.stderr.take().expect("hash-object stderr")),
    };
    if let Err(error) = writer.write_record(input, deadline) {
        terminate_hash_object_process(&mut child);
        writer.join_after_kill();
        readers.drain_and_join();
        panic!(
            "hash-object {} stdin transmission failed: {error}",
            program.display()
        );
    }
    drop(writer);

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                terminate_hash_object_process(&mut child);
                readers.drain_and_join();
                panic!("poll hash-object {} status: {error}", program.display());
            }
        }
        if hash_object_remaining(deadline).is_zero() {
            terminate_hash_object_process(&mut child);
            readers.drain_and_join();
            panic!(
                "hash-object {} exceeded the 10 second deadline",
                program.display()
            );
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = match receive_hash_object_pipe(&readers.stdout, deadline, "stdout") {
        Ok(output) => output,
        Err(error) => {
            terminate_hash_object_process(&mut child);
            readers.drain_and_join();
            panic!("read hash-object stdout: {error}");
        }
    };
    let stderr = match receive_hash_object_pipe(&readers.stderr, deadline, "stderr") {
        Ok(output) => output,
        Err(error) => {
            terminate_hash_object_process(&mut child);
            readers.drain_and_join();
            panic!("read hash-object stderr: {error}");
        }
    };
    readers.join();
    HashObjectCommandOutput {
        status: status.code().unwrap_or(1),
        stdout,
        stderr,
    }
}

#[cfg(unix)]
fn write_hash_object_record(
    stdin: &mut ChildStdin,
    input: &[u8],
    deadline: Instant,
) -> io::Result<()> {
    let fd = stdin.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut written = 0;
        while written < input.len() {
            if hash_object_remaining(deadline).is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "hash-object stdin write exceeded deadline",
                ));
            }
            let count =
                unsafe { libc::write(fd, input[written..].as_ptr().cast(), input.len() - written) };
            if count >= 0 {
                written += count as usize;
                continue;
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::WouldBlock {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            return Err(error);
        }
        stdin.flush()
    })();
    let restore = unsafe { libc::fcntl(fd, libc::F_SETFL, flags) };
    if result.is_ok() && restore < 0 {
        return Err(io::Error::last_os_error());
    }
    result
}

#[derive(Debug)]
enum HashObjectResponseError {
    Eof,
    Read(io::Error),
    Deadline,
}

impl fmt::Display for HashObjectResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eof => write!(formatter, "closed stdout before returning an object id"),
            Self::Read(error) => write!(formatter, "stdout read failed: {error}"),
            Self::Deadline => write!(formatter, "response exceeded the 10 second deadline"),
        }
    }
}

fn receive_hash_object_response(
    reader: &HashObjectLineReader,
    deadline: Instant,
) -> Result<Vec<u8>, HashObjectResponseError> {
    match reader
        .receiver
        .recv_timeout(hash_object_remaining(deadline))
    {
        Ok(Ok(Some(line))) => Ok(line),
        Ok(Ok(None)) => Err(HashObjectResponseError::Eof),
        Ok(Err(error)) => Err(HashObjectResponseError::Read(error)),
        Err(_) => Err(HashObjectResponseError::Deadline),
    }
}

fn run_hash_object_persistent(
    program: &Path,
    cwd: &Path,
    paths: &[&[u8]],
) -> HashObjectCommandOutput {
    let deadline = Instant::now() + HASH_OBJECT_TEST_DEADLINE;
    let mut command = Command::new(program);
    command
        .args(["hash-object", "-w", "--stdin-paths", "--no-filters"])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("spawn persistent hash-object: {error}"));
    let mut writer =
        HashObjectInputWriter::new(child.stdin.take().expect("persistent hash-object stdin"));
    let readers = HashObjectPersistentReaders {
        stdout: spawn_hash_object_stdout_reader(child.stdout.take().expect("persistent stdout")),
        stderr: spawn_hash_object_pipe_reader(child.stderr.take().expect("persistent stderr")),
    };
    let mut stdout_bytes = Vec::new();
    for path in paths {
        let mut record = Vec::with_capacity(path.len() + 1);
        record.extend_from_slice(path);
        record.push(b'\n');
        if let Err(error) = writer.write_record(&record, deadline) {
            terminate_hash_object_process(&mut child);
            writer.join_after_kill();
            readers.drain_and_join();
            panic!("write persistent hash-object pathname: {error}");
        }
        let response = match receive_hash_object_response(&readers.stdout, deadline) {
            Ok(response) => response,
            Err(error) => {
                terminate_hash_object_process(&mut child);
                writer.join_after_kill();
                readers.drain_and_join();
                panic!("read persistent hash-object response: {error}");
            }
        };
        if !response.ends_with(b"\n") {
            terminate_hash_object_process(&mut child);
            writer.join_after_kill();
            readers.drain_and_join();
            panic!("hash-object response lacks LF");
        }
        stdout_bytes.extend_from_slice(&response);
    }
    drop(writer);

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                terminate_hash_object_process(&mut child);
                readers.drain_and_join();
                panic!("poll persistent hash-object status: {error}");
            }
        }
        if hash_object_remaining(deadline).is_zero() {
            terminate_hash_object_process(&mut child);
            readers.drain_and_join();
            panic!("persistent hash-object exceeded the 10 second deadline");
        }
        thread::sleep(Duration::from_millis(10));
    };
    loop {
        match readers
            .stdout
            .receiver
            .recv_timeout(hash_object_remaining(deadline))
        {
            Ok(Ok(Some(line))) => {
                stdout_bytes.extend_from_slice(&line);
                readers.drain_and_join();
                panic!("persistent hash-object emitted an extra response after EOF");
            }
            Ok(Ok(None)) => break,
            Ok(Err(error)) => {
                readers.drain_and_join();
                panic!("read persistent hash-object stdout: {error}");
            }
            Err(error) => {
                readers.drain_and_join();
                panic!("drain persistent hash-object stdout: {error}");
            }
        }
    }
    let stderr = match receive_hash_object_pipe(&readers.stderr, deadline, "stderr") {
        Ok(output) => output,
        Err(error) => {
            readers.drain_and_join();
            panic!("read persistent hash-object stderr: {error}");
        }
    };
    readers.join();
    HashObjectCommandOutput {
        status: status.code().unwrap_or(1),
        stdout: stdout_bytes,
        stderr,
    }
}

fn init_exact_hash_object_repo(stock_git: &Path) -> TempDir {
    let repo = TempDir::new().expect("hash-object fixture repo");
    let output = run_hash_object_batch(stock_git, repo.path(), &["init", "-q"], &[]);
    assert_eq!(output.status, 0, "pinned Git init failed: {output:?}");
    fs::write(repo.path().join("one.txt"), b"one\n").expect("write one.txt");
    fs::write(repo.path().join("two.txt"), b"two\n").expect("write two.txt");
    fs::write(repo.path().join("with space.txt"), b"space\n").expect("write spaced file");
    fs::write(repo.path().join("escaped name.txt"), b"escaped\n").expect("write escaped file");
    fs::write(repo.path().join("tab\tname.txt"), b"tab\n").expect("write tab file");
    fs::write(repo.path().join("octal name.txt"), b"octal\n").expect("write octal file");
    repo
}

fn assert_hash_object_batch_case(stock_git: &Path, case: &HashObjectBatchCase) {
    let stock_repo = init_exact_hash_object_repo(stock_git);
    let zmin_repo = init_exact_hash_object_repo(stock_git);
    let stock = run_hash_object_batch(stock_git, stock_repo.path(), &case.args, &case.input);
    let zmin = run_hash_object_batch(
        Path::new(zmin_bin()),
        zmin_repo.path(),
        &case.args,
        &case.input,
    );
    assert_eq!(zmin, stock, "hash-object stdin-paths case {}", case.name);
}

#[test]
fn hash_object_stdin_paths_persistent_bidi_matches_pinned_git() {
    let stock_git = pinned_stock_git_bin();
    let stock_repo = init_exact_hash_object_repo(&stock_git);
    let zmin_repo = init_exact_hash_object_repo(&stock_git);
    let paths = [b"one.txt".as_slice(), b"two.txt".as_slice()];

    let stock = run_hash_object_persistent(&stock_git, stock_repo.path(), &paths);
    let zmin = run_hash_object_persistent(Path::new(zmin_bin()), zmin_repo.path(), &paths);
    assert_eq!(zmin, stock);
    assert_eq!(stock.status, 0);
    assert!(stock.stderr.is_empty());
    let object_ids = stock
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| String::from_utf8(line.to_vec()).expect("object id"))
        .collect::<Vec<_>>();
    assert_eq!(object_ids.len(), 2);

    for (program, repo) in [
        (&stock_git, &stock_repo),
        (&PathBuf::from(zmin_bin()), &zmin_repo),
    ] {
        for (object_id, expected) in object_ids.iter().zip([b"one\n", b"two\n"]) {
            let output =
                run_hash_object_batch(program, repo.path(), &["cat-file", "-p", object_id], &[]);
            assert_eq!(output.status, 0);
            assert_eq!(output.stdout, expected);
            assert!(output.stderr.is_empty());
        }
    }
}

#[test]
fn hash_object_stdin_paths_batch_edges_match_pinned_git() {
    let stock_git = pinned_stock_git_bin();
    let cases = vec![
        HashObjectBatchCase {
            name: "final unterminated",
            args: vec!["hash-object", "--stdin-paths"],
            input: b"one.txt".to_vec(),
        },
        HashObjectBatchCase {
            name: "CRLF",
            args: vec!["hash-object", "--stdin-paths"],
            input: b"one.txt\r\n".to_vec(),
        },
        HashObjectBatchCase {
            name: "empty record after successful record",
            args: vec!["hash-object", "--stdin-paths"],
            input: b"one.txt\n\n".to_vec(),
        },
        HashObjectBatchCase {
            name: "missing path",
            args: vec!["hash-object", "--stdin-paths"],
            input: b"missing.txt\n".to_vec(),
        },
        HashObjectBatchCase {
            name: "malformed C quote",
            args: vec!["hash-object", "--stdin-paths"],
            input: b"\"bad\\q\"\n".to_vec(),
        },
        HashObjectBatchCase {
            name: "quoted spaces",
            args: vec!["hash-object", "--stdin-paths"],
            input: b"\"with space.txt\"\n".to_vec(),
        },
        HashObjectBatchCase {
            name: "trailing material after closing C quote",
            args: vec!["hash-object", "--stdin-paths"],
            input: b"\"with space.txt\"trailing\n".to_vec(),
        },
        HashObjectBatchCase {
            name: "standard escape",
            args: vec!["hash-object", "--stdin-paths"],
            input: b"\"tab\\tname.txt\"\n".to_vec(),
        },
        HashObjectBatchCase {
            name: "octal escape",
            args: vec!["hash-object", "--stdin-paths"],
            input: b"\"octal\\040name.txt\"\n".to_vec(),
        },
        HashObjectBatchCase {
            name: "repeated path",
            args: vec!["hash-object", "--stdin-paths"],
            input: b"one.txt\none.txt\n".to_vec(),
        },
        HashObjectBatchCase {
            name: "literal mode",
            args: vec!["hash-object", "--literally", "--stdin-paths"],
            input: b"one.txt\n".to_vec(),
        },
        HashObjectBatchCase {
            name: "NUL path boundary",
            args: vec!["hash-object", "--stdin-paths"],
            input: b"one.txt\0ignored\n".to_vec(),
        },
    ];
    for case in &cases {
        assert_hash_object_batch_case(&stock_git, case);
    }
}

#[test]
fn hash_object_stdin_paths_oversized_record_is_bounded() {
    let stock_git = pinned_stock_git_bin();
    let stock_repo = init_exact_hash_object_repo(&stock_git);
    let zmin_repo = init_exact_hash_object_repo(&stock_git);
    let mut input = vec![b'a'; 256 * 1024 + 1];
    input.push(b'\n');

    let stock = run_hash_object_batch(
        &stock_git,
        stock_repo.path(),
        &["hash-object", "--stdin-paths"],
        &input,
    );
    assert_ne!(
        stock.status, 0,
        "the bounded fixture must not accidentally contain the oversized pathname"
    );
    assert!(stock.stdout.is_empty());
    assert!(!stock.stderr.is_empty());

    let zmin = run_hash_object_batch(
        Path::new(zmin_bin()),
        zmin_repo.path(),
        &["hash-object", "--stdin-paths"],
        &input,
    );
    assert_ne!(zmin.status, 0);
    assert!(zmin.stdout.is_empty());
    assert!(!zmin.stderr.is_empty());
    assert!(
        zmin.stderr.len() < 4096,
        "Zmin must reject the oversized record without echoing it: {} bytes",
        zmin.stderr.len()
    );
}

#[test]
fn hash_object_stdin_paths_literal_invalid_type_streams_each_record() {
    let stock_git = pinned_stock_git_bin();
    let repo = init_exact_hash_object_repo(&stock_git);
    let args = [
        "hash-object",
        "--literally",
        "-t",
        "custom",
        "--stdin-paths",
    ];
    let output = run_hash_object_batch(
        Path::new(zmin_bin()),
        repo.path(),
        &args,
        b"one.txt\ntwo.txt\n",
    );
    assert_eq!(output.status, 0);
    assert!(output.stderr.is_empty());
    let first = run_hash_object_batch(
        Path::new(zmin_bin()),
        repo.path(),
        &["hash-object", "--literally", "-t", "custom", "--stdin"],
        b"one\n",
    );
    let second = run_hash_object_batch(
        Path::new(zmin_bin()),
        repo.path(),
        &["hash-object", "--literally", "-t", "custom", "--stdin"],
        b"two\n",
    );
    assert_eq!(
        output.stdout,
        [first.stdout, second.stdout].concat(),
        "literal invalid-type stdin-paths must process every record"
    );
}

#[cfg(unix)]
#[test]
fn hash_object_stdin_paths_preserves_non_utf8_unix_path_bytes() {
    let stock_git = pinned_stock_git_bin();
    let stock_repo = init_exact_hash_object_repo(&stock_git);
    let zmin_repo = init_exact_hash_object_repo(&stock_git);
    let raw_name = OsString::from_vec(b"raw-\x80.txt".to_vec());
    if let Err(error) = fs::write(stock_repo.path().join(&raw_name), b"raw\n") {
        assert_eq!(
            error.raw_os_error(),
            Some(libc::EILSEQ),
            "unexpected non-UTF-8 pathname error: {error}"
        );
        return;
    }
    fs::write(zmin_repo.path().join(&raw_name), b"raw\n").expect("write zmin raw path");
    let input = b"raw-\x80.txt\n";
    let args = ["hash-object", "--stdin-paths"];
    let stock = run_hash_object_batch(&stock_git, stock_repo.path(), &args, input);
    let zmin = run_hash_object_batch(Path::new(zmin_bin()), zmin_repo.path(), &args, input);
    assert_eq!(zmin, stock);
    assert_eq!(stock.status, 0);
    assert!(stock.stderr.is_empty());
}

fn assert_stock_rejects_cat_file_long_alias(
    stock_git: &Path,
    repo: &Path,
    long_option: &str,
    object_id: &str,
) {
    let (status, stdout, stderr) = raw_command_output(
        stock_git,
        repo,
        &["cat-file", long_option, object_id],
        "stock Git cat-file extension boundary",
    );
    assert_ne!(
        status, 0,
        "stock Git accepted Zmin-only cat-file {long_option}"
    );
    assert!(
        stdout.is_empty(),
        "stock Git emitted stdout for rejected cat-file {long_option}"
    );
    assert!(
        !stderr.is_empty(),
        "stock Git emitted no diagnostic for rejected cat-file {long_option}"
    );
}

#[test]
fn hash_object_and_cat_file_match_stock_git() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");

    let git_id = git(repo.path(), ["hash-object", "-w", "a.txt"]);
    let zmin_id = run_zmin(repo.path(), ["hash-object", "-w", "a.txt"]);
    assert_eq!(zmin_id, git_id);
    assert_eq!(
        run_zmin(repo.path(), ["hash-object", "/dev/null"]),
        git(repo.path(), ["hash-object", "/dev/null"])
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["hash-object", "--stdin", "a.txt"], "stdin\n"),
        git_with_stdin(repo.path(), ["hash-object", "--stdin", "a.txt"], "stdin\n")
    );
    let large_stdin = vec![b'x'; 300 * 1024];
    let git_large_id =
        git_with_stdin_bytes(repo.path(), ["hash-object", "-w", "--stdin"], &large_stdin);
    let zmin_large_id =
        run_zmin_with_stdin_bytes(repo.path(), ["hash-object", "-w", "--stdin"], &large_stdin);
    assert_eq!(zmin_large_id, git_large_id);
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-s", &zmin_large_id]),
        git(repo.path(), ["cat-file", "-s", &git_large_id])
    );

    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-t", &git_id]),
        git(repo.path(), ["cat-file", "-t", &git_id])
    );
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-s", &git_id]),
        git(repo.path(), ["cat-file", "-s", &git_id])
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            ["cat-file", "--batch-all-objects", "--batch-check"]
        ),
        git(
            repo.path(),
            ["cat-file", "--batch-all-objects", "--batch-check"]
        )
    );
    let zmin_unordered = run_zmin(
        repo.path(),
        [
            "cat-file",
            "--batch-all-objects",
            "--batch-check",
            "--unordered",
        ],
    );
    let git_unordered = git(
        repo.path(),
        [
            "cat-file",
            "--batch-all-objects",
            "--batch-check",
            "--unordered",
        ],
    );
    assert_eq!(
        zmin_unordered.lines().collect::<BTreeSet<_>>(),
        git_unordered.lines().collect::<BTreeSet<_>>()
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            [
                "cat-file",
                "--batch-all-objects",
                "--batch-check",
                "--no-unordered",
            ],
        ),
        git(
            repo.path(),
            [
                "cat-file",
                "--batch-all-objects",
                "--batch-check",
                "--no-unordered",
            ],
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check"],
            &format!("{git_id}\n")
        ),
        git_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check"],
            &format!("{git_id}\n")
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check=%(objectname)"],
            &format!("{git_id}\n")
        ),
        git_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check=%(objectname)"],
            &format!("{git_id}\n")
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            [
                "cat-file",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)"
            ],
            &format!("{git_id}\n")
        ),
        git_with_stdin(
            repo.path(),
            [
                "cat-file",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)"
            ],
            &format!("{git_id}\n")
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["cat-file", "--batch-command", "--buffer"],
            &format!("info {git_id}\ncontents {git_id}\nflush\n")
        ),
        git_with_stdin(
            repo.path(),
            ["cat-file", "--batch-command", "--buffer"],
            &format!("info {git_id}\ncontents {git_id}\nflush\n")
        )
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["cat-file", "-e", &git_id]),
        git_status(repo.path(), ["cat-file", "-e", &git_id])
    );

    git(repo.path(), ["repack", "-adq"]);
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-t", &git_id]),
        git(repo.path(), ["cat-file", "-t", &git_id])
    );
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-s", &git_id]),
        git(repo.path(), ["cat-file", "-s", &git_id])
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check"],
            &format!("{git_id}\n")
        ),
        git_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check"],
            &format!("{git_id}\n")
        )
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["cat-file", "-e", &git_id]),
        git_status(repo.path(), ["cat-file", "-e", &git_id])
    );
}

#[test]
fn cat_file_long_aliases_match_short_forms_and_stock_rejects_them() {
    let stock_git = pinned_stock_git_bin();
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    let blob_id = git(repo.path(), ["hash-object", "-w", "a.txt"]);
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let commit_id = git(repo.path(), ["rev-parse", "HEAD"]);
    let missing_id = "0000000000000000000000000000000000000000";

    for (long_option, short_option) in [
        ("--type", "-t"),
        ("--size", "-s"),
        ("--exists", "-e"),
        ("--pretty", "-p"),
    ] {
        assert_cat_file_long_alias_matches_short(
            repo.path(),
            long_option,
            short_option,
            &blob_id,
            "blob",
        );
        assert_cat_file_long_alias_matches_short(
            repo.path(),
            long_option,
            short_option,
            &commit_id,
            "commit",
        );
        assert_cat_file_long_alias_matches_short(
            repo.path(),
            long_option,
            short_option,
            missing_id,
            "missing object",
        );
        assert_stock_rejects_cat_file_long_alias(&stock_git, repo.path(), long_option, &commit_id);
    }
}

#[test]
fn cat_file_mailmap_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "user.name", "Alias User"]);
    git(repo.path(), ["config", "user.email", "alias@example.com"]);
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);
    fs::write(
        repo.path().join(".mailmap"),
        b"Real User <real@example.com> Alias User <alias@example.com>\n",
    )
    .expect("write mailmap");
    git_with_env(repo.path(), ["tag", "-a", "v1", "-m", "tag message"]);

    let commit = git(repo.path(), ["rev-parse", "HEAD"]);
    let tag = git(repo.path(), ["rev-parse", "v1"]);

    for args in [
        vec!["cat-file", "-s", "--use-mailmap", commit.as_str()],
        vec!["cat-file", "-s", "--mailmap", commit.as_str()],
        vec![
            "cat-file",
            "-s",
            "--use-mailmap",
            "--no-mailmap",
            commit.as_str(),
        ],
        vec![
            "cat-file",
            "-s",
            "--mailmap",
            "--no-use-mailmap",
            commit.as_str(),
        ],
        vec!["cat-file", "-p", "--use-mailmap", commit.as_str()],
        vec!["cat-file", "-p", "--use-mailmap", tag.as_str()],
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), repo.path(), &args, "zmin cat-file mailmap"),
            command_any_output("git", repo.path(), &args, "git cat-file mailmap")
        );
    }

    let batch_stdin = format!("{commit}\n");
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--batch-check", "--use-mailmap"],
            &batch_stdin,
            "zmin cat-file batch-check use-mailmap",
        ),
        command_any_output_with_stdin(
            "git",
            repo.path(),
            &["cat-file", "--batch-check", "--use-mailmap"],
            &batch_stdin,
            "git cat-file batch-check use-mailmap",
        )
    );
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--batch-command", "--use-mailmap"],
            &format!("info {commit}\ncontents {commit}\n"),
            "zmin cat-file batch-command use-mailmap",
        ),
        command_any_output_with_stdin(
            "git",
            repo.path(),
            &["cat-file", "--batch-command", "--use-mailmap"],
            &format!("info {commit}\ncontents {commit}\n"),
            "git cat-file batch-command use-mailmap",
        )
    );
}

#[test]
fn hash_object_matches_stock_git_for_documented_long_option_modes() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"alpha\n").expect("write fixture");

    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["hash-object", "--path=a.txt", "--stdin"],
            "stdin\n"
        ),
        git_with_stdin(
            repo.path(),
            ["hash-object", "--path=a.txt", "--stdin"],
            "stdin\n"
        )
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["hash-object", "--stdin-paths"], "a.txt\n"),
        git_with_stdin(repo.path(), ["hash-object", "--stdin-paths"], "a.txt\n")
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["hash-object", "--stdin-paths", "--no-filters"],
            "a.txt\n",
        ),
        git_with_stdin(
            repo.path(),
            ["hash-object", "--stdin-paths", "--no-filters"],
            "a.txt\n",
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["hash-object", "--literally", "--stdin"],
            "stdin\n"
        ),
        git_with_stdin(
            repo.path(),
            ["hash-object", "--literally", "--stdin"],
            "stdin\n"
        )
    );

    assert_eq!(
        run_zmin_failure_output(
            repo.path(),
            &["hash-object", "--no-filters", "--stdin", "--path=a.txt"],
        ),
        git_failure_output(
            repo.path(),
            &["hash-object", "--no-filters", "--stdin", "--path=a.txt"],
        )
    );
    assert_eq!(
        run_zmin_failure_output(
            repo.path(),
            &["hash-object", "--stdin-paths", "--path=a.txt"]
        ),
        git_failure_output(
            repo.path(),
            &["hash-object", "--stdin-paths", "--path=a.txt"]
        )
    );
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["hash-object", "--stdin", "--stdin-paths"]),
        git_failure_output(repo.path(), &["hash-object", "--stdin", "--stdin-paths"])
    );
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["hash-object", "--stdin-paths", "a.txt"]),
        git_failure_output(repo.path(), &["hash-object", "--stdin-paths", "a.txt"])
    );
}

#[test]
fn hash_object_validates_malformed_tree_commit_and_tag_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("blob.txt"), b"foo").expect("write blob");
    let blob = git(repo.path(), ["hash-object", "-w", "blob.txt"]);
    let blob_bytes = hex_to_bytes(&blob);

    let malformed_tree = repo.path().join("malformed-tree.bin");
    fs::write(&malformed_tree, b"abc").expect("write malformed tree");
    let malformed_args = [
        "hash-object",
        "-t",
        "tree",
        malformed_tree.to_str().expect("malformed tree path"),
    ];
    let zmin_malformed = command_any_output(
        zmin_bin(),
        repo.path(),
        &malformed_args,
        "zmin malformed tree",
    );
    let git_malformed =
        command_any_output("git", repo.path(), &malformed_args, "git malformed tree");
    assert_eq!(zmin_malformed.0, git_malformed.0);
    assert!(zmin_malformed.2.contains("too-short tree object"));

    let empty_name_tree = repo.path().join("empty-name-tree.bin");
    let mut empty_name_content = b"100644 \0".to_vec();
    empty_name_content.extend_from_slice(&blob_bytes);
    fs::write(&empty_name_tree, empty_name_content).expect("write empty-name tree");
    let empty_name_args = [
        "hash-object",
        "-t",
        "tree",
        empty_name_tree.to_str().expect("empty-name tree path"),
    ];
    let zmin_empty_name = command_any_output(
        zmin_bin(),
        repo.path(),
        &empty_name_args,
        "zmin empty-name tree",
    );
    let git_empty_name =
        command_any_output("git", repo.path(), &empty_name_args, "git empty-name tree");
    assert_eq!(zmin_empty_name.0, git_empty_name.0);
    assert!(zmin_empty_name.2.contains("empty filename in tree entry"));

    let duplicate_tree = repo.path().join("duplicate-tree.bin");
    let mut duplicate_content = b"100644 file\0".to_vec();
    duplicate_content.extend_from_slice(&blob_bytes);
    duplicate_content.extend_from_slice(b"100644 file\0");
    duplicate_content.extend_from_slice(&blob_bytes);
    fs::write(&duplicate_tree, duplicate_content).expect("write duplicate tree");
    let duplicate_args = [
        "hash-object",
        "-t",
        "tree",
        duplicate_tree.to_str().expect("duplicate tree path"),
    ];
    let zmin_duplicate = command_any_output(
        zmin_bin(),
        repo.path(),
        &duplicate_args,
        "zmin duplicate tree",
    );
    let git_duplicate =
        command_any_output("git", repo.path(), &duplicate_args, "git duplicate tree");
    assert_eq!(zmin_duplicate.0, git_duplicate.0);
    assert!(zmin_duplicate.2.contains("duplicateEntries"));

    let zmin_commit = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["hash-object", "-t", "commit", "--stdin"],
        "",
        "zmin corrupt commit",
    );
    let git_commit = command_any_output_with_stdin(
        "git",
        repo.path(),
        &["hash-object", "-t", "commit", "--stdin"],
        "",
        "git corrupt commit",
    );
    assert_eq!(zmin_commit.0, git_commit.0);
    assert!(zmin_commit.2.contains("corrupt commit"));

    let zmin_tag = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["hash-object", "-t", "tag", "--stdin"],
        "",
        "zmin corrupt tag",
    );
    let git_tag = command_any_output_with_stdin(
        "git",
        repo.path(),
        &["hash-object", "-t", "tag", "--stdin"],
        "",
        "git corrupt tag",
    );
    assert_eq!(zmin_tag.0, git_tag.0);
    assert!(zmin_tag.2.contains("corrupt tag"));
}

#[test]
fn cat_file_unknown_filter_names_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let blob = git(repo.path(), ["rev-parse", "HEAD:a.txt"]);
    let stdin = format!("{blob}\n");

    for filter in ["bad:name", "bad=name"] {
        let arg = format!("--filter={filter}");
        let args = ["cat-file", "--batch", arg.as_str()];
        assert_eq!(
            command_any_output_with_stdin_bytes(
                zmin_bin(),
                repo.path(),
                &args,
                stdin.as_bytes(),
                "zmin",
            ),
            command_any_output_with_stdin_bytes("git", repo.path(), &args, stdin.as_bytes(), "git",),
            "filter: {filter}"
        );
    }
}

#[test]
fn cat_file_known_unsupported_filters_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let blob = git(repo.path(), ["rev-parse", "HEAD:a.txt"]);
    let stdin = format!("{blob}\n");

    for filter in ["tree:1", "sparse:oid=deadbeef", "combine:blob:none+tree:1"] {
        let arg = format!("--filter={filter}");
        let args = ["cat-file", "--batch", arg.as_str()];
        assert_eq!(
            command_any_output_with_stdin_bytes(
                zmin_bin(),
                repo.path(),
                &args,
                stdin.as_bytes(),
                "zmin",
            ),
            command_any_output_with_stdin_bytes("git", repo.path(), &args, stdin.as_bytes(), "git",),
            "filter: {filter}"
        );
    }
}

#[test]
fn cat_file_sparse_path_filter_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let blob = git(repo.path(), ["rev-parse", "HEAD:a.txt"]);
    let stdin = format!("{blob}\n");
    let args = ["cat-file", "--batch", "--filter=sparse:path=foo"];

    assert_eq!(
        command_any_output_with_stdin_bytes(
            zmin_bin(),
            repo.path(),
            &args,
            stdin.as_bytes(),
            "zmin"
        ),
        command_any_output_with_stdin_bytes("git", repo.path(), &args, stdin.as_bytes(), "git",),
    );
}

#[test]
fn cat_file_promisor_remote_hydrates_missing_blob_on_demand() {
    let source = git_init();
    configure_identity(source.path());
    fs::write(source.path().join("a.txt"), b"one\n").expect("write tracked file");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "initial"]);
    let source_head = git(source.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    let remote = TempDir::new().expect("remote dir");
    git_args(
        source.path(),
        &[
            "clone",
            "--bare",
            ".",
            remote.path().to_str().expect("remote path utf8"),
        ],
    );

    let repo = init_promisor_work_repo(remote.path(), &source_head);
    let blob = git(repo.path(), ["rev-list", "--objects", "HEAD"])
        .lines()
        .find_map(|line| line.strip_suffix(" a.txt").map(str::to_owned))
        .expect("blob id");
    let object_path = loose_object_path(repo.path(), &blob);
    assert!(
        object_path.is_file(),
        "expected loose object at {}",
        object_path.display()
    );
    fs::remove_file(&object_path).expect("remove local blob");
    assert!(!object_path.exists());

    assert_eq!(run_zmin(repo.path(), ["cat-file", "-t", &blob]), "blob");
    assert!(
        pack_contains_object(repo.path(), &blob),
        "expected demand hydration pack to contain {blob}"
    );

    assert_eq!(run_zmin(repo.path(), ["cat-file", "blob", &blob]), "one");
    assert!(
        pack_contains_object(repo.path(), &blob),
        "expected typed-object demand hydration pack to contain {blob}"
    );
}

#[test]
fn cat_file_promisor_remote_lazy_fetch_writes_ref_in_want_trace() {
    let source = git_init();
    configure_identity(source.path());
    fs::write(source.path().join("a.txt"), b"one\n").expect("write tracked file");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "initial"]);
    let source_head = git(source.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    let remote = TempDir::new().expect("remote dir");
    git_args(
        source.path(),
        &[
            "clone",
            "--bare",
            ".",
            remote.path().to_str().expect("remote path utf8"),
        ],
    );

    let repo = init_promisor_work_repo(remote.path(), &source_head);
    let head = git(repo.path(), ["rev-parse", "HEAD"]);
    fs::remove_dir_all(repo.path().join(".git/objects")).expect("remove local objects");
    fs::create_dir_all(repo.path().join(".git/objects")).expect("restore objects dir");
    git(repo.path(), ["config", "core.repositoryformatversion", "1"]);
    git(repo.path(), ["config", "extensions.partialclone", "origin"]);
    git(repo.path(), ["config", "protocol.version", "2"]);
    git(remote.path(), ["config", "uploadpack.allowrefinwant", "1"]);
    let trace = repo.path().join("trace.packet");
    let trace_value = trace.to_str().expect("trace path");

    let output = command_output_with_env(
        zmin_bin(),
        repo.path(),
        &["cat-file", "-p", &head],
        &[("GIT_TRACE_PACKET", trace_value)],
        "zmin cat-file lazy fetch",
    );

    assert_eq!(output.0, 0, "zmin lazy fetch failed: {}", output.2);
    let trace_contents = fs::read_to_string(&trace).expect("trace file");
    assert!(trace_contents.contains("fetch< fetch=shallow wait-for-done ref-in-want"));
}

#[test]
fn cat_file_promisor_tree_fetch_does_not_hydrate_blob_closure() {
    let source = git_init();
    configure_identity(source.path());
    fs::write(source.path().join("a.txt"), b"one\n").expect("write tracked file");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "initial"]);
    let source_head = git(source.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    let remote = TempDir::new().expect("remote dir");
    git_args(
        source.path(),
        &[
            "clone",
            "--bare",
            ".",
            remote.path().to_str().expect("remote path utf8"),
        ],
    );

    let zmin_repo = init_promisor_work_repo(remote.path(), &source_head);
    let git_repo = init_promisor_work_repo(remote.path(), &source_head);
    let tree = git(zmin_repo.path(), ["rev-parse", "HEAD^{tree}"]);
    let blob = git(zmin_repo.path(), ["rev-parse", "HEAD:a.txt"]);
    for repo in [zmin_repo.path(), git_repo.path()] {
        fs::remove_dir_all(repo.join(".git/objects")).expect("remove local objects");
        fs::create_dir_all(repo.join(".git/objects")).expect("restore objects dir");
        git(repo, ["config", "core.repositoryformatversion", "1"]);
        git(repo, ["config", "extensions.partialclone", "origin"]);
        git(repo, ["config", "protocol.version", "2"]);
    }
    git(
        remote.path(),
        ["config", "uploadpack.allowanysha1inwant", "1"],
    );
    git(remote.path(), ["config", "uploadpack.allowfilter", "1"]);

    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-p", &tree]),
        git(git_repo.path(), ["cat-file", "-p", &tree])
    );
    assert!(
        !loose_object_path(zmin_repo.path(), &blob).is_file(),
        "blob should remain absent from the local object database after fetching the tree object"
    );
}

#[test]
fn cat_file_promisor_remote_tries_next_promisor_when_first_remote_lacks_object() {
    let source = git_init();
    configure_identity(source.path());
    fs::write(source.path().join("foo.txt"), b"foo\n").expect("write first source file");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "initial"]);
    let source_head = git(source.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    let origin_remote = TempDir::new().expect("origin remote dir");
    git_args(
        source.path(),
        &[
            "clone",
            "--bare",
            ".",
            origin_remote.path().to_str().expect("origin path utf8"),
        ],
    );

    let repo = init_promisor_work_repo(origin_remote.path(), &source_head);

    let server2 = clone_repo_fixture(source.path());
    configure_identity(server2.path());
    fs::write(server2.path().join("bar.txt"), b"bar\n").expect("write second source file");
    git(server2.path(), ["add", "-A"]);
    git_with_env(server2.path(), ["commit", "-m", "bar"]);
    let server2_commit = git(server2.path(), ["rev-parse", "HEAD"]);

    let server2_remote = TempDir::new().expect("server2 remote dir");
    git_args(
        server2.path(),
        &[
            "clone",
            "--bare",
            ".",
            server2_remote.path().to_str().expect("server2 path utf8"),
        ],
    );
    git(
        server2_remote.path(),
        ["repack", "-a", "-d", "--write-bitmap-index"],
    );

    git(
        repo.path(),
        [
            "remote",
            "add",
            "server2",
            server2_remote.path().to_str().expect("server2 path utf8"),
        ],
    );
    git(repo.path(), ["config", "remote.origin.promisor", "true"]);
    git(repo.path(), ["config", "remote.server2.promisor", "true"]);
    git(repo.path(), ["fetch", "server2"]);

    fs::remove_dir_all(repo.path().join(".git/objects")).expect("remove local objects");
    fs::create_dir_all(repo.path().join(".git/objects")).expect("restore objects dir");

    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-p", &server2_commit]),
        git(repo.path(), ["cat-file", "-p", &server2_commit])
    );
    assert!(
        pack_contains_object(repo.path(), &server2_commit),
        "expected demand hydration pack to contain {server2_commit}"
    );

    let promisor_markers = fs::read_dir(repo.path().join(".git/objects/pack"))
        .expect("read pack dir")
        .map(|entry| entry.expect("pack entry").path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("promisor"))
        .count();
    assert_eq!(promisor_markers, 1);
}

#[test]
fn hash_object_write_prefers_bare_repo_at_current_directory() {
    let parent = git_init();
    let bare_path = parent.path().join("nested.git");
    run_zmin_args(
        parent.path(),
        &["init", "--bare", bare_path.to_str().expect("bare path")],
    );

    let object_id = run_zmin_with_stdin(&bare_path, ["hash-object", "-w", "--stdin"], "");
    assert_eq!(object_id, "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    assert!(
        bare_path
            .join("objects/e6/9de29bb2d1d6434b8b29ae775ad8c2e48c5391")
            .is_file()
    );
    assert!(
        !parent
            .path()
            .join(".git/objects/e6/9de29bb2d1d6434b8b29ae775ad8c2e48c5391")
            .exists()
    );
    assert_eq!(run_zmin(&bare_path, ["cat-file", "-s", &object_id]), "0");
}

#[test]
fn index_stage_object_paths_match_stock_git() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    fs::write(repo.path().join("b.txt"), b"hello\n").expect("write matching fixture");
    fs::write(repo.path().join("c.txt"), b"changed\n").expect("write changed fixture");
    git(repo.path(), ["add", "a.txt", "b.txt", "c.txt"]);

    for objectish in [":a.txt", ":0:a.txt"] {
        assert_eq!(
            run_zmin(repo.path(), ["rev-parse", objectish]),
            git(repo.path(), ["rev-parse", objectish])
        );
        assert_eq!(
            run_zmin(repo.path(), ["cat-file", "-p", objectish]),
            git(repo.path(), ["cat-file", "-p", objectish])
        );
    }
    assert_eq!(
        run_zmin_status(
            repo.path(),
            ["diff", "--raw", "--exit-code", ":a.txt", ":b.txt"]
        ),
        git_status(
            repo.path(),
            ["diff", "--raw", "--exit-code", ":a.txt", ":b.txt"]
        )
    );
    assert_eq!(
        run_zmin(repo.path(), ["diff", "--raw", ":a.txt", ":c.txt"]),
        git(repo.path(), ["diff", "--raw", ":a.txt", ":c.txt"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["diff", ":a.txt", ":c.txt"]),
        git(repo.path(), ["diff", ":a.txt", ":c.txt"])
    );
}

#[test]
fn ident_attribute_add_and_checkout_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join(".gitattributes"), b"*.i ident\n").expect("write attributes");
        fs::write(repo.join("id.i"), b"before\n$Id$\nafter\n").expect("write ident file");
    }

    git(git_repo.path(), ["add", "id.i"]);
    run_zmin(zmin_repo.path(), ["add", "id.i"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-p", ":id.i"]),
        git(git_repo.path(), ["cat-file", "-p", ":id.i"])
    );

    fs::remove_file(git_repo.path().join("id.i")).expect("remove git worktree file");
    fs::remove_file(zmin_repo.path().join("id.i")).expect("remove zmin worktree file");
    git(git_repo.path(), ["checkout", "--", "id.i"]);
    run_zmin(zmin_repo.path(), ["checkout", "--", "id.i"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("id.i")).expect("read zmin ident file"),
        fs::read_to_string(git_repo.path().join("id.i")).expect("read git ident file")
    );
}

#[test]
fn filter_attribute_add_and_checkout_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let rot13 = "tr 'A-Za-z' 'N-ZA-Mn-za-m'";
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "filter.rot13.clean", rot13]);
        git(repo, ["config", "filter.rot13.smudge", rot13]);
        fs::write(repo.join(".gitattributes"), b"*.t filter=rot13\n").expect("write attributes");
        fs::write(repo.join("message.t"), b"hello abc xyz\n").expect("write filtered file");
    }
    assert_eq!(
        run_zmin(zmin_repo.path(), ["hash-object", "message.t"]),
        git(git_repo.path(), ["hash-object", "message.t"])
    );

    git(git_repo.path(), ["add", "message.t"]);
    run_zmin(zmin_repo.path(), ["add", "message.t"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-p", ":message.t"]),
        git(git_repo.path(), ["cat-file", "-p", ":message.t"])
    );

    fs::remove_file(git_repo.path().join("message.t")).expect("remove git worktree file");
    fs::remove_file(zmin_repo.path().join("message.t")).expect("remove zmin worktree file");
    git(git_repo.path(), ["checkout", "--", "message.t"]);
    run_zmin(zmin_repo.path(), ["checkout", "--", "message.t"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("message.t")).expect("read zmin filtered file"),
        fs::read_to_string(git_repo.path().join("message.t")).expect("read git filtered file")
    );
}

#[test]
fn cat_file_filters_match_stock_git_for_eol_and_smudge_attributes() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "filter.rot13.clean", "cat"]);
    git(
        repo.path(),
        [
            "config",
            "filter.rot13.smudge",
            "tr 'A-Za-z' 'N-ZA-Mn-za-m'",
        ],
    );
    fs::write(
        repo.path().join(".gitattributes"),
        b"*.txt text eol=crlf\n*.rot filter=rot13\n",
    )
    .expect("write attributes");
    fs::write(repo.path().join("line.txt"), b"one\ntwo\n").expect("write eol file");
    fs::write(repo.path().join("message.rot"), b"hello abc xyz\n").expect("write filtered file");
    git(repo.path(), ["add", "."]);
    git_with_env(repo.path(), ["commit", "-m", "filters"]);

    let text_blob = git(repo.path(), ["rev-parse", "HEAD:line.txt"]);
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--filters", "HEAD:line.txt"]
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--filters", "HEAD:line.txt"]
        )
    );
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--filters", "--path=line.txt", &text_blob],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--filters", "--path=line.txt", &text_blob],
        )
    );

    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--filters", "HEAD:message.rot"],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--filters", "HEAD:message.rot"],
        )
    );
}

#[test]
fn cat_file_textconv_matches_stock_git_for_diff_driver_attributes() {
    let repo = git_init();
    configure_identity(repo.path());
    git(
        repo.path(),
        ["config", "diff.upper.textconv", "tr 'a-z' 'A-Z' <"],
    );
    fs::write(repo.path().join(".gitattributes"), b"*.bin diff=upper\n").expect("write attributes");
    fs::write(repo.path().join("payload.bin"), b"hello abc\n").expect("write payload");
    fs::write(repo.path().join("plain.txt"), b"plain\n").expect("write plain");
    git(repo.path(), ["add", "."]);
    git_with_env(repo.path(), ["commit", "-m", "textconv"]);

    let blob = git(repo.path(), ["rev-parse", "HEAD:payload.bin"]);
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--textconv", "HEAD:payload.bin"],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--textconv", "HEAD:payload.bin"],
        )
    );
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--textconv", "--path=payload.bin", &blob],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--textconv", "--path=payload.bin", &blob],
        )
    );
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--textconv", "HEAD:plain.txt"],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--textconv", "HEAD:plain.txt"],
        )
    );
}

#[test]
#[cfg(unix)]
fn process_filter_attribute_add_and_checkout_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let helper = git_repo.path().join("process-filter.pl");
    fs::write(
        &helper,
        r#"use strict;
use warnings;
binmode STDIN;
binmode STDOUT;
$| = 1;
open my $log, ">>", $ARGV[0] or die "log";
sub readpkt {
    my $header = "";
    my $read = read(STDIN, $header, 4);
    return undef unless defined($read) && $read == 4;
    my $len = hex($header);
    return "" if $len == 0;
    my $payload = "";
    read(STDIN, $payload, $len - 4) == $len - 4 or die "short read";
    return $payload;
}
sub readtext {
    my $value = readpkt();
    return undef unless defined $value;
    $value =~ s/\n$//;
    return $value;
}
sub writepkt {
    my ($payload) = @_;
    printf "%04x%s", length($payload) + 4, $payload;
}
sub flushpkt { print "0000"; }
sub rot13 {
    my ($value) = @_;
    $value =~ tr/A-Za-z/N-ZA-Mn-za-m/;
    return $value;
}
print $log "START\n";
die "client" unless readtext() eq "git-filter-client";
die "version" unless readtext() eq "version=2";
die "flush" unless readtext() eq "";
writepkt("git-filter-server");
writepkt("version=2");
flushpkt();
while ((my $cap = readtext()) ne "") {}
writepkt("capability=clean");
writepkt("capability=smudge");
flushpkt();
print $log "init handshake complete\n";
while (1) {
    my $command = readtext();
    last unless defined $command;
    $command =~ s/^command=// or die "command";
    my $path = readtext();
    $path =~ s/^pathname=// or die "path";
    while ((my $meta = readtext()) ne "") {}
    my $content = "";
    while ((my $packet = readpkt()) ne "") { $content .= $packet; }
    print $log "IN: $command $path\n";
    my $out = rot13($content);
    writepkt("status=success");
    flushpkt();
    while (length($out) > 0) {
        my $chunk = substr($out, 0, 65516, "");
        writepkt($chunk);
    }
    flushpkt();
    flushpkt();
}
print $log "STOP\n";
"#,
    )
    .expect("write process filter helper");
    let command = format!(
        "perl {} debug.log",
        shell_quote_for_test(&helper.to_string_lossy())
    );
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "filter.protocol.process", &command]);
        git(repo, ["config", "filter.protocol.required", "true"]);
        fs::write(repo.join(".gitattributes"), b"*.r filter=protocol\n").expect("write attributes");
        fs::write(repo.join("one.r"), b"hello abc\n").expect("write one");
        fs::write(repo.join("two.r"), b"xyz world\n").expect("write two");
    }
    assert_eq!(
        run_zmin(zmin_repo.path(), ["hash-object", "one.r"]),
        git(git_repo.path(), ["hash-object", "one.r"])
    );
    let _ = fs::remove_file(git_repo.path().join("debug.log"));
    let _ = fs::remove_file(zmin_repo.path().join("debug.log"));

    git(git_repo.path(), ["add", "."]);
    run_zmin(zmin_repo.path(), ["add", "."]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-p", ":one.r"]),
        git(git_repo.path(), ["cat-file", "-p", ":one.r"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-p", ":two.r"]),
        git(git_repo.path(), ["cat-file", "-p", ":two.r"])
    );
    let zmin_log = fs::read_to_string(zmin_repo.path().join("debug.log")).expect("read log");
    assert_eq!(zmin_log.matches("START\n").count(), 1);
    assert_eq!(zmin_log.matches("STOP\n").count(), 1);
    assert!(zmin_log.contains("IN: clean one.r\n"));
    assert!(zmin_log.contains("IN: clean two.r\n"));

    fs::remove_file(git_repo.path().join("one.r")).expect("remove git one");
    fs::remove_file(git_repo.path().join("two.r")).expect("remove git two");
    fs::remove_file(zmin_repo.path().join("one.r")).expect("remove zmin one");
    fs::remove_file(zmin_repo.path().join("two.r")).expect("remove zmin two");
    fs::remove_file(zmin_repo.path().join("debug.log")).expect("remove zmin log");
    git(git_repo.path(), ["checkout", "--", "one.r", "two.r"]);
    run_zmin(zmin_repo.path(), ["checkout", "--", "one.r", "two.r"]);
    assert_eq!(
        fs::read(zmin_repo.path().join("one.r")).expect("read zmin one"),
        fs::read(git_repo.path().join("one.r")).expect("read git one")
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("two.r")).expect("read zmin two"),
        fs::read(git_repo.path().join("two.r")).expect("read git two")
    );
    let zmin_log = fs::read_to_string(zmin_repo.path().join("debug.log")).expect("read log");
    assert_eq!(zmin_log.matches("START\n").count(), 1);
    assert_eq!(zmin_log.matches("STOP\n").count(), 1);
    assert!(zmin_log.contains("IN: smudge one.r\n"));
    assert!(zmin_log.contains("IN: smudge two.r\n"));
}

#[test]
#[cfg(unix)]
fn add_with_clean_filter_that_does_not_read_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join(".gitattributes"), b"big filter=epipe\n").expect("write attributes");
        fs::write(repo.join("big"), vec![b'a'; 128 * 1024 + 1]).expect("write large fixture");
    }

    git(
        git_repo.path(),
        ["config", "filter.epipe.clean", "echo xyzzy"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "filter.epipe.clean", "echo xyzzy"],
    );

    let stock = command_any_output("git", git_repo.path(), &["add", "big"], "stock git add big");
    let zmin = command_any_output(
        zmin_bin(),
        zmin_repo.path(),
        &["add", "big"],
        "zmin add big",
    );

    assert_eq!(zmin.0, stock.0, "stderr: {}", zmin.2);
    assert_eq!(zmin.1, stock.1);
    assert_eq!(zmin.2, stock.2);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "blob", ":big"]),
        git(git_repo.path(), ["cat-file", "blob", ":big"])
    );
}

#[test]
#[cfg(unix)]
fn process_filter_subcommand_metadata_matches_stock_git() {
    const FIXED_ENV: [(&str, &str); 6] = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
    ];

    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    let helper_dir = TempDir::new().expect("helper tempdir");
    let helper = helper_dir.path().join("process-filter-meta.pl");
    fs::write(
        &helper,
        r#"use strict;
use warnings;
binmode STDIN;
binmode STDOUT;
$| = 1;
open my $log, ">>", $ARGV[0] or die "log";
sub readpkt {
    my $header = "";
    my $read = read(STDIN, $header, 4);
    return undef unless defined($read) && $read == 4;
    my $len = hex($header);
    return "" if $len == 0;
    my $payload = "";
    read(STDIN, $payload, $len - 4) == $len - 4 or die "short read";
    return $payload;
}
sub readtext {
    my $value = readpkt();
    return undef unless defined $value;
    $value =~ s/\n$//;
    return $value;
}
sub writepkt {
    my ($payload) = @_;
    printf "%04x%s", length($payload) + 4, $payload;
}
sub flushpkt { print "0000"; }
sub rot13 {
    my ($value) = @_;
    $value =~ tr/A-Za-z/N-ZA-Mn-za-m/;
    return $value;
}
print $log "START\n";
die "client" unless readtext() eq "git-filter-client";
die "version" unless readtext() eq "version=2";
die "flush" unless readtext() eq "";
writepkt("git-filter-server");
writepkt("version=2");
flushpkt();
while ((my $cap = readtext()) ne "") {}
writepkt("capability=clean");
writepkt("capability=smudge");
flushpkt();
print $log "init handshake complete\n";
while (1) {
    my $command = readtext();
    last unless defined $command;
    $command =~ s/^command=// or die "command";
    my $path = readtext();
    $path =~ s/^pathname=// or die "path";
    my @meta;
    while ((my $meta = readtext()) ne "") {
        push @meta, $meta;
    }
    my $content = "";
    while ((my $packet = readpkt()) ne "") {
        $content .= $packet;
    }
    my $size = length($content);
    my $dots = "." x (($size + 65515) / 65516);
    print $log "IN: $command $path";
    print $log " " . join(" ", @meta) if @meta;
    print $log " $size [OK] -- OUT: $size $dots [OK]\n";
    my $out = rot13($content);
    writepkt("status=success");
    flushpkt();
    while (length($out) > 0) {
        my $chunk = substr($out, 0, 65516, "");
        writepkt($chunk);
    }
    flushpkt();
    flushpkt();
}
print $log "STOP\n";
"#,
    )
    .expect("write metadata helper");

    let command = format!(
        "perl {} debug.log",
        shell_quote_for_test(&helper.to_string_lossy())
    );

    let run_git = |cwd: &std::path::Path, args: &[&str], label: &str| {
        command_output_with_env("git", cwd, args, &FIXED_ENV, label)
    };
    let run_zmin_env = |cwd: &std::path::Path, args: &[&str], label: &str| {
        command_output_with_env(zmin_bin(), cwd, args, &FIXED_ENV, label)
    };
    let compare_logs = |stage: &str| {
        let git_log = fs::read_to_string(git_repo.path().join("debug.log")).expect("read git log");
        let zmin_log =
            fs::read_to_string(zmin_repo.path().join("debug.log")).expect("read zmin log");
        assert_eq!(zmin_log, git_log, "stage: {stage}");
    };
    let clear_logs = || {
        let _ = fs::remove_file(git_repo.path().join("debug.log"));
        let _ = fs::remove_file(zmin_repo.path().join("debug.log"));
    };

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join(".gitattributes"), b"*.r filter=protocol\n").expect("write attrs");
        fs::write(repo.join("test.r"), b"uryyb grfg.e\n").expect("write test");
        fs::write(repo.join("test2.r"), b"uryyb grfg2.e\n").expect("write test2");
        fs::create_dir_all(repo.join("testsubdir")).expect("mkdir subdir");
        fs::write(repo.join("testsubdir/test3 'sq',$x=.r"), b"uryyb fd\n").expect("write test3");
        fs::write(repo.join("test4-empty.r"), b"").expect("write empty");
    }
    git(
        git_repo.path(),
        ["config", "filter.protocol.process", &command],
    );
    git(
        git_repo.path(),
        ["config", "filter.protocol.required", "true"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "filter.protocol.process", &command],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "filter.protocol.required", "true"],
    );

    run_git(git_repo.path(), &["add", ".gitattributes"], "git add attrs");
    run_zmin_env(
        zmin_repo.path(),
        &["add", ".gitattributes"],
        "zmin add attrs",
    );
    run_git(
        git_repo.path(),
        &["commit", "-m", "test commit 1"],
        "git commit 1",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["commit", "-m", "test commit 1"],
        "zmin commit 1",
    );
    run_git(
        git_repo.path(),
        &["branch", "-M", "main"],
        "git branch -M main",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["branch", "-M", "main"],
        "zmin branch -M main",
    );
    run_git(
        git_repo.path(),
        &["branch", "empty-branch"],
        "git branch empty",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["branch", "empty-branch"],
        "zmin branch empty",
    );

    clear_logs();
    run_git(git_repo.path(), &["add", "."], "git add fixtures");
    run_zmin_env(zmin_repo.path(), &["add", "."], "zmin add fixtures");
    compare_logs("add");
    run_git(
        git_repo.path(),
        &["commit", "-m", "test commit 2"],
        "git commit 2",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["commit", "-m", "test commit 2"],
        "zmin commit 2",
    );

    fs::copy(
        git_repo.path().join("test.r"),
        git_repo.path().join("test5.r"),
    )
    .expect("copy git");
    fs::copy(
        zmin_repo.path().join("test.r"),
        zmin_repo.path().join("test5.r"),
    )
    .expect("copy zmin");
    run_git(git_repo.path(), &["add", "test5.r"], "git add test5");
    run_zmin_env(zmin_repo.path(), &["add", "test5.r"], "zmin add test5");
    run_git(
        git_repo.path(),
        &["commit", "-m", "test commit 3"],
        "git commit 3",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["commit", "-m", "test commit 3"],
        "zmin commit 3",
    );
    run_git(
        git_repo.path(),
        &["checkout", "empty-branch"],
        "git checkout empty",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["checkout", "empty-branch"],
        "zmin checkout empty",
    );

    clear_logs();
    run_git(
        git_repo.path(),
        &["rebase", "--onto", "empty-branch", "main^^", "main"],
        "git rebase",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["rebase", "--onto", "empty-branch", "main^^", "main"],
        "zmin rebase",
    );
    compare_logs("rebase");
    let git_main = run_git(
        git_repo.path(),
        &["rev-parse", "--verify", "main"],
        "git main2",
    )
    .1;
    let zmin_main = run_zmin_env(
        zmin_repo.path(),
        &["rev-parse", "--verify", "main"],
        "zmin main2",
    )
    .1;

    run_git(
        git_repo.path(),
        &["reset", "--hard", "empty-branch"],
        "git reset empty",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["reset", "--hard", "empty-branch"],
        "zmin reset empty",
    );
    clear_logs();
    run_git(
        git_repo.path(),
        &["reset", "--hard", git_main.as_str()],
        "git reset main",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["reset", "--hard", zmin_main.as_str()],
        "zmin reset main",
    );
    compare_logs("reset-main");

    run_git(
        git_repo.path(),
        &["branch", "old-main", git_main.as_str()],
        "git branch old-main",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["branch", "old-main", zmin_main.as_str()],
        "zmin branch old-main",
    );
    run_git(
        git_repo.path(),
        &["reset", "--hard", "empty-branch"],
        "git reset empty 2",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["reset", "--hard", "empty-branch"],
        "zmin reset empty 2",
    );
    clear_logs();
    run_git(
        git_repo.path(),
        &["reset", "--hard", "old-main"],
        "git reset old-main",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["reset", "--hard", "old-main"],
        "zmin reset old-main",
    );
    compare_logs("reset-old-main");

    run_git(
        git_repo.path(),
        &["checkout", "-b", "merge", "empty-branch"],
        "git checkout merge",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["checkout", "-b", "merge", "empty-branch"],
        "zmin checkout merge",
    );
    run_git(
        git_repo.path(),
        &["branch", "-f", "main", git_main.as_str()],
        "git force main",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["branch", "-f", "main", zmin_main.as_str()],
        "zmin force main",
    );
    clear_logs();
    run_git(git_repo.path(), &["merge", "main"], "git merge main");
    run_zmin_env(zmin_repo.path(), &["merge", "main"], "zmin merge main");
    compare_logs("merge-main");

    clear_logs();
    let _git_archive = command_stdout_bytes("git", git_repo.path(), &["archive", "main"]);
    let _zmin_archive = command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["archive", "main"]);
    compare_logs("archive-main");

    let git_tree = run_git(
        git_repo.path(),
        &["rev-parse", &format!("{git_main}^{{tree}}")],
        "git tree rev-parse",
    );
    let zmin_tree = run_zmin_env(
        zmin_repo.path(),
        &["rev-parse", &format!("{zmin_main}^{{tree}}")],
        "zmin tree rev-parse",
    );
    clear_logs();
    let _git_tree_archive =
        command_stdout_bytes("git", git_repo.path(), &["archive", git_tree.1.as_str()]);
    let _zmin_tree_archive = command_stdout_bytes(
        zmin_bin(),
        zmin_repo.path(),
        &["archive", zmin_tree.1.as_str()],
    );
    compare_logs("archive-tree");
}

#[test]
#[cfg(unix)]
fn lfs_process_filter_pointer_workflow_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    let helper_dir = TempDir::new().expect("helper tempdir");
    let helper = helper_dir.path().join("fake-lfs-filter.py");
    fs::write(
        &helper,
        r#"#!/usr/bin/env python3
import hashlib
import pathlib
import sys

store = pathlib.Path(sys.argv[1])
log_path = pathlib.Path(sys.argv[2])
store.mkdir(parents=True, exist_ok=True)
log_path.parent.mkdir(parents=True, exist_ok=True)

def read_exact(size):
    data = sys.stdin.buffer.read(size)
    if len(data) != size:
        raise SystemExit("short read")
    return data

def readpkt():
    header = sys.stdin.buffer.read(4)
    if not header:
        return None
    if len(header) != 4:
        raise SystemExit("short header")
    length = int(header, 16)
    if length == 0:
        return b""
    return read_exact(length - 4)

def readtext():
    value = readpkt()
    if value is None:
        return None
    if value == b"":
        return ""
    return value.decode("utf-8").rstrip("\n")

def writepkt(payload: bytes):
    sys.stdout.buffer.write(f"{len(payload) + 4:04x}".encode("ascii"))
    sys.stdout.buffer.write(payload)

def writetext(text: str):
    writepkt(text.encode("utf-8"))

def flushpkt():
    sys.stdout.buffer.write(b"0000")

def log(text: str):
    with log_path.open("a", encoding="utf-8") as handle:
        handle.write(text + "\n")

def read_packetized_content():
    chunks = []
    while True:
        payload = readpkt()
        if payload is None:
            raise SystemExit("unexpected eof")
        if payload == b"":
            return b"".join(chunks)
        chunks.append(payload)

def lfs_pointer(content: bytes) -> bytes:
    oid = hashlib.sha256(content).hexdigest()
    path = store / oid
    if not path.exists():
        path.write_bytes(content)
    return (
        "version https://git-lfs.github.com/spec/v1\n"
        f"oid sha256:{oid}\n"
        f"size {len(content)}\n"
    ).encode("utf-8")

def lfs_smudge(pointer: bytes) -> bytes:
    oid = None
    for line in pointer.decode("utf-8").splitlines():
        if line.startswith("oid sha256:"):
            oid = line.split(":", 1)[1]
            break
    if oid is None:
        return pointer
    return (store / oid).read_bytes()

assert readtext() == "git-filter-client"
assert readtext() == "version=2"
assert readtext() == ""
writetext("git-filter-server")
writetext("version=2")
flushpkt()
sys.stdout.buffer.flush()
while True:
    line = readtext()
    if line == "":
        break
    if line is None:
        raise SystemExit("missing capabilities")
writetext("capability=clean")
writetext("capability=smudge")
flushpkt()
sys.stdout.buffer.flush()
log("START")
while True:
    command = readtext()
    if command is None:
        break
    command = command.removeprefix("command=")
    pathname = readtext().removeprefix("pathname=")
    while True:
        meta = readtext()
        if meta == "":
            break
    content = read_packetized_content()
    log(f"{command} {pathname}")
    if command == "clean":
        out = lfs_pointer(content)
    elif command == "smudge":
        out = lfs_smudge(content)
    else:
        writetext("status=abort")
        flushpkt()
        flushpkt()
        sys.stdout.buffer.flush()
        continue
    writetext("status=success")
    flushpkt()
    if out:
        writepkt(out)
    flushpkt()
    flushpkt()
    sys.stdout.buffer.flush()
log("STOP")
"#,
    )
    .expect("write fake lfs helper");

    let git_command = format!(
        "python3 {} {} {}",
        shell_quote_for_test(&helper.to_string_lossy()),
        shell_quote_for_test(&git_repo.path().join("lfs-store").to_string_lossy()),
        shell_quote_for_test(&git_repo.path().join("lfs.log").to_string_lossy()),
    );
    let zmin_command = format!(
        "python3 {} {} {}",
        shell_quote_for_test(&helper.to_string_lossy()),
        shell_quote_for_test(&zmin_repo.path().join("lfs-store").to_string_lossy()),
        shell_quote_for_test(&zmin_repo.path().join("lfs.log").to_string_lossy()),
    );

    let payload = b"\x00zmin-lfs-payload\xff\nsecond-line\n";
    for (repo, command) in [
        (git_repo.path(), git_command.as_str()),
        (zmin_repo.path(), zmin_command.as_str()),
    ] {
        git(repo, ["config", "filter.lfs.process", command]);
        git(repo, ["config", "filter.lfs.required", "true"]);
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write lfs attributes");
        fs::write(repo.join("asset.bin"), payload).expect("write asset");
    }

    git(git_repo.path(), ["add", "."]);
    run_zmin(zmin_repo.path(), ["add", "."]);
    let git_pointer =
        command_stdout_bytes("git", git_repo.path(), &["cat-file", "-p", ":asset.bin"]);
    let zmin_pointer = command_stdout_bytes(
        zmin_bin(),
        zmin_repo.path(),
        &["cat-file", "-p", ":asset.bin"],
    );
    assert_eq!(zmin_pointer, git_pointer);
    assert!(
        String::from_utf8_lossy(&zmin_pointer)
            .starts_with("version https://git-lfs.github.com/spec/v1\n"),
        "expected LFS pointer, got {}",
        String::from_utf8_lossy(&zmin_pointer)
    );

    git_with_env(git_repo.path(), ["commit", "-m", "lfs asset"]);
    run_zmin(zmin_repo.path(), ["commit", "-m", "lfs asset"]);

    fs::remove_file(git_repo.path().join("asset.bin")).expect("remove git asset");
    fs::remove_file(zmin_repo.path().join("asset.bin")).expect("remove zmin asset");
    git(git_repo.path(), ["checkout", "--", "asset.bin"]);
    run_zmin(zmin_repo.path(), ["checkout", "--", "asset.bin"]);
    assert_eq!(
        fs::read(git_repo.path().join("asset.bin")).expect("read git asset"),
        payload
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("asset.bin")).expect("read zmin asset"),
        payload
    );
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            zmin_repo.path(),
            &["cat-file", "--filters", "HEAD:asset.bin"]
        ),
        command_stdout_bytes(
            "git",
            git_repo.path(),
            &["cat-file", "--filters", "HEAD:asset.bin"]
        )
    );

    let zmin_log = fs::read_to_string(zmin_repo.path().join("lfs.log")).expect("read zmin lfs log");
    assert!(zmin_log.contains("clean asset.bin\n"));
    assert!(zmin_log.contains("smudge asset.bin\n"));
}

#[cfg(unix)]
fn shell_quote_for_test(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

#[test]
fn cat_file_resolves_reflog_selector_for_ref_name_ending_with_at_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("hello"), b"hello\n").expect("write hello");
    git(repo.path(), ["add", "hello"]);
    git_with_env(repo.path(), ["commit", "-m", "hello"]);
    run_zmin(repo.path(), ["branch", "foo@"]);

    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-p", "foo@@{0}:hello"]),
        git(repo.path(), ["cat-file", "-p", "HEAD:hello"])
    );
}

#[test]
fn show_matches_stock_git_for_raw_commits_trees_blobs_and_tags() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "tag.gpgSign", "false"]);
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write a");
    fs::write(repo.path().join("dir/b.txt"), b"nested\n").expect("write b");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git_with_env(repo.path(), ["tag", "-a", "v1", "-m", "tag message"]);

    for args in [
        ["show", "HEAD:a.txt"].as_slice(),
        ["show", "HEAD^{tree}"].as_slice(),
        ["show", "HEAD"].as_slice(),
        ["show", "--oneline", "HEAD"].as_slice(),
        ["show", "--format=%H", "HEAD"].as_slice(),
        ["show", "--stat", "HEAD"].as_slice(),
        ["show", "--numstat", "--format=%H", "HEAD"].as_slice(),
        ["show", "--shortstat", "HEAD"].as_slice(),
        ["show", "--raw", "--format=%H", "HEAD"].as_slice(),
        ["show", "--summary", "--format=%H", "HEAD"].as_slice(),
        ["show", "--name-only", "--format=%H", "HEAD"].as_slice(),
        ["show", "--name-status", "--format=%H", "HEAD"].as_slice(),
        ["show", "--no-patch", "--format=raw", "HEAD"].as_slice(),
        ["show", "--format=raw", "HEAD"].as_slice(),
        ["show", "--no-patch", "--format=%H", "HEAD"].as_slice(),
        ["show", "--no-patch", "--pretty=format:%an <%ae>", "HEAD"].as_slice(),
        ["show", "--no-patch", "--oneline", "HEAD"].as_slice(),
        ["show", "--no-patch", "HEAD"].as_slice(),
        ["show", "--no-patch", "--format=raw", "v1"].as_slice(),
        ["show", "v1"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn treeish_path_resolution_and_ls_tree_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("src")).expect("create src");
    fs::write(repo.path().join("README.md"), b"hello\n").expect("write readme");
    fs::write(repo.path().join("src/main.rs"), b"fn main() {}\n").expect("write source");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    fs::write(repo.path().join("README.md"), b"hello again\n").expect("modify readme");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);

    for args in [
        ["rev-parse", "HEAD^{tree}"].as_slice(),
        ["rev-parse", "HEAD:src/main.rs"].as_slice(),
        ["cat-file", "-p", "HEAD:src/main.rs"].as_slice(),
        ["cat-file", "-p", "HEAD^{tree}"].as_slice(),
        ["rev-parse", "HEAD~1"].as_slice(),
        ["rev-parse", "HEAD~1^{tree}"].as_slice(),
        ["ls-tree", "HEAD"].as_slice(),
        ["ls-tree", "HEAD^{tree}"].as_slice(),
        ["ls-tree", "-r", "--name-only", "HEAD"].as_slice(),
        ["ls-tree", "-r", "-t", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn ls_tree_extended_options_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("src/deep")).expect("create nested dirs");
    fs::write(repo.path().join("README.md"), b"hello\n").expect("write readme");
    fs::write(repo.path().join("src/main.rs"), b"fn main() {}\n").expect("write source");
    fs::write(repo.path().join("src/deep/lib.rs"), b"pub fn lib() {}\n").expect("write nested");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    for args in [
        ["ls-tree", "-d", "HEAD", "src"].as_slice(),
        ["ls-tree", "-l", "HEAD"].as_slice(),
        ["ls-tree", "--object-only", "HEAD"].as_slice(),
        ["ls-tree", "--abbrev=10", "HEAD"].as_slice(),
        ["ls-tree", "--name-status", "HEAD"].as_slice(),
        ["ls-tree", "--format=%(objectname) %(path)", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    assert_eq!(
        command_stdout_bytes(zmin_bin(), repo.path(), &["ls-tree", "-z", "HEAD"]),
        command_stdout_bytes("git", repo.path(), &["ls-tree", "-z", "HEAD"])
    );
}

fn ls_tree_abbrev_fixture(program: &Path, sha256: bool) -> (TempDir, String, usize) {
    let repo = TempDir::new().expect("ls-tree abbrev fixture repo");
    let init_args = if sha256 {
        vec!["init", "-q", "--object-format=sha256"]
    } else {
        vec!["init", "-q"]
    };
    let init = raw_command_output(program, repo.path(), &init_args, "ls-tree abbrev init");
    assert_eq!(init.0, 0, "ls-tree abbrev init: {:?}", init.2);
    let object_id_len = if sha256 { 64 } else { 40 };
    let blob = raw_command_output_with_stdin(
        program,
        repo.path(),
        &["hash-object", "-w", "--stdin"],
        b"ls-tree abbrev\n",
        "ls-tree abbrev blob",
    );
    assert_eq!(blob.0, 0, "ls-tree abbrev blob: {:?}", blob.2);
    let blob_id = String::from_utf8(blob.1)
        .expect("ls-tree abbrev blob id utf8")
        .trim()
        .to_owned();
    assert_eq!(blob_id.len(), object_id_len);
    let mut tree_input = b"100644 file".to_vec();
    tree_input.push(0);
    tree_input.extend_from_slice(&hex_to_bytes(&blob_id));
    let algorithm = if sha256 {
        GitHashAlgorithm::Sha256
    } else {
        GitHashAlgorithm::Sha1
    };
    let tree_id = write_loose_object(repo.path(), algorithm, b"tree", &tree_input);
    assert_eq!(tree_id.len(), object_id_len);
    (repo, tree_id, object_id_len)
}

fn append_repository_config(repo: &Path, contents: &str) {
    let mut config = fs::OpenOptions::new()
        .append(true)
        .open(repo.join(".git/config"))
        .expect("open repository config for duplicate entry");
    config
        .write_all(contents.as_bytes())
        .expect("append duplicate repository config");
}

fn ls_tree_auto_collision_fixture(program: &Path, sha256: bool) -> (TempDir, String) {
    let repo = TempDir::new().expect("ls-tree auto collision fixture repo");
    let init_args = if sha256 {
        vec!["init", "-q", "--object-format=sha256"]
    } else {
        vec!["init", "-q"]
    };
    let init = raw_command_output(
        program,
        repo.path(),
        &init_args,
        "ls-tree auto collision init",
    );
    assert_eq!(init.0, 0, "ls-tree auto collision init: {:?}", init.2);
    let algorithm = if sha256 {
        GitHashAlgorithm::Sha256
    } else {
        GitHashAlgorithm::Sha1
    };
    let payloads = if sha256 {
        [
            ("a", b"abbrev-sha256-collision-004695".as_slice()),
            ("b", b"abbrev-sha256-collision-020299".as_slice()),
            ("safe", b"abbrev-safe-third".as_slice()),
        ]
    } else {
        [
            ("a", b"abbrev-sha1-collision-006687".as_slice()),
            ("b", b"abbrev-sha1-collision-040110".as_slice()),
            ("safe", b"abbrev-safe-third".as_slice()),
        ]
    };
    let ids = payloads
        .iter()
        .map(|(_, payload)| write_loose_object(repo.path(), algorithm, b"blob", payload))
        .collect::<Vec<_>>();
    let mut tree_input = Vec::new();
    for ((name, _), id) in payloads.iter().zip(&ids) {
        tree_input.extend_from_slice(b"100644 ");
        tree_input.extend_from_slice(name.as_bytes());
        tree_input.push(0);
        tree_input.extend_from_slice(&hex_to_bytes(id));
    }
    let tree_id = write_loose_object(repo.path(), algorithm, b"tree", &tree_input);
    let configured = raw_command_output(
        program,
        repo.path(),
        &["config", "core.abbrev", "auto"],
        "configure ls-tree auto collision",
    );
    assert_eq!(
        configured.0, 0,
        "configure auto collision: {:?}",
        configured.2
    );
    (repo, tree_id)
}

fn ls_tree_auto_packed_fixture(
    stock: &Path,
    zmin: &Path,
    sha256: bool,
) -> (TempDir, TempDir, String, String) {
    const PACKED_OBJECTS: usize = 16_384;
    let stock_repo = TempDir::new().expect("stock ls-tree auto packed fixture repo");
    let zmin_repo = TempDir::new().expect("zmin ls-tree auto packed fixture repo");
    let init_args = if sha256 {
        vec!["init", "-q", "--object-format=sha256"]
    } else {
        vec!["init", "-q"]
    };
    for (program, repo, label) in [
        (stock, stock_repo.path(), "stock ls-tree auto packed init"),
        (zmin, zmin_repo.path(), "zmin ls-tree auto packed init"),
    ] {
        let init = raw_command_output(program, repo, &init_args, label);
        assert_eq!(init.0, 0, "{label}: {:?}", init.2);
    }
    let mut import = Vec::new();
    for index in 0..PACKED_OBJECTS {
        let payload = format!("packed-{index:05}\n");
        import.extend_from_slice(b"blob\ndata ");
        import.extend_from_slice(payload.len().to_string().as_bytes());
        import.extend_from_slice(b"\n");
        import.extend_from_slice(payload.as_bytes());
    }
    import.extend_from_slice(b"done\n");
    let imported = raw_command_output_with_stdin(
        stock,
        stock_repo.path(),
        &["fast-import"],
        &import,
        "populate stock ls-tree auto packed fixture",
    );
    assert_eq!(imported.0, 0, "stock auto packed import: {:?}", imported.2);
    let stock_pack_dir = stock_repo.path().join(".git/objects/pack");
    let zmin_pack_dir = zmin_repo.path().join(".git/objects/pack");
    for entry in fs::read_dir(&stock_pack_dir).expect("read stock auto packed objects") {
        let entry = entry.expect("stock auto packed entry");
        let path = entry.path();
        if matches!(
            path.extension().and_then(OsStr::to_str),
            Some("pack") | Some("idx")
        ) {
            fs::copy(&path, zmin_pack_dir.join(entry.file_name()))
                .expect("copy auto packed index to zmin fixture");
        }
    }
    let algorithm = if sha256 {
        GitHashAlgorithm::Sha256
    } else {
        GitHashAlgorithm::Sha1
    };
    let blob_id = write_loose_object(stock_repo.path(), algorithm, b"blob", b"visible\n");
    let zmin_blob_id = write_loose_object(zmin_repo.path(), algorithm, b"blob", b"visible\n");
    assert_eq!(blob_id, zmin_blob_id, "packed fixture blob identity");
    let mut tree_input = b"100644 visible\0".to_vec();
    tree_input.extend_from_slice(&hex_to_bytes(&blob_id));
    let tree_id = write_loose_object(stock_repo.path(), algorithm, b"tree", &tree_input);
    let zmin_tree_id = write_loose_object(zmin_repo.path(), algorithm, b"tree", &tree_input);
    assert_eq!(tree_id, zmin_tree_id, "packed fixture tree identity");
    for (program, repo, label) in [
        (
            stock,
            stock_repo.path(),
            "configure stock auto packed fixture",
        ),
        (zmin, zmin_repo.path(), "configure zmin auto packed fixture"),
    ] {
        let configured =
            raw_command_output(program, repo, &["config", "core.abbrev", "auto"], label);
        assert_eq!(configured.0, 0, "{label}: {:?}", configured.2);
    }
    (stock_repo, zmin_repo, tree_id, zmin_tree_id)
}

fn assert_ls_tree_abbrev_width(
    output: &[u8],
    object_name_only: bool,
    expected: usize,
    case_name: &str,
) {
    let line = output
        .strip_suffix(b"\n")
        .unwrap_or_else(|| panic!("{case_name} did not end with LF: {output:?}"));
    let object_name = if object_name_only {
        line
    } else {
        line.split(|byte| *byte == b' ' || *byte == b'\t')
            .nth(2)
            .unwrap_or_else(|| panic!("{case_name} has no object name: {output:?}"))
    };
    assert_eq!(
        object_name.len(),
        expected,
        "{case_name} object-name width: output={output:?}"
    );
}

#[test]
fn ls_tree_abbrev_zero_and_no_abbrev_match_pinned_git_sha1_and_sha256() {
    let stock = pinned_stock_git_bin();
    let zmin = PathBuf::from(zmin_bin());
    for sha256 in [false, true] {
        let (stock_repo, stock_tree, object_id_len) = ls_tree_abbrev_fixture(&stock, sha256);
        let (zmin_repo, zmin_tree, zmin_object_id_len) = ls_tree_abbrev_fixture(&zmin, sha256);
        assert_eq!(object_id_len, zmin_object_id_len);
        for abbreviation in [
            "",
            "--abbrev",
            "--abbrev=0",
            "--no-abbrev",
            "--abbrev=1",
            "--abbrev=7",
            "--abbrev=100",
            "--abbrev=-1",
            "--abbrev=+1",
        ] {
            let expected_width = match abbreviation {
                "" | "--abbrev=0" | "--no-abbrev" | "--abbrev=100" => object_id_len,
                "--abbrev" => 7,
                "--abbrev=1" | "--abbrev=-1" | "--abbrev=+1" => 4,
                "--abbrev=7" => 7,
                _ => unreachable!("abbreviation case is fixed above"),
            };
            for mode in [
                ("default", None, false),
                ("long", Some("-l"), false),
                ("object-only", Some("--object-only"), true),
                ("format-objectname", Some("--format=%(objectname)"), true),
            ] {
                let mut args = vec!["ls-tree".to_owned(), "-r".to_owned()];
                if let Some(option) = mode.1 {
                    args.push(option.to_owned());
                }
                if !abbreviation.is_empty() {
                    args.push(abbreviation.to_owned());
                }
                args.push(stock_tree.clone());
                let stock_args = args.iter().map(String::as_str).collect::<Vec<_>>();
                let mut zmin_args = args.clone();
                let tree_arg = zmin_args.last_mut().expect("ls-tree abbrev tree argument");
                *tree_arg = zmin_tree.clone();
                let zmin_args = zmin_args.iter().map(String::as_str).collect::<Vec<_>>();
                let stock_output = raw_command_output(
                    &stock,
                    stock_repo.path(),
                    &stock_args,
                    "pinned stock ls-tree abbreviation",
                );
                let zmin_output = raw_command_output(
                    &zmin,
                    zmin_repo.path(),
                    &zmin_args,
                    "zmin ls-tree abbreviation",
                );
                assert_eq!(
                    stock_output, zmin_output,
                    "sha256={sha256} abbreviation={abbreviation:?} mode={}",
                    mode.0
                );
                assert_eq!(stock_output.0, 0);
                assert_ls_tree_abbrev_width(
                    &zmin_output.1,
                    mode.2,
                    expected_width,
                    &format!(
                        "sha256={sha256} abbreviation={abbreviation:?} mode={}",
                        mode.0
                    ),
                );
            }
        }

        for (program, repo) in [(&stock, stock_repo.path()), (&zmin, zmin_repo.path())] {
            let configured = raw_command_output(
                program,
                repo,
                &["config", "core.abbrev", "10"],
                "configure ls-tree core.abbrev",
            );
            assert_eq!(configured.0, 0, "configure core.abbrev: {:?}", configured.2);
        }
        for mode in [
            ("default", Vec::<&str>::new(), false),
            ("long", vec!["-l"], false),
            ("object-only", vec!["--object-only"], true),
            ("format-objectname", vec!["--format=%(objectname)"], true),
        ] {
            let mut stock_args = vec!["ls-tree", "--abbrev"];
            stock_args.extend(mode.1.iter().copied());
            stock_args.push(stock_tree.as_str());
            let mut zmin_args = stock_args.clone();
            *zmin_args.last_mut().expect("configured zmin tree argument") = zmin_tree.as_str();
            let stock_output = raw_command_output(
                &stock,
                stock_repo.path(),
                &stock_args,
                "configured stock bare abbrev",
            );
            let zmin_output = raw_command_output(
                &zmin,
                zmin_repo.path(),
                &zmin_args,
                "configured zmin bare abbrev",
            );
            assert_eq!(
                stock_output, zmin_output,
                "configured bare abbrev sha256={sha256} mode={}",
                mode.0
            );
            assert_eq!(stock_output.0, 0);
            assert_ls_tree_abbrev_width(
                &zmin_output.1,
                mode.2,
                10,
                &format!("configured bare abbrev sha256={sha256} mode={}", mode.0),
            );
        }

        for (program, repo) in [(&stock, stock_repo.path()), (&zmin, zmin_repo.path())] {
            let configured = raw_command_output(
                program,
                repo,
                &["config", "core.abbrev", "auto"],
                "configure automatic ls-tree core.abbrev",
            );
            assert_eq!(
                configured.0, 0,
                "configure automatic core.abbrev: {:?}",
                configured.2
            );
        }
        let stock_output = raw_command_output(
            &stock,
            stock_repo.path(),
            &["ls-tree", "--abbrev", stock_tree.as_str()],
            "automatic stock bare abbrev",
        );
        let zmin_output = raw_command_output(
            &zmin,
            zmin_repo.path(),
            &["ls-tree", "--abbrev", zmin_tree.as_str()],
            "automatic zmin bare abbrev",
        );
        assert_eq!(
            stock_output, zmin_output,
            "automatic bare abbrev sha256={sha256}"
        );
        assert_ls_tree_abbrev_width(
            &zmin_output.1,
            false,
            7,
            &format!("automatic bare abbrev sha256={sha256}"),
        );
    }
}

#[test]
fn ls_tree_cli_abbrev_numeric_errors_match_pinned_git() {
    let stock = pinned_stock_git_bin();
    let zmin = PathBuf::from(zmin_bin());
    let cases = [
        ("2147483648", true),
        ("9223372036854775808", true),
        ("bogus", false),
        ("0x10", false),
    ];
    for sha256 in [false, true] {
        let (stock_repo, stock_tree, _) = ls_tree_abbrev_fixture(&stock, sha256);
        let (zmin_repo, zmin_tree, _) = ls_tree_abbrev_fixture(&zmin, sha256);
        for (value, succeeds) in cases {
            let abbreviation = format!("--abbrev={value}");
            let stock_args = ["ls-tree", abbreviation.as_str(), stock_tree.as_str()];
            let zmin_args = ["ls-tree", abbreviation.as_str(), zmin_tree.as_str()];
            let stock_output = raw_command_output(
                &stock,
                stock_repo.path(),
                &stock_args,
                "pinned stock cli abbreviation numeric",
            );
            let zmin_output = raw_command_output(
                &zmin,
                zmin_repo.path(),
                &zmin_args,
                "zmin cli abbreviation numeric",
            );
            assert_eq!(stock_output, zmin_output, "sha256={sha256} value={value:?}");
            assert_eq!(
                zmin_output.0 == 0,
                succeeds,
                "sha256={sha256} value={value:?}"
            );
            if !succeeds {
                assert_eq!(
                    zmin_output.2, b"error: option `abbrev' expects a numerical value\n",
                    "sha256={sha256} value={value:?}"
                );
            }
        }
    }
}

#[test]
fn ls_tree_auto_extends_only_colliding_objects_sha1_and_sha256() {
    let stock = pinned_stock_git_bin();
    let zmin = PathBuf::from(zmin_bin());
    for sha256 in [false, true] {
        let (stock_repo, stock_tree) = ls_tree_auto_collision_fixture(&stock, sha256);
        let (zmin_repo, zmin_tree) = ls_tree_auto_collision_fixture(&zmin, sha256);
        let stock_output = raw_command_output(
            &stock,
            stock_repo.path(),
            &["ls-tree", "--abbrev", stock_tree.as_str()],
            "pinned stock auto collision ls-tree",
        );
        let zmin_output = raw_command_output(
            &zmin,
            zmin_repo.path(),
            &["ls-tree", "--abbrev", zmin_tree.as_str()],
            "zmin auto collision ls-tree",
        );
        assert_eq!(stock_output, zmin_output, "auto collision sha256={sha256}");
        assert_eq!(zmin_output.0, 0);
        let widths = zmin_output
            .1
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| {
                line.split(|byte| *byte == b' ' || *byte == b'\t')
                    .nth(2)
                    .expect("auto collision object name")
                    .len()
            })
            .collect::<Vec<_>>();
        assert_eq!(widths, [8, 8, 7], "auto collision widths sha256={sha256}");

        let stock_explicit = raw_command_output(
            &stock,
            stock_repo.path(),
            &["ls-tree", "--abbrev=7", stock_tree.as_str()],
            "pinned stock explicit minimum collision ls-tree",
        );
        let zmin_explicit = raw_command_output(
            &zmin,
            zmin_repo.path(),
            &["ls-tree", "--abbrev=7", zmin_tree.as_str()],
            "zmin explicit minimum collision ls-tree",
        );
        assert_eq!(
            stock_explicit, zmin_explicit,
            "explicit collision sha256={sha256}"
        );
        assert_eq!(zmin_explicit.0, 0);
        let explicit_widths = zmin_explicit
            .1
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| {
                line.split(|byte| *byte == b' ' || *byte == b'\t')
                    .nth(2)
                    .expect("explicit collision object name")
                    .len()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            explicit_widths,
            [8, 8, 7],
            "explicit collision widths sha256={sha256}"
        );

        for (program, repo) in [(&stock, stock_repo.path()), (&zmin, zmin_repo.path())] {
            let configured = raw_command_output(
                program,
                repo,
                &["config", "core.abbrev", "7"],
                "configure numeric minimum collision fixture",
            );
            assert_eq!(
                configured.0, 0,
                "configure numeric collision: {:?}",
                configured.2
            );
        }
        let stock_configured = raw_command_output(
            &stock,
            stock_repo.path(),
            &["ls-tree", "--abbrev", stock_tree.as_str()],
            "pinned stock configured minimum collision ls-tree",
        );
        let zmin_configured = raw_command_output(
            &zmin,
            zmin_repo.path(),
            &["ls-tree", "--abbrev", zmin_tree.as_str()],
            "zmin configured minimum collision ls-tree",
        );
        assert_eq!(
            stock_configured, zmin_configured,
            "configured collision sha256={sha256}"
        );
        assert_eq!(zmin_configured.0, 0);
        let configured_widths = zmin_configured
            .1
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| {
                line.split(|byte| *byte == b' ' || *byte == b'\t')
                    .nth(2)
                    .expect("configured collision object name")
                    .len()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            configured_widths,
            [8, 8, 7],
            "configured collision widths sha256={sha256}"
        );

        let format_args = [
            "ls-tree",
            "--abbrev=7",
            "--format=%(objectname)%x09%(path)",
            stock_tree.as_str(),
        ];
        let stock_format = raw_command_output(
            &stock,
            stock_repo.path(),
            &format_args,
            "pinned stock formatted collision ls-tree",
        );
        let zmin_format_args = [
            "ls-tree",
            "--abbrev=7",
            "--format=%(objectname)%x09%(path)",
            zmin_tree.as_str(),
        ];
        let zmin_format = raw_command_output(
            &zmin,
            zmin_repo.path(),
            &zmin_format_args,
            "zmin formatted collision ls-tree",
        );
        assert_eq!(
            zmin_format, stock_format,
            "formatted collision sha256={sha256}"
        );
    }
}

#[test]
fn ls_tree_auto_uses_packed_object_count_boundary_sha1_and_sha256() {
    let stock = pinned_stock_git_bin();
    let zmin = PathBuf::from(zmin_bin());
    for sha256 in [false, true] {
        let (stock_repo, zmin_repo, stock_tree, zmin_tree) =
            ls_tree_auto_packed_fixture(&stock, &zmin, sha256);
        let stock_output = raw_command_output(
            &stock,
            stock_repo.path(),
            &["ls-tree", "--abbrev", stock_tree.as_str()],
            "pinned stock packed auto ls-tree",
        );
        let zmin_output = raw_command_output(
            &zmin,
            zmin_repo.path(),
            &["ls-tree", "--abbrev", zmin_tree.as_str()],
            "zmin packed auto ls-tree",
        );
        assert_eq!(stock_output, zmin_output, "packed auto sha256={sha256}");
        assert_eq!(zmin_output.0, 0, "packed zmin auto: {:?}", zmin_output.2);

        for (program, repo) in [(&stock, stock_repo.path()), (&zmin, zmin_repo.path())] {
            let unset = raw_command_output(
                program,
                repo,
                &["config", "--unset", "core.abbrev"],
                "unset packed auto core.abbrev",
            );
            assert_eq!(unset.0, 0, "unset packed auto config: {:?}", unset.2);
        }
        let stock_missing = raw_command_output(
            &stock,
            stock_repo.path(),
            &["ls-tree", "--abbrev", stock_tree.as_str()],
            "pinned stock packed missing-config auto ls-tree",
        );
        let zmin_missing = raw_command_output(
            &zmin,
            zmin_repo.path(),
            &["ls-tree", "--abbrev", zmin_tree.as_str()],
            "zmin packed missing-config auto ls-tree",
        );
        assert_eq!(
            stock_missing, zmin_missing,
            "packed missing-config auto sha256={sha256}"
        );
        assert_ls_tree_abbrev_width(
            &zmin_missing.1,
            false,
            8,
            &format!("packed missing-config auto sha256={sha256}"),
        );
    }
}

#[test]
fn ls_tree_implicit_core_abbrev_entry_matches_pinned_git() {
    let stock = pinned_stock_git_bin();
    let zmin = PathBuf::from(zmin_bin());
    for sha256 in [false, true] {
        let (stock_repo, stock_tree, _) = ls_tree_abbrev_fixture(&stock, sha256);
        let (zmin_repo, zmin_tree, _) = ls_tree_abbrev_fixture(&zmin, sha256);
        for repo in [stock_repo.path(), zmin_repo.path()] {
            fs::write(repo.join(".git/config"), b"[core]\n\tabbrev\n")
                .expect("write implicit core.abbrev entry");
        }
        let stock_output = raw_command_output(
            &stock,
            stock_repo.path(),
            &["ls-tree", stock_tree.as_str()],
            "pinned stock implicit core.abbrev",
        );
        let zmin_output = raw_command_output(
            &zmin,
            zmin_repo.path(),
            &["ls-tree", zmin_tree.as_str()],
            "zmin implicit core.abbrev",
        );
        assert_eq!(stock_output, zmin_output, "sha256={sha256}");
        assert_eq!(zmin_output.0, 128);
        assert!(zmin_output.1.is_empty());
    }
}

#[test]
fn ls_tree_invalid_core_abbrev_wins_over_help() {
    let stock = pinned_stock_git_bin();
    let zmin = PathBuf::from(zmin_bin());
    for sha256 in [false, true] {
        let stock_repo = TempDir::new().expect("stock help precedence repo");
        let zmin_repo = TempDir::new().expect("zmin help precedence repo");
        let init_args = if sha256 {
            vec!["init", "-q", "--object-format=sha256"]
        } else {
            vec!["init", "-q"]
        };
        for (program, repo) in [(&stock, stock_repo.path()), (&zmin, zmin_repo.path())] {
            let initialized =
                raw_command_output(program, repo, &init_args, "initialize help precedence repo");
            assert_eq!(initialized.0, 0, "sha256={sha256}: {:?}", initialized.2);
            let object_format = if sha256 {
                "\tobjectformat = sha256\n"
            } else {
                ""
            };
            let config = format!(
                "[core]\n\trepositoryformatversion = {}\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n{object_format}\tabbrev = 3\n",
                if sha256 { 1 } else { 0 }
            );
            fs::write(repo.join(".git/config"), config).expect("write help precedence config");
        }
        let stock_output = raw_command_output(
            &stock,
            stock_repo.path(),
            &["ls-tree", "-h"],
            "pinned stock invalid config help",
        );
        let zmin_output = raw_command_output(
            &zmin,
            zmin_repo.path(),
            &["ls-tree", "-h"],
            "zmin invalid config help",
        );
        assert_eq!(stock_output, zmin_output, "sha256={sha256}");
    }
}

#[test]
fn ls_tree_invalid_core_abbrev_wins_over_help_outside_repository() {
    let stock = pinned_stock_git_bin();
    let zmin = PathBuf::from(zmin_bin());
    let cwd = TempDir::new().expect("ls-tree outside-repository help directory");
    let args = ["-c", "core.abbrev=bogus", "ls-tree", "-h"];
    let stock_output = raw_command_output(
        &stock,
        cwd.path(),
        &args,
        "pinned stock invalid config help outside repository",
    );
    let zmin_output = raw_command_output(
        &zmin,
        cwd.path(),
        &args,
        "zmin invalid config help outside repository",
    );
    assert_eq!(stock_output, zmin_output);
    assert_eq!(zmin_output.0, 128);
    assert!(zmin_output.1.is_empty());
}

#[test]
fn ls_tree_help_config_phases_match_pinned_git() {
    let stock = pinned_stock_git_bin();
    let zmin = PathBuf::from(zmin_bin());

    let global_invalid = TempDir::new().expect("invalid global config directory");
    fs::write(
        global_invalid.path().join("gitconfig"),
        b"[core]\n\tabbrev = bogus\n",
    )
    .expect("write invalid global config");
    let outside = TempDir::new().expect("outside-repository help directory");
    for args in [
        ["ls-tree", "-h"].as_slice(),
        ["-c", "core.abbrev=bogus", "ls-tree", "-h"].as_slice(),
    ] {
        let stock_output = raw_command_output_with_env(
            &stock,
            outside.path(),
            args,
            &[(
                "GIT_CONFIG_GLOBAL",
                &global_invalid.path().join("gitconfig"),
            )],
            "pinned stock global-invalid ls-tree help",
        );
        let zmin_output = raw_command_output_with_env(
            &zmin,
            outside.path(),
            args,
            &[(
                "GIT_CONFIG_GLOBAL",
                &global_invalid.path().join("gitconfig"),
            )],
            "zmin global-invalid ls-tree help",
        );
        assert_eq!(stock_output, zmin_output, "global invalid args={args:?}");
        assert_eq!(zmin_output.0, 128);
    }

    let global_valid = TempDir::new().expect("valid global config directory");
    fs::write(
        global_valid.path().join("gitconfig"),
        b"[core]\n\tabbrev = 7\n",
    )
    .expect("write valid global config");
    let args = ["-c", "core.abbrev=bogus", "ls-tree", "-h"];
    let stock_output = raw_command_output_with_env(
        &stock,
        outside.path(),
        &args,
        &[("GIT_CONFIG_GLOBAL", &global_valid.path().join("gitconfig"))],
        "pinned stock command-invalid outside-repository help",
    );
    let zmin_output = raw_command_output_with_env(
        &zmin,
        outside.path(),
        &args,
        &[("GIT_CONFIG_GLOBAL", &global_valid.path().join("gitconfig"))],
        "zmin command-invalid outside-repository help",
    );
    assert_eq!(stock_output, zmin_output);
    assert_eq!(zmin_output.0, 128);

    for (local, command, expected_success, label) in [
        ("bogus", "10", false, "local invalid wins"),
        ("10", "bogus", false, "command invalid follows local"),
    ] {
        let (stock_repo, stock_tree, _) = ls_tree_abbrev_fixture(&stock, false);
        let (zmin_repo, zmin_tree, _) = ls_tree_abbrev_fixture(&zmin, false);
        append_repository_config(
            stock_repo.path(),
            &format!("\n[core]\n\tabbrev = {local}\n"),
        );
        append_repository_config(zmin_repo.path(), &format!("\n[core]\n\tabbrev = {local}\n"));
        let stock_args = ["-c", &format!("core.abbrev={command}"), "ls-tree", "-h"];
        let zmin_args = ["-c", &format!("core.abbrev={command}"), "ls-tree", "-h"];
        let stock_output = raw_command_output(&stock, stock_repo.path(), &stock_args, label);
        let zmin_output = raw_command_output(&zmin, zmin_repo.path(), &zmin_args, label);
        assert_eq!(stock_output, zmin_output, "{label}");
        assert_eq!(zmin_output.0 == 0, expected_success, "{label}");
        let _ = (stock_tree, zmin_tree);
    }

    for (first, second) in [("bogus", "0"), ("0", "bogus")] {
        let (stock_repo, _, _) = ls_tree_abbrev_fixture(&stock, false);
        let (zmin_repo, _, _) = ls_tree_abbrev_fixture(&zmin, false);
        let stock_args = [
            "-c",
            &format!("core.abbrev={first}"),
            "-c",
            &format!("core.abbrev={second}"),
            "ls-tree",
            "-h",
        ];
        let zmin_args = stock_args;
        let stock_output = raw_command_output(
            &stock,
            stock_repo.path(),
            &stock_args,
            "pinned stock duplicate invalid command config",
        );
        let zmin_output = raw_command_output(
            &zmin,
            zmin_repo.path(),
            &zmin_args,
            "zmin duplicate invalid command config",
        );
        assert_eq!(stock_output, zmin_output, "first={first} second={second}");
        assert_eq!(zmin_output.0, 128);
    }
}

#[test]
fn ls_tree_duplicate_core_abbrev_entries_validate_in_order() {
    let stock = pinned_stock_git_bin();
    let zmin = PathBuf::from(zmin_bin());
    let file_cases = [
        (
            "invalid-before-valid",
            "\n[core]\n\tabbrev = bogus\n\tabbrev = 10\n",
            false,
        ),
        (
            "valid-before-invalid",
            "\n[core]\n\tabbrev = 10\n\tabbrev = bogus\n",
            false,
        ),
        (
            "valid-last-wins",
            "\n[core]\n\tabbrev = 7\n\tabbrev = 10\n",
            true,
        ),
    ];
    let command_cases = [
        ("invalid-before-valid", "bogus", "10", false),
        ("valid-before-invalid", "10", "bogus", false),
        ("valid-last-wins", "7", "10", true),
    ];
    for sha256 in [false, true] {
        for (label, contents, succeeds) in file_cases {
            let (stock_repo, stock_tree, _) = ls_tree_abbrev_fixture(&stock, sha256);
            let (zmin_repo, zmin_tree, _) = ls_tree_abbrev_fixture(&zmin, sha256);
            append_repository_config(stock_repo.path(), contents);
            append_repository_config(zmin_repo.path(), contents);
            let stock_output = raw_command_output(
                &stock,
                stock_repo.path(),
                &["ls-tree", "--abbrev", stock_tree.as_str()],
                "pinned stock duplicate file config",
            );
            let zmin_output = raw_command_output(
                &zmin,
                zmin_repo.path(),
                &["ls-tree", "--abbrev", zmin_tree.as_str()],
                "zmin duplicate file config",
            );
            assert_eq!(stock_output, zmin_output, "sha256={sha256} file={label}");
            assert_eq!(zmin_output.0 == 0, succeeds, "sha256={sha256} file={label}");
            if succeeds {
                assert_ls_tree_abbrev_width(
                    &zmin_output.1,
                    false,
                    10,
                    &format!("sha256={sha256} file={label}"),
                );
            }
        }
        for (label, first, second, succeeds) in command_cases {
            let (stock_repo, stock_tree, _) = ls_tree_abbrev_fixture(&stock, sha256);
            let (zmin_repo, zmin_tree, _) = ls_tree_abbrev_fixture(&zmin, sha256);
            let stock_args = vec![
                "-c".to_owned(),
                format!("core.abbrev={first}"),
                "-c".to_owned(),
                format!("core.abbrev={second}"),
                "ls-tree".to_owned(),
                "--abbrev".to_owned(),
                stock_tree,
            ];
            let mut zmin_args = stock_args.clone();
            *zmin_args.last_mut().expect("command zmin tree argument") = zmin_tree;
            let stock_refs = stock_args.iter().map(String::as_str).collect::<Vec<_>>();
            let zmin_refs = zmin_args.iter().map(String::as_str).collect::<Vec<_>>();
            let stock_output = raw_command_output(
                &stock,
                stock_repo.path(),
                &stock_refs,
                "pinned stock duplicate command config",
            );
            let zmin_output = raw_command_output(
                &zmin,
                zmin_repo.path(),
                &zmin_refs,
                "zmin duplicate command config",
            );
            assert_eq!(stock_output, zmin_output, "sha256={sha256} command={label}");
            assert_eq!(
                zmin_output.0 == 0,
                succeeds,
                "sha256={sha256} command={label}"
            );
            if succeeds {
                assert_ls_tree_abbrev_width(
                    &zmin_output.1,
                    false,
                    10,
                    &format!("sha256={sha256} command={label}"),
                );
            }
        }
    }
}

#[test]
fn ls_tree_cli_abbrev_leading_c_isspace_matches_pinned_git() {
    let stock = pinned_stock_git_bin();
    let zmin = PathBuf::from(zmin_bin());
    let values = [" 7", "\t7", "\r7", "\n7", "\u{b}7", "\u{c}7", "7 ", "7\t"];
    for sha256 in [false, true] {
        let (stock_repo, stock_tree, _) = ls_tree_abbrev_fixture(&stock, sha256);
        let (zmin_repo, zmin_tree, _) = ls_tree_abbrev_fixture(&zmin, sha256);
        for value in values {
            let abbreviation = format!("--abbrev={value}");
            let stock_args = ["ls-tree", abbreviation.as_str(), stock_tree.as_str()];
            let zmin_args = ["ls-tree", abbreviation.as_str(), zmin_tree.as_str()];
            let stock_output = raw_command_output(
                &stock,
                stock_repo.path(),
                &stock_args,
                "pinned stock C isspace abbreviation",
            );
            let zmin_output = raw_command_output(
                &zmin,
                zmin_repo.path(),
                &zmin_args,
                "zmin C isspace abbreviation",
            );
            assert_eq!(stock_output, zmin_output, "sha256={sha256} value={value:?}");
        }
    }
}

#[test]
fn ls_tree_core_abbrev_config_values_match_pinned_git_sha1_and_sha256() {
    let stock = pinned_stock_git_bin();
    let zmin = PathBuf::from(zmin_bin());
    let values = [
        "0",
        "1",
        "3",
        "-1",
        "+1",
        "",
        "4",
        "10",
        "100",
        "010",
        "0x10",
        "1k",
        "1M",
        "1G",
        "2147483647",
        "2147483648",
        "9223372036854775807",
        "0x",
        "08",
        "09",
        "0b1",
        "00x10",
        "auto",
        "no",
        "false",
        "off",
        "bogus",
        "1z",
    ];
    for sha256 in [false, true] {
        let (stock_repo, stock_tree, object_id_len) = ls_tree_abbrev_fixture(&stock, sha256);
        let (zmin_repo, zmin_tree, zmin_object_id_len) = ls_tree_abbrev_fixture(&zmin, sha256);
        assert_eq!(object_id_len, zmin_object_id_len);
        for (program, repo) in [(&stock, stock_repo.path()), (&zmin, zmin_repo.path())] {
            let configured = raw_command_output(
                program,
                repo,
                &["config", "core.precomposeunicode", "true"],
                "align ls-tree config fixture",
            );
            assert_eq!(
                configured.0, 0,
                "configure precomposeunicode: {:?}",
                configured.2
            );
        }
        if sha256 {
            for key in ["core.zminfixtureone", "core.zminfixturetwo"] {
                let configured = raw_command_output(
                    &zmin,
                    zmin_repo.path(),
                    &["config", key, "true"],
                    "align sha256 ls-tree config fixture",
                );
                assert_eq!(configured.0, 0, "configure {key}: {:?}", configured.2);
            }
        }
        for value in values {
            let stock_config = raw_command_output(
                &stock,
                stock_repo.path(),
                &["config", "core.abbrev", value],
                "configure pinned stock core.abbrev",
            );
            let zmin_config = raw_command_output(
                &zmin,
                zmin_repo.path(),
                &["config", "core.abbrev", value],
                "configure zmin core.abbrev",
            );
            assert_eq!(
                stock_config, zmin_config,
                "sha256={sha256} config={value:?} configuration"
            );

            let stock_output = raw_command_output(
                &stock,
                stock_repo.path(),
                &["ls-tree", "--abbrev", stock_tree.as_str()],
                "pinned stock configured ls-tree",
            );
            let zmin_output = raw_command_output(
                &zmin,
                zmin_repo.path(),
                &["ls-tree", "--abbrev", zmin_tree.as_str()],
                "zmin configured ls-tree",
            );
            assert_eq!(
                stock_output, zmin_output,
                "sha256={sha256} config={value:?} configured ls-tree"
            );
            let expected_width = match value {
                "4" => Some(4),
                "10" => Some(10),
                "010" => Some(8),
                "0x10" => Some(16),
                "" | "100" | "1k" | "1M" | "1G" | "2147483647" | "auto" | "no" | "false"
                | "off" => Some(if value == "auto" { 7 } else { object_id_len }),
                _ => None,
            };
            if let Some(expected_width) = expected_width {
                assert_ls_tree_abbrev_width(
                    &zmin_output.1,
                    false,
                    expected_width,
                    &format!("sha256={sha256} config={value:?}"),
                );
            } else {
                assert_eq!(zmin_output.0, 128);
                assert!(zmin_output.1.is_empty());
            }
        }
    }
}

#[test]
fn ls_tree_invalid_core_abbrev_is_rejected_before_cli_overrides() {
    let stock = pinned_stock_git_bin();
    let zmin = PathBuf::from(zmin_bin());
    let invalid_values = [
        "0",
        "1",
        "3",
        "-1",
        "+1",
        "0x",
        "08",
        "09",
        "0b1",
        "00x10",
        "2147483648",
        "-2147483649",
        "9223372036854775807",
        "bogus",
        "1z",
    ];
    let cli_variants = [
        Vec::<&str>::new(),
        vec!["--abbrev"],
        vec!["--abbrev=7"],
        vec!["--abbrev=0"],
        vec!["--no-abbrev"],
        vec!["--long", "--object-only"],
    ];
    for sha256 in [false, true] {
        let (stock_repo, stock_tree, _) = ls_tree_abbrev_fixture(&stock, sha256);
        let (zmin_repo, zmin_tree, _) = ls_tree_abbrev_fixture(&zmin, sha256);
        for (program, repo) in [(&stock, stock_repo.path()), (&zmin, zmin_repo.path())] {
            let configured = raw_command_output(
                program,
                repo,
                &["config", "core.precomposeunicode", "true"],
                "align invalid ls-tree config fixture",
            );
            assert_eq!(configured.0, 0, "align invalid config: {:?}", configured.2);
        }
        if sha256 {
            for key in ["core.zminfixtureone", "core.zminfixturetwo"] {
                let configured = raw_command_output(
                    &zmin,
                    zmin_repo.path(),
                    &["config", key, "true"],
                    "align invalid sha256 ls-tree config fixture",
                );
                assert_eq!(configured.0, 0, "configure {key}: {:?}", configured.2);
            }
        }
        for value in invalid_values {
            for (program, repo) in [(&stock, stock_repo.path()), (&zmin, zmin_repo.path())] {
                let configured = raw_command_output(
                    program,
                    repo,
                    &["config", "core.abbrev", value],
                    "configure invalid core.abbrev",
                );
                assert_eq!(configured.0, 0, "configure {value:?}: {:?}", configured.2);
            }
            for cli in &cli_variants {
                let mut stock_args = vec!["ls-tree"];
                stock_args.extend(cli.iter().copied());
                stock_args.push(stock_tree.as_str());
                let mut zmin_args = stock_args.clone();
                *zmin_args.last_mut().expect("invalid zmin tree argument") = zmin_tree.as_str();
                let stock_output = raw_command_output(
                    &stock,
                    stock_repo.path(),
                    &stock_args,
                    "pinned stock invalid core.abbrev",
                );
                let zmin_output = raw_command_output(
                    &zmin,
                    zmin_repo.path(),
                    &zmin_args,
                    "zmin invalid core.abbrev",
                );
                assert_eq!(
                    stock_output, zmin_output,
                    "sha256={sha256} value={value:?} cli={cli:?}"
                );
                assert_eq!(zmin_output.0, 128);
                assert!(zmin_output.1.is_empty());
            }
        }
    }
}

#[test]
fn ls_tree_command_line_core_abbrev_errors_match_pinned_git() {
    let stock = pinned_stock_git_bin();
    let zmin = PathBuf::from(zmin_bin());
    let invalid_values = [
        (
            "bogus",
            "fatal: bad numeric config value 'bogus' for 'core.abbrev': invalid unit\n",
        ),
        (
            "0x",
            "fatal: bad numeric config value '0x' for 'core.abbrev': invalid unit\n",
        ),
        (
            "1",
            "error: abbrev length out of range: 1\nfatal: unable to parse 'core.abbrev' from command-line config\n",
        ),
        (
            "2147483648",
            "fatal: bad numeric config value '2147483648' for 'core.abbrev': out of range\n",
        ),
    ];
    let cli_variants = [
        Vec::<&str>::new(),
        vec!["--abbrev"],
        vec!["--abbrev=7"],
        vec!["--abbrev=0"],
        vec!["--no-abbrev"],
    ];
    for sha256 in [false, true] {
        let (stock_repo, stock_tree, _) = ls_tree_abbrev_fixture(&stock, sha256);
        let (zmin_repo, zmin_tree, _) = ls_tree_abbrev_fixture(&zmin, sha256);
        for (value, expected_stderr) in invalid_values {
            for cli in &cli_variants {
                let mut stock_args = vec![
                    "-c".to_owned(),
                    format!("core.abbrev={value}"),
                    "ls-tree".to_owned(),
                ];
                stock_args.extend(cli.iter().map(|argument| (*argument).to_owned()));
                stock_args.push(stock_tree.clone());
                let stock_refs = stock_args.iter().map(String::as_str).collect::<Vec<_>>();
                let mut zmin_args = stock_args.clone();
                *zmin_args
                    .last_mut()
                    .expect("command-line zmin tree argument") = zmin_tree.clone();
                let zmin_refs = zmin_args.iter().map(String::as_str).collect::<Vec<_>>();
                let stock_output = raw_command_output(
                    &stock,
                    stock_repo.path(),
                    &stock_refs,
                    "pinned stock command-line core.abbrev",
                );
                let zmin_output = raw_command_output(
                    &zmin,
                    zmin_repo.path(),
                    &zmin_refs,
                    "zmin command-line core.abbrev",
                );
                assert_eq!(
                    stock_output, zmin_output,
                    "sha256={sha256} value={value:?} cli={cli:?}"
                );
                assert_eq!(zmin_output.0, 128);
                assert!(zmin_output.1.is_empty());
                assert_eq!(
                    zmin_output.2,
                    expected_stderr.as_bytes(),
                    "sha256={sha256} value={value:?} cli={cli:?} command-line origin"
                );
            }
        }
    }
}

const RAW_LS_TREE_PATH: &[u8] = b"raw\"tab\tline\n\\high\x80";

fn raw_ls_tree_fixture(program: &Path) -> (TempDir, String) {
    let repo = TempDir::new().expect("raw ls-tree fixture repo");
    let init = raw_command_output(program, repo.path(), &["init", "-q"], "raw ls-tree init");
    assert_eq!(init.0, 0, "raw ls-tree init: {:?}", init.2);
    fs::create_dir(repo.path().join("nested")).expect("raw ls-tree nested cwd");
    let blob = raw_command_output_with_stdin(
        program,
        repo.path(),
        &["hash-object", "-w", "--stdin"],
        b"raw-path\n",
        "raw ls-tree blob",
    );
    assert_eq!(blob.0, 0, "raw ls-tree blob: {:?}", blob.2);
    let blob_id = String::from_utf8(blob.1)
        .expect("raw ls-tree blob id utf8")
        .trim()
        .to_owned();
    let mut inner_input = b"100644 ".to_vec();
    inner_input.extend_from_slice(RAW_LS_TREE_PATH);
    inner_input.push(0);
    inner_input.extend_from_slice(&hex_to_bytes(&blob_id));
    let inner = raw_command_output_with_stdin(
        program,
        repo.path(),
        &["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
        &inner_input,
        "raw ls-tree inner tree",
    );
    assert_eq!(inner.0, 0, "raw ls-tree inner tree: {:?}", inner.2);
    let inner_id = String::from_utf8(inner.1)
        .expect("raw ls-tree inner id utf8")
        .trim()
        .to_owned();
    let mut root_input = b"40000 nested".to_vec();
    root_input.push(0);
    root_input.extend_from_slice(&hex_to_bytes(&inner_id));
    let root = raw_command_output_with_stdin(
        program,
        repo.path(),
        &["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
        &root_input,
        "raw ls-tree root tree",
    );
    assert_eq!(root.0, 0, "raw ls-tree root tree: {:?}", root.2);
    let root_id = String::from_utf8(root.1)
        .expect("raw ls-tree root id utf8")
        .trim()
        .to_owned();
    (repo, root_id)
}

#[test]
fn ls_tree_preserves_raw_non_utf8_paths_across_renderers() {
    let stock = pinned_stock_git_bin();
    let zmin = PathBuf::from(zmin_bin());
    let format = "--format=%(objectmode)%x09%(objecttype)%x09%(objectname)%x09%(path)";
    let command_specs = vec![
        vec!["ls-tree", "-r", "TREE"],
        vec!["ls-tree", "-r", "-z", "TREE"],
        vec!["ls-tree", "-r", "-l", "TREE"],
        vec!["ls-tree", "-r", "-l", "-z", "TREE"],
        vec!["ls-tree", "-r", "--name-only", "TREE"],
        vec!["ls-tree", "-r", "--name-only", "-z", "TREE"],
        vec!["ls-tree", "-r", "--name-status", "TREE"],
        vec!["ls-tree", "-r", "--name-status", "-z", "TREE"],
        vec!["ls-tree", "-r", "--object-only", "TREE"],
        vec!["ls-tree", "-r", "--object-only", "-z", "TREE"],
        vec!["ls-tree", "-r", "--abbrev=7", "-z", "TREE"],
        vec!["ls-tree", "-r", "--abbrev=10", "-z", "TREE"],
        vec!["ls-tree", "-r", format, "TREE"],
        vec!["ls-tree", "-r", "-z", format, "TREE"],
        vec!["ls-tree", "-r", "--full-name", "-z", "TREE"],
        vec!["ls-tree", "-r", "--full-tree", "-z", "TREE"],
    ];
    for quote_path in ["true", "false"] {
        let (stock_repo, stock_tree) = raw_ls_tree_fixture(&stock);
        let (zmin_repo, zmin_tree) = raw_ls_tree_fixture(&zmin);
        for program_repo in [
            (&stock, &stock_repo, stock_tree.as_str()),
            (&zmin, &zmin_repo, zmin_tree.as_str()),
        ] {
            let config = raw_command_output(
                program_repo.0,
                program_repo.1.path(),
                &["config", "core.quotePath", quote_path],
                "raw ls-tree quotePath",
            );
            assert_eq!(config.0, 0, "set quotePath {quote_path}: {:?}", config.2);
        }
        for spec in &command_specs {
            let stock_args = spec
                .iter()
                .map(|arg| {
                    if *arg == "TREE" {
                        stock_tree.as_str()
                    } else {
                        arg
                    }
                })
                .collect::<Vec<_>>();
            let zmin_args = spec
                .iter()
                .map(|arg| {
                    if *arg == "TREE" {
                        zmin_tree.as_str()
                    } else {
                        arg
                    }
                })
                .collect::<Vec<_>>();
            let full_path_mode = spec
                .iter()
                .any(|arg| *arg == "--full-name" || *arg == "--full-tree");
            let stock_cwd = if full_path_mode {
                stock_repo.path().join("nested")
            } else {
                stock_repo.path().to_path_buf()
            };
            let zmin_cwd = if full_path_mode {
                zmin_repo.path().join("nested")
            } else {
                zmin_repo.path().to_path_buf()
            };
            let stock_output =
                raw_command_output(&stock, &stock_cwd, &stock_args, "pinned stock raw ls-tree");
            let zmin_output = raw_command_output(&zmin, &zmin_cwd, &zmin_args, "zmin raw ls-tree");
            assert_eq!(
                stock_output, zmin_output,
                "quotePath={quote_path} args={spec:?}"
            );
            let nul = spec.iter().any(|arg| *arg == "-z");
            let object_only = spec.iter().any(|arg| *arg == "--object-only");
            let format_path = spec.iter().any(|arg| arg.starts_with("--format="));
            if nul && !object_only && !format_path {
                assert!(
                    zmin_output.1.windows(1).any(|window| window == b"\x80"),
                    "NUL-delimited output lost raw 0x80: quotePath={quote_path} args={spec:?} stock={:?} zmin={:?}",
                    stock_output.1,
                    zmin_output.1
                );
                assert!(
                    !zmin_output
                        .1
                        .windows(3)
                        .any(|window| window == b"\xef\xbf\xbd")
                );
                assert!(
                    zmin_output
                        .1
                        .windows(RAW_LS_TREE_PATH.len())
                        .any(|window| window == RAW_LS_TREE_PATH),
                    "NUL-delimited output changed raw path bytes: quotePath={quote_path} args={spec:?} output={:?}",
                    zmin_output.1
                );
            }
            if format_path {
                let suffix = if nul {
                    b"\"\0".as_slice()
                } else {
                    b"\"\n".as_slice()
                };
                assert!(
                    zmin_output.1.ends_with(suffix),
                    "format path must be C-quoted before its terminator: quotePath={quote_path} args={spec:?} output={:?}",
                    zmin_output.1
                );
                assert!(
                    zmin_output.1.windows(2).any(|window| window == b"\\\\"),
                    "format path must escape the backslash: quotePath={quote_path} args={spec:?} output={:?}",
                    zmin_output.1
                );
            }
            if full_path_mode {
                assert!(
                    zmin_output
                        .1
                        .windows(b"nested/".len())
                        .any(|window| window == b"nested/"),
                    "full path mode did not retain the repository-root prefix: quotePath={quote_path} args={spec:?} output={:?}",
                    zmin_output.1
                );
            }
        }
    }
}

#[test]
fn ls_tree_subdir_full_name_and_full_tree_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir/sub")).expect("create nested dirs");
    fs::write(repo.path().join("README.md"), b"root\n").expect("write root");
    fs::write(repo.path().join("dir/file.txt"), b"child\n").expect("write child");
    fs::write(repo.path().join("dir/sub/nested.txt"), b"nested\n").expect("write nested");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    let cwd = repo.path().join("dir");
    for args in [
        ["ls-tree", "HEAD"].as_slice(),
        ["ls-tree", "--full-name", "HEAD"].as_slice(),
        ["ls-tree", "--full-tree", "HEAD"].as_slice(),
        ["ls-tree", "-r", "HEAD"].as_slice(),
        ["ls-tree", "-r", "--full-name", "HEAD"].as_slice(),
        ["ls-tree", "-r", "--full-tree", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_stdout_bytes(zmin_bin(), &cwd, args),
            command_stdout_bytes("git", &cwd, args),
            "args: {args:?}"
        );
    }
}

#[test]
fn unpack_file_matches_stock_git_blob_behavior() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    let blob = git(repo.path(), ["hash-object", "-w", "a.txt"]);

    let zmin_path = run_zmin(repo.path(), ["unpack-file", &blob]);
    assert!(zmin_path.starts_with(".merge_file_"));
    assert_eq!(
        fs::read(repo.path().join(&zmin_path)).expect("read zmin unpacked file"),
        b"hello\n"
    );

    let git_path = git(repo.path(), ["unpack-file", &blob]);
    assert!(git_path.starts_with(".merge_file_"));
    assert_eq!(
        fs::read(repo.path().join(&git_path)).expect("read git unpacked file"),
        b"hello\n"
    );

    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let commit = git(repo.path(), ["rev-parse", "HEAD"]);
    assert_eq!(
        run_zmin_status(repo.path(), ["unpack-file", &commit]),
        git_status(repo.path(), ["unpack-file", &commit])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["unpack-file", "deadbeef"]),
        git_status(repo.path(), ["unpack-file", "deadbeef"])
    );
}

#[test]
fn show_index_matches_stock_git_for_pack_index_stdin() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["repack", "-adq"]);
    let idx = fs::read(first_pack_index(repo.path())).expect("read pack index");

    assert_eq!(
        run_zmin_with_stdin_bytes(repo.path(), ["show-index"], &idx),
        git_with_stdin_bytes(repo.path(), ["show-index"], &idx)
    );
}

#[test]
fn show_index_option_order_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["repack", "-adq"]);
    let idx = fs::read(first_pack_index(repo.path())).expect("read pack index");

    for args in [
        &[
            "show-index",
            "--object-format=sha256",
            "--object-format=sha1",
        ][..],
        &["show-index", "--object-format=bogus", "--no-object-format"][..],
        &["show-index", "--no-object-format", "--object-format=bogus"][..],
        &["show-index", "--object-format=sha256"][..],
        &[
            "show-index",
            "--object-format=sha1",
            "--object-format=sha256",
        ][..],
        &["show-index", "--no-object-format", "--object-format=sha256"][..],
        &[
            "show-index",
            "--object-format",
            "sha1",
            "--object-format=sha1",
        ][..],
        &[
            "show-index",
            "--object-format=sha1",
            "--object-format",
            "sha1",
        ][..],
        &[
            "show-index",
            "--no-object-format",
            "--object-format=sha1",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--object-format=sha1",
            "--no-object-format",
            "--object-format=sha1",
        ][..],
        &["show-index", "--object-format=sha256", "--no-object-format"][..],
        &[
            "show-index",
            "--no-object-format",
            "--object-format=bogus",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--object-format=bogus",
            "--no-object-format",
            "--object-format=sha1",
        ][..],
        &[
            "show-index",
            "--object-format",
            "sha1",
            "--object-format",
            "sha1",
        ][..],
        &[
            "show-index",
            "--object-format=sha1",
            "--object-format=sha1",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--no-object-format",
            "--object-format",
            "sha1",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--object-format",
            "sha1",
            "--no-object-format",
            "--object-format",
            "sha1",
        ][..],
        &[
            "show-index",
            "--object-format=sha256",
            "--object-format=sha256",
        ][..],
        &[
            "show-index",
            "--object-format=sha256",
            "--object-format=sha256",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--object-format=bogus",
            "--no-object-format",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--no-object-format",
            "--object-format=sha1",
            "--object-format=sha1",
        ][..],
        &[
            "show-index",
            "--object-format=sha1",
            "--no-object-format",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--no-object-format",
            "--object-format=sha256",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--object-format",
            "sha1",
            "--object-format",
            "sha1",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--object-format",
            "sha1",
            "--object-format",
            "sha1",
            "--no-object-format",
            "--object-format",
            "sha1",
        ][..],
        &[
            "show-index",
            "--no-object-format",
            "--object-format",
            "sha1",
            "--object-format",
            "sha1",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--object-format=sha1",
            "--object-format=sha1",
            "--object-format=sha1",
        ][..],
        &[
            "show-index",
            "--object-format=sha256",
            "--object-format=sha256",
            "--object-format=sha256",
        ][..],
        &[
            "show-index",
            "--object-format=sha256",
            "--object-format=sha256",
            "--no-object-format",
            "--object-format=sha1",
        ][..],
        &[
            "show-index",
            "--object-format=bogus",
            "--no-object-format",
            "--object-format=sha1",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--no-object-format",
            "--no-object-format",
            "--object-format=sha1",
        ][..],
        &[
            "show-index",
            "--no-object-format",
            "--no-object-format",
            "--object-format=sha256",
        ][..],
        &[
            "show-index",
            "--object-format=sha1",
            "--no-object-format",
            "--object-format=sha256",
            "--no-object-format",
        ][..],
    ] {
        assert_eq!(
            command_any_output_with_stdin_bytes(zmin_bin(), repo.path(), &args, &idx, "zmin"),
            command_any_output_with_stdin_bytes("git", repo.path(), &args, &idx, "git")
        );
    }
}

#[test]
fn show_index_rejects_unsupported_pack_index_version_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["repack", "-adq"]);
    let idx = rewrite_pack_index_version(&first_pack_index(repo.path()), 3);

    assert_eq!(
        command_any_output_with_stdin_bytes(zmin_bin(), repo.path(), &["show-index"], &idx, "zmin"),
        command_any_output_with_stdin_bytes("git", repo.path(), &["show-index"], &idx, "git")
    );
}

#[test]
fn update_server_info_matches_stock_git_for_bare_repo() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(dir.path(), ["init", "-b", "main", "source"]);
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"hello\n").expect("write fixture");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["branch", "feature"]);
    git(&source, ["tag", "lightweight"]);
    git_with_env(&source, ["tag", "-a", "annotated", "-m", "tag message"]);
    git(
        dir.path(),
        [
            "clone",
            "--bare",
            source.to_str().expect("source path"),
            "git.git",
        ],
    );
    git(
        dir.path(),
        [
            "clone",
            "--bare",
            source.to_str().expect("source path"),
            "zmin.git",
        ],
    );
    let git_repo = dir.path().join("git.git");
    let zmin_repo = dir.path().join("zmin.git");

    git(&git_repo, ["update-server-info"]);
    run_zmin(&zmin_repo, ["update-server-info"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.join("info/refs")).expect("read zmin info refs"),
        fs::read_to_string(git_repo.join("info/refs")).expect("read git info refs")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.join("objects/info/packs")).expect("read zmin packs info"),
        fs::read_to_string(git_repo.join("objects/info/packs")).expect("read git packs info")
    );

    git(&git_repo, ["repack", "-adq"]);
    git(&zmin_repo, ["repack", "-adq"]);
    git(&git_repo, ["update-server-info", "-f"]);
    run_zmin(&zmin_repo, ["update-server-info", "-f"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.join("info/refs")).expect("read packed zmin info refs"),
        fs::read_to_string(git_repo.join("info/refs")).expect("read packed git info refs")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.join("objects/info/packs"))
            .expect("read packed zmin packs info"),
        fs::read_to_string(git_repo.join("objects/info/packs"))
            .expect("read packed git packs info")
    );
}

#[test]
fn count_objects_matches_stock_git_for_loose_and_packed_objects() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    assert_eq!(
        run_zmin(repo.path(), ["count-objects"]),
        git(repo.path(), ["count-objects"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-H"]),
        git(repo.path(), ["count-objects", "-H"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-vH"]),
        git(repo.path(), ["count-objects", "-vH"])
    );

    git(repo.path(), ["repack", "-adq"]);
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-vH"]),
        git(repo.path(), ["count-objects", "-vH"])
    );
}

#[test]
fn count_objects_in_pack_counts_pack_index_entries_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"one\n").expect("write first");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    git(repo.path(), ["repack", "-adq"]);

    let saved_pack_dir = repo.path().join("saved-pack");
    fs::create_dir_all(&saved_pack_dir).expect("create saved pack dir");
    for entry in fs::read_dir(repo.path().join(".git/objects/pack")).expect("read pack dir") {
        let path = entry.expect("pack entry").path();
        if path.is_file() {
            fs::copy(
                &path,
                saved_pack_dir.join(path.file_name().expect("pack file name")),
            )
            .expect("save pack file");
        }
    }

    fs::write(repo.path().join("b.txt"), b"two\n").expect("write second");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    git(repo.path(), ["repack", "-adq"]);
    for entry in fs::read_dir(&saved_pack_dir).expect("read saved pack dir") {
        let path = entry.expect("saved pack entry").path();
        fs::copy(
            &path,
            repo.path()
                .join(".git/objects/pack")
                .join(path.file_name().expect("saved pack file name")),
        )
        .expect("restore saved pack file");
    }

    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
}

#[test]
fn count_objects_prune_packable_counts_loose_objects_once_with_duplicate_packs() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"one\n").expect("write first");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    let blob = git(repo.path(), ["rev-parse", "HEAD:a.txt"]);
    let loose_path = repo
        .path()
        .join(".git/objects")
        .join(&blob[..2])
        .join(&blob[2..]);
    let loose_copy = fs::read(&loose_path).expect("read loose blob");
    git(repo.path(), ["repack", "-adq"]);

    let saved_pack_dir = repo.path().join("saved-pack");
    fs::create_dir_all(&saved_pack_dir).expect("create saved pack dir");
    for entry in fs::read_dir(repo.path().join(".git/objects/pack")).expect("read pack dir") {
        let path = entry.expect("pack entry").path();
        if path.is_file() {
            fs::copy(
                &path,
                saved_pack_dir.join(path.file_name().expect("pack file name")),
            )
            .expect("save pack file");
        }
    }

    fs::write(repo.path().join("b.txt"), b"two\n").expect("write second");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    git(repo.path(), ["repack", "-adq"]);
    for entry in fs::read_dir(&saved_pack_dir).expect("read saved pack dir") {
        let path = entry.expect("saved pack entry").path();
        fs::copy(
            &path,
            repo.path()
                .join(".git/objects/pack")
                .join(path.file_name().expect("saved pack file name")),
        )
        .expect("restore saved pack file");
    }
    fs::create_dir_all(loose_path.parent().expect("loose parent")).expect("recreate loose parent");
    fs::write(&loose_path, loose_copy).expect("restore loose blob");

    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
}

#[test]
fn write_tree_and_commit_tree_match_stock_git_objects() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    fs::create_dir_all(repo.path().join("src")).expect("create src");
    fs::write(repo.path().join("src/lib.rs"), b"pub fn fixture() {}\n").expect("write source");
    git(repo.path(), ["add", "-A"]);

    let git_tree = git(repo.path(), ["write-tree"]);
    let zmin_tree = run_zmin(repo.path(), ["write-tree"]);
    assert_eq!(zmin_tree, git_tree);
    assert_eq!(
        run_zmin(repo.path(), ["write-tree", "--prefix=src"]),
        git(repo.path(), ["write-tree", "--prefix=src"])
    );

    let git_root = git_with_env(repo.path(), ["commit-tree", &git_tree, "-m", "root"]);
    let zmin_root = run_zmin_with_env(repo.path(), ["commit-tree", &git_tree, "-m", "root"]);
    assert_eq!(zmin_root, git_root);
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-p", &zmin_root]),
        git(repo.path(), ["cat-file", "-p", &git_root])
    );

    fs::write(repo.path().join("a.txt"), b"second\n").expect("modify fixture");
    git(repo.path(), ["add", "-A"]);
    let tree = git(repo.path(), ["write-tree"]);
    let git_child = git_with_env(
        repo.path(),
        ["commit-tree", &tree, "-p", &git_root, "-m", "child"],
    );
    let zmin_child = run_zmin_with_env(
        repo.path(),
        ["commit-tree", &tree, "-p", &zmin_root, "-m", "child"],
    );
    assert_eq!(zmin_child, git_child);

    let git_dedup = git_with_env(
        repo.path(),
        [
            "commit-tree",
            &tree,
            "-p",
            &git_root,
            "-p",
            &git_root,
            "-m",
            "dedup",
        ],
    );
    let zmin_dedup = run_zmin_with_env(
        repo.path(),
        [
            "commit-tree",
            &tree,
            "-p",
            &zmin_root,
            "-p",
            &zmin_root,
            "-m",
            "dedup",
        ],
    );
    assert_eq!(zmin_dedup, git_dedup);
}

#[test]
fn write_tree_reuses_index_cache_without_returning_missing_tree_object() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    fs::create_dir_all(repo.path().join("src")).expect("create src");
    fs::write(repo.path().join("src/lib.rs"), b"pub fn fixture() {}\n").expect("write source");
    git(repo.path(), ["add", "-A"]);

    let first_tree = run_zmin(repo.path(), ["write-tree"]);
    assert_eq!(first_tree, git(repo.path(), ["write-tree"]));

    let cache_path = repo.path().join(".git/zmin/write-tree-cache-v2");
    assert!(cache_path.is_file(), "write-tree cache file should exist");

    let loose_tree = loose_object_path(repo.path(), &first_tree);
    fs::remove_file(&loose_tree).expect("remove loose tree object");
    assert!(
        !loose_tree.exists(),
        "test setup should remove the original loose tree object"
    );

    let second_tree = run_zmin(repo.path(), ["write-tree"]);
    assert_eq!(second_tree, first_tree);
    assert!(
        loose_tree.exists(),
        "cached write-tree should rebuild a missing tree object instead of returning a stale id"
    );
}

#[test]
fn show_pretty_raw_for_commit_tree_records_tree_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("a.txt"), b"hello\n").expect("write fixture");
        git(repo, ["add", "-A"]);
    }
    let git_tree = git(git_repo.path(), ["write-tree"]);
    let zmin_tree = git(zmin_repo.path(), ["write-tree"]);
    assert_eq!(zmin_tree, git_tree);

    let git_commit = git_with_stdin(git_repo.path(), ["commit-tree", &git_tree], "NO\n");
    let zmin_commit = run_zmin_with_stdin(zmin_repo.path(), ["commit-tree", &zmin_tree], "NO\n");
    let git_raw = git(
        git_repo.path(),
        ["show", "--pretty=raw", "--no-patch", &git_commit],
    );
    let zmin_raw = run_zmin(
        zmin_repo.path(),
        ["show", "--pretty=raw", "--no-patch", &zmin_commit],
    );

    assert_eq!(
        zmin_raw
            .lines()
            .find(|line| line.starts_with("tree "))
            .map(str::to_owned),
        git_raw
            .lines()
            .find(|line| line.starts_with("tree "))
            .map(str::to_owned)
    );
    assert!(
        zmin_raw
            .lines()
            .take_while(|line| !line.starts_with("author "))
            .any(|line| line == format!("tree {zmin_tree}")),
        "show --pretty=raw should print tree before author: {zmin_raw}"
    );
}

#[test]
fn read_tree_matches_stock_git_for_tree_empty_and_prefix() {
    let git_repo = two_commit_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    let tree = git(git_repo.path(), ["rev-parse", "HEAD^{tree}"]);

    git(git_repo.path(), ["read-tree", &tree]);
    run_zmin(zmin_repo.path(), ["read-tree", &tree]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["write-tree"]),
        git(git_repo.path(), ["write-tree"])
    );

    git(git_repo.path(), ["read-tree", "--empty"]);
    run_zmin(zmin_repo.path(), ["read-tree", "--empty"]);
    git(git_repo.path(), ["read-tree", "-m", &tree]);
    run_zmin(zmin_repo.path(), ["read-tree", "-m", &tree]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );

    git(git_repo.path(), ["read-tree", "--empty"]);
    run_zmin(zmin_repo.path(), ["read-tree", "--empty"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );

    git(git_repo.path(), ["read-tree", "--prefix=import/", &tree]);
    run_zmin(zmin_repo.path(), ["read-tree", "--prefix=import/", &tree]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["write-tree"]),
        git(git_repo.path(), ["write-tree"])
    );
}

#[test]
fn read_and_write_tree_reject_null_sha_entries_like_stock_git() {
    let repo = git_init();
    let tree = git_with_stdin(
        repo.path(),
        ["mktree"],
        "160000 commit 0000000000000000000000000000000000000000\tbroken\n",
    );
    let args = ["read-tree", tree.trim()];
    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );

    let allow = [("GIT_ALLOW_NULL_SHA1", "1")];
    assert_eq!(
        command_output_with_env(zmin_bin(), repo.path(), &args, &allow, "zmin"),
        command_output_with_env("git", repo.path(), &args, &allow, "git")
    );
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["write-tree"]),
        command_failure_output_with_env("git", repo.path(), &["write-tree"], &[], "git")
    );
}

#[test]
fn read_tree_partial_clone_prefetches_missing_blobs_and_respects_no_lazy() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("one"), b"foo\n").expect("write one");
    fs::write(source.join("two"), b"bar\n").expect("write two");
    git(&source, ["add", "one", "two"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["config", "uploadpack.allowfilter", "true"]);
    git(&source, ["config", "uploadpack.allowanysha1inwant", "true"]);
    let tree = git(&source, ["rev-parse", "HEAD^{tree}"]);
    let one = git(&source, ["rev-parse", "HEAD:one"]);
    let two = git(&source, ["rev-parse", "HEAD:two"]);
    let client = dir.path().join("client.git");
    let source_url = format!("file://{}", source.display());
    run_zmin_args(
        dir.path(),
        &[
            "clone",
            "--bare",
            "--filter=blob:none",
            &source_url,
            client.to_str().expect("client path"),
        ],
    );

    for object_id in [&one, &two] {
        assert_ne!(
            run_zmin_status(&client, ["--no-lazy-fetch", "cat-file", "-e", object_id]),
            0,
            "partial clone unexpectedly contains {object_id}"
        );
    }

    let first_prefetch = raw_command_output(
        zmin_bin(),
        &client,
        &["read-tree", &tree, &tree],
        "zmin read-tree partial clone",
    );
    assert_eq!(first_prefetch.0, 0, "partial prefetch failed");
    for object_id in [&one, &two] {
        assert_eq!(
            run_zmin_status(&client, ["--no-lazy-fetch", "cat-file", "-e", object_id]),
            0,
            "read-tree did not prefetch {object_id}"
        );
    }

    let second_prefetch = raw_command_output(
        zmin_bin(),
        &client,
        &["read-tree", &tree, &tree],
        "zmin read-tree already hydrated partial clone",
    );
    assert_eq!(second_prefetch.0, 0, "already hydrated read-tree failed");
    for object_id in [&one, &two] {
        assert_eq!(
            run_zmin_status(&client, ["--no-lazy-fetch", "cat-file", "-e", object_id]),
            0,
            "repeat read-tree changed hydrated state for {object_id}"
        );
    }

    let no_lazy_client = dir.path().join("no-lazy-client.git");
    run_zmin_args(
        dir.path(),
        &[
            "clone",
            "--bare",
            "--filter=blob:none",
            &source_url,
            no_lazy_client.to_str().expect("no-lazy client path"),
        ],
    );
    for object_id in [&one, &two] {
        assert_ne!(
            run_zmin_status(
                &no_lazy_client,
                ["--no-lazy-fetch", "cat-file", "-e", object_id]
            ),
            0,
            "no-lazy setup unexpectedly contains {object_id}"
        );
    }
    let no_lazy_output = raw_command_output(
        zmin_bin(),
        &no_lazy_client,
        &["--no-lazy-fetch", "read-tree", &tree, &tree],
        "zmin no-lazy partial read-tree",
    );
    assert_eq!(no_lazy_output.0, 0, "no-lazy read-tree failed");
    for object_id in [&one, &two] {
        assert_ne!(
            run_zmin_status(
                &no_lazy_client,
                ["--no-lazy-fetch", "cat-file", "-e", object_id]
            ),
            0,
            "no-lazy read-tree materialized {object_id}"
        );
    }
}

#[test]
fn read_tree_partial_clone_dry_run_does_not_prefetch_missing_blobs() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let dir = TempDir::new().expect("partial dry-run temp dir");
        let source = dir.path().join("source");
        let mut init_args = vec!["init"];
        if sha256 {
            init_args.push("--object-format=sha256");
        }
        init_args.extend(["-b", "main", "source"]);
        let init = raw_command_output(
            stock_git.as_path(),
            dir.path(),
            &init_args,
            "pinned source init",
        );
        assert_eq!(init.0, 0, "sha256={sha256}");
        configure_identity(&source);
        fs::write(source.join("one"), b"one\n").expect("write partial one");
        fs::write(source.join("two"), b"two\n").expect("write partial two");
        git(&source, ["add", "one", "two"]);
        git_with_env(&source, ["commit", "-m", "partial source"]);
        git(&source, ["config", "uploadpack.allowfilter", "true"]);
        git(&source, ["config", "uploadpack.allowanysha1inwant", "true"]);
        let tree = git(&source, ["rev-parse", "HEAD^{tree}"]);
        let blobs = [
            git(&source, ["rev-parse", "HEAD:one"]),
            git(&source, ["rev-parse", "HEAD:two"]),
        ];
        let source_url = format!("file://{}", source.display());
        let stock_client = dir.path().join("stock-client");
        let zmin_client = dir.path().join("zmin-client");
        let stock_clone = raw_command_output(
            stock_git.as_path(),
            dir.path(),
            &[
                "clone",
                "--filter=blob:none",
                "--no-checkout",
                source_url.as_str(),
                stock_client.to_str().expect("stock client path utf8"),
            ],
            "pinned partial dry-run clone",
        );
        assert_eq!(stock_clone.0, 0, "sha256={sha256}");
        let zmin_setup = raw_command_output(
            stock_git.as_path(),
            dir.path(),
            &[
                "clone",
                "--filter=blob:none",
                "--no-checkout",
                source_url.as_str(),
                zmin_client.to_str().expect("zmin client path utf8"),
            ],
            "pinned partial dry-run zmin-side setup",
        );
        assert_eq!(zmin_setup.0, 0, "sha256={sha256}");

        let object_layout = |repo: &Path| {
            fn visit(root: &Path, path: &Path, files: &mut Vec<String>) {
                for entry in fs::read_dir(path).expect("read partial object layout") {
                    let entry = entry.expect("read partial object layout entry");
                    let entry_path = entry.path();
                    if entry_path.is_dir() {
                        visit(root, &entry_path, files);
                    } else {
                        files.push(
                            entry_path
                                .strip_prefix(root)
                                .expect("partial object layout relative path")
                                .to_string_lossy()
                                .into_owned(),
                        );
                    }
                }
            }
            let root = repo.join(".git/objects");
            let mut files = Vec::new();
            visit(&root, &root, &mut files);
            files.sort();
            files
        };
        for (program, repo, label) in [
            (
                stock_git.as_path(),
                stock_client.as_path(),
                "pinned partial dry-run setup",
            ),
            (
                Path::new(zmin_bin()),
                zmin_client.as_path(),
                "zmin partial dry-run setup",
            ),
        ] {
            for blob in &blobs {
                let output = raw_command_output(
                    program.to_str().expect("partial program path utf8"),
                    repo,
                    &["--no-lazy-fetch", "cat-file", "-e", blob.trim()],
                    label,
                );
                assert_ne!(output.0, 0, "dry-run setup hydrated {blob}");
            }
        }
        let stock_before = object_layout(&stock_client);
        let zmin_before = object_layout(&zmin_client);
        let stock_output = raw_command_output(
            stock_git.as_path(),
            &stock_client,
            &[
                "--no-lazy-fetch",
                "read-tree",
                "--dry-run",
                "-u",
                "-m",
                tree.trim(),
                tree.trim(),
            ],
            "pinned partial dry-run read-tree",
        );
        let zmin_output = raw_command_output(
            zmin_bin(),
            &zmin_client,
            &[
                "--no-lazy-fetch",
                "read-tree",
                "--dry-run",
                "-u",
                "-m",
                tree.trim(),
                tree.trim(),
            ],
            "zmin partial dry-run read-tree",
        );
        assert_eq!(zmin_output, stock_output, "sha256={sha256}");
        assert_ne!(
            stock_output.0, 0,
            "pinned partial dry-run unexpectedly succeeded"
        );
        assert_eq!(
            object_layout(&stock_client),
            stock_before,
            "stock dry-run changed objects"
        );
        assert_eq!(
            object_layout(&zmin_client),
            zmin_before,
            "zmin dry-run changed objects"
        );
        for (program, repo, label) in [
            (
                stock_git.as_path(),
                stock_client.as_path(),
                "pinned partial dry-run post-state",
            ),
            (
                Path::new(zmin_bin()),
                zmin_client.as_path(),
                "zmin partial dry-run post-state",
            ),
        ] {
            for blob in &blobs {
                let output = raw_command_output(
                    program.to_str().expect("partial program path utf8"),
                    repo,
                    &["--no-lazy-fetch", "cat-file", "-e", blob.trim()],
                    label,
                );
                assert_ne!(output.0, 0, "dry-run hydrated {blob}");
            }
        }
    }
}

#[test]
fn read_tree_recurse_submodules_optional_values_match_pinned_git() {
    let stock_git = required_pinned_stock_git();
    let source = two_commit_repo();
    let tree = git(source.path(), ["rev-parse", "HEAD^{tree}"]);
    for args in [
        ["read-tree", "--recurse-submodules=yes", tree.trim()].as_slice(),
        ["read-tree", "--recurse-submodules=true", tree.trim()].as_slice(),
        ["read-tree", "--recurse-submodules=on", tree.trim()].as_slice(),
        ["read-tree", "--recurse-submodules=1", tree.trim()].as_slice(),
        ["read-tree", "--recurse-submodules=2", tree.trim()].as_slice(),
        ["read-tree", "--recurse-submodules=", tree.trim()].as_slice(),
        ["read-tree", "--recurse-submodules=no", tree.trim()].as_slice(),
        ["read-tree", "--recurse-submodules=false", tree.trim()].as_slice(),
        ["read-tree", "--recurse-submodules=off", tree.trim()].as_slice(),
        ["read-tree", "--recurse-submodules=0", tree.trim()].as_slice(),
        [
            "read-tree",
            "--recurse-submodules",
            "--no-recurse-submodules",
            tree.trim(),
        ]
        .as_slice(),
        [
            "read-tree",
            "--no-recurse-submodules",
            "--recurse-submodules",
            tree.trim(),
        ]
        .as_slice(),
    ] {
        let zmin_repo = clone_repo_fixture(source.path());
        let stock_output = raw_command_output(
            stock_git.to_str().expect("pinned Git path utf8"),
            source.path(),
            args,
            "pinned read-tree recurse value",
        );
        let zmin_output = raw_command_output(
            zmin_bin(),
            zmin_repo.path(),
            args,
            "zmin read-tree recurse value",
        );
        assert_eq!(zmin_output, stock_output, "args: {args:?}");
    }

    let invalid_args = ["read-tree", "--recurse-submodules=invalid", tree.trim()];
    let stock_invalid = raw_command_output(
        stock_git.to_str().expect("pinned Git path utf8"),
        source.path(),
        &invalid_args,
        "pinned invalid read-tree recurse value",
    );
    let zmin_invalid_repo = clone_repo_fixture(source.path());
    let zmin_invalid = raw_command_output(
        zmin_bin(),
        zmin_invalid_repo.path(),
        &invalid_args,
        "zmin invalid read-tree recurse value",
    );
    assert_eq!(stock_invalid.0, 128);
    assert_eq!(zmin_invalid, stock_invalid);
}

#[test]
fn read_tree_recurses_into_populated_submodule_for_sha1_and_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let child_stock = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        let child_zmin = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        let super_stock = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        let super_zmin = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };

        for child in [child_stock.path(), child_zmin.path()] {
            configure_identity(child);
            fs::write(child.join("child.txt"), b"one\n").expect("write child file");
            git(child, ["add", "child.txt"]);
            git_with_env(child, ["commit", "-m", "one"]);
            fs::write(child.join("child.txt"), b"two\n").expect("write child update");
            git(child, ["add", "child.txt"]);
            git_with_env(child, ["commit", "-m", "two"]);
        }

        for (super_repo, child) in [
            (super_stock.path(), child_stock.path()),
            (super_zmin.path(), child_zmin.path()),
        ] {
            configure_identity(super_repo);
            let child_url = child.to_str().expect("child path utf8");
            fs::write(
                super_repo.join(".gitmodules"),
                format!("[submodule \"sub\"]\n\tpath = sub\n\turl = {child_url}\n"),
            )
            .expect("write gitmodules");
            git(super_repo, ["add", ".gitmodules"]);
            let first = git(child, ["rev-parse", "HEAD^"]).trim().to_owned();
            git(
                super_repo,
                [
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("160000,{first},sub"),
                ],
            );
            git_with_env(super_repo, ["commit", "-m", "submodule one"]);
            let second = git(child, ["rev-parse", "HEAD"]).trim().to_owned();
            git(
                super_repo,
                [
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("160000,{second},sub"),
                ],
            );
            git_with_env(super_repo, ["commit", "-m", "submodule two"]);
            git(super_repo, ["config", "submodule.sub.url", child_url]);
            fs::create_dir_all(super_repo.join(".git/modules"))
                .expect("create module git directory");
            git(
                super_repo,
                [
                    "clone",
                    "--separate-git-dir",
                    super_repo
                        .join(".git/modules/sub")
                        .to_str()
                        .expect("module git dir path utf8"),
                    child_url,
                    "sub",
                ],
            );
            git(&super_repo.join("sub"), ["checkout", "--detach", &first]);
            git(super_repo, ["read-tree", "HEAD^"]);
        }

        let stock_sub = super_stock.path().join("sub");
        let zmin_sub = super_zmin.path().join("sub");
        let roots = ReadTreeFixtureRoots::from_paths(
            sha256,
            &[
                super_stock.path(),
                super_zmin.path(),
                child_stock.path(),
                child_zmin.path(),
                &stock_sub,
                &zmin_sub,
            ],
            super_stock.path(),
            super_zmin.path(),
        );
        let args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD"];
        let stock_output = raw_command_output(
            stock_git.to_str().expect("pinned Git path utf8"),
            super_stock.path(),
            &args,
            "pinned recursive read-tree",
        );
        let zmin_output = raw_command_output(
            zmin_bin(),
            super_zmin.path(),
            &args,
            "zmin recursive read-tree",
        );
        assert_eq!(zmin_output, stock_output, "sha256={sha256}");
        let stage_shape = |text: &str| {
            text.lines()
                .map(|line| {
                    let mut fields = line.split_whitespace();
                    let mode = fields.next().expect("stage mode").to_owned();
                    let _object_id = fields.next().expect("stage object id");
                    let path = fields.next().expect("stage path").to_owned();
                    (mode, path)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            stage_shape(&run_zmin(super_zmin.path(), ["ls-files", "--stage"])),
            stage_shape(&git(super_stock.path(), ["ls-files", "--stage"])),
            "superproject index shape differs for sha256={sha256}"
        );
        let stock_gitlink = git(super_stock.path(), ["ls-files", "--stage", "sub"])
            .split_whitespace()
            .nth(1)
            .expect("stock gitlink")
            .to_owned();
        let zmin_gitlink = git(super_zmin.path(), ["ls-files", "--stage", "sub"])
            .split_whitespace()
            .nth(1)
            .expect("zmin gitlink")
            .to_owned();
        assert_eq!(
            git(&super_stock.path().join("sub"), ["rev-parse", "HEAD"]).trim(),
            stock_gitlink,
            "stock nested HEAD does not match its gitlink for sha256={sha256}"
        );
        assert_eq!(
            git(&super_zmin.path().join("sub"), ["rev-parse", "HEAD"]).trim(),
            zmin_gitlink,
            "zmin nested HEAD does not match its gitlink for sha256={sha256}"
        );
        assert_eq!(
            fs::read(super_zmin.path().join("sub/child.txt")).expect("read zmin child"),
            b"two\n"
        );
        assert_eq!(
            fs::read(super_stock.path().join("sub/child.txt")).expect("read stock child"),
            b"two\n"
        );

        let first = git(&super_stock.path().join("sub"), ["rev-parse", "HEAD^"]);
        let second = git(&super_stock.path().join("sub"), ["rev-parse", "HEAD"]);
        for (ordered_args, recurse_expected) in [
            (
                [
                    "read-tree",
                    "-u",
                    "-m",
                    "--recurse-submodules",
                    "--no-recurse-submodules",
                    "HEAD",
                ],
                false,
            ),
            (
                [
                    "read-tree",
                    "-u",
                    "-m",
                    "--no-recurse-submodules",
                    "--recurse-submodules",
                    "HEAD",
                ],
                true,
            ),
            (
                [
                    "read-tree",
                    "-u",
                    "-m",
                    "--recurse-submodules=2",
                    "--recurse-submodules=0",
                    "HEAD",
                ],
                false,
            ),
            (
                [
                    "read-tree",
                    "-u",
                    "-m",
                    "--recurse-submodules=0",
                    "--recurse-submodules=2",
                    "HEAD",
                ],
                true,
            ),
        ] {
            git(super_stock.path(), ["read-tree", "HEAD^"]);
            git(super_zmin.path(), ["read-tree", "HEAD^"]);
            git(
                &super_stock.path().join("sub"),
                ["checkout", "--detach", first.trim()],
            );
            git(
                &super_zmin.path().join("sub"),
                ["checkout", "--detach", first.trim()],
            );

            let stock_output = raw_command_output(
                stock_git.to_str().expect("pinned Git path utf8"),
                super_stock.path(),
                &ordered_args,
                "pinned ordered recursive read-tree",
            );
            let zmin_output = raw_command_output(
                zmin_bin(),
                super_zmin.path(),
                &ordered_args,
                "zmin ordered recursive read-tree",
            );
            assert_eq!(
                zmin_output, stock_output,
                "sha256={sha256}, args={ordered_args:?}"
            );

            let stock_nested_head = git(&super_stock.path().join("sub"), ["rev-parse", "HEAD"]);
            let zmin_nested_head = git(&super_zmin.path().join("sub"), ["rev-parse", "HEAD"]);
            assert_eq!(zmin_nested_head, stock_nested_head);
            if recurse_expected {
                assert_eq!(stock_nested_head.trim(), second.trim());
            } else {
                assert_eq!(stock_nested_head.trim(), first.trim());
            }
        }

        for repo in [super_stock.path(), super_zmin.path()] {
            git(repo, ["read-tree", "HEAD^"]);
            git(&repo.join("sub"), ["checkout", "--detach", first.trim()]);
        }
        let stock_empty_recurse_before = snapshot_read_tree_nested(super_stock.path());
        let zmin_empty_recurse_before = snapshot_read_tree_nested(super_zmin.path());
        let empty_recurse_args = ["read-tree", "-u", "-m", "--recurse-submodules=", "HEAD"];
        let stock_empty_recurse = raw_command_output(
            stock_git.as_path(),
            super_stock.path(),
            &empty_recurse_args,
            "pinned empty recurse value on gitlink",
        );
        let zmin_empty_recurse = raw_command_output(
            zmin_bin(),
            super_zmin.path(),
            &empty_recurse_args,
            "zmin empty recurse value on gitlink",
        );
        assert_eq!(zmin_empty_recurse, stock_empty_recurse, "sha256={sha256}");
        assert_eq!(stock_empty_recurse.0, 0, "sha256={sha256}");
        assert_eq!(
            git(&super_stock.path().join("sub"), ["rev-parse", "HEAD"]).trim(),
            first.trim(),
            "pinned empty recurse value unexpectedly updated child, sha256={sha256}"
        );
        assert_eq!(
            git(&super_zmin.path().join("sub"), ["rev-parse", "HEAD"]).trim(),
            first.trim(),
            "zmin empty recurse value unexpectedly updated child, sha256={sha256}"
        );
        for repo in [super_stock.path(), super_zmin.path()] {
            let target = git(repo, ["rev-parse", "HEAD:sub"]);
            assert!(
                git(repo, ["ls-files", "--stage", "sub"]).contains(target.trim()),
                "empty recurse did not advance parent gitlink, sha256={sha256}"
            );
        }
        let stock_empty_recurse_after = snapshot_read_tree_nested(super_stock.path());
        let zmin_empty_recurse_after = snapshot_read_tree_nested(super_zmin.path());
        assert_ne!(
            stock_empty_recurse_after.root_index, stock_empty_recurse_before.root_index,
            "empty recurse did not update pinned parent index, sha256={sha256}"
        );
        assert_ne!(
            zmin_empty_recurse_after.root_index, zmin_empty_recurse_before.root_index,
            "empty recurse did not update zmin parent index, sha256={sha256}"
        );
        assert_read_tree_module_snapshot_unchanged(
            &stock_empty_recurse_after.child,
            &stock_empty_recurse_before.child,
            "pinned empty recurse",
            "child",
        );
        assert_read_tree_module_snapshot_unchanged(
            &zmin_empty_recurse_after.child,
            &zmin_empty_recurse_before.child,
            "zmin empty recurse",
            "child",
        );
        assert_read_tree_nested_semantics_equal(
            super_stock.path(),
            super_zmin.path(),
            &roots,
            "empty recurse cross-side state",
        );

        for repo in [super_stock.path(), super_zmin.path()] {
            git(repo, ["read-tree", "HEAD^"]);
            git(&repo.join("sub"), ["checkout", "--detach", first.trim()]);
            let mut config = fs::OpenOptions::new()
                .append(true)
                .open(repo.join(".git/config"))
                .expect("open repository config for implicit booleans");
            writeln!(config, "\n[submodule \"sub\"]\n\tactive")
                .expect("write implicit active config");
            writeln!(config, "\n[submodule]\n\trecurse").expect("write implicit recurse config");
        }
        let implicit_args = ["read-tree", "-u", "-m", "HEAD"];
        let stock_implicit = raw_command_output(
            stock_git.to_str().expect("pinned Git path utf8"),
            super_stock.path(),
            &implicit_args,
            "pinned implicit submodule booleans",
        );
        let zmin_implicit = raw_command_output(
            zmin_bin(),
            super_zmin.path(),
            &implicit_args,
            "zmin implicit submodule booleans",
        );
        assert_eq!(zmin_implicit, stock_implicit, "sha256={sha256}");
        assert_eq!(stock_implicit.0, 0, "sha256={sha256}");
        assert_eq!(
            git(&super_stock.path().join("sub"), ["rev-parse", "HEAD"]).trim(),
            second.trim(),
            "stock implicit recursion did not update child for sha256={sha256}"
        );
        assert_eq!(
            git(&super_zmin.path().join("sub"), ["rev-parse", "HEAD"]).trim(),
            second.trim(),
            "zmin implicit recursion did not update child for sha256={sha256}"
        );

        for repo in [super_stock.path(), super_zmin.path()] {
            git(repo, ["read-tree", "HEAD^"]);
            git(&repo.join("sub"), ["checkout", "--detach", first.trim()]);
            git(repo, ["config", "submodule.sub.active", "not-a-bool"]);
        }
        let invalid_active_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD"];
        let stock_invalid_active = raw_command_output(
            stock_git.to_str().expect("pinned Git path utf8"),
            super_stock.path(),
            &invalid_active_args,
            "pinned invalid submodule.active",
        );
        let zmin_invalid_active = raw_command_output(
            zmin_bin(),
            super_zmin.path(),
            &invalid_active_args,
            "zmin invalid submodule.active",
        );
        assert_eq!(zmin_invalid_active, stock_invalid_active, "sha256={sha256}");
        assert_eq!(stock_invalid_active.0, 128, "sha256={sha256}");

        for active_value in [
            "",
            ":(not-a-magic)sub",
            ":(unterminated",
            ":(attr:vendored)sub",
        ] {
            for repo in [super_stock.path(), super_zmin.path()] {
                let _ = command_any_output(
                    stock_git.to_str().expect("pinned Git path utf8"),
                    repo,
                    &["config", "--unset-all", "submodule.sub.active"],
                    "clear per-module active config",
                );
                let _ = command_any_output(
                    stock_git.to_str().expect("pinned Git path utf8"),
                    repo,
                    &["config", "--unset-all", "submodule.active"],
                    "clear active config",
                );
                git(repo, ["config", "submodule.active", active_value]);
                git(repo, ["read-tree", "HEAD^"]);
                git(&repo.join("sub"), ["checkout", "--detach", first.trim()]);
            }
            let active_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD"];
            let stock_active = raw_command_output(
                stock_git.to_str().expect("pinned Git path utf8"),
                super_stock.path(),
                &active_args,
                "pinned invalid submodule.active pathspec",
            );
            let zmin_active = raw_command_output(
                zmin_bin(),
                super_zmin.path(),
                &active_args,
                "zmin invalid submodule.active pathspec",
            );
            assert_eq!(
                zmin_active, stock_active,
                "sha256={sha256}, value={active_value:?}"
            );
            if active_value != ":(attr:vendored)sub" {
                assert_eq!(
                    stock_active.0, 128,
                    "sha256={sha256}, value={active_value:?}"
                );
            }
        }
    }
}

struct ReadTreeRemovalFixture {
    stock_super: TempDir,
    zmin_super: TempDir,
    _stock_child: TempDir,
    _zmin_child: TempDir,
    roots: ReadTreeFixtureRoots,
}

#[derive(Debug, Clone)]
struct ReadTreeFixtureRoots {
    exact_roots: Vec<PathBuf>,
    object_hex_len: usize,
    stock_root_admin_before: Option<ReadTreeAdminLayoutSnapshot>,
    zmin_root_admin_before: Option<ReadTreeAdminLayoutSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ReadTreeAdminRoleCounts {
    total: usize,
    loose: usize,
    pack_data: usize,
    pack_index: usize,
    pack_rev: usize,
    pack_bitmap: usize,
    pack_mtimes: usize,
    pack_keep: usize,
    pack_promisor: usize,
    other_aux: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ReadTreeAdminLayoutSnapshot {
    roles: BTreeSet<String>,
    counts: ReadTreeAdminRoleCounts,
    non_object_contents: BTreeMap<String, Vec<u8>>,
}

impl ReadTreeFixtureRoots {
    fn from_paths(sha256: bool, roots: &[&Path], stock_super: &Path, zmin_super: &Path) -> Self {
        let object_hex_len = read_tree_object_hex_len(sha256);
        let mut exact_roots = Vec::with_capacity(roots.len() * 2);
        for root in roots {
            let root = (*root).to_path_buf();
            let mut candidates = vec![root.clone()];
            if let Ok(canonical_root) = fs::canonicalize(&root) {
                candidates.push(canonical_root);
            }
            if cfg!(target_os = "macos") {
                let mut aliases = Vec::new();
                for candidate in &candidates {
                    if let Ok(without_private) = candidate.strip_prefix("/private") {
                        aliases.push(PathBuf::from("/").join(without_private));
                    } else if candidate.is_absolute() {
                        aliases.push(
                            PathBuf::from("/private").join(
                                candidate
                                    .strip_prefix("/")
                                    .expect("absolute fixture root without root prefix"),
                            ),
                        );
                    }
                }
                candidates.extend(aliases);
            }
            for candidate in candidates {
                if !exact_roots.contains(&candidate) {
                    exact_roots.push(candidate);
                }
            }
        }
        let temp_roots = exact_roots.iter().map(PathBuf::as_path).collect::<Vec<_>>();
        let stock_root_admin_before =
            read_tree_admin_layout_snapshot(stock_super, "", object_hex_len, &temp_roots);
        let zmin_root_admin_before =
            read_tree_admin_layout_snapshot(zmin_super, "", object_hex_len, &temp_roots);
        Self {
            exact_roots,
            object_hex_len,
            stock_root_admin_before,
            zmin_root_admin_before,
        }
    }

    fn temp_root_refs(&self) -> Vec<&Path> {
        self.exact_roots.iter().map(PathBuf::as_path).collect()
    }
}

fn read_tree_object_hex_len(sha256: bool) -> usize {
    if sha256 { 64 } else { 40 }
}

fn read_tree_removal_fixture(sha256: bool) -> ReadTreeRemovalFixture {
    let stock_child = if sha256 {
        pinned_git_init_sha256()
    } else {
        git_init()
    };
    let zmin_child = if sha256 {
        pinned_git_init_sha256()
    } else {
        git_init()
    };
    let stock_super = if sha256 {
        pinned_git_init_sha256()
    } else {
        git_init()
    };
    let zmin_super = if sha256 {
        pinned_git_init_sha256()
    } else {
        git_init()
    };

    for child in [stock_child.path(), zmin_child.path()] {
        configure_identity(child);
        fs::write(child.join("child.txt"), b"one\n").expect("write child file");
        git(child, ["add", "child.txt"]);
        git_with_env(child, ["commit", "-m", "one"]);
        fs::write(child.join("child.txt"), b"two\n").expect("write child update");
        fs::write(child.join("new.txt"), b"new\n").expect("write child addition");
        git(child, ["add", "child.txt"]);
        git(child, ["add", "new.txt"]);
        git_with_env(child, ["commit", "-m", "two"]);
    }

    for (super_repo, child) in [
        (stock_super.path(), stock_child.path()),
        (zmin_super.path(), zmin_child.path()),
    ] {
        configure_identity(super_repo);
        let child_url = child.to_str().expect("child path utf8");
        let committed_child_url = "file:///read-tree-child";
        fs::write(
            super_repo.join(".gitmodules"),
            format!("[submodule \"sub\"]\n\tpath = sub\n\turl = {committed_child_url}\n"),
        )
        .expect("write gitmodules");
        git(super_repo, ["add", ".gitmodules"]);
        let first = git(child, ["rev-parse", "HEAD^"]).trim().to_owned();
        git(
            super_repo,
            [
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{first},sub"),
            ],
        );
        git_with_env(super_repo, ["commit", "-m", "submodule one"]);
        let second = git(child, ["rev-parse", "HEAD"]).trim().to_owned();
        git(
            super_repo,
            [
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{second},sub"),
            ],
        );
        git_with_env(super_repo, ["commit", "-m", "submodule two"]);
        git(super_repo, ["rm", "--cached", "-f", "sub"]);
        git_with_env(super_repo, ["commit", "-m", "remove submodule"]);
        git(super_repo, ["config", "submodule.sub.url", child_url]);
        fs::create_dir_all(super_repo.join(".git/modules")).expect("create module git directory");
        git(
            super_repo,
            [
                "clone",
                "--separate-git-dir",
                super_repo
                    .join(".git/modules/sub")
                    .to_str()
                    .expect("module git dir path utf8"),
                child_url,
                "sub",
            ],
        );
        git(&super_repo.join("sub"), ["checkout", "--detach", &first]);
        git(super_repo, ["read-tree", "HEAD^^"]);
    }

    let stock_sub = stock_super.path().join("sub");
    let zmin_sub = zmin_super.path().join("sub");
    let roots = ReadTreeFixtureRoots::from_paths(
        sha256,
        &[
            stock_super.path(),
            zmin_super.path(),
            stock_child.path(),
            zmin_child.path(),
            &stock_sub,
            &zmin_sub,
        ],
        stock_super.path(),
        zmin_super.path(),
    );
    ReadTreeRemovalFixture {
        stock_super,
        zmin_super,
        _stock_child: stock_child,
        _zmin_child: zmin_child,
        roots,
    }
}

struct ReadTreeNestedFixture {
    stock_super: TempDir,
    zmin_super: TempDir,
    _stock_middle: TempDir,
    _zmin_middle: TempDir,
    _stock_inner: TempDir,
    _zmin_inner: TempDir,
    roots: ReadTreeFixtureRoots,
}

fn create_nested_read_tree_side(sha256: bool) -> (TempDir, TempDir, TempDir) {
    let inner = if sha256 {
        pinned_git_init_sha256()
    } else {
        git_init()
    };
    let middle = if sha256 {
        pinned_git_init_sha256()
    } else {
        git_init()
    };
    let super_repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        git_init()
    };

    configure_identity(inner.path());
    fs::write(inner.path().join("inner.txt"), b"one\n").expect("write nested inner file");
    git(inner.path(), ["add", "inner.txt"]);
    git_with_env(inner.path(), ["commit", "-m", "inner one"]);
    let inner_one = git(inner.path(), ["rev-parse", "HEAD"]).trim().to_owned();
    fs::write(inner.path().join("inner.txt"), b"two\n").expect("write nested inner update");
    git(inner.path(), ["add", "inner.txt"]);
    git_with_env(inner.path(), ["commit", "-m", "inner two"]);
    let inner_two = git(inner.path(), ["rev-parse", "HEAD"]).trim().to_owned();

    configure_identity(middle.path());
    let committed_inner_url = "file:///read-tree-inner";
    fs::write(
        middle.path().join(".gitmodules"),
        format!("[submodule \"nested\"]\n\tpath = nested\n\turl = {committed_inner_url}\n"),
    )
    .expect("write middle gitmodules");
    git(middle.path(), ["add", ".gitmodules"]);
    git(
        middle.path(),
        [
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{inner_one},nested"),
        ],
    );
    git_with_env(middle.path(), ["commit", "-m", "middle one"]);
    let middle_one = git(middle.path(), ["rev-parse", "HEAD"]).trim().to_owned();
    git(
        middle.path(),
        [
            "update-index",
            "--cacheinfo",
            &format!("160000,{inner_two},nested"),
        ],
    );
    git_with_env(middle.path(), ["commit", "-m", "middle two"]);
    let middle_two = git(middle.path(), ["rev-parse", "HEAD"]).trim().to_owned();

    configure_identity(super_repo.path());
    let committed_middle_url = "file:///read-tree-middle";
    fs::write(
        super_repo.path().join(".gitmodules"),
        format!("[submodule \"child\"]\n\tpath = child\n\turl = {committed_middle_url}\n"),
    )
    .expect("write super gitmodules");
    git(super_repo.path(), ["add", ".gitmodules"]);
    git(
        super_repo.path(),
        [
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{middle_one},child"),
        ],
    );
    git_with_env(super_repo.path(), ["commit", "-m", "super one"]);
    git(
        super_repo.path(),
        [
            "update-index",
            "--cacheinfo",
            &format!("160000,{middle_two},child"),
        ],
    );
    git_with_env(super_repo.path(), ["commit", "-m", "super two"]);
    git(super_repo.path(), ["rm", "--cached", "-f", "child"]);
    git_with_env(super_repo.path(), ["commit", "-m", "remove child"]);

    for repo in [inner.path(), middle.path(), super_repo.path()] {
        git(repo, ["config", "protocol.file.allow", "always"]);
    }
    (inner, middle, super_repo)
}

fn prepare_nested_read_tree_side(inner: &Path, middle: &Path, super_repo: &Path) {
    let inner_one = git(inner, ["rev-parse", "HEAD^"]).trim().to_owned();
    let middle_one = git(middle, ["rev-parse", "HEAD^"]).trim().to_owned();
    git(
        super_repo,
        ["config", "submodule.child.url", middle.to_str().unwrap()],
    );
    fs::create_dir_all(super_repo.join(".git/modules")).expect("create child admin directory");
    git(
        super_repo,
        [
            "-c",
            "protocol.file.allow=always",
            "clone",
            "--separate-git-dir",
            super_repo
                .join(".git/modules/child")
                .to_str()
                .expect("child admin path utf8"),
            middle.to_str().expect("middle source path utf8"),
            "child",
        ],
    );
    let child = super_repo.join("child");
    git(
        &child,
        ["config", "submodule.nested.url", inner.to_str().unwrap()],
    );
    let child_git_dir = PathBuf::from(git(&child, ["rev-parse", "--git-dir"]).trim());
    fs::create_dir_all(child_git_dir.join("modules")).expect("create nested admin directory");
    git(
        &child,
        [
            "-c",
            "protocol.file.allow=always",
            "clone",
            "--separate-git-dir",
            child_git_dir
                .join("modules/nested")
                .to_str()
                .expect("nested admin path utf8"),
            inner.to_str().expect("inner source path utf8"),
            "nested",
        ],
    );
    git(&child.join("nested"), ["checkout", "--detach", &inner_one]);
    git(&child, ["checkout", "--detach", &middle_one]);
    git(super_repo, ["read-tree", "--reset", "HEAD^^"]);
}

fn nested_read_tree_fixture(sha256: bool) -> ReadTreeNestedFixture {
    let (stock_inner, stock_middle, stock_super) = create_nested_read_tree_side(sha256);
    let (zmin_inner, zmin_middle, zmin_super) = create_nested_read_tree_side(sha256);
    prepare_nested_read_tree_side(stock_inner.path(), stock_middle.path(), stock_super.path());
    prepare_nested_read_tree_side(zmin_inner.path(), zmin_middle.path(), zmin_super.path());
    let stock_child = stock_super.path().join("child");
    let zmin_child = zmin_super.path().join("child");
    let stock_nested = stock_super.path().join("child/nested");
    let zmin_nested = zmin_super.path().join("child/nested");
    let roots = ReadTreeFixtureRoots::from_paths(
        sha256,
        &[
            stock_super.path(),
            zmin_super.path(),
            stock_middle.path(),
            zmin_middle.path(),
            stock_inner.path(),
            zmin_inner.path(),
            &stock_child,
            &zmin_child,
            &stock_nested,
            &zmin_nested,
        ],
        stock_super.path(),
        zmin_super.path(),
    );
    ReadTreeNestedFixture {
        stock_super,
        zmin_super,
        _stock_middle: stock_middle,
        _zmin_middle: zmin_middle,
        _stock_inner: stock_inner,
        _zmin_inner: zmin_inner,
        roots,
    }
}

#[test]
fn read_tree_nested_gitlink_preflight_and_removal_match_pinned_git_for_sha1_and_sha256() {
    let stock_git = required_pinned_stock_git();
    assert_read_tree_removal_validation_is_iterative();
    for sha256 in [false, true] {
        let update = nested_read_tree_fixture(sha256);
        let update_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD^"];
        let stock_update = raw_command_output(
            stock_git.as_path(),
            update.stock_super.path(),
            &update_args,
            "pinned nested update",
        );
        let zmin_update = raw_command_output(
            zmin_bin(),
            update.zmin_super.path(),
            &update_args,
            "zmin nested update",
        );
        assert_eq!(zmin_update, stock_update, "sha256={sha256}");
        assert_eq!(stock_update.0, 0, "sha256={sha256}");
        for (repo, middle, inner) in [
            (
                update.stock_super.path(),
                update._stock_middle.path(),
                update._stock_inner.path(),
            ),
            (
                update.zmin_super.path(),
                update._zmin_middle.path(),
                update._zmin_inner.path(),
            ),
        ] {
            let expected_middle = git(middle, ["rev-parse", "HEAD"]);
            let expected_inner = git(inner, ["rev-parse", "HEAD"]);
            assert_eq!(
                git(&repo.join("child"), ["rev-parse", "HEAD"]).trim(),
                expected_middle.trim(),
                "middle submodule did not reach target"
            );
            assert_eq!(
                git(&repo.join("child/nested"), ["rev-parse", "HEAD"],).trim(),
                expected_inner.trim(),
                "inner submodule did not reach target"
            );
            assert!(
                git(repo, ["ls-files", "--stage", "child"]).contains(expected_middle.trim()),
                "root index did not retain the expected middle gitlink"
            );
            assert!(
                git(&repo.join("child"), ["ls-files", "--stage", "nested"])
                    .contains(expected_inner.trim()),
                "middle index did not retain the expected inner gitlink"
            );
            assert_eq!(
                fs::read(repo.join("child/nested/inner.txt")).expect("read nested updated file"),
                b"two\n"
            );
        }

        let nested_absent = nested_read_tree_fixture(sha256);
        for repo in [
            nested_absent.stock_super.path(),
            nested_absent.zmin_super.path(),
        ] {
            fs::remove_dir_all(repo.join("child/nested"))
                .expect("remove unchanged nested worktree");
            fs::remove_dir_all(repo.join(".git/modules/child/modules/nested"))
                .expect("remove unchanged nested admin");
        }
        let stock_unchanged_before = git(
            nested_absent.stock_super.path(),
            ["ls-files", "--stage", "child"],
        );
        let zmin_unchanged_before = git(
            nested_absent.zmin_super.path(),
            ["ls-files", "--stage", "child"],
        );
        let unchanged_nested_args = [
            "read-tree",
            "-u",
            "-m",
            "--recurse-submodules",
            "HEAD^^",
            "HEAD^^",
        ];
        let stock_unchanged_nested = raw_command_output(
            stock_git.as_path(),
            nested_absent.stock_super.path(),
            &unchanged_nested_args,
            "pinned unchanged nested no-admin read-tree",
        );
        let zmin_unchanged_nested = raw_command_output(
            zmin_bin(),
            nested_absent.zmin_super.path(),
            &unchanged_nested_args,
            "zmin unchanged nested no-admin read-tree",
        );
        assert_eq!(
            zmin_unchanged_nested, stock_unchanged_nested,
            "sha256={sha256}"
        );
        assert_eq!(
            stock_unchanged_nested.0, 0,
            "sha256={sha256}, stock={stock_unchanged_nested:?}, zmin={zmin_unchanged_nested:?}"
        );
        let stock_unchanged_path = nested_absent
            .stock_super
            .path()
            .join("child/nested")
            .exists();
        let zmin_unchanged_path = nested_absent
            .zmin_super
            .path()
            .join("child/nested")
            .exists();
        let stock_unchanged_admin = nested_absent
            .stock_super
            .path()
            .join(".git/modules/child/modules/nested")
            .exists();
        let zmin_unchanged_admin = nested_absent
            .zmin_super
            .path()
            .join(".git/modules/child/modules/nested")
            .exists();
        assert_eq!(zmin_unchanged_path, stock_unchanged_path, "sha256={sha256}");
        assert_eq!(
            zmin_unchanged_admin, stock_unchanged_admin,
            "sha256={sha256}"
        );
        assert!(
            !stock_unchanged_path,
            "pinned unchanged nested worktree was recreated"
        );
        assert!(
            !stock_unchanged_admin,
            "pinned unchanged nested admin was recreated"
        );
        assert_eq!(
            git(
                nested_absent.stock_super.path(),
                ["ls-files", "--stage", "child"]
            ),
            stock_unchanged_before,
            "stock unchanged nested state changed, sha256={sha256}"
        );
        assert_eq!(
            git(
                nested_absent.zmin_super.path(),
                ["ls-files", "--stage", "child"]
            ),
            zmin_unchanged_before,
            "zmin unchanged nested state changed, sha256={sha256}"
        );

        let absent_transition = nested_read_tree_fixture(sha256);
        for repo in [
            absent_transition.stock_super.path(),
            absent_transition.zmin_super.path(),
        ] {
            fs::remove_dir_all(repo.join("child/nested"))
                .expect("remove transition nested worktree");
            fs::remove_dir_all(repo.join(".git/modules/child/modules/nested"))
                .expect("remove transition nested admin");
        }
        let stock_transition_before = git(
            absent_transition.stock_super.path(),
            ["ls-files", "--stage", "child"],
        );
        let zmin_transition_before = git(
            absent_transition.zmin_super.path(),
            ["ls-files", "--stage", "child"],
        );
        let stock_transition_child_before = git(
            &absent_transition.stock_super.path().join("child"),
            ["rev-parse", "HEAD"],
        );
        let zmin_transition_child_before = git(
            &absent_transition.zmin_super.path().join("child"),
            ["rev-parse", "HEAD"],
        );
        let stock_absent_transition_before =
            snapshot_read_tree_nested(absent_transition.stock_super.path());
        let zmin_absent_transition_before =
            snapshot_read_tree_nested(absent_transition.zmin_super.path());
        let absent_transition_args = [
            "read-tree",
            "-u",
            "-m",
            "--recurse-submodules",
            "HEAD^^",
            "HEAD^",
        ];
        let stock_absent_transition = raw_command_output(
            stock_git.as_path(),
            absent_transition.stock_super.path(),
            &absent_transition_args,
            "pinned absent nested transition",
        );
        let zmin_absent_transition = raw_command_output(
            zmin_bin(),
            absent_transition.zmin_super.path(),
            &absent_transition_args,
            "zmin absent nested transition",
        );
        assert_eq!(
            zmin_absent_transition, stock_absent_transition,
            "sha256={sha256}"
        );
        assert_eq!(stock_absent_transition.0, 128, "sha256={sha256}");
        let stock_absent_transition_path = absent_transition
            .stock_super
            .path()
            .join("child/nested")
            .exists();
        let zmin_absent_transition_path = absent_transition
            .zmin_super
            .path()
            .join("child/nested")
            .exists();
        let stock_absent_transition_admin = absent_transition
            .stock_super
            .path()
            .join(".git/modules/child/modules/nested")
            .exists();
        let zmin_absent_transition_admin = absent_transition
            .zmin_super
            .path()
            .join(".git/modules/child/modules/nested")
            .exists();
        assert!(
            stock_absent_transition_path || stock_absent_transition_admin,
            "pinned failure did not leave a nested checkout attempt, sha256={sha256}"
        );
        assert_eq!(
            zmin_absent_transition_path, stock_absent_transition_path,
            "zmin nested transition path state diverged, sha256={sha256}"
        );
        assert_eq!(
            zmin_absent_transition_admin, stock_absent_transition_admin,
            "zmin nested transition admin state diverged, sha256={sha256}"
        );
        assert!(stock_absent_transition_path, "sha256={sha256}");
        assert!(stock_absent_transition_admin, "sha256={sha256}");
        assert_read_tree_connected_worktree_state(
            absent_transition.stock_super.path(),
            "child/nested",
            "child/modules/nested",
        );
        assert_read_tree_connected_worktree_state(
            absent_transition.zmin_super.path(),
            "child/nested",
            "child/modules/nested",
        );
        assert_read_tree_failure_admin_shape(
            absent_transition.stock_super.path(),
            "child/nested",
            "child/modules/nested",
            true,
        );
        assert_read_tree_failure_admin_shape(
            absent_transition.zmin_super.path(),
            "child/nested",
            "child/modules/nested",
            true,
        );
        let stock_absent_transition_after =
            snapshot_read_tree_nested(absent_transition.stock_super.path());
        let zmin_absent_transition_after =
            snapshot_read_tree_nested(absent_transition.zmin_super.path());
        assert_eq!(
            stock_absent_transition_after.root_index, stock_absent_transition_before.root_index,
            "pinned failed transition preserved the parent index, sha256={sha256}"
        );
        assert_eq!(
            zmin_absent_transition_after.root_index, zmin_absent_transition_before.root_index,
            "Zmin failed transition changed the parent index, sha256={sha256}"
        );
        assert_eq!(
            read_tree_worktree_files_without_marker(&stock_absent_transition_after.child),
            read_tree_worktree_files_without_marker(&zmin_absent_transition_after.child)
        );
        assert_eq!(
            read_tree_worktree_files_without_marker(&stock_absent_transition_after.nested),
            read_tree_worktree_files_without_marker(&zmin_absent_transition_after.nested)
        );
        let stock_absent_transition_index = git(
            absent_transition.stock_super.path(),
            ["ls-files", "--stage", "child"],
        );
        let zmin_absent_transition_index = git(
            absent_transition.zmin_super.path(),
            ["ls-files", "--stage", "child"],
        );
        assert_eq!(stock_transition_before, stock_absent_transition_index);
        assert_eq!(zmin_transition_before, zmin_absent_transition_index);
        assert_eq!(
            git(
                &absent_transition.stock_super.path().join("child"),
                ["rev-parse", "HEAD"],
            )
            .trim(),
            stock_transition_child_before.trim()
        );
        assert_eq!(
            git(
                &absent_transition.zmin_super.path().join("child"),
                ["rev-parse", "HEAD"],
            ),
            zmin_transition_child_before
        );
        assert_eq!(
            stock_transition_before, stock_absent_transition_index,
            "stock parent index changed on failed nested transition, sha256={sha256}"
        );
        assert!(zmin_transition_before.contains(zmin_transition_child_before.trim()));

        let admin_only = nested_read_tree_fixture(sha256);
        for repo in [admin_only.stock_super.path(), admin_only.zmin_super.path()] {
            fs::remove_dir_all(repo.join("child/nested"))
                .expect("remove admin-only nested worktree");
            assert!(repo.join(".git/modules/child/modules/nested").is_dir());
        }
        let stock_admin_parent_before = git(
            admin_only.stock_super.path(),
            ["ls-files", "--stage", "child"],
        );
        let zmin_admin_parent_before = git(
            admin_only.zmin_super.path(),
            ["ls-files", "--stage", "child"],
        );
        let stock_admin_config_before = fs::read(
            admin_only
                .stock_super
                .path()
                .join(".git/modules/child/modules/nested/config"),
        )
        .expect("read stock admin-only nested config");
        let zmin_admin_config_before = fs::read(
            admin_only
                .zmin_super
                .path()
                .join(".git/modules/child/modules/nested/config"),
        )
        .expect("read zmin admin-only nested config");
        let stock_admin_child_before = git(
            &admin_only.stock_super.path().join("child"),
            ["rev-parse", "HEAD"],
        );
        let zmin_admin_child_before = git(
            &admin_only.zmin_super.path().join("child"),
            ["rev-parse", "HEAD"],
        );
        let stock_admin_before_state = snapshot_read_tree_nested(admin_only.stock_super.path());
        let zmin_admin_before_state = snapshot_read_tree_nested(admin_only.zmin_super.path());
        let admin_only_args = [
            "read-tree",
            "-u",
            "-m",
            "--recurse-submodules",
            "HEAD^^",
            "HEAD^",
        ];
        let stock_admin_only = raw_command_output(
            stock_git.as_path(),
            admin_only.stock_super.path(),
            &admin_only_args,
            "pinned admin-only nested transition",
        );
        let zmin_admin_only = raw_command_output(
            zmin_bin(),
            admin_only.zmin_super.path(),
            &admin_only_args,
            "zmin admin-only nested transition",
        );
        assert_eq!(zmin_admin_only, stock_admin_only, "sha256={sha256}");
        let stock_admin_only_nested = admin_only.stock_super.path().join("child/nested").exists();
        let zmin_admin_only_nested = admin_only.zmin_super.path().join("child/nested").exists();
        assert_eq!(
            zmin_admin_only_nested, stock_admin_only_nested,
            "sha256={sha256}"
        );
        let stock_admin_only_admin = admin_only
            .stock_super
            .path()
            .join(".git/modules/child/modules/nested")
            .is_dir();
        let zmin_admin_only_admin = admin_only
            .zmin_super
            .path()
            .join(".git/modules/child/modules/nested")
            .is_dir();
        assert_eq!(
            zmin_admin_only_admin, stock_admin_only_admin,
            "sha256={sha256}"
        );
        let stock_admin_config_after = fs::read(
            admin_only
                .stock_super
                .path()
                .join(".git/modules/child/modules/nested/config"),
        )
        .expect("read stock resulting admin-only nested config");
        let zmin_admin_config_after = fs::read(
            admin_only
                .zmin_super
                .path()
                .join(".git/modules/child/modules/nested/config"),
        )
        .expect("read zmin resulting admin-only nested config");
        for config in [&stock_admin_config_after, &zmin_admin_config_after] {
            assert!(
                config
                    .windows(b"\tworktree = ".len())
                    .any(|window| window == b"\tworktree = "),
                "sha256={sha256}"
            );
        }
        assert_ne!(stock_admin_config_after, stock_admin_config_before);
        assert_ne!(zmin_admin_config_after, zmin_admin_config_before);
        let zmin_admin_target_child =
            git(admin_only.zmin_super.path(), ["rev-parse", "HEAD^:child"]);
        let stock_admin_target_child =
            git(admin_only.stock_super.path(), ["rev-parse", "HEAD^:child"]);
        let stock_admin_parent_after = git(
            admin_only.stock_super.path(),
            ["ls-files", "--stage", "child"],
        );
        let zmin_admin_parent_after = git(
            admin_only.zmin_super.path(),
            ["ls-files", "--stage", "child"],
        );
        assert!(stock_admin_parent_after.contains(stock_admin_target_child.trim()));
        assert!(zmin_admin_parent_after.contains(zmin_admin_target_child.trim()));
        assert!(zmin_admin_parent_after.contains(zmin_admin_target_child.trim()));
        assert!(stock_admin_parent_before.contains(stock_admin_child_before.trim()));
        assert!(zmin_admin_parent_before.contains(zmin_admin_child_before.trim()));
        assert_eq!(
            git(
                &admin_only.zmin_super.path().join("child"),
                ["rev-parse", "HEAD"]
            ),
            zmin_admin_target_child
        );
        assert_eq!(
            git(
                &admin_only.stock_super.path().join("child"),
                ["rev-parse", "HEAD"]
            ),
            stock_admin_target_child
        );
        let stock_admin_after_state = snapshot_read_tree_nested(admin_only.stock_super.path());
        let zmin_admin_after_state = snapshot_read_tree_nested(admin_only.zmin_super.path());
        assert_ne!(
            stock_admin_after_state.root_index,
            stock_admin_before_state.root_index
        );
        assert_ne!(
            zmin_admin_after_state.root_index,
            zmin_admin_before_state.root_index
        );
        assert!(stock_admin_after_state.nested.worktree_exists);
        assert!(zmin_admin_after_state.nested.worktree_exists);
        assert!(stock_admin_after_state.nested.marker.is_some());
        assert!(zmin_admin_after_state.nested.marker.is_some());

        let admin_only_unchanged = nested_read_tree_fixture(sha256);
        let admin_only_unchanged_configs = [
            fs::read(
                admin_only_unchanged
                    .stock_super
                    .path()
                    .join(".git/modules/child/modules/nested/config"),
            )
            .expect("read unchanged stock admin-only config"),
            fs::read(
                admin_only_unchanged
                    .zmin_super
                    .path()
                    .join(".git/modules/child/modules/nested/config"),
            )
            .expect("read unchanged zmin admin-only config"),
        ];
        for repo in [
            admin_only_unchanged.stock_super.path(),
            admin_only_unchanged.zmin_super.path(),
        ] {
            fs::remove_dir_all(repo.join("child/nested"))
                .expect("remove unchanged admin-only worktree");
        }
        let stock_unchanged_admin_stage = git(
            admin_only_unchanged.stock_super.path(),
            ["ls-files", "--stage"],
        );
        let zmin_unchanged_admin_stage = git(
            admin_only_unchanged.zmin_super.path(),
            ["ls-files", "--stage"],
        );
        let stock_unchanged_admin_tree =
            git(admin_only_unchanged.stock_super.path(), ["write-tree"]);
        let zmin_unchanged_admin_tree = git(admin_only_unchanged.zmin_super.path(), ["write-tree"]);
        let unchanged_admin_args = [
            "read-tree",
            "-u",
            "-m",
            "--recurse-submodules",
            "HEAD^^",
            "HEAD^^",
        ];
        let stock_admin_unchanged = raw_command_output(
            stock_git.as_path(),
            admin_only_unchanged.stock_super.path(),
            &unchanged_admin_args,
            "pinned unchanged admin-only nested transition",
        );
        let zmin_admin_unchanged = raw_command_output(
            zmin_bin(),
            admin_only_unchanged.zmin_super.path(),
            &unchanged_admin_args,
            "zmin unchanged admin-only nested transition",
        );
        assert_eq!(
            zmin_admin_unchanged, stock_admin_unchanged,
            "sha256={sha256}"
        );
        assert_eq!(stock_admin_unchanged.0, 0, "sha256={sha256}");
        for (repo, config_before) in [
            (
                admin_only_unchanged.stock_super.path(),
                &admin_only_unchanged_configs[0],
            ),
            (
                admin_only_unchanged.zmin_super.path(),
                &admin_only_unchanged_configs[1],
            ),
        ] {
            assert!(!repo.join("child/nested").exists());
            assert!(repo.join(".git/modules/child/modules/nested").is_dir());
            assert_eq!(
                fs::read(repo.join(".git/modules/child/modules/nested/config"))
                    .expect("read unchanged admin-only config"),
                config_before.as_slice()
            );
        }
        assert_eq!(
            git(
                admin_only_unchanged.stock_super.path(),
                ["ls-files", "--stage"],
            ),
            stock_unchanged_admin_stage
        );
        assert_eq!(
            git(
                admin_only_unchanged.zmin_super.path(),
                ["ls-files", "--stage"],
            ),
            zmin_unchanged_admin_stage
        );
        assert_eq!(
            git(admin_only_unchanged.stock_super.path(), ["write-tree"]),
            stock_unchanged_admin_tree
        );
        assert_eq!(
            git(admin_only_unchanged.zmin_super.path(), ["write-tree"]),
            zmin_unchanged_admin_tree
        );

        let stale_marker = nested_read_tree_fixture(sha256);
        for repo in [
            stale_marker.stock_super.path(),
            stale_marker.zmin_super.path(),
        ] {
            fs::remove_dir_all(repo.join("child/nested"))
                .expect("remove stale-marker nested worktree");
            fs::remove_dir_all(repo.join(".git/modules/child/modules/nested"))
                .expect("remove stale-marker nested admin");
            fs::create_dir_all(repo.join("child/nested")).expect("create stale-marker directory");
            fs::write(
                repo.join("child/nested/.git"),
                b"gitdir: ../.git/modules/nested\n",
            )
            .expect("write stale nested gitfile");
        }
        let stock_stale_marker_before = snapshot_read_tree_nested(stale_marker.stock_super.path());
        let zmin_stale_marker_before = snapshot_read_tree_nested(stale_marker.zmin_super.path());
        let stale_args = [
            "read-tree",
            "-u",
            "-m",
            "--recurse-submodules",
            "HEAD^^",
            "HEAD^",
        ];
        let stock_stale_marker = raw_command_output(
            stock_git.as_path(),
            stale_marker.stock_super.path(),
            &stale_args,
            "pinned stale nested marker",
        );
        let zmin_stale_marker = raw_command_output(
            zmin_bin(),
            stale_marker.zmin_super.path(),
            &stale_args,
            "zmin stale nested marker",
        );
        assert_eq!(zmin_stale_marker, stock_stale_marker, "sha256={sha256}");
        assert_eq!(stock_stale_marker.0, 128, "sha256={sha256}");
        assert!(String::from_utf8_lossy(&stock_stale_marker.2).contains("Submodule 'child'"));
        assert!(
            stale_marker
                .stock_super
                .path()
                .join("child/nested/.git")
                .is_file()
        );
        assert!(
            stale_marker
                .zmin_super
                .path()
                .join("child/nested/.git")
                .is_file()
        );
        let stock_stale_marker_admin = stale_marker
            .stock_super
            .path()
            .join(".git/modules/child/modules/nested")
            .exists();
        let zmin_stale_marker_admin = stale_marker
            .zmin_super
            .path()
            .join(".git/modules/child/modules/nested")
            .exists();
        assert_eq!(
            zmin_stale_marker_admin, stock_stale_marker_admin,
            "sha256={sha256}"
        );
        assert_read_tree_failure_admin_shape(
            stale_marker.stock_super.path(),
            "child/nested",
            "child/modules/nested",
            false,
        );
        assert_read_tree_failure_admin_shape(
            stale_marker.zmin_super.path(),
            "child/nested",
            "child/modules/nested",
            false,
        );
        assert_read_tree_nested_snapshot_unchanged(
            &snapshot_read_tree_nested(stale_marker.stock_super.path()),
            &stock_stale_marker_before,
            "stock stale marker",
        );
        assert_read_tree_nested_snapshot_unchanged(
            &snapshot_read_tree_nested(stale_marker.zmin_super.path()),
            &zmin_stale_marker_before,
            "zmin stale marker",
        );

        let stale_remnants = nested_read_tree_fixture(sha256);
        for repo in [
            stale_remnants.stock_super.path(),
            stale_remnants.zmin_super.path(),
        ] {
            fs::remove_dir_all(repo.join("child/nested")).expect("remove remnants nested worktree");
            fs::remove_dir_all(repo.join(".git/modules/child/modules/nested"))
                .expect("remove remnants nested admin");
            fs::create_dir_all(repo.join("child/nested")).expect("create remnants directory");
            fs::write(repo.join("child/nested/leftover.txt"), b"leftover\n")
                .expect("write nested tracked remnant");
        }
        let stock_stale_remnants_before =
            snapshot_read_tree_nested(stale_remnants.stock_super.path());
        let zmin_stale_remnants_before =
            snapshot_read_tree_nested(stale_remnants.zmin_super.path());
        let stock_stale_remnants = raw_command_output(
            stock_git.as_path(),
            stale_remnants.stock_super.path(),
            &stale_args,
            "pinned stale nested remnants",
        );
        let zmin_stale_remnants = raw_command_output(
            zmin_bin(),
            stale_remnants.zmin_super.path(),
            &stale_args,
            "zmin stale nested remnants",
        );
        assert_eq!(zmin_stale_remnants, stock_stale_remnants, "sha256={sha256}");
        assert_eq!(stock_stale_remnants.0, 128, "sha256={sha256}");
        assert!(String::from_utf8_lossy(&stock_stale_remnants.2).contains("Submodule 'child'"));
        assert!(
            stale_remnants
                .stock_super
                .path()
                .join("child/nested/leftover.txt")
                .is_file()
        );
        assert!(
            stale_remnants
                .zmin_super
                .path()
                .join("child/nested/leftover.txt")
                .is_file()
        );
        let stock_stale_remnants_admin = stale_remnants
            .stock_super
            .path()
            .join(".git/modules/child/modules/nested")
            .exists();
        let zmin_stale_remnants_admin = stale_remnants
            .zmin_super
            .path()
            .join(".git/modules/child/modules/nested")
            .exists();
        assert_eq!(
            zmin_stale_remnants_admin, stock_stale_remnants_admin,
            "sha256={sha256}"
        );
        let stock_stale_remnants_after =
            snapshot_read_tree_nested(stale_remnants.stock_super.path());
        let zmin_stale_remnants_after = snapshot_read_tree_nested(stale_remnants.zmin_super.path());
        assert_eq!(
            read_tree_worktree_files_without_marker(&stock_stale_remnants_before.nested),
            read_tree_worktree_files_without_marker(&stock_stale_remnants_after.nested),
            "pinned stale-remnant snapshot changed nested files, sha256={sha256}"
        );
        assert_eq!(
            read_tree_worktree_files_without_marker(&zmin_stale_remnants_before.nested),
            read_tree_worktree_files_without_marker(&zmin_stale_remnants_after.nested),
            "zmin stale-remnant snapshot changed nested files, sha256={sha256}"
        );
        assert_eq!(
            stock_stale_remnants_before.root_index, stock_stale_remnants_after.root_index,
            "pinned stale-remnant failure changed root index, sha256={sha256}"
        );
        assert_eq!(
            zmin_stale_remnants_before.root_index, zmin_stale_remnants_after.root_index,
            "zmin stale-remnant failure changed root index, sha256={sha256}"
        );
        assert_eq!(
            read_tree_worktree_files_without_marker(&stock_stale_remnants_after.child),
            read_tree_worktree_files_without_marker(&zmin_stale_remnants_after.child)
        );
        assert_eq!(
            read_tree_worktree_files_without_marker(&stock_stale_remnants_after.nested),
            read_tree_worktree_files_without_marker(&zmin_stale_remnants_after.nested)
        );
        assert_eq!(
            stock_stale_remnants_after.nested.worktree_exists,
            zmin_stale_remnants_after.nested.worktree_exists
        );
        assert_eq!(
            stock_stale_remnants_after.nested.admin_exists,
            zmin_stale_remnants_after.nested.admin_exists
        );
        assert_read_tree_failure_admin_shape(
            stale_remnants.stock_super.path(),
            "child/nested",
            "child/modules/nested",
            true,
        );
        assert_read_tree_failure_admin_shape(
            stale_remnants.zmin_super.path(),
            "child/nested",
            "child/modules/nested",
            true,
        );
        assert_read_tree_nested_semantics_equal(
            stale_remnants.stock_super.path(),
            stale_remnants.zmin_super.path(),
            &stale_remnants.roots,
            "stale-remnant cross-side state",
        );

        let unstaged = nested_read_tree_fixture(sha256);
        for repo in [unstaged.stock_super.path(), unstaged.zmin_super.path()] {
            fs::write(repo.join("child/nested/inner.txt"), b"nested unstaged\n")
                .expect("write nested unstaged tracked file");
        }
        let remove_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD"];
        let stock_unstaged_before = snapshot_read_tree_nested(unstaged.stock_super.path());
        let zmin_unstaged_before = snapshot_read_tree_nested(unstaged.zmin_super.path());
        let stock_unstaged = raw_command_output(
            stock_git.as_path(),
            unstaged.stock_super.path(),
            &remove_args,
            "pinned nested unstaged removal",
        );
        let zmin_unstaged = raw_command_output(
            zmin_bin(),
            unstaged.zmin_super.path(),
            &remove_args,
            "zmin nested unstaged removal",
        );
        assert_eq!(zmin_unstaged, stock_unstaged, "sha256={sha256}");
        assert_eq!(stock_unstaged.0, 0, "sha256={sha256}");
        for (repo, before) in [
            (unstaged.stock_super.path(), stock_unstaged_before),
            (unstaged.zmin_super.path(), zmin_unstaged_before),
        ] {
            let after = snapshot_read_tree_nested(repo);
            assert_ne!(after.root_index, before.root_index);
            assert!(!repo.join("child").exists());
            assert!(!repo.join("child/nested").exists());
            assert!(
                !git(repo, ["ls-files", "--stage"])
                    .lines()
                    .any(|line| line.ends_with("\tchild"))
            );
            assert!(
                !git(
                    repo,
                    ["--git-dir", ".git/modules/child", "ls-files", "--stage"]
                )
                .lines()
                .any(|line| line.ends_with("\tnested"))
            );
            for config in [after.child.admin_config, after.nested.admin_config] {
                assert!(
                    !String::from_utf8_lossy(&config.expect("removal preserved admin config"))
                        .contains("core.worktree")
                );
            }
        }

        let untracked = nested_read_tree_fixture(sha256);
        for repo in [untracked.stock_super.path(), untracked.zmin_super.path()] {
            fs::write(
                repo.join("child/nested/keep.txt"),
                b"preserve nested untracked\n",
            )
            .expect("write nested untracked file");
        }
        let stock_untracked_before = snapshot_read_tree_nested(untracked.stock_super.path());
        let zmin_untracked_before = snapshot_read_tree_nested(untracked.zmin_super.path());
        let remove_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD"];
        let stock_untracked = raw_command_output(
            stock_git.as_path(),
            untracked.stock_super.path(),
            &remove_args,
            "pinned nested untracked removal",
        );
        let zmin_untracked = raw_command_output(
            zmin_bin(),
            untracked.zmin_super.path(),
            &remove_args,
            "zmin nested untracked removal",
        );
        assert_eq!(zmin_untracked, stock_untracked, "sha256={sha256}");
        assert_eq!(stock_untracked.0, 0, "sha256={sha256}");
        let stock_untracked_after = snapshot_read_tree_nested(untracked.stock_super.path());
        let zmin_untracked_after = snapshot_read_tree_nested(untracked.zmin_super.path());
        assert_ne!(
            stock_untracked_after.root_index, stock_untracked_before.root_index,
            "pinned nested untracked removal did not update root index, sha256={sha256}"
        );
        assert_ne!(
            zmin_untracked_after.root_index, zmin_untracked_before.root_index,
            "zmin nested untracked removal did not update root index, sha256={sha256}"
        );
        for repo in [untracked.stock_super.path(), untracked.zmin_super.path()] {
            assert_eq!(
                fs::read(repo.join("child/nested/keep.txt")).expect("read preserved nested file"),
                b"preserve nested untracked\n"
            );
            assert!(
                !git(repo, ["ls-files", "--stage"])
                    .lines()
                    .any(|line| line.ends_with("\tchild"))
            );
            assert!(
                !git(
                    repo,
                    ["--git-dir", ".git/modules/child", "ls-files", "--stage"]
                )
                .lines()
                .any(|line| line.ends_with("\tnested"))
            );
            assert!(!repo.join("child/.git").exists());
            assert!(
                !String::from_utf8_lossy(
                    &fs::read(repo.join(".git/modules/child/config"))
                        .expect("read preserved child admin config")
                )
                .contains("core.worktree")
            );
            assert!(
                !String::from_utf8_lossy(
                    &fs::read(repo.join(".git/modules/child/modules/nested/config"))
                        .expect("read preserved nested admin config")
                )
                .contains("core.worktree")
            );
        }
        assert_read_tree_nested_semantics_equal(
            untracked.stock_super.path(),
            untracked.zmin_super.path(),
            &untracked.roots,
            "nested untracked removal cross-side state",
        );

        let refusal = nested_read_tree_fixture(sha256);
        for repo in [refusal.stock_super.path(), refusal.zmin_super.path()] {
            let nested = repo.join("child/nested");
            fs::write(nested.join("inner.txt"), b"staged nested\n")
                .expect("write staged nested file");
            git(&nested, ["add", "inner.txt"]);
        }
        let before = |repo: &Path| {
            (
                git(repo, ["ls-files", "--stage"]),
                git(&repo.join("child"), ["ls-files", "--stage"]),
                git(&repo.join("child/nested"), ["ls-files", "--stage"]),
                git(&repo.join("child"), ["rev-parse", "HEAD"]),
                git(&repo.join("child/nested"), ["rev-parse", "HEAD"]),
                fs::read(repo.join("child/nested/inner.txt")).expect("read staged nested bytes"),
            )
        };
        let stock_before = before(refusal.stock_super.path());
        let zmin_before = before(refusal.zmin_super.path());
        let stock_refusal_state = snapshot_read_tree_nested(refusal.stock_super.path());
        let zmin_refusal_state = snapshot_read_tree_nested(refusal.zmin_super.path());
        let remove_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD"];
        let stock_refusal = raw_command_output(
            stock_git.as_path(),
            refusal.stock_super.path(),
            &remove_args,
            "pinned nested staged refusal",
        );
        let zmin_refusal = raw_command_output(
            zmin_bin(),
            refusal.zmin_super.path(),
            &remove_args,
            "zmin nested staged refusal",
        );
        assert_eq!(zmin_refusal, stock_refusal, "sha256={sha256}");
        assert_eq!(stock_refusal.0, 128, "sha256={sha256}");
        assert!(
            String::from_utf8_lossy(&stock_refusal.2).contains("child/nested"),
            "nested refusal omitted full path: {:?}",
            String::from_utf8_lossy(&stock_refusal.2)
        );
        assert_eq!(before(refusal.stock_super.path()), stock_before);
        assert_eq!(before(refusal.zmin_super.path()), zmin_before);
        assert_eq!(
            snapshot_read_tree_nested(refusal.stock_super.path()),
            stock_refusal_state
        );
        assert_eq!(
            snapshot_read_tree_nested(refusal.zmin_super.path()),
            zmin_refusal_state
        );

        let stock_reset_before = snapshot_read_tree_nested(refusal.stock_super.path());
        let zmin_reset_before = snapshot_read_tree_nested(refusal.zmin_super.path());
        let reset_args = ["read-tree", "--reset", "-u", "--recurse-submodules", "HEAD"];
        let stock_reset = raw_command_output(
            stock_git.as_path(),
            refusal.stock_super.path(),
            &reset_args,
            "pinned nested reset removal",
        );
        assert_eq!(stock_reset.0, 0, "sha256={sha256}");
        let stock_reset_expected =
            capture_read_tree_nested_reset_expectation(refusal.stock_super.path());
        let zmin_reset = raw_command_output(
            zmin_bin(),
            refusal.zmin_super.path(),
            &reset_args,
            "zmin nested reset removal",
        );
        assert_eq!(zmin_reset, stock_reset, "sha256={sha256}");
        assert_eq!(stock_reset.0, 0, "sha256={sha256}");
        let zmin_reset_expected = stock_reset_expected.clone();
        for (repo, before, expected) in [
            (
                refusal.stock_super.path(),
                stock_reset_before,
                stock_reset_expected,
            ),
            (
                refusal.zmin_super.path(),
                zmin_reset_before,
                zmin_reset_expected,
            ),
        ] {
            let after = snapshot_read_tree_nested(repo);
            assert_ne!(
                after.root_index, before.root_index,
                "reset left parent index unchanged"
            );
            assert!(!repo.join("child").exists());
            assert!(repo.join(".git/modules/child/index").exists());
            assert!(
                repo.join(".git/modules/child/modules/nested/index")
                    .exists()
            );
            assert!(
                !git(repo, ["ls-files", "--stage"])
                    .lines()
                    .any(|line| line.ends_with("\tchild"))
            );
            assert_eq!(
                git(repo, ["write-tree"]).trim(),
                git(repo, ["rev-parse", "HEAD^{tree}"]).trim(),
                "reset parent index does not match HEAD"
            );
            let temp_roots = refusal.roots.temp_root_refs();
            assert_read_tree_nested_reset_expectation(
                repo,
                &expected,
                refusal.roots.object_hex_len,
                &temp_roots,
            );
            assert!(
                !String::from_utf8_lossy(
                    &fs::read(repo.join(".git/modules/child/config"))
                        .expect("read child admin config")
                )
                .contains("core.worktree")
            );
            assert!(
                !String::from_utf8_lossy(
                    &fs::read(repo.join(".git/modules/child/modules/nested/config"))
                        .expect("read nested admin config")
                )
                .contains("core.worktree")
            );
        }
        assert_read_tree_nested_semantics_equal(
            refusal.stock_super.path(),
            refusal.zmin_super.path(),
            &refusal.roots,
            "nested reset removal post-state",
        );
    }
}

#[test]
fn read_tree_recursive_removal_is_atomic_and_preserves_untracked_for_sha1_and_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let clean_fixture = read_tree_removal_fixture(sha256);
        let populate_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD^"];
        let stock_populate = raw_command_output(
            stock_git.as_path(),
            clean_fixture.stock_super.path(),
            &populate_args,
            "pinned clean populate",
        );
        let zmin_populate = raw_command_output(
            zmin_bin(),
            clean_fixture.zmin_super.path(),
            &populate_args,
            "zmin clean populate",
        );
        assert_eq!(zmin_populate, stock_populate, "sha256={sha256}");
        let stock_clean_before = snapshot_read_tree_nested(clean_fixture.stock_super.path());
        let zmin_clean_before = snapshot_read_tree_nested(clean_fixture.zmin_super.path());
        let clean_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD"];
        let stock_clean = raw_command_output(
            stock_git.to_str().expect("pinned Git path utf8"),
            clean_fixture.stock_super.path(),
            &clean_args,
            "pinned recursive clean removal",
        );
        let zmin_clean = raw_command_output(
            zmin_bin(),
            clean_fixture.zmin_super.path(),
            &clean_args,
            "zmin recursive clean removal",
        );
        assert_eq!(zmin_clean, stock_clean, "sha256={sha256}");
        assert_eq!(stock_clean.0, 0, "clean removal failed");
        let stock_clean_after = snapshot_read_tree_nested(clean_fixture.stock_super.path());
        let zmin_clean_after = snapshot_read_tree_nested(clean_fixture.zmin_super.path());
        assert_ne!(
            stock_clean_after.root_index, stock_clean_before.root_index,
            "pinned clean removal did not update parent index, sha256={sha256}"
        );
        assert_ne!(
            zmin_clean_after.root_index, zmin_clean_before.root_index,
            "zmin clean removal did not update parent index, sha256={sha256}"
        );
        assert_read_tree_nested_semantics_equal(
            clean_fixture.stock_super.path(),
            clean_fixture.zmin_super.path(),
            &clean_fixture.roots,
            "clean removal cross-side state",
        );
        for repo in [
            clean_fixture.stock_super.path(),
            clean_fixture.zmin_super.path(),
        ] {
            assert!(!repo.join("sub/.git").exists());
            assert!(!repo.join("sub").exists());
            assert!(
                !git(repo, ["ls-files", "--stage"])
                    .lines()
                    .any(|line| line.ends_with("\tsub"))
            );
            assert_eq!(
                git(repo, ["write-tree"]).trim(),
                git(repo, ["rev-parse", "HEAD^{tree}"]).trim(),
                "clean removal left a non-HEAD parent index"
            );
        }

        let fixture = read_tree_removal_fixture(sha256);
        let args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD^"];
        let stock_populate = raw_command_output(
            stock_git.to_str().expect("pinned Git path utf8"),
            fixture.stock_super.path(),
            &args,
            "pinned recursive populate before removal",
        );
        let zmin_populate = raw_command_output(
            zmin_bin(),
            fixture.zmin_super.path(),
            &args,
            "zmin recursive populate before removal",
        );
        assert_eq!(zmin_populate, stock_populate, "sha256={sha256}");
        assert!(fixture.zmin_super.path().join("sub/child.txt").exists());

        for child in [
            fixture.stock_super.path().join("sub"),
            fixture.zmin_super.path().join("sub"),
        ] {
            fs::write(child.join("untracked.txt"), b"keep\n").expect("write untracked child");
        }
        let stock_dirty_before = snapshot_read_tree_nested(fixture.stock_super.path());
        let zmin_dirty_before = snapshot_read_tree_nested(fixture.zmin_super.path());
        let dirty_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD"];
        let stock_dirty = raw_command_output(
            stock_git.to_str().expect("pinned Git path utf8"),
            fixture.stock_super.path(),
            &dirty_args,
            "pinned recursive dirty removal",
        );
        let zmin_dirty = raw_command_output(
            zmin_bin(),
            fixture.zmin_super.path(),
            &dirty_args,
            "zmin recursive dirty removal",
        );
        assert_eq!(zmin_dirty, stock_dirty, "sha256={sha256}");
        assert_eq!(stock_dirty.0, 0, "dirty removal failed");
        let stock_dirty_after = snapshot_read_tree_nested(fixture.stock_super.path());
        let zmin_dirty_after = snapshot_read_tree_nested(fixture.zmin_super.path());
        assert_ne!(
            stock_dirty_after.root_index, stock_dirty_before.root_index,
            "pinned dirty removal did not update parent index, sha256={sha256}"
        );
        assert_ne!(
            zmin_dirty_after.root_index, zmin_dirty_before.root_index,
            "zmin dirty removal did not update parent index, sha256={sha256}"
        );
        assert!(
            fixture
                .stock_super
                .path()
                .join("sub/untracked.txt")
                .is_file()
        );
        assert!(
            fixture
                .zmin_super
                .path()
                .join("sub/untracked.txt")
                .is_file()
        );
        assert_read_tree_nested_semantics_equal(
            fixture.stock_super.path(),
            fixture.zmin_super.path(),
            &fixture.roots,
            "dirty removal cross-side state",
        );
    }
}

#[test]
fn read_tree_recursive_dry_run_preserves_submodule_state_for_sha1_and_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let fixture = nested_read_tree_fixture(sha256);
        let stock_before = snapshot_read_tree_nested(fixture.stock_super.path());
        let zmin_before = snapshot_read_tree_nested(fixture.zmin_super.path());
        let args = [
            "read-tree",
            "--dry-run",
            "-u",
            "-m",
            "--recurse-submodules",
            "HEAD",
        ];
        let stock_output = raw_command_output(
            stock_git.as_path(),
            fixture.stock_super.path(),
            &args,
            "pinned recursive read-tree dry-run",
        );
        let zmin_output = raw_command_output(
            zmin_bin(),
            fixture.zmin_super.path(),
            &args,
            "zmin recursive read-tree dry-run",
        );
        assert_eq!(zmin_output, stock_output, "sha256={sha256}");
        assert_eq!(stock_output.0, 0, "sha256={sha256}");
        assert_eq!(
            snapshot_read_tree_nested(fixture.stock_super.path()),
            stock_before,
            "stock dry-run mutated submodule state, sha256={sha256}"
        );
        assert_eq!(
            snapshot_read_tree_nested(fixture.zmin_super.path()),
            zmin_before,
            "zmin dry-run mutated submodule state, sha256={sha256}"
        );
    }
}

#[test]
fn read_tree_removed_absent_gitlink_is_a_noop_for_sha1_and_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        for reset in [false, true] {
            let fixture = read_tree_removal_fixture(sha256);
            for repo in [fixture.stock_super.path(), fixture.zmin_super.path()] {
                fs::remove_dir_all(repo.join("sub")).expect("remove absent submodule worktree");
                fs::remove_dir_all(repo.join(".git/modules/sub"))
                    .expect("remove absent submodule admin");
            }
            let args = if reset {
                vec!["read-tree", "--reset", "--recurse-submodules", "HEAD"]
            } else {
                vec!["read-tree", "-u", "-m", "--recurse-submodules", "HEAD"]
            };
            let stock = raw_command_output(
                stock_git.as_path(),
                fixture.stock_super.path(),
                &args,
                "pinned removed absent submodule",
            );
            let zmin = raw_command_output(
                zmin_bin(),
                fixture.zmin_super.path(),
                &args,
                "zmin removed absent submodule",
            );
            assert_eq!(zmin, stock, "sha256={sha256}, reset={reset}");
            assert_eq!(stock.0, 0, "sha256={sha256}, reset={reset}");
            assert!(stock.1.is_empty(), "sha256={sha256}, reset={reset}");
            assert!(stock.2.is_empty(), "sha256={sha256}, reset={reset}");
            for repo in [fixture.stock_super.path(), fixture.zmin_super.path()] {
                assert!(!repo.join("sub").exists(), "sha256={sha256}, reset={reset}");
                assert!(
                    !repo.join(".git/modules/sub").exists(),
                    "sha256={sha256}, reset={reset}"
                );
                assert!(
                    !git(repo, ["ls-files", "--stage"])
                        .lines()
                        .any(|line| line.ends_with("\tsub"))
                );
            }
        }
    }
}

#[test]
fn read_tree_root_unchanged_absent_and_admin_only_states_match_pinned_git() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        for admin_only in [false, true] {
            for reset in [false, true] {
                let fixture = read_tree_removal_fixture(sha256);
                let fixture_roots = fixture.roots.clone();
                for repo in [fixture.stock_super.path(), fixture.zmin_super.path()] {
                    if !admin_only {
                        fs::remove_dir_all(repo.join("sub"))
                            .expect("remove unchanged root worktree");
                    } else {
                        fs::remove_dir_all(repo.join("sub"))
                            .expect("remove unchanged admin-only root worktree");
                    }
                    if !admin_only {
                        fs::remove_dir_all(repo.join(".git/modules/sub"))
                            .expect("remove unchanged root admin");
                    }
                }
                let stock_before =
                    snapshot_read_tree_module(fixture.stock_super.path(), "sub", "modules/sub");
                let zmin_before =
                    snapshot_read_tree_module(fixture.zmin_super.path(), "sub", "modules/sub");
                let args = if reset {
                    vec![
                        "read-tree",
                        "--reset",
                        "-u",
                        "--recurse-submodules",
                        "HEAD^^",
                    ]
                } else {
                    vec![
                        "read-tree",
                        "-u",
                        "-m",
                        "--recurse-submodules",
                        "HEAD^^",
                        "HEAD^^",
                    ]
                };
                let stock = raw_command_output(
                    stock_git.as_path(),
                    fixture.stock_super.path(),
                    &args,
                    "pinned unchanged root submodule",
                );
                let zmin = raw_command_output(
                    zmin_bin(),
                    fixture.zmin_super.path(),
                    &args,
                    "zmin unchanged root submodule",
                );
                assert_eq!(
                    zmin, stock,
                    "sha256={sha256}, admin_only={admin_only}, reset={reset}"
                );
                if reset && !admin_only {
                    assert_ne!(
                        stock.0, 0,
                        "sha256={sha256}, admin_only={admin_only}, reset={reset}"
                    );
                } else {
                    assert_eq!(
                        stock.0, 0,
                        "sha256={sha256}, admin_only={admin_only}, reset={reset}"
                    );
                    assert!(
                        stock.1.is_empty(),
                        "sha256={sha256}, admin_only={admin_only}, reset={reset}"
                    );
                    assert!(
                        stock.2.is_empty(),
                        "sha256={sha256}, admin_only={admin_only}, reset={reset}"
                    );
                }
                let stock_after =
                    snapshot_read_tree_module(fixture.stock_super.path(), "sub", "modules/sub");
                let zmin_after =
                    snapshot_read_tree_module(fixture.zmin_super.path(), "sub", "modules/sub");
                if !reset {
                    assert_read_tree_module_snapshot_unchanged(
                        &stock_after,
                        &stock_before,
                        "pinned root state changed",
                        "root",
                    );
                    assert_read_tree_module_snapshot_unchanged(
                        &zmin_after,
                        &zmin_before,
                        "zmin root state changed",
                        "root",
                    );
                } else {
                    assert!(
                        stock_after.worktree_exists,
                        "pinned reset did not create the root submodule worktree"
                    );
                    assert!(
                        stock_after.marker.is_some(),
                        "pinned reset did not create the root submodule marker"
                    );
                    assert!(
                        stock_after.admin_exists,
                        "pinned reset did not create the root submodule admin"
                    );
                    if admin_only {
                        assert_eq!(
                            stock_after.admin_head, stock_before.admin_head,
                            "pinned reset changed the admin HEAD"
                        );
                    }
                }
                let temp_roots = fixture_roots.temp_root_refs();
                assert_read_tree_module_semantics_equal(
                    fixture.stock_super.path(),
                    fixture.zmin_super.path(),
                    "sub",
                    "modules/sub",
                    &stock_after,
                    &zmin_after,
                    &temp_roots,
                    fixture_roots.object_hex_len,
                    "root state diverged",
                );
                assert_eq!(
                    stock_after.admin_head, zmin_after.admin_head,
                    "root admin HEAD diverged"
                );
                if stock_after.admin_exists {
                    assert_eq!(
                        normalize_read_tree_index_for_fixture(&git(
                            &fixture.stock_super.path().join(".git/modules/sub"),
                            ["ls-files", "--stage"],
                        )),
                        normalize_read_tree_index_for_fixture(&git(
                            &fixture.zmin_super.path().join(".git/modules/sub"),
                            ["ls-files", "--stage"],
                        )),
                        "root admin index entries diverged"
                    );
                }
                if reset {
                    assert!(stock_after.worktree_exists);
                    assert!(stock_after.marker.is_some());
                    assert!(stock_after.admin_exists);
                } else {
                    assert!(!stock_after.worktree_exists);
                    assert_eq!(stock_after.admin_exists, admin_only);
                }
            }
        }
    }
}

#[test]
fn read_tree_submodule_validation_index_output_is_atomic_for_sha1_and_sha256() {
    let stock_git = required_pinned_stock_git();
    let mut normalized_outputs = Vec::new();
    for sha256 in [false, true] {
        let fixture = read_tree_removal_fixture(sha256);
        for (program, repo) in [
            (stock_git.as_path(), fixture.stock_super.path()),
            (Path::new(zmin_bin()), fixture.zmin_super.path()),
        ] {
            let child = repo.join("sub");
            let child_id = git(&child, ["rev-parse", "HEAD"]).trim().to_owned();
            let gitmodules = fs::read_to_string(repo.join(".gitmodules"))
                .expect("read sibling validation gitmodules");
            git(repo, ["config", "submodule.later.url", "missing-later"]);
            git(repo, ["add", ".gitmodules"]);
            git(
                repo,
                [
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("160000,{child_id},sub"),
                ],
            );
            git_with_env(repo, ["commit", "-m", "sibling base"]);
            let sibling_base = git(repo, ["rev-parse", "HEAD"]).trim().to_owned();
            fs::write(
                repo.join(".gitmodules"),
                format!(
                    "{gitmodules}[submodule \"later\"]\n\tpath = later\n\turl = missing-later\n"
                ),
            )
            .expect("write later sibling gitmodules");
            git(repo, ["add", ".gitmodules"]);
            git(
                repo,
                [
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("160000,{child_id},later"),
                ],
            );
            git_with_env(repo, ["commit", "-m", "sibling failure"]);
            let sibling_failure = git(repo, ["rev-parse", "HEAD"]).trim().to_owned();
            git(repo, ["read-tree", "--reset", &sibling_base]);
            let before_index = fs::read(repo.join(".git/index")).expect("read real index");
            let before_child_head = git(&child, ["rev-parse", "HEAD"]);
            let before_state = (
                before_index.clone(),
                snapshot_read_tree_module(repo, "sub", "modules/sub"),
                snapshot_read_tree_module(repo, "later", "modules/later"),
                read_tree_optional_file(&repo.join(".git/config")),
                read_tree_optional_file(&repo.join(".gitmodules")),
            );
            let alt_index = repo.join("alt.index");
            assert!(!alt_index.exists());
            let args = [
                "read-tree",
                "-u",
                "-m",
                "--recurse-submodules",
                "--index-output=alt.index",
                &sibling_failure,
            ];
            let output = raw_command_output(
                program,
                repo,
                &args,
                "read-tree sibling validation with index-output",
            );
            normalized_outputs.push((
                output.0,
                output.1.clone(),
                normalize_read_tree_temp_path(&output.2, repo),
            ));
            assert_eq!(output.0, 128, "sha256={sha256}, program={program:?}");
            assert_eq!(
                fs::read(repo.join(".git/index")).expect("read unchanged real index"),
                before_index,
                "validation mutated the real index, sha256={sha256}, program={program:?}"
            );
            assert_eq!(
                git(&child, ["rev-parse", "HEAD"]),
                before_child_head,
                "validation mutated the first sibling, sha256={sha256}, program={program:?}"
            );
            assert!(!alt_index.exists());
            let after_state = (
                fs::read(repo.join(".git/index")).expect("read final real index"),
                snapshot_read_tree_module(repo, "sub", "modules/sub"),
                snapshot_read_tree_module(repo, "later", "modules/later"),
                read_tree_optional_file(&repo.join(".git/config")),
                read_tree_optional_file(&repo.join(".gitmodules")),
            );
            assert_eq!(after_state, before_state);
        }
    }
    assert_eq!(normalized_outputs.len(), 4);
    assert_eq!(normalized_outputs[1], normalized_outputs[0]);
    assert_eq!(normalized_outputs[3], normalized_outputs[2]);
}

fn make_read_tree_submodule_embedded(repo: &Path) {
    make_read_tree_submodule_embedded_at(repo, "sub", "modules/sub");
}

fn make_read_tree_submodule_embedded_at(repo: &Path, worktree_path: &str, admin_path: &str) {
    let worktree_git = repo.join(worktree_path).join(".git");
    let admin_git = repo.join(".git").join(admin_path);
    fs::remove_file(&worktree_git).unwrap_or_else(|error| {
        panic!("remove linked submodule gitfile {worktree_git:?}: {error}")
    });
    fs::rename(admin_git, &worktree_git).expect("embed submodule git directory");
    let config_path = worktree_git.join("config");
    let config = fs::read_to_string(&config_path).expect("read embedded submodule config");
    let config = config
        .lines()
        .filter(|line| !line.trim_start().starts_with("worktree ="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(config_path, format!("{config}\n")).expect("write embedded submodule config");
}

fn normalize_read_tree_temp_path(stderr: &[u8], repo: &Path) -> Vec<u8> {
    String::from_utf8_lossy(stderr)
        .replace(&repo.display().to_string(), "<repo>")
        .into_bytes()
}

fn normalize_read_tree_command_output(
    output: &(i32, Vec<u8>, Vec<u8>),
    repo: &Path,
) -> (i32, Vec<u8>, Vec<u8>) {
    (
        output.0,
        output.1.clone(),
        normalize_read_tree_temp_path(&output.2, repo),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadTreeModuleSnapshot {
    worktree_exists: bool,
    worktree_files: Option<Vec<(String, Vec<u8>)>>,
    marker: Option<Vec<u8>>,
    admin_exists: bool,
    admin_index: Option<Vec<u8>>,
    admin_head: Option<Vec<u8>>,
    admin_config: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadTreeNestedSnapshot {
    root_index: Option<Vec<u8>>,
    root_config: Option<Vec<u8>>,
    child: ReadTreeModuleSnapshot,
    nested: ReadTreeModuleSnapshot,
}

fn read_tree_file_snapshot(root: &Path) -> Option<Vec<(String, Vec<u8>)>> {
    if !root.exists() {
        return None;
    }

    fn visit(root: &Path, path: &Path, files: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(path).expect("read read-tree worktree snapshot") {
            let entry = entry.expect("read read-tree worktree snapshot entry");
            let entry_path = entry.path();
            if entry_path.is_dir() {
                visit(root, &entry_path, files);
            } else if entry_path.is_file() {
                let relative = entry_path
                    .strip_prefix(root)
                    .expect("read-tree snapshot relative path")
                    .to_string_lossy()
                    .into_owned();
                files.push((
                    relative,
                    fs::read(&entry_path).expect("read read-tree worktree snapshot file"),
                ));
            }
        }
    }

    let mut files = Vec::new();
    visit(root, root, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Some(files)
}

fn read_tree_optional_file(path: &Path) -> Option<Vec<u8>> {
    path.is_file()
        .then(|| fs::read(path).expect("read read-tree snapshot file"))
}

fn normalize_read_tree_index_for_fixture(index: &str) -> String {
    index
        .lines()
        .map(|line| {
            let Some((entry, path)) = line.split_once('\t') else {
                return line.to_owned();
            };
            if path != ".gitmodules" {
                let fields = entry.split_whitespace().collect::<Vec<_>>();
                if fields.first() == Some(&"160000") {
                    assert_eq!(fields.len(), 3, "unexpected gitlink index entry: {line}");
                    return format!("{} <gitlink-oid> {}\t{path}", fields[0], fields[2]);
                }
                return line.to_owned();
            }
            let fields = entry.split_whitespace().collect::<Vec<_>>();
            assert_eq!(fields.len(), 3, "unexpected read-tree index entry: {line}");
            format!("{} <gitmodules-blob> {}\t{path}", fields[0], fields[2])
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn snapshot_read_tree_module(
    repo: &Path,
    module_path: &str,
    admin_path: &str,
) -> ReadTreeModuleSnapshot {
    let worktree = repo.join(module_path);
    let admin = repo.join(".git").join(admin_path);
    ReadTreeModuleSnapshot {
        worktree_exists: worktree.exists(),
        worktree_files: read_tree_file_snapshot(&worktree),
        marker: read_tree_optional_file(&worktree.join(".git")),
        admin_exists: admin.is_dir(),
        admin_index: read_tree_optional_file(&admin.join("index")),
        admin_head: read_tree_optional_file(&admin.join("HEAD")),
        admin_config: read_tree_optional_file(&admin.join("config")),
    }
}

fn snapshot_read_tree_nested(repo: &Path) -> ReadTreeNestedSnapshot {
    ReadTreeNestedSnapshot {
        root_index: read_tree_optional_file(&repo.join(".git/index")),
        root_config: read_tree_optional_file(&repo.join(".git/config")),
        child: snapshot_read_tree_module(repo, "child", "modules/child"),
        nested: snapshot_read_tree_module(repo, "child/nested", "modules/child/modules/nested"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadTreeNestedResetExpectation {
    child_admin_head: Option<String>,
    nested_admin_head: Option<String>,
    child_index: String,
    nested_index: String,
}

fn capture_read_tree_nested_reset_expectation(repo: &Path) -> ReadTreeNestedResetExpectation {
    let target = raw_command_output(
        required_pinned_stock_git(),
        repo,
        &["ls-tree", "HEAD", "child"],
        "pinned reset target child entry",
    );
    assert_eq!(target.0, 0, "read reset target child entry: {target:?}");
    assert!(
        target.1.is_empty(),
        "reset fixture unexpectedly has a child at target HEAD: {:?}",
        String::from_utf8_lossy(&target.1)
    );
    ReadTreeNestedResetExpectation {
        child_admin_head: read_tree_admin_head_semantics(repo, "modules/child"),
        nested_admin_head: read_tree_admin_head_semantics(repo, "modules/child/modules/nested"),
        child_index: read_tree_admin_index_semantics(repo, "modules/child").unwrap_or_default(),
        nested_index: read_tree_admin_index_semantics(repo, "modules/child/modules/nested")
            .unwrap_or_default(),
    }
}

fn assert_read_tree_nested_reset_expectation(
    repo: &Path,
    expected: &ReadTreeNestedResetExpectation,
    object_hex_len: usize,
    temp_roots: &[&Path],
) {
    let child = snapshot_read_tree_module(repo, "child", "modules/child");
    let nested = snapshot_read_tree_module(repo, "child/nested", "modules/child/modules/nested");
    assert!(!child.worktree_exists, "reset recreated child worktree");
    assert!(!nested.worktree_exists, "reset recreated nested worktree");
    assert!(child.marker.is_none(), "reset left child marker");
    assert!(nested.marker.is_none(), "reset left nested marker");
    assert!(child.admin_exists, "reset removed child admin");
    assert!(nested.admin_exists, "reset removed nested admin");
    assert_eq!(
        read_tree_admin_head_semantics(repo, "modules/child"),
        expected.child_admin_head
    );
    assert_eq!(
        read_tree_admin_head_semantics(repo, "modules/child/modules/nested"),
        expected.nested_admin_head
    );
    assert_eq!(
        read_tree_admin_index_semantics(repo, "modules/child").unwrap_or_default(),
        expected.child_index
    );
    assert_eq!(
        read_tree_admin_index_semantics(repo, "modules/child/modules/nested").unwrap_or_default(),
        expected.nested_index
    );
    for (module, admin) in [
        (&child, "modules/child"),
        (&nested, "modules/child/modules/nested"),
    ] {
        assert!(
            module.admin_config.is_some(),
            "reset removed .git/{admin} config"
        );
        assert!(
            !String::from_utf8_lossy(module.admin_config.as_ref().unwrap())
                .contains("core.worktree"),
            "reset retained .git/{admin} core.worktree"
        );
        assert!(
            read_tree_admin_file_set(repo, admin, object_hex_len)
                .is_some_and(|files| files.contains("config")
                    && files.contains("HEAD")
                    && files.contains("index")),
            "reset .git/{admin} file roles are incomplete"
        );
    }
    assert_eq!(
        read_tree_core_worktree_semantics(
            repo,
            "child",
            "modules/child",
            child.admin_config.as_ref(),
        ),
        None
    );
    assert_eq!(
        read_tree_core_worktree_semantics(
            repo,
            "child/nested",
            "modules/child/modules/nested",
            nested.admin_config.as_ref(),
        ),
        None
    );
    let child_contents =
        read_tree_admin_non_object_contents(repo, "modules/child", object_hex_len, temp_roots)
            .expect("child admin contents");
    let nested_contents = read_tree_admin_non_object_contents(
        repo,
        "modules/child/modules/nested",
        object_hex_len,
        temp_roots,
    )
    .expect("nested admin contents");
    assert!(child_contents.contains_key("config") && child_contents.contains_key("HEAD"));
    assert!(nested_contents.contains_key("config") && nested_contents.contains_key("HEAD"));
}

fn read_tree_worktree_files_without_marker(
    module: &ReadTreeModuleSnapshot,
) -> Option<Vec<(String, Vec<u8>)>> {
    module.worktree_files.as_ref().map(|files| {
        files
            .iter()
            .filter(|(path, _)| path != ".git" && path != ".gitmodules")
            .cloned()
            .collect()
    })
}

fn normalize_read_tree_config_value(value: &str, temp_roots: &[&Path]) -> String {
    let mut normalized = value.to_owned();
    for root in temp_roots {
        let root = root.to_string_lossy();
        if !root.is_empty() {
            normalized = normalized.replace(root.as_ref(), "<temp-root>");
        }
    }
    normalized
}

#[test]
fn read_tree_fixture_root_normalization_rejects_unexpected_absolute_root() {
    let fixture_root = TempDir::new().expect("fixture root");
    let expected_root = fixture_root.path().join("stock/super");
    let expected_value = format!("{}\nchild-suffix", expected_root.display());
    let expected = normalize_read_tree_config_value(&expected_value, &[&expected_root]);
    assert_eq!(expected, "<temp-root>\nchild-suffix");

    let unexpected_root = fixture_root.path().join("tampered/absolute");
    let unexpected_value = format!("{}\nchild-suffix", unexpected_root.display());
    let unexpected = normalize_read_tree_config_value(&unexpected_value, &[&expected_root]);
    assert_eq!(unexpected, unexpected_value);
    assert_ne!(unexpected, expected);
}

#[test]
fn read_tree_admin_layout_detects_unknown_object_and_content_tampering() {
    let repo = TempDir::new().expect("admin tamper repo");
    let admin = repo.path().join(".git");
    fs::create_dir_all(admin.join("objects/aa")).expect("create admin object fanout");
    fs::write(admin.join("config"), b"[core]\n\tbare = true\n").expect("write admin config");
    let temp_roots = vec![repo.path()];
    let before = read_tree_admin_layout_snapshot(repo.path(), "", 40, &temp_roots)
        .expect("read untampered admin layout");
    fs::write(admin.join("objects/aa/unknown"), b"aux").expect("write unknown object artifact");
    let after_object = read_tree_admin_layout_snapshot(repo.path(), "", 40, &temp_roots)
        .expect("read object-tampered admin layout");
    assert_eq!(before.counts.other_aux, 0);
    assert_eq!(after_object.counts.other_aux, 1);
    assert_ne!(before.roles, after_object.roles);
    fs::write(admin.join("config"), b"[core]\n\tbare = false\n").expect("tamper admin config");
    let after_content = read_tree_admin_layout_snapshot(repo.path(), "", 40, &temp_roots)
        .expect("read content-tampered admin layout");
    assert_ne!(
        before.non_object_contents,
        after_content.non_object_contents
    );
}

fn normalize_read_tree_config(config: Option<&Vec<u8>>, temp_roots: &[&Path]) -> Option<Vec<u8>> {
    config.map(|config| {
        config
            .split(|byte| *byte == b'\n')
            .map(|line| {
                let line = String::from_utf8_lossy(line)
                    .trim_end_matches('\r')
                    .to_owned();
                let Some((prefix, value)) = line.split_once('=') else {
                    return line;
                };
                let key = prefix.split_whitespace().last().unwrap_or_default();
                if !matches!(key, "url" | "worktree" | "gitdir") {
                    return line;
                }
                format!(
                    "{}={}",
                    prefix,
                    normalize_read_tree_config_value(value, temp_roots)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes()
    })
}

fn read_tree_marker_semantics(
    repo: &Path,
    module_path: &str,
    admin_path: &str,
    marker: Option<&Vec<u8>>,
) -> Option<&'static str> {
    let marker = marker?;
    let marker = String::from_utf8_lossy(marker);
    let target = marker.strip_prefix("gitdir:")?.trim();
    let worktree = repo.join(module_path);
    let admin = repo.join(".git").join(admin_path);
    let marker_target = worktree.join(target);
    match (fs::canonicalize(marker_target), fs::canonicalize(admin)) {
        (Ok(marker_target), Ok(admin)) if marker_target == admin => Some("connected"),
        (Ok(_), Ok(_)) => Some("other-existing-admin"),
        _ => Some("stale"),
    }
}

fn read_tree_core_worktree_semantics(
    repo: &Path,
    module_path: &str,
    admin_path: &str,
    config: Option<&Vec<u8>>,
) -> Option<&'static str> {
    let config = config?;
    let config = String::from_utf8_lossy(config);
    let worktree_value = config
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("worktree = "))?;
    let worktree = fs::canonicalize(repo.join(module_path));
    let resolved = fs::canonicalize(repo.join(".git").join(admin_path).join(worktree_value));
    match (resolved, worktree) {
        (Ok(resolved), Ok(worktree)) if resolved == worktree => Some("connected"),
        (Ok(_), Ok(_)) => Some("other-worktree"),
        _ => Some("missing-worktree"),
    }
}

fn read_tree_admin_file_set(
    repo: &Path,
    admin_path: &str,
    object_hex_len: usize,
) -> Option<BTreeSet<String>> {
    let root = repo.join(".git").join(admin_path);
    if !root.is_dir() {
        return None;
    }

    fn visit(root: &Path, path: &Path, files: &mut Vec<String>, skip_nested_modules: bool) {
        for entry in fs::read_dir(path).expect("read read-tree admin snapshot") {
            let entry = entry.expect("read read-tree admin snapshot entry");
            let entry_path = entry.path();
            if entry_path.is_dir() {
                if !(skip_nested_modules && entry.file_name() == "modules") {
                    visit(root, &entry_path, files, skip_nested_modules);
                }
            } else if entry_path.is_file() {
                files.push(
                    entry_path
                        .strip_prefix(root)
                        .expect("read-tree admin snapshot relative path")
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }

    let mut files = Vec::new();
    visit(&root, &root, &mut files, admin_path.is_empty());
    files.sort();
    let mut category_counts = BTreeMap::<String, usize>::new();
    let mut normalized = BTreeSet::new();
    for relative in files {
        let path = Path::new(&relative);
        let components = path
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .collect::<Vec<_>>();
        let Some(objects_index) = components
            .iter()
            .position(|component| *component == "objects")
        else {
            normalized.insert(relative);
            continue;
        };
        let object_prefix = components[..=objects_index].join("/");
        let Some(object_child) = components.get(objects_index + 1) else {
            normalized.insert(relative);
            continue;
        };
        if *object_child == "pack" && components.len() == objects_index + 3 {
            let filename = components[objects_index + 2];
            let valid_pack = filename
                .strip_prefix("pack-")
                .and_then(|name| name.split_once('.'))
                .is_some_and(|(object_name, extension)| {
                    object_name.len() == object_hex_len
                        && object_name.as_bytes().iter().all(u8::is_ascii_hexdigit)
                        && matches!(
                            extension,
                            "pack" | "idx" | "rev" | "bitmap" | "mtimes" | "keep" | "promisor"
                        )
                });
            if valid_pack {
                let extension = Path::new(filename)
                    .extension()
                    .and_then(|value| value.to_str())
                    .expect("validated pack extension");
                let category = format!("{object_prefix}/pack/pack.{extension}");
                let ordinal = category_counts.entry(category.clone()).or_default();
                normalized.insert(format!("{category}/{ordinal}"));
                *ordinal += 1;
            } else {
                normalized.insert(relative);
            }
        } else if components.len() == objects_index + 3
            && object_child.len() == 2
            && object_child.as_bytes().iter().all(u8::is_ascii_hexdigit)
            && components[objects_index + 2].len() == object_hex_len - 2
            && components[objects_index + 2]
                .as_bytes()
                .iter()
                .all(u8::is_ascii_hexdigit)
        {
            let category = format!("{object_prefix}/{object_child}/<loose>");
            let ordinal = category_counts.entry(category.clone()).or_default();
            normalized.insert(format!("{category}/{ordinal}"));
            *ordinal += 1;
        } else {
            normalized.insert(relative);
        }
    }
    Some(normalized)
}

fn read_tree_admin_file_roles(
    repo: &Path,
    admin_path: &str,
    object_hex_len: usize,
) -> Option<BTreeSet<String>> {
    read_tree_admin_file_set(repo, admin_path, object_hex_len).map(|files| {
        files
            .into_iter()
            .map(|file| {
                let components = file.split('/').collect::<Vec<_>>();
                if components.len() >= 4
                    && components[components.len() - 3].len() == 2
                    && components[components.len() - 3]
                        .as_bytes()
                        .iter()
                        .all(u8::is_ascii_hexdigit)
                    && components[components.len() - 2] == "<loose>"
                {
                    let prefix = components[..components.len() - 3].join("/");
                    format!("{prefix}/<loose>/{}", components[components.len() - 1])
                } else {
                    file
                }
            })
            .collect()
    })
}

fn assert_read_tree_admin_fanouts_are_real(repo: &Path, admin_path: &str, object_hex_len: usize) {
    let Some(files) = read_tree_admin_file_set(repo, admin_path, object_hex_len) else {
        return;
    };
    for file in files {
        let components = file.split('/').collect::<Vec<_>>();
        if components.len() >= 4 && components[components.len() - 2] == "<loose>" {
            let fanout = components[components.len() - 3];
            assert_eq!(fanout.len(), 2, "invalid loose fanout in {file}");
            assert!(
                fanout.as_bytes().iter().all(u8::is_ascii_hexdigit),
                "non-hex loose fanout in {file}"
            );
        }
    }
}

fn read_tree_admin_non_object_files(
    repo: &Path,
    admin_path: &str,
    object_hex_len: usize,
) -> Option<BTreeSet<String>> {
    read_tree_admin_file_set(repo, admin_path, object_hex_len).map(|files| {
        files
            .into_iter()
            .filter(|file| !file.split('/').any(|component| component == "objects"))
            .collect()
    })
}

fn read_tree_admin_non_object_contents(
    repo: &Path,
    admin_path: &str,
    object_hex_len: usize,
    temp_roots: &[&Path],
) -> Option<BTreeMap<String, Vec<u8>>> {
    let root = repo.join(".git").join(admin_path);
    if !root.is_dir() {
        return None;
    }

    fn visit(root: &Path, path: &Path, files: &mut Vec<PathBuf>, skip_nested_modules: bool) {
        for entry in fs::read_dir(path).expect("read read-tree admin content snapshot") {
            let entry = entry.expect("read read-tree admin content entry");
            let entry_path = entry.path();
            if entry_path.is_dir() {
                if !(skip_nested_modules && entry.file_name() == "modules") {
                    visit(root, &entry_path, files, skip_nested_modules);
                }
            } else if entry_path.is_file() {
                files.push(
                    entry_path
                        .strip_prefix(root)
                        .expect("admin relative path")
                        .to_path_buf(),
                );
            }
        }
    }

    let mut paths = Vec::new();
    visit(&root, &root, &mut paths, admin_path.is_empty());
    let mut contents = BTreeMap::new();
    for relative in paths {
        if relative
            .components()
            .any(|component| component.as_os_str() == "objects")
        {
            continue;
        }
        let relative_name = relative.to_string_lossy().into_owned();
        let value = fs::read(root.join(&relative)).expect("read admin content");
        let value = if relative_name == "config" || relative_name.ends_with("/config") {
            normalize_read_tree_config(Some(&value), temp_roots).expect("config snapshot")
        } else if relative_name == "index" || relative_name.ends_with("/index") {
            let relative_admin = relative_name.strip_suffix("/index").unwrap_or("");
            let admin_index = if admin_path.is_empty() {
                relative_admin.to_owned()
            } else if relative_admin.is_empty() {
                admin_path.to_owned()
            } else {
                format!("{admin_path}/{relative_admin}")
            };
            read_tree_admin_index_semantics(repo, &admin_index)
                .expect("semantic admin index snapshot")
                .into_bytes()
        } else if (relative_name == "HEAD" || relative_name.ends_with("/HEAD"))
            && !relative_name.starts_with("logs/")
            && !relative_name.contains("/logs/")
        {
            read_tree_head_semantics(&value).into_bytes()
        } else if relative_name == "packed-refs"
            || relative_name.ends_with("/packed-refs")
            || relative_name.starts_with("refs/")
            || relative_name.starts_with("logs/")
            || relative_name.contains("/refs/")
            || relative_name.contains("/logs/")
        {
            normalize_read_tree_admin_text(&value, object_hex_len, temp_roots)
        } else {
            value
        };
        contents.insert(relative_name, value);
    }
    Some(contents)
}

fn normalize_read_tree_admin_text(
    value: &[u8],
    object_hex_len: usize,
    temp_roots: &[&Path],
) -> Vec<u8> {
    let text = String::from_utf8_lossy(value);
    let text = normalize_read_tree_config_value(&text, temp_roots);
    let bytes = text.as_bytes();
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let timestamp_end = index.saturating_add(10);
        let is_reflog_timestamp = timestamp_end < bytes.len()
            && bytes[index..timestamp_end].iter().all(u8::is_ascii_digit)
            && bytes[timestamp_end] == b' '
            && timestamp_end + 6 <= bytes.len()
            && matches!(bytes[timestamp_end + 1], b'+' | b'-')
            && bytes[timestamp_end + 2..timestamp_end + 6]
                .iter()
                .all(u8::is_ascii_digit);
        if is_reflog_timestamp {
            normalized.extend_from_slice(b"<reflog-timestamp>");
            index = timestamp_end;
            continue;
        }
        let end = index.saturating_add(object_hex_len);
        let is_object_id = end <= bytes.len()
            && bytes[index..end].iter().all(u8::is_ascii_hexdigit)
            && (index == 0 || !bytes[index - 1].is_ascii_hexdigit())
            && (end == bytes.len() || !bytes[end].is_ascii_hexdigit());
        if is_object_id {
            normalized.extend_from_slice(format!("<object-id-{object_hex_len}>").as_bytes());
            index = end;
        } else {
            normalized.push(bytes[index]);
            index += 1;
        }
    }
    normalized
}

fn read_tree_admin_role_counts(files: &BTreeSet<String>) -> ReadTreeAdminRoleCounts {
    let mut counts = ReadTreeAdminRoleCounts::default();
    for file in files {
        if !file.split('/').any(|component| component == "objects") {
            continue;
        }
        counts.total += 1;
        let components = file.split('/').collect::<Vec<_>>();
        if components.iter().any(|component| *component == "<loose>") {
            counts.loose += 1;
            continue;
        }
        let Some(pack_index) = components.iter().position(|component| *component == "pack") else {
            counts.other_aux += 1;
            continue;
        };
        let role = components.get(pack_index + 1).copied().unwrap_or_default();
        match role {
            "pack.pack" => counts.pack_data += 1,
            "pack.idx" => counts.pack_index += 1,
            "pack.rev" => counts.pack_rev += 1,
            "pack.bitmap" => counts.pack_bitmap += 1,
            "pack.mtimes" => counts.pack_mtimes += 1,
            "pack.keep" => counts.pack_keep += 1,
            "pack.promisor" => counts.pack_promisor += 1,
            _ => counts.other_aux += 1,
        }
    }
    counts
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ReadTreeAdminLayoutDelta {
    role_added: BTreeSet<String>,
    role_removed: BTreeSet<String>,
    count_delta: BTreeMap<String, isize>,
    non_object_changed: BTreeSet<String>,
}

fn read_tree_admin_layout_snapshot(
    repo: &Path,
    admin_path: &str,
    object_hex_len: usize,
    temp_roots: &[&Path],
) -> Option<ReadTreeAdminLayoutSnapshot> {
    let roles = read_tree_admin_file_roles(repo, admin_path, object_hex_len)?;
    Some(ReadTreeAdminLayoutSnapshot {
        counts: read_tree_admin_role_counts(&roles),
        roles,
        non_object_contents: read_tree_admin_non_object_contents(
            repo,
            admin_path,
            object_hex_len,
            temp_roots,
        )?,
    })
}

fn read_tree_admin_layout_delta(
    before: Option<&ReadTreeAdminLayoutSnapshot>,
    after: Option<&ReadTreeAdminLayoutSnapshot>,
) -> ReadTreeAdminLayoutDelta {
    let empty = ReadTreeAdminLayoutSnapshot::default();
    let before = before.unwrap_or(&empty);
    let after = after.unwrap_or(&empty);
    let mut count_delta = BTreeMap::new();
    let before_counts = [
        ("total", before.counts.total),
        ("loose", before.counts.loose),
        ("pack_data", before.counts.pack_data),
        ("pack_index", before.counts.pack_index),
        ("pack_rev", before.counts.pack_rev),
        ("pack_bitmap", before.counts.pack_bitmap),
        ("pack_mtimes", before.counts.pack_mtimes),
        ("pack_keep", before.counts.pack_keep),
        ("pack_promisor", before.counts.pack_promisor),
        ("other_aux", before.counts.other_aux),
    ];
    let after_counts = [
        ("total", after.counts.total),
        ("loose", after.counts.loose),
        ("pack_data", after.counts.pack_data),
        ("pack_index", after.counts.pack_index),
        ("pack_rev", after.counts.pack_rev),
        ("pack_bitmap", after.counts.pack_bitmap),
        ("pack_mtimes", after.counts.pack_mtimes),
        ("pack_keep", after.counts.pack_keep),
        ("pack_promisor", after.counts.pack_promisor),
        ("other_aux", after.counts.other_aux),
    ];
    for ((name, before), (_, after)) in before_counts.into_iter().zip(after_counts) {
        let delta = after as isize - before as isize;
        if delta != 0 {
            count_delta.insert(name.to_owned(), delta);
        }
    }
    let role_added = after.roles.difference(&before.roles).cloned().collect();
    let role_removed = before.roles.difference(&after.roles).cloned().collect();
    let names = before
        .non_object_contents
        .keys()
        .chain(after.non_object_contents.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let non_object_changed = names
        .into_iter()
        .filter(|name| before.non_object_contents.get(name) != after.non_object_contents.get(name))
        .collect();
    ReadTreeAdminLayoutDelta {
        role_added,
        role_removed,
        count_delta,
        non_object_changed,
    }
}

fn assert_read_tree_admin_root_object_layout(
    repo: &Path,
    object_hex_len: usize,
    temp_roots: &[&Path],
    context: &str,
) {
    let Some(layout) = read_tree_admin_layout_snapshot(repo, "", object_hex_len, temp_roots) else {
        return;
    };
    assert!(
        repo.join(".git/objects").is_dir(),
        "{context}: root admin object directory missing"
    );
    assert!(
        layout.counts.total > 0,
        "{context}: root admin object layout is empty"
    );
    assert_eq!(
        layout.counts.total,
        layout.counts.loose
            + layout.counts.pack_data
            + layout.counts.pack_index
            + layout.counts.pack_rev
            + layout.counts.pack_bitmap
            + layout.counts.pack_mtimes
            + layout.counts.pack_keep
            + layout.counts.pack_promisor
            + layout.counts.other_aux,
        "{context}: root admin object cardinality accounting"
    );
}

fn read_tree_gitmodules_semantics(gitmodules: &Path, temp_roots: &[&Path]) -> Option<Vec<String>> {
    if !gitmodules.is_file() {
        return None;
    }
    let parent = gitmodules.parent().expect(".gitmodules parent");
    let name = gitmodules
        .file_name()
        .and_then(|name| name.to_str())
        .expect(".gitmodules filename");
    let output = raw_command_output(
        required_pinned_stock_git(),
        parent,
        &["config", "--file", name, "--null", "--list"],
        "pinned .gitmodules semantic listing",
    );
    assert_eq!(output.0, 0, "unable to parse {gitmodules:?}: {output:?}");
    let mut values = output
        .1
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| {
            let separator = record
                .iter()
                .position(|byte| *byte == b'\n')
                .expect("git config --null --list key/value record");
            let (key, value) = record.split_at(separator);
            let value = &value[1..];
            let key = String::from_utf8(key.to_vec()).expect(".gitmodules key utf8");
            let value = String::from_utf8(value.to_vec()).expect(".gitmodules value utf8");
            let value = if key.ends_with(".url") || key.ends_with(".path") {
                normalize_read_tree_config_value(&value, temp_roots)
            } else {
                value
            };
            format!("{key}={value}")
        })
        .collect::<Vec<_>>();
    values.sort();
    Some(values)
}

fn read_tree_admin_index_semantics(repo: &Path, admin_path: &str) -> Option<String> {
    let admin = repo.join(".git").join(admin_path);
    if !admin.join("index").is_file() {
        return None;
    }
    let admin = admin.to_str().expect("admin path utf8");
    let output = raw_command_output(
        required_pinned_stock_git(),
        repo,
        &["--git-dir", admin, "ls-files", "--stage"],
        "pinned admin index semantic listing",
    );
    assert_eq!(output.0, 0, "unable to list admin index: {output:?}");
    Some(normalize_read_tree_index_for_fixture(
        &String::from_utf8(output.1).expect("admin index listing utf8"),
    ))
}

fn read_tree_admin_head_semantics(repo: &Path, admin_path: &str) -> Option<String> {
    let head = repo.join(".git").join(admin_path).join("HEAD");
    head.is_file()
        .then(|| fs::read(head).expect("read admin HEAD"))
        .map(|value| read_tree_head_semantics(&value))
}

fn read_tree_head_semantics(value: &[u8]) -> String {
    let value = String::from_utf8_lossy(value).trim().to_owned();
    if value.starts_with("ref: ") {
        value
    } else if value.len() == 40 || value.len() == 64 {
        assert!(
            value.as_bytes().iter().all(u8::is_ascii_hexdigit),
            "detached HEAD is not an object id: {value}"
        );
        format!("detached-object-format-{}", value.len())
    } else {
        value
    }
}

fn read_tree_root_worktree_files(repo: &Path, temp_roots: &[&Path]) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    for entry in fs::read_dir(repo).expect("read root worktree snapshot") {
        let entry = entry.expect("read root worktree snapshot entry");
        let path = entry.path();
        if path.is_file() {
            let name = entry
                .file_name()
                .to_str()
                .expect("root worktree filename utf8")
                .to_owned();
            if name != ".git" {
                let content = if name == ".gitmodules" {
                    read_tree_gitmodules_semantics(&path, temp_roots)
                        .expect("root .gitmodules semantic snapshot")
                        .join("\n")
                        .into_bytes()
                } else {
                    fs::read(path).expect("read root worktree snapshot file")
                };
                files.push((name, content));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn assert_read_tree_module_semantics_equal(
    stock_repo: &Path,
    zmin_repo: &Path,
    module_path: &str,
    admin_path: &str,
    stock: &ReadTreeModuleSnapshot,
    zmin: &ReadTreeModuleSnapshot,
    temp_roots: &[&Path],
    object_hex_len: usize,
    context: &str,
) {
    assert_eq!(
        stock.worktree_exists, zmin.worktree_exists,
        "{context}: {module_path} worktree existence"
    );
    assert_eq!(
        read_tree_worktree_files_without_marker(stock),
        read_tree_worktree_files_without_marker(zmin),
        "{context}: {module_path} worktree files"
    );
    assert_eq!(
        stock.marker.is_some(),
        zmin.marker.is_some(),
        "{context}: {module_path} marker presence"
    );
    assert_eq!(
        read_tree_marker_semantics(stock_repo, module_path, admin_path, stock.marker.as_ref()),
        read_tree_marker_semantics(zmin_repo, module_path, admin_path, zmin.marker.as_ref()),
        "{context}: {module_path} marker semantics"
    );
    assert_eq!(
        stock.admin_exists, zmin.admin_exists,
        "{context}: {module_path} admin existence"
    );
    assert_eq!(
        normalize_read_tree_config(stock.admin_config.as_ref(), temp_roots),
        normalize_read_tree_config(zmin.admin_config.as_ref(), temp_roots),
        "{context}: {module_path} config semantics"
    );
    assert_eq!(
        read_tree_core_worktree_semantics(
            stock_repo,
            module_path,
            admin_path,
            stock.admin_config.as_ref(),
        ),
        read_tree_core_worktree_semantics(
            zmin_repo,
            module_path,
            admin_path,
            zmin.admin_config.as_ref(),
        ),
        "{context}: {module_path} resolved core.worktree"
    );
    assert_read_tree_admin_fanouts_are_real(stock_repo, admin_path, object_hex_len);
    assert_read_tree_admin_fanouts_are_real(zmin_repo, admin_path, object_hex_len);
    assert_eq!(
        read_tree_admin_index_semantics(stock_repo, admin_path),
        read_tree_admin_index_semantics(zmin_repo, admin_path),
        "{context}: {module_path} admin index semantics"
    );
    assert_eq!(
        read_tree_admin_head_semantics(stock_repo, admin_path),
        read_tree_admin_head_semantics(zmin_repo, admin_path),
        "{context}: {module_path} admin HEAD semantics"
    );
}

fn assert_read_tree_nested_semantics_equal(
    stock_repo: &Path,
    zmin_repo: &Path,
    fixture_roots: &ReadTreeFixtureRoots,
    context: &str,
) {
    let temp_roots = fixture_roots.temp_root_refs();
    assert_eq!(
        read_tree_root_worktree_files(stock_repo, &temp_roots),
        read_tree_root_worktree_files(zmin_repo, &temp_roots),
        "{context}: root worktree files"
    );
    assert_eq!(
        read_tree_head_semantics(
            &fs::read(stock_repo.join(".git/HEAD")).expect("read stock root HEAD"),
        ),
        read_tree_head_semantics(
            &fs::read(zmin_repo.join(".git/HEAD")).expect("read zmin root HEAD"),
        ),
        "{context}: root HEAD semantics"
    );
    assert_eq!(
        read_tree_admin_index_semantics(stock_repo, ""),
        read_tree_admin_index_semantics(zmin_repo, ""),
        "{context}: root index semantics"
    );
    let stock_root_after =
        read_tree_admin_layout_snapshot(stock_repo, "", fixture_roots.object_hex_len, &temp_roots);
    let zmin_root_after =
        read_tree_admin_layout_snapshot(zmin_repo, "", fixture_roots.object_hex_len, &temp_roots);
    let stock_root_before = fixture_roots.stock_root_admin_before.as_ref();
    let zmin_root_before = fixture_roots.zmin_root_admin_before.as_ref();
    assert_eq!(
        stock_root_before.map(|layout| &layout.non_object_contents),
        zmin_root_before.map(|layout| &layout.non_object_contents),
        "{context}: root admin initial non-object contents"
    );
    assert_eq!(
        stock_root_after
            .as_ref()
            .map(|layout| &layout.non_object_contents),
        zmin_root_after
            .as_ref()
            .map(|layout| &layout.non_object_contents),
        "{context}: root admin final non-object contents"
    );
    assert_eq!(
        stock_root_after.as_ref().map(|layout| &layout.roles),
        zmin_root_after.as_ref().map(|layout| &layout.roles),
        "{context}: root admin final roles"
    );
    assert_eq!(
        stock_root_after.as_ref().map(|layout| &layout.counts),
        zmin_root_after.as_ref().map(|layout| &layout.counts),
        "{context}: root admin final role counts"
    );
    assert_read_tree_admin_fanouts_are_real(stock_repo, "", fixture_roots.object_hex_len);
    assert_read_tree_admin_fanouts_are_real(zmin_repo, "", fixture_roots.object_hex_len);
    assert_read_tree_admin_root_object_layout(
        stock_repo,
        fixture_roots.object_hex_len,
        &temp_roots,
        context,
    );
    assert_read_tree_admin_root_object_layout(
        zmin_repo,
        fixture_roots.object_hex_len,
        &temp_roots,
        context,
    );
    assert_eq!(
        read_tree_admin_layout_delta(
            fixture_roots.stock_root_admin_before.as_ref(),
            stock_root_after.as_ref(),
        ),
        read_tree_admin_layout_delta(
            fixture_roots.zmin_root_admin_before.as_ref(),
            zmin_root_after.as_ref(),
        ),
        "{context}: root admin role delta"
    );
    assert_eq!(
        read_tree_admin_non_object_files(stock_repo, "", fixture_roots.object_hex_len),
        read_tree_admin_non_object_files(zmin_repo, "", fixture_roots.object_hex_len),
        "{context}: root admin non-object file roles"
    );
    assert_eq!(
        normalize_read_tree_config(
            snapshot_read_tree_nested(stock_repo).root_config.as_ref(),
            &temp_roots,
        ),
        normalize_read_tree_config(
            snapshot_read_tree_nested(zmin_repo).root_config.as_ref(),
            &temp_roots,
        ),
        "{context}: root config semantics"
    );
    assert_eq!(
        read_tree_gitmodules_semantics(&stock_repo.join(".gitmodules"), &temp_roots),
        read_tree_gitmodules_semantics(&zmin_repo.join(".gitmodules"), &temp_roots),
        "{context}: root .gitmodules semantics"
    );
    let stock = snapshot_read_tree_nested(stock_repo);
    let zmin = snapshot_read_tree_nested(zmin_repo);
    assert_read_tree_module_semantics_equal(
        stock_repo,
        zmin_repo,
        "child",
        "modules/child",
        &stock.child,
        &zmin.child,
        &temp_roots,
        fixture_roots.object_hex_len,
        context,
    );
    assert_read_tree_module_semantics_equal(
        stock_repo,
        zmin_repo,
        "child/nested",
        "modules/child/modules/nested",
        &stock.nested,
        &zmin.nested,
        &temp_roots,
        fixture_roots.object_hex_len,
        context,
    );
    for module_path in ["child", "child/nested"] {
        assert_eq!(
            read_tree_gitmodules_semantics(
                &stock_repo.join(module_path).join(".gitmodules"),
                &temp_roots,
            ),
            read_tree_gitmodules_semantics(
                &zmin_repo.join(module_path).join(".gitmodules"),
                &temp_roots,
            ),
            "{context}: {module_path} .gitmodules semantics"
        );
    }
}

fn assert_read_tree_failure_admin_shape(
    repo: &Path,
    module_path: &str,
    module_name: &str,
    admin_present: bool,
) {
    let worktree = repo.join(module_path);
    let admin = repo.join(".git/modules").join(module_name);
    if !admin_present {
        assert!(!admin.exists(), "unexpected failure admin directory");
        return;
    }
    let mut entries = fs::read_dir(&admin)
        .expect("read failure admin directory")
        .map(|entry| {
            entry
                .expect("read failure admin entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(entries, vec!["config".to_owned()]);
    assert!(!admin.join("objects").exists());
    assert!(!admin.join("HEAD").exists());
    assert!(!admin.join("index").exists());
    let marker =
        fs::read_to_string(worktree.join(".git")).expect("read failure submodule git marker");
    let marker_target = marker
        .strip_prefix("gitdir:")
        .expect("failure marker prefix")
        .trim();
    assert_eq!(
        fs::canonicalize(worktree.join(marker_target)).expect("resolve failure marker"),
        fs::canonicalize(&admin).expect("resolve failure admin")
    );
    let config = fs::read_to_string(admin.join("config")).expect("read failure admin config");
    let worktree_value = config
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("worktree = "))
        .expect("failure admin core.worktree");
    assert_eq!(
        fs::canonicalize(admin.join(worktree_value)).expect("resolve failure core.worktree"),
        fs::canonicalize(worktree).expect("resolve failure worktree")
    );
}

fn assert_read_tree_nested_snapshot_unchanged(
    actual: &ReadTreeNestedSnapshot,
    expected: &ReadTreeNestedSnapshot,
    context: &str,
) {
    assert!(
        actual.root_index == expected.root_index,
        "{context}: root index"
    );
    assert!(
        actual.root_config == expected.root_config,
        "{context}: root config"
    );
    assert_read_tree_module_snapshot_unchanged(&actual.child, &expected.child, context, "child");
    assert_read_tree_module_snapshot_unchanged(&actual.nested, &expected.nested, context, "nested");
}

fn assert_read_tree_module_snapshot_unchanged(
    actual: &ReadTreeModuleSnapshot,
    expected: &ReadTreeModuleSnapshot,
    context: &str,
    module: &str,
) {
    assert!(
        actual.worktree_exists == expected.worktree_exists,
        "{context} {module}: worktree existence"
    );
    assert!(
        actual.worktree_files == expected.worktree_files,
        "{context} {module}: worktree files"
    );
    assert!(
        actual.marker == expected.marker,
        "{context} {module}: marker"
    );
    assert!(
        actual.admin_exists == expected.admin_exists,
        "{context} {module}: admin existence"
    );
    assert!(
        actual.admin_index == expected.admin_index,
        "{context} {module}: admin index"
    );
    assert!(
        actual.admin_head == expected.admin_head,
        "{context} {module}: admin HEAD"
    );
    assert!(
        actual.admin_config == expected.admin_config,
        "{context} {module}: admin config"
    );
}

fn assert_read_tree_connected_worktree_state(repo: &Path, module_path: &str, module_name: &str) {
    let worktree = repo.join(module_path);
    let admin = repo.join(".git/modules").join(module_name);
    let marker = fs::read_to_string(worktree.join(".git")).expect("read submodule git marker");
    let marker_target = marker
        .strip_prefix("gitdir:")
        .expect("submodule marker prefix")
        .trim();
    assert_eq!(
        fs::canonicalize(worktree.join(marker_target)).expect("resolve submodule marker"),
        fs::canonicalize(&admin).expect("resolve submodule admin"),
        "submodule marker target diverged"
    );
    let config = fs::read_to_string(admin.join("config")).expect("read submodule admin config");
    let worktree_value = config
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("worktree = "))
        .expect("submodule admin core.worktree");
    assert_eq!(
        fs::canonicalize(admin.join(worktree_value)).expect("resolve core.worktree"),
        fs::canonicalize(worktree).expect("resolve submodule worktree"),
        "submodule core.worktree target diverged"
    );
}

fn assert_read_tree_removal_validation_is_iterative() {
    let source = include_str!("../src/runtime/submodule.rs");
    let start = source
        .find("fn validate_read_tree_submodule_removal_worktree(")
        .expect("iterative read-tree removal validator");
    let end = source[start..]
        .find("pub(crate) fn checkout_read_tree_submodules(")
        .map(|offset| start + offset)
        .expect("read-tree removal validator end");
    let body = &source[start..end];
    assert!(body.contains("while let Some(child) = pending.pop()"));
    assert!(body.contains("drop(index)"));
    assert!(body.contains("drop(child_index)"));
    assert_eq!(
        body.matches("validate_read_tree_submodule_removal_worktree(")
            .count(),
        1,
        "removal validator must not recurse with full indexes"
    );
}

#[test]
fn read_tree_worktree_plan_and_indexed_submodule_invariants() {
    let worktree = include_str!("../src/cli/commands/worktree_impl.rs");
    let plan_start = worktree
        .find("struct ReadTreeWorktreePlan")
        .expect("read-tree immutable worktree plan");
    let apply_start = worktree
        .find("fn read_tree_update_worktree(")
        .expect("read-tree worktree apply");
    assert!(plan_start < apply_start);
    let plan_source = &worktree[plan_start..apply_start];
    assert!(plan_source.contains("ReadTreeWorktreeSnapshot"));
    assert!(plan_source.contains("ReadTreeBytePathIndex"));
    assert!(plan_source.contains("OnceLock"));
    assert!(plan_source.contains("read_tree_entry_needs_checkout"));
    assert!(plan_source.contains("read_tree_preflight_checkout_path"));

    let apply_end = worktree[apply_start..]
        .find("fn read_tree_same_materialized_entry")
        .map(|offset| apply_start + offset)
        .expect("read-tree apply end");
    let apply_source = &worktree[apply_start..apply_end];
    for forbidden in [
        "read_tree_entry_needs_checkout(",
        "read_tree_preflight_checkout_path(",
        "tracked_path_set_for_repo(",
        "GitIgnore::load_from_root",
        "untracked_files(",
        "ignored_untracked_files(",
    ] {
        assert!(
            !apply_source.contains(forbidden),
            "read-tree apply must not re-plan or rescan: {forbidden}"
        );
    }

    let submodule = include_str!("../src/runtime/submodule.rs");
    assert!(submodule.contains("struct ReadTreeSubmodulePathIndex"));
    assert!(submodule.contains("fn module_for_path(&self, path: &[u8])"));
    assert!(!submodule.contains("frame.modules.iter().find"));
    assert!(!submodule.contains("modules.iter().find"));
    let validate_start = submodule
        .find("pub(crate) fn validate_read_tree_submodule_worktrees(")
        .expect("read-tree submodule worktree validation");
    let validate_end = submodule[validate_start..]
        .find("fn validate_read_tree_submodule_worktrees_with_context")
        .map(|offset| validate_start + offset)
        .expect("read-tree submodule validation context helper");
    let validate_source = &submodule[validate_start..validate_end];
    assert!(validate_source.contains("if !recurse || force"));
    assert!(!validate_source.contains("ReadTreeSubmoduleFrameContext::read("));
    let target_validate_start = submodule
        .find("pub(crate) fn validate_read_tree_submodule_targets(")
        .expect("read-tree target validation");
    let target_validate_end = submodule[target_validate_start..]
        .find("fn read_tree_submodule_validation_targets(")
        .map(|offset| target_validate_start + offset)
        .expect("read-tree target validation end");
    let target_validate_source = &submodule[target_validate_start..target_validate_end];
    assert!(target_validate_source.contains("Some(root_operation)"));
    assert!(!target_validate_source.contains("targets.into_iter().filter"));
    let collect_start = submodule
        .find("fn read_tree_submodule_validation_targets(")
        .expect("read-tree target collector");
    let collect_end = submodule[collect_start..]
        .find("fn validate_read_tree_submodule_target_list(")
        .map(|offset| collect_start + offset)
        .expect("read-tree target collector end");
    let collect_source = &submodule[collect_start..collect_end];
    assert!(collect_source.contains("root_operation.is_some_and"));
    assert!(collect_source.contains("read_tree_submodule_worktree_state"));
    let target_list_start = submodule
        .find("fn validate_read_tree_submodule_target_list(")
        .expect("read-tree target validation list");
    let target_list_end = submodule[target_list_start..]
        .find("fn read_read_tree_submodule_target_index(")
        .map(|offset| target_list_start + offset)
        .expect("read-tree target validation list end");
    let target_list_source = &submodule[target_list_start..target_list_end];
    assert!(!target_list_source.contains("read_tree_submodule_worktree_state"));
    assert!(!target_list_source.contains("admin_dir.exists()"));
    assert!(submodule.contains("root_operation.nested_operation()"));
    assert!(submodule.contains("Self::OneWayMerge | Self::SingleTree => Self::Merge"));
    assert!(submodule.contains("let nested_operation = root_operation.nested_operation();"));
    assert!(submodule.contains("nested_operation,\n            &child_frame,\n            nested_operation.unpopulated_policy(),"));
    assert!(submodule.contains("root_operation_origin: ReadTreeSubmoduleRootOperation"));
    assert!(submodule.contains("root_operation_origin,"));
    assert!(submodule.contains("struct ReadTreeSubmoduleRootContexts"));
    assert!(submodule.contains("struct ReadTreeSubmoduleRemovalPathArena"));
    assert!(submodule.contains("path_id: ReadTreeSubmoduleRemovalPathId"));
    assert!(!submodule.contains("ignored.iter().any"));
    assert!(!submodule.contains("untracked.iter().any"));
    assert!(!submodule.contains("String::from_utf8_lossy(candidate)"));

    let command = worktree
        .find("pub(crate) fn read_tree_command(")
        .expect("read-tree command");
    let command_end = worktree[command..]
        .find("fn reject_null_read_tree_entries")
        .map(|offset| command + offset)
        .expect("read-tree command end");
    let command_source = &worktree[command..command_end];
    assert!(command_source.contains("ReadTreeSubmoduleRootContexts::read"));
    assert!(command_source.contains("let root_submodule_contexts = if recurse_submodules"));
    assert!(command_source.contains("let include_original_submodule_context"));
    assert!(command_source.contains("!reset || (update_worktree && !dry_run)"));
    assert!(command_source.contains("merge && treeish.len() > 1"));
    assert!(command_source.contains("ReadTreeSubmoduleRootOperation::OneWayMerge"));
    assert!(!command_source.contains("if recurse_submodules || update_worktree"));
    assert!(command_source.matches("super_prefix.as_deref()").count() >= 4);
    assert!(command_source.contains("if !dry_run"));
    assert!(command_source.contains("prefetch_checkout_index_missing_objects"));
    assert!(command_source.contains("read_tree_prefetch_missing_objects"));
    assert!(submodule.contains("preserves_unpopulated_worktrees"));
    assert!(submodule.contains("matches!(self, Self::Merge)"));
    assert!(!submodule.contains("Self::Merge | Self::OneWayMerge"));
}

#[test]
fn read_tree_no_recurse_skips_malformed_activation_sha1_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let setup = |repo: &Path| {
            configure_identity(repo);
            fs::write(
                repo.join(".gitmodules"),
                "[submodule \"sub\"]\n\tpath = sub\n\turl = file:///unused\n",
            )
            .expect("write malformed activation gitmodules");
            git(repo, ["add", ".gitmodules"]);
            git_with_env(repo, ["commit", "-m", "malformed activation base"]);
            let base = git(repo, ["rev-parse", "HEAD"]).trim().to_owned();
            let module_id = if sha256 {
                "1111111111111111111111111111111111111111111111111111111111111111"
            } else {
                "1111111111111111111111111111111111111111"
            };
            git(
                repo,
                [
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("160000,{module_id},sub"),
                ],
            );
            git_with_env(repo, ["commit", "-m", "malformed activation"]);
            let target = git(repo, ["rev-parse", "HEAD"]).trim().to_owned();
            git(repo, ["read-tree", "--reset", &base]);
            git(repo, ["config", "submodule.sub.active", ":(unterminated"]);
            (base, target)
        };
        let stock_repo = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        let zmin_repo = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        let (stock_base, stock_target) = setup(stock_repo.path());
        let (zmin_base, zmin_target) = setup(zmin_repo.path());
        assert_eq!(
            (&stock_base, &stock_target),
            (&zmin_base, &zmin_target),
            "sha256={sha256}"
        );
        let no_recurse_args = [
            "read-tree",
            "-u",
            "-m",
            "--no-recurse-submodules",
            stock_base.as_str(),
            stock_target.as_str(),
        ];
        let stock_no_recurse = raw_command_output(
            stock_git.as_path(),
            stock_repo.path(),
            &no_recurse_args,
            "pinned no-recurse malformed activation",
        );
        let zmin_no_recurse = raw_command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "read-tree",
                "-u",
                "-m",
                "--no-recurse-submodules",
                zmin_base.as_str(),
                zmin_target.as_str(),
            ],
            "zmin no-recurse malformed activation",
        );
        assert_eq!(zmin_no_recurse, stock_no_recurse, "sha256={sha256}");
        assert_eq!(stock_no_recurse.0, 0, "sha256={sha256}");
        assert!(stock_no_recurse.1.is_empty(), "sha256={sha256}");
        assert!(stock_no_recurse.2.is_empty(), "sha256={sha256}");
        assert!(git(stock_repo.path(), ["ls-files", "--stage"]).contains("\tsub"));
        assert!(git(zmin_repo.path(), ["ls-files", "--stage"]).contains("\tsub"));
        assert_eq!(
            stock_repo.path().join("sub").exists(),
            zmin_repo.path().join("sub").exists(),
            "no-recurse worktree state diverged, sha256={sha256}"
        );
        assert!(
            !stock_repo.path().join("sub/.git").exists(),
            "sha256={sha256}"
        );
        assert!(
            !zmin_repo.path().join("sub/.git").exists(),
            "sha256={sha256}"
        );

        let recursive = |repo: &Path, program: &Path, target: &str| {
            git(repo, ["read-tree", "--reset", target]);
            raw_command_output(
                program,
                repo,
                &["read-tree", "-u", "-m", "--recurse-submodules", target],
                "recursive malformed activation control",
            )
        };
        let stock_recursive = recursive(stock_repo.path(), stock_git.as_path(), &stock_target);
        let zmin_recursive = recursive(zmin_repo.path(), Path::new(zmin_bin()), &zmin_target);
        assert_eq!(zmin_recursive, stock_recursive, "sha256={sha256}");
        assert_ne!(
            stock_recursive.0, 0,
            "malformed activation was not exercised"
        );
        assert!(!stock_recursive.2.is_empty(), "sha256={sha256}");
    }
}

#[test]
fn read_tree_recursive_super_prefix_matches_pinned_dirty_transition_sha1_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let fixture = nested_read_tree_fixture(sha256);
        for (program, repo, label) in [
            (
                stock_git.as_path(),
                fixture.stock_super.path(),
                "pinned prefix setup",
            ),
            (
                Path::new(zmin_bin()),
                fixture.zmin_super.path(),
                "zmin prefix setup",
            ),
        ] {
            let _ = (program, label);
            fs::write(repo.join("child/nested/inner.txt"), b"prefix dirty\n")
                .expect("write nested prefix dirty file");
            git(&repo.join("child/nested"), ["add", "inner.txt"]);
        }
        let stock_before = snapshot_read_tree_nested(fixture.stock_super.path());
        let zmin_before = snapshot_read_tree_nested(fixture.zmin_super.path());
        let stock_args = [
            "read-tree",
            "--super-prefix",
            "fictional/",
            "-u",
            "-m",
            "--recurse-submodules",
            "HEAD",
        ];
        let zmin_args = stock_args;
        let stock_output = raw_command_output(
            stock_git.as_path(),
            fixture.stock_super.path(),
            &stock_args,
            "pinned recursive super-prefix dirty transition",
        );
        let zmin_output = raw_command_output(
            zmin_bin(),
            fixture.zmin_super.path(),
            &zmin_args,
            "zmin recursive super-prefix dirty transition",
        );
        assert_eq!(zmin_output, stock_output, "sha256={sha256}");
        assert_ne!(
            stock_output.0, 0,
            "dirty recursive transition was not exercised"
        );
        assert!(
            stock_output
                .2
                .windows(b"fictional/".len())
                .any(|window| window == b"fictional/"),
            "super-prefix did not reach recursive diagnostic: {:?}",
            String::from_utf8_lossy(&stock_output.2)
        );
        assert_read_tree_nested_snapshot_unchanged(
            &snapshot_read_tree_nested(fixture.stock_super.path()),
            &stock_before,
            "pinned recursive super-prefix failure",
        );
        assert_read_tree_nested_snapshot_unchanged(
            &snapshot_read_tree_nested(fixture.zmin_super.path()),
            &zmin_before,
            "zmin recursive super-prefix failure",
        );
    }
}

#[test]
fn read_tree_recursive_super_prefix_stale_marker_matches_pinned_sha1_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let fixture = nested_read_tree_fixture(sha256);
        for repo in [fixture.stock_super.path(), fixture.zmin_super.path()] {
            fs::remove_dir_all(repo.join("child/nested"))
                .expect("remove stale-prefix nested worktree");
            fs::remove_dir_all(repo.join(".git/modules/child/modules/nested"))
                .expect("remove stale-prefix nested admin");
            fs::create_dir_all(repo.join("child/nested"))
                .expect("create stale-prefix nested worktree");
            fs::write(
                repo.join("child/nested/.git"),
                b"gitdir: ../.git/modules/nested\n",
            )
            .expect("write stale-prefix nested marker");
        }
        let stock_before = snapshot_read_tree_nested(fixture.stock_super.path());
        let zmin_before = snapshot_read_tree_nested(fixture.zmin_super.path());
        let args = [
            "read-tree",
            "--super-prefix",
            "fictional/",
            "-u",
            "-m",
            "--recurse-submodules",
            "HEAD^^",
            "HEAD^",
        ];
        let stock = raw_command_output(
            stock_git.as_path(),
            fixture.stock_super.path(),
            &args,
            "pinned stale-prefix nested marker",
        );
        let zmin = raw_command_output(
            zmin_bin(),
            fixture.zmin_super.path(),
            &args,
            "zmin stale-prefix nested marker",
        );
        assert_eq!(zmin, stock, "sha256={sha256}");
        assert_eq!(
            stock,
            (
                128,
                Vec::new(),
                b"fatal: not a git repository: nested/../.git/modules/nested\n\
error: Submodule 'child' could not be updated.\n\
error: Submodule 'fictional/child' cannot checkout new HEAD.\n"
                    .to_vec(),
            ),
            "sha256={sha256}"
        );
        assert_read_tree_nested_snapshot_unchanged(
            &snapshot_read_tree_nested(fixture.stock_super.path()),
            &stock_before,
            "pinned stale-prefix nested marker",
        );
        assert_read_tree_nested_snapshot_unchanged(
            &snapshot_read_tree_nested(fixture.zmin_super.path()),
            &zmin_before,
            "zmin stale-prefix nested marker",
        );
    }
}

#[test]
fn read_tree_unchanged_empty_nested_gitlink_lifecycle_matches_pinned_sha1_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        for admin_only in [false, true] {
            for reset in [false, true] {
                let fixture = nested_read_tree_fixture(sha256);
                for repo in [fixture.stock_super.path(), fixture.zmin_super.path()] {
                    fs::remove_dir_all(repo.join("child/nested"))
                        .expect("remove unchanged empty nested worktree");
                    if !admin_only {
                        fs::remove_dir_all(repo.join(".git/modules/child/modules/nested"))
                            .expect("remove unchanged empty nested admin");
                    }
                    fs::create_dir_all(repo.join("child/nested"))
                        .expect("create unchanged empty nested worktree");
                }
                let stock_before = snapshot_read_tree_nested(fixture.stock_super.path());
                let zmin_before = snapshot_read_tree_nested(fixture.zmin_super.path());
                let stock_args = if reset {
                    vec![
                        "read-tree",
                        "--reset",
                        "-u",
                        "--recurse-submodules",
                        "HEAD^^",
                    ]
                } else {
                    vec![
                        "read-tree",
                        "-u",
                        "-m",
                        "--recurse-submodules",
                        "HEAD^^",
                        "HEAD^^",
                    ]
                };
                let zmin_args = stock_args.clone();
                let stock = raw_command_output(
                    stock_git.as_path(),
                    fixture.stock_super.path(),
                    &stock_args,
                    "pinned unchanged empty nested lifecycle",
                );
                let zmin = raw_command_output(
                    zmin_bin(),
                    fixture.zmin_super.path(),
                    &zmin_args,
                    "zmin unchanged empty nested lifecycle",
                );
                assert_eq!(
                    zmin, stock,
                    "sha256={sha256}, admin={admin_only}, reset={reset}"
                );
                assert_eq!(
                    stock.0,
                    if reset && !admin_only { 128 } else { 0 },
                    "sha256={sha256}, admin={admin_only}, reset={reset}"
                );
                assert_read_tree_nested_semantics_equal(
                    fixture.stock_super.path(),
                    fixture.zmin_super.path(),
                    &fixture.roots,
                    &format!(
                        "unchanged empty nested lifecycle sha256={sha256} admin={admin_only} reset={reset}"
                    ),
                );
                let stock_nested = snapshot_read_tree_module(
                    fixture.stock_super.path(),
                    "child/nested",
                    "modules/child/modules/nested",
                );
                let zmin_nested = snapshot_read_tree_module(
                    fixture.zmin_super.path(),
                    "child/nested",
                    "modules/child/modules/nested",
                );
                assert!(stock_nested.worktree_exists);
                assert!(zmin_nested.worktree_exists);
                assert_eq!(stock_nested.marker.is_some(), reset);
                assert_eq!(zmin_nested.marker.is_some(), reset);
                assert_eq!(stock_nested.admin_exists, admin_only || reset);
                assert_eq!(zmin_nested.admin_exists, admin_only || reset);
                assert_eq!(
                    read_tree_worktree_files_without_marker(&stock_before.nested),
                    read_tree_worktree_files_without_marker(&zmin_before.nested),
                );
            }
        }
    }
}

#[test]
fn read_tree_one_way_merge_submodule_lifecycle_matches_pinned_sha1_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        for admin_only in [false, true] {
            let fixture = nested_read_tree_fixture(sha256);
            for repo in [fixture.stock_super.path(), fixture.zmin_super.path()] {
                fs::remove_dir_all(repo.join("child/nested"))
                    .expect("remove unchanged one-way nested worktree");
                if !admin_only {
                    fs::remove_dir_all(repo.join(".git/modules/child/modules/nested"))
                        .expect("remove unchanged one-way nested admin");
                }
            }
            let args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD^^"];
            let stock = raw_command_output(
                stock_git.as_path(),
                fixture.stock_super.path(),
                &args,
                "pinned one-way unchanged nested gitlink",
            );
            let zmin = raw_command_output(
                zmin_bin(),
                fixture.zmin_super.path(),
                &args,
                "zmin one-way unchanged nested gitlink",
            );
            assert_eq!(zmin, stock, "sha256={sha256}, admin={admin_only}");
            assert_read_tree_nested_semantics_equal(
                fixture.stock_super.path(),
                fixture.zmin_super.path(),
                &fixture.roots,
                &format!("one-way unchanged nested sha256={sha256} admin={admin_only}"),
            );
        }

        let changed = nested_read_tree_fixture(sha256);
        for repo in [changed.stock_super.path(), changed.zmin_super.path()] {
            fs::remove_dir_all(repo.join("child")).expect("remove changed one-way child worktree");
        }
        let changed_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD^"];
        let stock_changed = raw_command_output(
            stock_git.as_path(),
            changed.stock_super.path(),
            &changed_args,
            "pinned one-way changed nested gitlink",
        );
        let zmin_changed = raw_command_output(
            zmin_bin(),
            changed.zmin_super.path(),
            &changed_args,
            "zmin one-way changed nested gitlink",
        );
        assert_eq!(zmin_changed, stock_changed, "sha256={sha256}");
        assert_eq!(stock_changed.0, 0, "sha256={sha256}");
        assert_read_tree_nested_semantics_equal(
            changed.stock_super.path(),
            changed.zmin_super.path(),
            &changed.roots,
            &format!("one-way changed nested sha256={sha256}"),
        );
        let stock_child_exists = changed.stock_super.path().join("child").exists();
        let zmin_child_exists = changed.zmin_super.path().join("child").exists();
        assert_eq!(stock_child_exists, zmin_child_exists, "sha256={sha256}");
        for (repo, middle) in [
            (changed.stock_super.path(), changed._stock_middle.path()),
            (changed.zmin_super.path(), changed._zmin_middle.path()),
        ] {
            let target = git(middle, ["rev-parse", "HEAD"]).trim().to_owned();
            assert!(git(repo, ["ls-files", "--stage", "child"]).contains(&target));
            assert!(repo.join(".git/modules/child").exists());
        }

        let changed_nested = nested_read_tree_fixture(sha256);
        for repo in [
            changed_nested.stock_super.path(),
            changed_nested.zmin_super.path(),
        ] {
            fs::remove_dir_all(repo.join("child/nested")).expect("remove changed nested worktree");
            fs::remove_dir_all(repo.join(".git/modules/child/modules/nested"))
                .expect("remove changed nested admin");
        }
        let changed_nested_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD^"];
        let stock_changed_nested = raw_command_output(
            stock_git.as_path(),
            changed_nested.stock_super.path(),
            &changed_nested_args,
            "pinned one-way changed nested gitlink",
        );
        let zmin_changed_nested = raw_command_output(
            zmin_bin(),
            changed_nested.zmin_super.path(),
            &changed_nested_args,
            "zmin one-way changed nested gitlink",
        );
        assert_eq!(zmin_changed_nested, stock_changed_nested, "sha256={sha256}");
        assert_read_tree_nested_semantics_equal(
            changed_nested.stock_super.path(),
            changed_nested.zmin_super.path(),
            &changed_nested.roots,
            &format!("one-way changed nested sha256={sha256}"),
        );

        let one_way = nested_read_tree_fixture(sha256);
        let plain = nested_read_tree_fixture(sha256);
        for repo in [one_way.stock_super.path(), one_way.zmin_super.path()] {
            fs::remove_dir_all(repo.join("child/nested"))
                .expect("remove one-way control nested worktree");
            fs::remove_dir_all(repo.join(".git/modules/child/modules/nested"))
                .expect("remove one-way control nested admin");
        }
        for repo in [plain.stock_super.path(), plain.zmin_super.path()] {
            fs::remove_dir_all(repo.join("child/nested"))
                .expect("remove plain one-tree nested worktree");
            fs::remove_dir_all(repo.join(".git/modules/child/modules/nested"))
                .expect("remove plain one-tree nested admin");
        }
        let one_way_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD^"];
        let plain_args = ["read-tree", "-u", "--recurse-submodules", "HEAD^"];
        let stock_one_way = raw_command_output(
            stock_git.as_path(),
            one_way.stock_super.path(),
            &one_way_args,
            "pinned one-way lifecycle control",
        );
        let zmin_one_way = raw_command_output(
            zmin_bin(),
            one_way.zmin_super.path(),
            &one_way_args,
            "zmin one-way lifecycle control",
        );
        let stock_plain = raw_command_output(
            stock_git.as_path(),
            plain.stock_super.path(),
            &plain_args,
            "pinned plain one-tree lifecycle control",
        );
        let zmin_plain = raw_command_output(
            zmin_bin(),
            plain.zmin_super.path(),
            &plain_args,
            "zmin plain one-tree lifecycle control",
        );
        assert_eq!(zmin_one_way, stock_one_way, "sha256={sha256}, one-way");
        assert_eq!(zmin_plain, stock_plain, "sha256={sha256}, plain");
        assert_ne!(stock_one_way, stock_plain, "sha256={sha256}");
        assert_read_tree_nested_semantics_equal(
            one_way.stock_super.path(),
            one_way.zmin_super.path(),
            &one_way.roots,
            &format!("one-way control sha256={sha256}"),
        );
        assert_read_tree_nested_semantics_equal(
            plain.stock_super.path(),
            plain.zmin_super.path(),
            &plain.roots,
            &format!("plain one-tree control sha256={sha256}"),
        );
    }
}

#[test]
fn read_tree_ignored_directory_gitlink_matches_pinned_sha1_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        for reset in [false, true] {
            let setup = |repo: &Path| {
                configure_identity(repo);
                fs::write(repo.join(".gitignore"), b"ignored/\n").expect("write ignored rule");
                git(repo, ["add", ".gitignore"]);
                git_with_env(repo, ["commit", "-m", "ignored base"]);
                let base = git(repo, ["rev-parse", "HEAD"]).trim().to_owned();
                let module_id = if sha256 {
                    "2222222222222222222222222222222222222222222222222222222222222222"
                } else {
                    "2222222222222222222222222222222222222222"
                };
                git(
                    repo,
                    [
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        &format!("160000,{module_id},ignored/sub"),
                    ],
                );
                git_with_env(repo, ["commit", "-m", "ignored gitlink"]);
                let target = git(repo, ["rev-parse", "HEAD"]).trim().to_owned();
                git(repo, ["read-tree", "--reset", &base]);
                fs::create_dir_all(repo.join("ignored")).expect("create ignored directory");
                fs::write(repo.join("ignored/keep.txt"), b"keep\n").expect("write ignored file");
                (base, target)
            };
            let stock_repo = if sha256 {
                pinned_git_init_sha256()
            } else {
                git_init()
            };
            let zmin_repo = if sha256 {
                pinned_git_init_sha256()
            } else {
                git_init()
            };
            let (stock_base, stock_target) = setup(stock_repo.path());
            let (zmin_base, zmin_target) = setup(zmin_repo.path());
            assert_eq!((&stock_base, &stock_target), (&zmin_base, &zmin_target));
            let stock_before_index =
                fs::read(stock_repo.path().join(".git/index")).expect("read stock base index");
            let zmin_before_index =
                fs::read(zmin_repo.path().join(".git/index")).expect("read zmin base index");
            assert_eq!(
                git(stock_repo.path(), ["ls-files", "--stage"]),
                git(zmin_repo.path(), ["ls-files", "--stage"]),
                "sha256={sha256}, reset={reset} base index entries"
            );
            assert_eq!(
                fs::read(stock_repo.path().join("ignored/keep.txt"))
                    .expect("read stock ignored file before"),
                b"keep\n"
            );
            assert_eq!(
                fs::read(zmin_repo.path().join("ignored/keep.txt"))
                    .expect("read zmin ignored file before"),
                b"keep\n"
            );
            assert!(!stock_repo.path().join("ignored/sub/.git").exists());
            assert!(!zmin_repo.path().join("ignored/sub/.git").exists());
            assert!(!stock_repo.path().join(".git/modules/sub").exists());
            assert!(!zmin_repo.path().join(".git/modules/sub").exists());
            let stock_args = if reset {
                vec!["read-tree", "--reset", "-u", stock_target.as_str()]
            } else {
                vec![
                    "read-tree",
                    "-u",
                    "-m",
                    stock_base.as_str(),
                    stock_target.as_str(),
                ]
            };
            let zmin_args = if reset {
                vec!["read-tree", "--reset", "-u", zmin_target.as_str()]
            } else {
                vec![
                    "read-tree",
                    "-u",
                    "-m",
                    zmin_base.as_str(),
                    zmin_target.as_str(),
                ]
            };
            let stock_output = raw_command_output(
                stock_git.as_path(),
                stock_repo.path(),
                &stock_args,
                "pinned ignored directory gitlink",
            );
            let zmin_output = raw_command_output(
                zmin_bin(),
                zmin_repo.path(),
                &zmin_args,
                "zmin ignored directory gitlink",
            );
            assert_eq!(zmin_output, stock_output, "sha256={sha256}, reset={reset}");
            assert_eq!(stock_output.0, 0, "sha256={sha256}, reset={reset}");
            let stock_after_index =
                fs::read(stock_repo.path().join(".git/index")).expect("read stock target index");
            let zmin_after_index =
                fs::read(zmin_repo.path().join(".git/index")).expect("read zmin target index");
            assert_eq!(
                git(stock_repo.path(), ["ls-files", "--stage"]),
                git(zmin_repo.path(), ["ls-files", "--stage"]),
                "sha256={sha256}, reset={reset} target index entries"
            );
            assert_ne!(stock_after_index, stock_before_index);
            assert_ne!(zmin_after_index, zmin_before_index);
            for repo in [stock_repo.path(), zmin_repo.path()] {
                let stage = git(repo, ["ls-files", "--stage"]);
                assert!(stage.contains("160000"));
                assert!(stage.contains("ignored/sub"));
                assert!(repo.join("ignored").is_dir());
                assert!(repo.join("ignored/sub").is_dir());
                assert!(!repo.join("ignored/sub/.git").exists());
                assert_eq!(
                    fs::read(repo.join("ignored/keep.txt")).expect("read ignored file after"),
                    b"keep\n"
                );
                assert!(!repo.join(".git/modules/sub").exists());
            }
            assert_eq!(
                git(stock_repo.path(), ["ls-files", "--stage"]),
                git(zmin_repo.path(), ["ls-files", "--stage"])
            );
        }
    }
}

#[test]
fn read_tree_dirty_gitmodules_is_rejected_normally_and_reset_overwrites_for_sha1_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let setup = |repo: &Path| {
            configure_identity(repo);
            fs::write(
                repo.join(".gitmodules"),
                "[submodule \"sub\"]\n\tpath = sub\n\turl = file:///old\n",
            )
            .expect("write original gitmodules");
            git(repo, ["add", ".gitmodules"]);
            git_with_env(repo, ["commit", "-m", "original gitmodules"]);
            let base = git(repo, ["rev-parse", "HEAD"]).trim().to_owned();
            fs::write(
                repo.join(".gitmodules"),
                "[submodule \"sub\"]\n\tpath = sub\n\turl = file:///new\n",
            )
            .expect("write dirty gitmodules");
            git(repo, ["add", ".gitmodules"]);
            let target_tree = git(repo, ["write-tree"]).trim().to_owned();
            git(repo, ["read-tree", &base]);
            (base, target_tree)
        };
        let stock_repo = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        let zmin_repo = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        let (stock_base, stock_target_tree) = setup(stock_repo.path());
        let (zmin_base, zmin_target_tree) = setup(zmin_repo.path());
        assert_eq!(stock_base, zmin_base, "sha256={sha256}");
        assert_eq!(stock_target_tree, zmin_target_tree, "sha256={sha256}");

        let normal_args = [
            "read-tree",
            "-u",
            "-m",
            stock_base.as_str(),
            stock_target_tree.as_str(),
        ];
        let stock_normal = raw_command_output(
            stock_git.as_path(),
            stock_repo.path(),
            &normal_args,
            "pinned dirty gitmodules merge",
        );
        let zmin_normal_args = [
            "read-tree",
            "-u",
            "-m",
            zmin_base.as_str(),
            zmin_target_tree.as_str(),
        ];
        let zmin_normal = raw_command_output(
            zmin_bin(),
            zmin_repo.path(),
            &zmin_normal_args,
            "zmin dirty gitmodules merge",
        );
        assert_eq!(zmin_normal, stock_normal, "sha256={sha256}");
        assert_ne!(
            stock_normal.0, 0,
            "dirty gitmodules merge unexpectedly succeeded"
        );
        assert_eq!(
            git(stock_repo.path(), ["ls-files", "--stage"]),
            git(zmin_repo.path(), ["ls-files", "--stage"]),
            "normal dirty gitmodules changed the index, sha256={sha256}"
        );
        assert_eq!(
            fs::read(stock_repo.path().join(".gitmodules")).expect("read stock gitmodules"),
            fs::read(zmin_repo.path().join(".gitmodules")).expect("read zmin gitmodules"),
            "normal dirty gitmodules changed the worktree, sha256={sha256}"
        );

        let stock_reset_args = ["read-tree", "--reset", "-u", stock_target_tree.as_str()];
        let zmin_reset_args = ["read-tree", "--reset", "-u", zmin_target_tree.as_str()];
        let stock_reset = raw_command_output(
            stock_git.as_path(),
            stock_repo.path(),
            &stock_reset_args,
            "pinned dirty gitmodules reset",
        );
        let zmin_reset = raw_command_output(
            zmin_bin(),
            zmin_repo.path(),
            &zmin_reset_args,
            "zmin dirty gitmodules reset",
        );
        assert_eq!(zmin_reset, stock_reset, "sha256={sha256}");
        assert_eq!(stock_reset.0, 0, "dirty gitmodules reset failed");
        assert_eq!(
            fs::read(stock_repo.path().join(".gitmodules")).expect("read stock reset gitmodules"),
            b"[submodule \"sub\"]\n\tpath = sub\n\turl = file:///new\n".to_vec(),
            "pinned reset did not overwrite gitmodules, sha256={sha256}"
        );
        assert_eq!(
            fs::read(zmin_repo.path().join(".gitmodules")).expect("read zmin reset gitmodules"),
            fs::read(stock_repo.path().join(".gitmodules")).expect("read stock reset gitmodules"),
            "zmin reset gitmodules state diverged, sha256={sha256}"
        );
    }
}

#[test]
fn read_tree_absorbed_marker_dirty_removal_matches_pinned_sha1_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let normal = read_tree_removal_fixture(sha256);
        let populate_args = [
            "read-tree",
            "-u",
            "-m",
            "--recurse-submodules",
            "HEAD^^",
            "HEAD^",
        ];
        let remove_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD"];
        for (program, repo, label) in [
            (
                stock_git.as_path(),
                normal.stock_super.path(),
                "pinned absorbed-marker populate",
            ),
            (
                Path::new(zmin_bin()),
                normal.zmin_super.path(),
                "zmin absorbed-marker populate",
            ),
        ] {
            let output = raw_command_output(
                program.to_str().expect("program path utf8"),
                repo,
                &populate_args,
                label,
            );
            assert_eq!(output.0, 0, "sha256={sha256}, label={label}");
            assert!(
                repo.join("sub/.git").is_file(),
                "missing linked submodule marker before embedded conversion: {label}"
            );
            make_read_tree_submodule_embedded(repo);
            fs::write(repo.join("sub/child.txt"), b"absorbed dirty\n")
                .expect("write absorbed dirty tracked file");
        }
        let before = |repo: &Path| {
            (
                git(repo, ["ls-files", "--stage"]),
                git(&repo.join("sub"), ["ls-files", "--stage"]),
                fs::read(repo.join("sub/child.txt")).expect("read absorbed dirty file"),
                read_tree_file_snapshot(&repo.join("sub")).expect("read absorbed worktree"),
            )
        };
        let stock_before = before(normal.stock_super.path());
        let zmin_before = before(normal.zmin_super.path());
        let stock_normal = raw_command_output(
            stock_git.as_path(),
            normal.stock_super.path(),
            &remove_args,
            "pinned absorbed-marker dirty removal",
        );
        let zmin_normal = raw_command_output(
            zmin_bin(),
            normal.zmin_super.path(),
            &remove_args,
            "zmin absorbed-marker dirty removal",
        );
        assert_eq!(
            normalize_read_tree_command_output(&zmin_normal, normal.zmin_super.path()),
            normalize_read_tree_command_output(&stock_normal, normal.stock_super.path()),
            "sha256={sha256}"
        );
        assert_ne!(
            stock_normal.0, 0,
            "absorbed dirty removal unexpectedly succeeded"
        );
        assert_eq!(before(normal.stock_super.path()), stock_before);
        assert_eq!(before(normal.zmin_super.path()), zmin_before);

        let reset = read_tree_removal_fixture(sha256);
        let reset_args = ["read-tree", "-u", "--reset", "--recurse-submodules", "HEAD"];
        for (program, repo, label) in [
            (
                stock_git.as_path(),
                reset.stock_super.path(),
                "pinned absorbed-marker reset populate",
            ),
            (
                Path::new(zmin_bin()),
                reset.zmin_super.path(),
                "zmin absorbed-marker reset populate",
            ),
        ] {
            let output = raw_command_output(
                program.to_str().expect("program path utf8"),
                repo,
                &populate_args,
                label,
            );
            assert_eq!(output.0, 0, "sha256={sha256}, label={label}");
            assert!(
                repo.join("sub/.git").is_file(),
                "missing linked submodule marker before embedded reset: {label}"
            );
            make_read_tree_submodule_embedded(repo);
            fs::write(repo.join("sub/child.txt"), b"absorbed reset dirty\n")
                .expect("write absorbed reset dirty file");
        }
        let stock_reset = raw_command_output(
            stock_git.as_path(),
            reset.stock_super.path(),
            &reset_args,
            "pinned absorbed-marker reset",
        );
        let zmin_reset = raw_command_output(
            zmin_bin(),
            reset.zmin_super.path(),
            &reset_args,
            "zmin absorbed-marker reset",
        );
        assert_eq!(
            normalize_read_tree_command_output(&zmin_reset, reset.zmin_super.path()),
            normalize_read_tree_command_output(&stock_reset, reset.stock_super.path()),
            "sha256={sha256}"
        );
        assert_eq!(stock_reset.0, 0, "absorbed reset failed");
        for repo in [reset.stock_super.path(), reset.zmin_super.path()] {
            assert!(
                !repo.join("sub/.git").exists(),
                "reset left absorbed marker"
            );
            assert!(!repo.join("sub").exists(), "reset left absorbed worktree");
            assert!(!git(repo, ["ls-files", "--stage"]).contains("\tsub"));
        }

        let nested = nested_read_tree_fixture(sha256);
        for (program, repo, label) in [
            (
                stock_git.as_path(),
                nested.stock_super.path(),
                "pinned nested absorbed-marker populate",
            ),
            (
                Path::new(zmin_bin()),
                nested.zmin_super.path(),
                "zmin nested absorbed-marker populate",
            ),
        ] {
            let output = raw_command_output(
                program.to_str().expect("program path utf8"),
                repo,
                &populate_args,
                label,
            );
            assert_eq!(output.0, 0, "sha256={sha256}, label={label}");
            assert!(
                repo.join("child/nested/.git").is_file(),
                "missing linked nested marker before embedded conversion: {label}"
            );
            make_read_tree_submodule_embedded_at(
                repo,
                "child/nested",
                "modules/child/modules/nested",
            );
            fs::write(
                repo.join("child/nested/inner.txt"),
                b"nested absorbed dirty\n",
            )
            .expect("write nested absorbed dirty file");
        }
        let nested_stock_before = snapshot_read_tree_nested(nested.stock_super.path());
        let nested_zmin_before = snapshot_read_tree_nested(nested.zmin_super.path());
        let nested_stock = raw_command_output(
            stock_git.as_path(),
            nested.stock_super.path(),
            &remove_args,
            "pinned nested absorbed-marker removal",
        );
        let nested_zmin = raw_command_output(
            zmin_bin(),
            nested.zmin_super.path(),
            &remove_args,
            "zmin nested absorbed-marker removal",
        );
        assert_eq!(
            normalize_read_tree_command_output(&nested_zmin, nested.zmin_super.path()),
            normalize_read_tree_command_output(&nested_stock, nested.stock_super.path()),
            "sha256={sha256}"
        );
        assert_eq!(nested_stock.0, 0, "nested absorbed dirty removal failed");
        let nested_stock_after = snapshot_read_tree_nested(nested.stock_super.path());
        let nested_zmin_after = snapshot_read_tree_nested(nested.zmin_super.path());
        assert_ne!(
            nested_stock_after.root_index, nested_stock_before.root_index,
            "pinned nested absorbed removal did not update the parent index"
        );
        assert_ne!(
            nested_zmin_after.root_index, nested_zmin_before.root_index,
            "zmin nested absorbed removal did not update the parent index"
        );
        assert_read_tree_nested_semantics_equal(
            nested.stock_super.path(),
            nested.zmin_super.path(),
            &nested.roots,
            "absorbed nested removal post-state",
        );
    }
}

#[test]
fn read_tree_many_gitlinks_use_indexed_paths_for_sha1_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let setup = |repo: &Path| {
            configure_identity(repo);
            let mut gitmodules = String::new();
            for index in 0..96 {
                gitmodules.push_str(&format!(
                    "[submodule \"module-{index:03}\"]\n\tpath = modules/{index:03}\n\turl = file:///module-{index:03}\n"
                ));
            }
            fs::write(repo.join(".gitmodules"), gitmodules).expect("write many gitmodules");
            git(repo, ["add", ".gitmodules"]);
            let module_id = "1111111111111111111111111111111111111111";
            let module_id = if sha256 {
                "1111111111111111111111111111111111111111111111111111111111111111"
            } else {
                module_id
            };
            for index in 0..96 {
                let path = format!("modules/{index:03}");
                git(
                    repo,
                    [
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        &format!("160000,{module_id},{path}"),
                    ],
                );
            }
            git_with_env(repo, ["commit", "-m", "many gitlinks"]);
            let target = git(repo, ["rev-parse", "HEAD"]).trim().to_owned();
            git(repo, ["read-tree", "--empty"]);
            fs::remove_file(repo.join(".gitmodules")).expect("remove many gitmodules worktree");
            target
        };
        let stock_repo = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        let zmin_repo = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        let stock_target = setup(stock_repo.path());
        let zmin_target = setup(zmin_repo.path());
        assert_eq!(stock_target, zmin_target, "sha256={sha256}");
        let args = [
            "read-tree",
            "-u",
            "-m",
            "--no-recurse-submodules",
            stock_target.as_str(),
        ];
        let stock_output = raw_command_output(
            stock_git.as_path(),
            stock_repo.path(),
            &args,
            "pinned many-gitlink read-tree",
        );
        let zmin_output = raw_command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "read-tree",
                "-u",
                "-m",
                "--no-recurse-submodules",
                zmin_target.as_str(),
            ],
            "zmin many-gitlink read-tree",
        );
        assert_eq!(zmin_output, stock_output, "sha256={sha256}");
        assert_eq!(stock_output.0, 0, "many-gitlink read-tree failed");
        for repo in [stock_repo.path(), zmin_repo.path()] {
            let stage = git(repo, ["ls-files", "--stage"]);
            assert_eq!(
                stage.lines().filter(|line| line.contains("160000")).count(),
                96,
                "many-gitlink index count diverged, sha256={sha256}"
            );
            assert_eq!(
                fs::read_to_string(repo.join(".gitmodules"))
                    .expect("many gitmodules checkout")
                    .matches("[submodule \"")
                    .count(),
                96,
                "many-gitlink module table count diverged, sha256={sha256}"
            );
        }
    }
}

#[test]
fn read_tree_recursive_missing_inactive_and_embedded_controls_match_pinned_git() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let inactive = read_tree_removal_fixture(sha256);
        let populate_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD^"];
        for (program, repo, label) in [
            (
                stock_git.as_path(),
                inactive.stock_super.path(),
                "pinned inactive populate",
            ),
            (
                Path::new(zmin_bin()),
                inactive.zmin_super.path(),
                "zmin inactive populate",
            ),
        ] {
            let output = raw_command_output(
                program.to_str().expect("git path utf8"),
                repo,
                &populate_args,
                label,
            );
            assert_eq!(output.0, 0, "sha256={sha256}, label={label}");
        }
        for repo in [inactive.stock_super.path(), inactive.zmin_super.path()] {
            git(repo, ["config", "submodule.sub.active", "false"]);
        }
        let inactive_snapshot = |repo: &Path| {
            (
                git(&repo.join("sub"), ["rev-parse", "HEAD"]),
                fs::read(repo.join("sub/child.txt")).expect("read inactive child bytes"),
                fs::read(repo.join(".git/modules/sub/config")).expect("read inactive admin config"),
            )
        };
        let inactive_before_stock = inactive_snapshot(inactive.stock_super.path());
        let inactive_before_zmin = inactive_snapshot(inactive.zmin_super.path());
        let inactive_index_before_stock =
            read_tree_optional_file(&inactive.stock_super.path().join(".git/index"));
        let inactive_index_before_zmin =
            read_tree_optional_file(&inactive.zmin_super.path().join(".git/index"));
        let inactive_module_before_stock =
            snapshot_read_tree_module(inactive.stock_super.path(), "sub", "modules/sub");
        let inactive_module_before_zmin =
            snapshot_read_tree_module(inactive.zmin_super.path(), "sub", "modules/sub");
        let remove_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD"];
        let stock_inactive = raw_command_output(
            stock_git.as_path(),
            inactive.stock_super.path(),
            &remove_args,
            "pinned inactive removal",
        );
        let zmin_inactive = raw_command_output(
            zmin_bin(),
            inactive.zmin_super.path(),
            &remove_args,
            "zmin inactive removal",
        );
        assert_eq!(zmin_inactive, stock_inactive, "sha256={sha256}");
        assert_eq!(stock_inactive.0, 0, "sha256={sha256}");
        for (repo, before) in [
            (inactive.stock_super.path(), inactive_before_stock),
            (inactive.zmin_super.path(), inactive_before_zmin),
        ] {
            assert!(
                !git(repo, ["ls-files", "--stage"])
                    .lines()
                    .any(|line| line.ends_with("\tsub"))
            );
            assert_eq!(inactive_snapshot(repo), before);
        }
        assert_ne!(
            read_tree_optional_file(&inactive.stock_super.path().join(".git/index")),
            inactive_index_before_stock
        );
        assert_ne!(
            read_tree_optional_file(&inactive.zmin_super.path().join(".git/index")),
            inactive_index_before_zmin
        );
        assert_eq!(
            snapshot_read_tree_module(inactive.stock_super.path(), "sub", "modules/sub"),
            inactive_module_before_stock
        );
        assert_eq!(
            snapshot_read_tree_module(inactive.zmin_super.path(), "sub", "modules/sub"),
            inactive_module_before_zmin
        );

        let absent = read_tree_removal_fixture(sha256);
        let populate_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD^"];
        for (program, repo) in [
            (stock_git.as_path(), absent.stock_super.path()),
            (Path::new(zmin_bin()), absent.zmin_super.path()),
        ] {
            assert_eq!(
                raw_command_output(
                    program,
                    repo,
                    &populate_args,
                    "populate absent-worktree case"
                )
                .0,
                0
            );
            fs::remove_dir_all(repo.join("sub")).expect("remove already-absent worktree");
        }
        let admin_configs = [
            fs::read(absent.stock_super.path().join(".git/modules/sub/config"))
                .expect("read stock absent-worktree admin config"),
            fs::read(absent.zmin_super.path().join(".git/modules/sub/config"))
                .expect("read zmin absent-worktree admin config"),
        ];
        let absent_index_before_stock =
            read_tree_optional_file(&absent.stock_super.path().join(".git/index"));
        let absent_index_before_zmin =
            read_tree_optional_file(&absent.zmin_super.path().join(".git/index"));
        let absent_module_before_stock =
            snapshot_read_tree_module(absent.stock_super.path(), "sub", "modules/sub");
        let absent_module_before_zmin =
            snapshot_read_tree_module(absent.zmin_super.path(), "sub", "modules/sub");
        let stock_absent = raw_command_output(
            stock_git.as_path(),
            absent.stock_super.path(),
            &remove_args,
            "pinned absent-worktree removal",
        );
        let zmin_absent = raw_command_output(
            zmin_bin(),
            absent.zmin_super.path(),
            &remove_args,
            "zmin absent-worktree removal",
        );
        assert_eq!(zmin_absent, stock_absent, "sha256={sha256}");
        assert_eq!(stock_absent.0, 0, "sha256={sha256}");
        assert_eq!(
            fs::read(absent.stock_super.path().join(".git/modules/sub/config"))
                .expect("read stock unchanged absent-worktree config"),
            admin_configs[0]
        );
        assert_eq!(
            fs::read(absent.zmin_super.path().join(".git/modules/sub/config"))
                .expect("read zmin unchanged absent-worktree config"),
            admin_configs[1]
        );
        for repo in [absent.stock_super.path(), absent.zmin_super.path()] {
            let index = git(repo, ["ls-files", "--stage"]);
            assert!(!index.lines().any(|line| line.ends_with("\tsub")));
        }
        assert_ne!(
            read_tree_optional_file(&absent.stock_super.path().join(".git/index")),
            absent_index_before_stock
        );
        assert_ne!(
            read_tree_optional_file(&absent.zmin_super.path().join(".git/index")),
            absent_index_before_zmin
        );
        assert_eq!(
            snapshot_read_tree_module(absent.stock_super.path(), "sub", "modules/sub"),
            absent_module_before_stock
        );
        assert_eq!(
            snapshot_read_tree_module(absent.zmin_super.path(), "sub", "modules/sub"),
            absent_module_before_zmin
        );

        let missing = read_tree_removal_fixture(sha256);
        let missing_id = "f".repeat(if sha256 { 64 } else { 40 });
        for repo in [missing.stock_super.path(), missing.zmin_super.path()] {
            git(repo, ["read-tree", "HEAD^^"]);
            git(
                repo,
                [
                    "update-index",
                    "--cacheinfo",
                    &format!("160000,{missing_id},sub"),
                ],
            );
            git_with_env(repo, ["commit", "-m", "missing nested target"]);
        }
        let before_stock = git(missing.stock_super.path(), ["ls-files", "--stage"]);
        let before_zmin = git(missing.zmin_super.path(), ["ls-files", "--stage"]);
        let missing_state_before_stock = (
            read_tree_optional_file(&missing.stock_super.path().join(".git/index")),
            snapshot_read_tree_module(missing.stock_super.path(), "sub", "modules/sub"),
        );
        let missing_state_before_zmin = (
            read_tree_optional_file(&missing.zmin_super.path().join(".git/index")),
            snapshot_read_tree_module(missing.zmin_super.path(), "sub", "modules/sub"),
        );
        let stock_missing = raw_command_output(
            stock_git.as_path(),
            missing.stock_super.path(),
            &remove_args,
            "pinned missing nested target",
        );
        let zmin_missing = raw_command_output(
            zmin_bin(),
            missing.zmin_super.path(),
            &remove_args,
            "zmin missing nested target",
        );
        assert_eq!(zmin_missing, stock_missing, "sha256={sha256}");
        assert_eq!(
            stock_missing.0, 0,
            "missing nested target did not preserve Git's status"
        );
        assert!(
            stock_missing
                .2
                .windows(missing_id.len())
                .any(|window| window == missing_id.as_bytes()),
            "missing nested target diagnostic omitted the target id"
        );
        assert_eq!(
            git(missing.zmin_super.path(), ["ls-files", "--stage"]),
            before_zmin,
            "zmin mutated parent index after missing nested target"
        );
        assert_eq!(
            before_stock,
            git(missing.stock_super.path(), ["ls-files", "--stage"]),
            "stock missing-target control changed parent index"
        );
        assert_eq!(
            (
                read_tree_optional_file(&missing.stock_super.path().join(".git/index")),
                snapshot_read_tree_module(missing.stock_super.path(), "sub", "modules/sub"),
            ),
            missing_state_before_stock
        );
        assert_eq!(
            (
                read_tree_optional_file(&missing.zmin_super.path().join(".git/index")),
                snapshot_read_tree_module(missing.zmin_super.path(), "sub", "modules/sub"),
            ),
            missing_state_before_zmin
        );

        let no_admin = read_tree_removal_fixture(sha256);
        let no_admin_id = "f".repeat(if sha256 { 64 } else { 40 });
        for repo in [no_admin.stock_super.path(), no_admin.zmin_super.path()] {
            git(repo, ["read-tree", "HEAD^^"]);
            git(
                repo,
                [
                    "update-index",
                    "--cacheinfo",
                    &format!("160000,{no_admin_id},sub"),
                ],
            );
            git_with_env(repo, ["commit", "-m", "missing without admin"]);
            fs::remove_dir_all(repo.join("sub")).expect("remove no-admin worktree");
            fs::remove_dir_all(repo.join(".git/modules/sub")).expect("remove no-admin admin");
        }
        let no_admin_before_stock_index = git(no_admin.stock_super.path(), ["ls-files", "--stage"]);
        let no_admin_before_zmin_index = git(no_admin.zmin_super.path(), ["ls-files", "--stage"]);
        let stock_no_admin = raw_command_output(
            stock_git.as_path(),
            no_admin.stock_super.path(),
            &remove_args,
            "pinned missing gitlink without admin",
        );
        let zmin_no_admin = raw_command_output(
            zmin_bin(),
            no_admin.zmin_super.path(),
            &remove_args,
            "zmin missing gitlink without admin",
        );
        assert_eq!(zmin_no_admin, stock_no_admin, "sha256={sha256}");
        assert_eq!(stock_no_admin.0, 128, "sha256={sha256}");
        assert_eq!(
            stock_no_admin.2,
            b"fatal: not a git repository: ../.git/modules/sub\n\
fatal: could not reset submodule index\n",
            "sha256={sha256}"
        );
        assert_eq!(
            git(no_admin.stock_super.path(), ["ls-files", "--stage"]),
            no_admin_before_stock_index,
            "stock top-level no-admin changed its index, sha256={sha256}"
        );
        assert_eq!(
            git(no_admin.zmin_super.path(), ["ls-files", "--stage"]),
            no_admin_before_zmin_index,
            "zmin top-level no-admin changed its index, sha256={sha256}"
        );
        let stock_no_admin_path = no_admin.stock_super.path().join("sub");
        let zmin_no_admin_path = no_admin.zmin_super.path().join("sub");
        let stock_no_admin_admin = no_admin.stock_super.path().join(".git/modules/sub");
        let zmin_no_admin_admin = no_admin.zmin_super.path().join(".git/modules/sub");
        assert_eq!(
            stock_no_admin_path.exists(),
            zmin_no_admin_path.exists(),
            "top-level no-admin worktree state diverged, sha256={sha256}"
        );
        assert_eq!(
            stock_no_admin_admin.exists(),
            zmin_no_admin_admin.exists(),
            "top-level no-admin admin state diverged, sha256={sha256}"
        );
        assert!(stock_no_admin_path.exists(), "sha256={sha256}");
        assert!(stock_no_admin_admin.exists(), "sha256={sha256}");
        assert_eq!(
            stock_no_admin_path.join(".git").is_file(),
            zmin_no_admin_path.join(".git").is_file(),
            "top-level no-admin marker state diverged, sha256={sha256}"
        );
        assert_read_tree_connected_worktree_state(no_admin.stock_super.path(), "sub", "sub");
        assert_read_tree_connected_worktree_state(no_admin.zmin_super.path(), "sub", "sub");
        assert_read_tree_failure_admin_shape(no_admin.stock_super.path(), "sub", "sub", true);
        assert_read_tree_failure_admin_shape(no_admin.zmin_super.path(), "sub", "sub", true);

        for repo in [missing.stock_super.path(), missing.zmin_super.path()] {
            git(repo, ["read-tree", "--reset", "HEAD^^"]);
        }
        let transition_before_stock = git(missing.stock_super.path(), ["ls-files", "--stage"]);
        let transition_before_zmin = git(missing.zmin_super.path(), ["ls-files", "--stage"]);
        let transition_child_snapshot = |repo: &Path| {
            (
                git(&repo.join("sub"), ["rev-parse", "HEAD"]),
                fs::read(repo.join("sub/child.txt")).expect("read transition child bytes"),
                fs::read(repo.join("sub/.git")).expect("read transition gitfile"),
                fs::read(repo.join(".git/modules/sub/config"))
                    .expect("read transition admin config"),
            )
        };
        let transition_child_before_stock = transition_child_snapshot(missing.stock_super.path());
        let transition_child_before_zmin = transition_child_snapshot(missing.zmin_super.path());
        let stock_missing_transition = raw_command_output(
            stock_git.as_path(),
            missing.stock_super.path(),
            &remove_args,
            "pinned valid-to-missing nested target",
        );
        let zmin_missing_transition = raw_command_output(
            zmin_bin(),
            missing.zmin_super.path(),
            &remove_args,
            "zmin valid-to-missing nested target",
        );
        assert_eq!(
            zmin_missing_transition, stock_missing_transition,
            "sha256={sha256}"
        );
        assert_eq!(stock_missing_transition.0, 128, "sha256={sha256}");
        assert!(!String::from_utf8_lossy(&stock_missing_transition.2).contains("not uptodate"));
        assert_eq!(
            git(missing.stock_super.path(), ["ls-files", "--stage"]),
            transition_before_stock,
            "stock changed the parent index after a missing transition"
        );
        assert_eq!(
            git(missing.zmin_super.path(), ["ls-files", "--stage"]),
            transition_before_zmin,
            "zmin changed the parent index after a missing transition"
        );
        assert_eq!(
            transition_child_snapshot(missing.stock_super.path()),
            transition_child_before_stock,
            "stock changed child state after a missing transition"
        );
        assert_eq!(
            transition_child_snapshot(missing.zmin_super.path()),
            transition_child_before_zmin,
            "zmin changed child state after a missing transition"
        );

        let embedded_clean = read_tree_removal_fixture(sha256);
        for (program, repo, label) in [
            (
                stock_git.as_path(),
                embedded_clean.stock_super.path(),
                "pinned embedded populate",
            ),
            (
                Path::new(zmin_bin()),
                embedded_clean.zmin_super.path(),
                "zmin embedded populate",
            ),
        ] {
            let output = raw_command_output(
                program.to_str().expect("git path utf8"),
                repo,
                &populate_args,
                label,
            );
            assert_eq!(output.0, 0, "sha256={sha256}, label={label}");
            make_read_tree_submodule_embedded(repo);
        }
        let stock_embedded = raw_command_output(
            stock_git.as_path(),
            embedded_clean.stock_super.path(),
            &remove_args,
            "pinned embedded clean removal",
        );
        let zmin_embedded = raw_command_output(
            zmin_bin(),
            embedded_clean.zmin_super.path(),
            &remove_args,
            "zmin embedded clean removal",
        );
        assert_eq!(zmin_embedded.0, stock_embedded.0, "sha256={sha256}");
        assert_eq!(zmin_embedded.1, stock_embedded.1, "sha256={sha256}");
        assert_eq!(
            normalize_read_tree_temp_path(&zmin_embedded.2, embedded_clean.zmin_super.path()),
            normalize_read_tree_temp_path(&stock_embedded.2, embedded_clean.stock_super.path()),
            "sha256={sha256}"
        );
        assert_eq!(stock_embedded.0, 0, "embedded clean removal failed");
        assert!(!embedded_clean.zmin_super.path().join("sub").exists());

        let embedded_dirty = read_tree_removal_fixture(sha256);
        for (program, repo, label) in [
            (
                stock_git.as_path(),
                embedded_dirty.stock_super.path(),
                "pinned embedded dirty populate",
            ),
            (
                Path::new(zmin_bin()),
                embedded_dirty.zmin_super.path(),
                "zmin embedded dirty populate",
            ),
        ] {
            let output = raw_command_output(
                program.to_str().expect("git path utf8"),
                repo,
                &populate_args,
                label,
            );
            assert_eq!(output.0, 0, "sha256={sha256}, label={label}");
            make_read_tree_submodule_embedded(repo);
            fs::write(repo.join("sub/child.txt"), b"dirty\n").expect("write embedded dirty file");
        }
        let embedded_dirty_snapshot = |repo: &Path| {
            (
                git(repo, ["ls-files", "--stage"]),
                fs::read(repo.join("sub/child.txt")).expect("read embedded dirty bytes"),
                fs::read(repo.join("sub/.git/config")).expect("read embedded dirty config"),
            )
        };
        let embedded_dirty_before_stock =
            embedded_dirty_snapshot(embedded_dirty.stock_super.path());
        let embedded_dirty_before_zmin = embedded_dirty_snapshot(embedded_dirty.zmin_super.path());
        let stock_embedded_dirty = raw_command_output(
            stock_git.as_path(),
            embedded_dirty.stock_super.path(),
            &remove_args,
            "pinned embedded dirty refusal",
        );
        let zmin_embedded_dirty = raw_command_output(
            zmin_bin(),
            embedded_dirty.zmin_super.path(),
            &remove_args,
            "zmin embedded dirty refusal",
        );
        assert_eq!(zmin_embedded_dirty, stock_embedded_dirty, "sha256={sha256}");
        assert_ne!(
            stock_embedded_dirty.0, 0,
            "embedded dirty removal unexpectedly succeeded"
        );
        assert!(
            embedded_dirty
                .zmin_super
                .path()
                .join("sub/child.txt")
                .exists()
        );
        assert_eq!(
            embedded_dirty_snapshot(embedded_dirty.stock_super.path()),
            embedded_dirty_before_stock
        );
        assert_eq!(
            embedded_dirty_snapshot(embedded_dirty.zmin_super.path()),
            embedded_dirty_before_zmin
        );
    }
}

#[test]
fn read_tree_nested_dirty_update_refusal_and_reset_match_pinned_git_for_sha1_and_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        for dirty_kind in ["tracked", "staged", "untracked"] {
            let fixture = read_tree_removal_fixture(sha256);
            let prepare_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD^^"];
            let stock_prepare = raw_command_output(
                stock_git.as_path(),
                fixture.stock_super.path(),
                &prepare_args,
                "pinned nested dirty preparation",
            );
            let zmin_prepare = raw_command_output(
                zmin_bin(),
                fixture.zmin_super.path(),
                &prepare_args,
                "zmin nested dirty preparation",
            );
            assert_eq!(
                zmin_prepare, stock_prepare,
                "sha256={sha256}, kind={dirty_kind}"
            );
            let before_stock = git(fixture.stock_super.path(), ["ls-files", "--stage"]);
            let before_zmin = git(fixture.zmin_super.path(), ["ls-files", "--stage"]);
            for repo in [fixture.stock_super.path(), fixture.zmin_super.path()] {
                let child = repo.join("sub");
                match dirty_kind {
                    "tracked" => fs::write(child.join("child.txt"), b"dirty\n")
                        .expect("write tracked dirty child"),
                    "staged" => {
                        fs::write(child.join("child.txt"), b"staged\n")
                            .expect("write staged dirty child");
                        git(&child, ["add", "child.txt"]);
                    }
                    "untracked" => fs::write(child.join("new.txt"), b"untracked\n")
                        .expect("write untracked collision"),
                    _ => unreachable!("known dirty kind"),
                }
            }
            let stock_update_before = snapshot_read_tree_nested(fixture.stock_super.path());
            let zmin_update_before = snapshot_read_tree_nested(fixture.zmin_super.path());
            let update_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD^"];
            let stock_update = raw_command_output(
                stock_git.as_path(),
                fixture.stock_super.path(),
                &update_args,
                "pinned nested dirty refusal",
            );
            let zmin_update = raw_command_output(
                zmin_bin(),
                fixture.zmin_super.path(),
                &update_args,
                "zmin nested dirty refusal",
            );
            assert_eq!(
                zmin_update, stock_update,
                "sha256={sha256}, kind={dirty_kind}"
            );
            if dirty_kind == "tracked" || dirty_kind == "staged" {
                assert_ne!(stock_update.0, 0, "dirty update unexpectedly succeeded");
                assert_eq!(
                    git(fixture.stock_super.path(), ["ls-files", "--stage"]),
                    before_stock,
                    "stock parent index changed after refusal"
                );
                assert_eq!(
                    git(fixture.zmin_super.path(), ["ls-files", "--stage"]),
                    before_zmin,
                    "zmin parent index changed after refusal"
                );
                assert_eq!(
                    snapshot_read_tree_nested(fixture.stock_super.path()),
                    stock_update_before
                );
                assert_eq!(
                    snapshot_read_tree_nested(fixture.zmin_super.path()),
                    zmin_update_before
                );
            } else {
                assert_eq!(stock_update.0, 0, "pinned update unexpectedly failed");
            }

            let reset_args = [
                "read-tree",
                "--reset",
                "-u",
                "--recurse-submodules",
                "HEAD^",
            ];
            let stock_reset = raw_command_output(
                stock_git.as_path(),
                fixture.stock_super.path(),
                &reset_args,
                "pinned nested dirty reset",
            );
            let zmin_reset = raw_command_output(
                zmin_bin(),
                fixture.zmin_super.path(),
                &reset_args,
                "zmin nested dirty reset",
            );
            assert_eq!(
                zmin_reset, stock_reset,
                "sha256={sha256}, kind={dirty_kind}"
            );
            assert_eq!(stock_reset.0, 0, "reset did not recover dirty update");
            for repo in [fixture.stock_super.path(), fixture.zmin_super.path()] {
                let target = git(repo, ["rev-parse", "HEAD^:sub"]);
                assert_eq!(
                    git(&repo.join("sub"), ["rev-parse", "HEAD"]).trim(),
                    target.trim(),
                    "nested HEAD does not match reset gitlink"
                );
                assert_eq!(
                    fs::read(repo.join("sub/child.txt")).expect("read reset child"),
                    b"two\n"
                );
                assert!(git(repo, ["ls-files", "--stage", "sub"]).contains(target.trim()));
                assert_eq!(
                    git(repo, ["write-tree"]).trim(),
                    git(repo, ["rev-parse", "HEAD^^{tree}"]).trim(),
                    "reset parent tree does not match target"
                );
            }
            assert_eq!(
                normalize_read_tree_index_for_fixture(&git(
                    fixture.stock_super.path(),
                    ["ls-files", "--stage"],
                )),
                normalize_read_tree_index_for_fixture(&git(
                    fixture.zmin_super.path(),
                    ["ls-files", "--stage"],
                )),
                "reset root index differs from pinned Git"
            );
            assert_read_tree_nested_semantics_equal(
                fixture.stock_super.path(),
                fixture.zmin_super.path(),
                &fixture.roots,
                "nested dirty reset post-state",
            );
            for admin_path in [".git/modules/sub", ".git/modules/sub/modules/nested"] {
                let stock_admin = fixture.stock_super.path().join(admin_path);
                let zmin_admin = fixture.zmin_super.path().join(admin_path);
                if stock_admin.is_dir() && zmin_admin.is_dir() {
                    assert_eq!(
                        normalize_read_tree_index_for_fixture(&git(
                            &stock_admin,
                            ["ls-files", "--stage"],
                        )),
                        normalize_read_tree_index_for_fixture(&git(
                            &zmin_admin,
                            ["ls-files", "--stage"],
                        )),
                        "reset {admin_path} index differs from pinned Git"
                    );
                    assert_eq!(
                        git(&stock_admin, ["rev-parse", "HEAD"]),
                        git(&zmin_admin, ["rev-parse", "HEAD"]),
                        "reset {admin_path} HEAD differs from pinned Git"
                    );
                }
            }
        }
    }
}

#[test]
fn read_tree_nested_dirty_check_after_stat_refresh_matches_pinned_git_for_sha1_and_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let fixture = read_tree_removal_fixture(sha256);
        let prepare_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD^^"];
        assert_eq!(
            raw_command_output(
                stock_git.as_path(),
                fixture.stock_super.path(),
                &prepare_args,
                "pinned SHA256 stat preparation"
            ),
            raw_command_output(
                zmin_bin(),
                fixture.zmin_super.path(),
                &prepare_args,
                "zmin SHA256 stat preparation"
            )
        );
        for repo in [fixture.stock_super.path(), fixture.zmin_super.path()] {
            let child = repo.join("sub");
            fs::write(child.join("child.txt"), b"one\n").expect("touch SHA256 child bytes");
        }
        let mut refresh_outputs = Vec::new();
        for (program, repo, label) in [
            (
                stock_git.as_path(),
                fixture.stock_super.path(),
                "pinned SHA256 stat refresh",
            ),
            (
                Path::new(zmin_bin()),
                fixture.zmin_super.path(),
                "zmin SHA256 stat refresh",
            ),
        ] {
            let child = repo.join("sub");
            refresh_outputs.push(raw_command_output(
                program.to_str().expect("git path utf8"),
                &child,
                &["update-index", "--really-refresh"],
                label,
            ));
        }
        assert_eq!(refresh_outputs[0], refresh_outputs[1]);
        for (program, repo, label) in [
            (
                stock_git.as_path(),
                fixture.stock_super.path(),
                "pinned SHA256 stat clean check",
            ),
            (
                Path::new(zmin_bin()),
                fixture.zmin_super.path(),
                "zmin SHA256 stat clean check",
            ),
        ] {
            let output = raw_command_output(
                program.to_str().expect("git path utf8"),
                &repo.join("sub"),
                &["diff-files", "--quiet"],
                label,
            );
            assert_eq!(output.0, 0, "same-bytes stat refresh left dirt: {label}");
        }
        let stock_before_update = (
            read_tree_optional_file(&fixture.stock_super.path().join(".git/index")),
            snapshot_read_tree_module(fixture.stock_super.path(), "sub", "modules/sub"),
        );
        let zmin_before_update = (
            read_tree_optional_file(&fixture.zmin_super.path().join(".git/index")),
            snapshot_read_tree_module(fixture.zmin_super.path(), "sub", "modules/sub"),
        );
        let update_args = ["read-tree", "-u", "-m", "--recurse-submodules", "HEAD^"];
        let stock_update = raw_command_output(
            stock_git.as_path(),
            fixture.stock_super.path(),
            &update_args,
            "pinned SHA256 stat-dirty refusal",
        );
        let zmin_update = raw_command_output(
            zmin_bin(),
            fixture.zmin_super.path(),
            &update_args,
            "zmin SHA256 stat-dirty refusal",
        );
        assert_eq!(zmin_update, stock_update);
        assert_eq!(stock_update.0, 0);
        let stock_target = git(fixture.stock_super.path(), ["rev-parse", "HEAD^:sub"]);
        let zmin_target = git(fixture.zmin_super.path(), ["rev-parse", "HEAD^:sub"]);
        let stock_after_update = (
            read_tree_optional_file(&fixture.stock_super.path().join(".git/index")),
            snapshot_read_tree_module(fixture.stock_super.path(), "sub", "modules/sub"),
        );
        let zmin_after_update = (
            read_tree_optional_file(&fixture.zmin_super.path().join(".git/index")),
            snapshot_read_tree_module(fixture.zmin_super.path(), "sub", "modules/sub"),
        );
        assert_ne!(stock_after_update.0, stock_before_update.0);
        assert_ne!(zmin_after_update.0, zmin_before_update.0);
        assert_eq!(
            git(
                &fixture.stock_super.path().join("sub"),
                ["rev-parse", "HEAD"]
            )
            .trim(),
            stock_target.trim()
        );
        assert_eq!(
            git(
                &fixture.zmin_super.path().join("sub"),
                ["rev-parse", "HEAD"]
            )
            .trim(),
            zmin_target.trim()
        );
        assert_eq!(
            fs::read(fixture.stock_super.path().join("sub/child.txt"))
                .expect("read stock stat-refresh target"),
            b"two\n"
        );
        assert_eq!(
            fs::read(fixture.zmin_super.path().join("sub/child.txt"))
                .expect("read zmin stat-refresh target"),
            b"two\n"
        );
        assert!(
            git(fixture.stock_super.path(), ["ls-files", "--stage", "sub"])
                .contains(stock_target.trim())
        );
        assert!(
            git(fixture.zmin_super.path(), ["ls-files", "--stage", "sub"])
                .contains(zmin_target.trim())
        );
        assert_ne!(
            stock_after_update.1.admin_head,
            stock_before_update.1.admin_head
        );
        assert_ne!(
            zmin_after_update.1.admin_head,
            zmin_before_update.1.admin_head
        );
    }
}

#[test]
fn read_tree_uses_active_object_format_for_sha1_and_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let stock_repo = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        let zmin_repo = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        for repo in [stock_repo.path(), zmin_repo.path()] {
            configure_identity(repo);
            fs::write(repo.join("tracked.txt"), b"read-tree\n").expect("write tracked file");
            git(repo, ["add", "tracked.txt"]);
            git_with_env(repo, ["commit", "-m", "initial"]);
        }
        let stock_index_before =
            fs::read(stock_repo.path().join(".git/index")).expect("read pinned pre-dry-run index");
        let zmin_index_before =
            fs::read(zmin_repo.path().join(".git/index")).expect("read zmin pre-dry-run index");
        let stock_worktree_before =
            fs::read(stock_repo.path().join("tracked.txt")).expect("read pinned worktree");
        let zmin_worktree_before =
            fs::read(zmin_repo.path().join("tracked.txt")).expect("read zmin worktree");
        let empty_dry_run_args = ["read-tree", "--empty", "--dry-run"];
        let stock_empty_dry_run = raw_command_output(
            stock_git.as_path(),
            stock_repo.path(),
            &empty_dry_run_args,
            "pinned empty dry-run read-tree",
        );
        let zmin_empty_dry_run = raw_command_output(
            zmin_bin(),
            zmin_repo.path(),
            &empty_dry_run_args,
            "zmin empty dry-run read-tree",
        );
        assert_eq!(zmin_empty_dry_run, stock_empty_dry_run, "sha256={sha256}");
        assert_eq!(stock_empty_dry_run.0, 0, "sha256={sha256}");
        assert_eq!(
            fs::read(stock_repo.path().join(".git/index")).expect("read pinned post-dry-run index"),
            stock_index_before,
            "pinned --empty --dry-run changed index, sha256={sha256}"
        );
        assert_eq!(
            fs::read(zmin_repo.path().join(".git/index")).expect("read zmin post-dry-run index"),
            zmin_index_before,
            "zmin --empty --dry-run changed index, sha256={sha256}"
        );
        assert_eq!(
            fs::read(stock_repo.path().join("tracked.txt"))
                .expect("read pinned post-dry-run worktree"),
            stock_worktree_before
        );
        assert_eq!(
            fs::read(zmin_repo.path().join("tracked.txt"))
                .expect("read zmin post-dry-run worktree"),
            zmin_worktree_before
        );

        let empty_index_output_args = ["read-tree", "--empty", "--index-output=alt.index"];
        let stock_empty_index_output = raw_command_output(
            stock_git.as_path(),
            stock_repo.path(),
            &empty_index_output_args,
            "pinned empty index-output read-tree",
        );
        let zmin_empty_index_output = raw_command_output(
            zmin_bin(),
            zmin_repo.path(),
            &empty_index_output_args,
            "zmin empty index-output read-tree",
        );
        assert_eq!(
            zmin_empty_index_output, stock_empty_index_output,
            "sha256={sha256}"
        );
        assert_eq!(stock_empty_index_output.0, 0, "sha256={sha256}");
        for repo in [stock_repo.path(), zmin_repo.path()] {
            assert!(repo.join("alt.index").is_file());
            assert!(
                command_any_output(
                    stock_git.to_str().expect("pinned Git path utf8"),
                    repo,
                    &["ls-files", "-s"],
                    "list empty index-output index",
                )
                .1
                .contains("tracked.txt")
            );
            assert!(
                command_output_with_env(
                    stock_git.to_str().expect("pinned Git path utf8"),
                    repo,
                    &["ls-files", "-s"],
                    &[("GIT_INDEX_FILE", "alt.index")],
                    "list empty index-output index",
                )
                .1
                .is_empty()
            );
        }
        let empty_args = ["read-tree", "--empty"];
        let stock_empty = raw_command_output(
            stock_git.as_path(),
            stock_repo.path(),
            &empty_args,
            "pinned active-format empty read-tree",
        );
        let zmin_empty = raw_command_output(
            zmin_bin(),
            zmin_repo.path(),
            &empty_args,
            "zmin active-format empty read-tree",
        );
        assert_eq!(
            zmin_empty, stock_empty,
            "empty tuple differs for sha256={sha256}"
        );
        assert_eq!(
            stock_empty.0, 0,
            "empty read-tree failed for sha256={sha256}"
        );
        for (program, repo) in [
            (stock_git.as_path(), stock_repo.path()),
            (Path::new(zmin_bin()), zmin_repo.path()),
        ] {
            let listed = raw_command_output(
                program,
                repo,
                &["ls-files", "-s"],
                "list active-format empty index",
            );
            assert_eq!(listed.0, 0);
            assert!(listed.1.is_empty());
            let written = raw_command_output(program, repo, &["write-tree"], "write empty tree");
            assert_eq!(written.0, 0);
            assert_eq!(written.1.trim_ascii().len(), if sha256 { 64 } else { 40 });
            let index_bytes = fs::read(repo.join(".git/index")).expect("read empty index");
            assert!(index_bytes.len() > if sha256 { 32 } else { 20 });
        }
        let tree = git(stock_repo.path(), ["rev-parse", "HEAD^{tree}"]);
        let args = ["read-tree", tree.trim()];
        let stock_output = raw_command_output(
            stock_git.to_str().expect("pinned Git path utf8"),
            stock_repo.path(),
            &args,
            "pinned read-tree active object format",
        );
        let zmin_output = raw_command_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin read-tree active object format",
        );
        assert_eq!(
            stock_output, zmin_output,
            "read-tree tuple differs for sha256={sha256}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(stock_repo.path(), ["ls-files", "-s"]),
            "read-tree index differs for sha256={sha256}"
        );
    }
}

fn read_tree_branch_fixture(sha256: bool) -> TempDir {
    let repo = if sha256 {
        pinned_git_init_sha256()
    } else {
        git_init()
    };
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("keep")).expect("create sparse keep directory");
    fs::create_dir_all(repo.path().join("drop")).expect("create sparse drop directory");
    fs::write(repo.path().join("keep/item.txt"), b"keep one\n").expect("write sparse keep file");
    fs::write(repo.path().join("drop/item.txt"), b"drop one\n").expect("write sparse drop file");
    git(repo.path(), ["add", "keep", "drop"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    fs::write(repo.path().join("keep/item.txt"), b"keep two\n").expect("write sparse keep update");
    fs::write(repo.path().join("drop/item.txt"), b"drop two\n").expect("write sparse drop update");
    git(repo.path(), ["add", "keep", "drop"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    repo
}

#[test]
fn read_tree_sparse_checkout_and_exclude_per_directory_match_pinned_git_for_sha1_and_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let stock_sparse = read_tree_branch_fixture(sha256);
        let zmin_sparse = read_tree_branch_fixture(sha256);
        for repo in [stock_sparse.path(), zmin_sparse.path()] {
            git(repo, ["sparse-checkout", "init", "--cone"]);
            git(repo, ["sparse-checkout", "set", "keep"]);
        }
        let sparse_args = ["read-tree", "-m", "-u", "HEAD^"];
        let stock_sparse_output = raw_command_output(
            stock_git.as_path(),
            stock_sparse.path(),
            &sparse_args,
            "pinned active sparse-checkout read-tree",
        );
        let zmin_sparse_output = raw_command_output(
            zmin_bin(),
            zmin_sparse.path(),
            &sparse_args,
            "zmin active sparse-checkout read-tree",
        );
        assert_eq!(zmin_sparse_output, stock_sparse_output, "sha256={sha256}");
        assert_eq!(stock_sparse_output.0, 0, "sha256={sha256}");
        for repo in [stock_sparse.path(), zmin_sparse.path()] {
            assert_eq!(
                fs::read(repo.join("keep/item.txt")).expect("read sparse keep"),
                b"keep one\n"
            );
            assert!(
                !repo.join("drop").exists(),
                "active sparse checkout left drop/ in {}",
                repo.display()
            );
            assert_eq!(
                fs::read(repo.join(".git/info/sparse-checkout"))
                    .expect("read sparse-checkout file"),
                b"/*\n!/*/\n/keep/\n"
            );
        }
        assert_eq!(
            normalize_read_tree_config(
                read_tree_optional_file(&stock_sparse.path().join(".git/config")).as_ref(),
                &[stock_sparse.path(), zmin_sparse.path()],
            ),
            normalize_read_tree_config(
                read_tree_optional_file(&zmin_sparse.path().join(".git/config")).as_ref(),
                &[stock_sparse.path(), zmin_sparse.path()],
            ),
            "sparse config semantics diverged, sha256={sha256}"
        );
        let stock_exclude = read_tree_branch_fixture(sha256);
        let zmin_exclude = read_tree_branch_fixture(sha256);
        for repo in [stock_exclude.path(), zmin_exclude.path()] {
            git(repo, ["read-tree", "--empty"]);
            fs::write(repo.join(".gitignore"), b"keep/item.txt\n")
                .expect("write per-directory ignore");
            fs::write(repo.join("keep/item.txt"), b"ignored collision\n")
                .expect("write ignored collision");
            fs::remove_file(repo.join("drop/item.txt")).expect("remove clean exclude target");
            fs::remove_dir(repo.join("drop")).expect("remove clean exclude directory");
        }
        let exclude_args = [
            "read-tree",
            "-m",
            "-u",
            "--exclude-per-directory=.gitignore",
            "HEAD",
        ];
        let stock_exclude_output = raw_command_output(
            stock_git.as_path(),
            stock_exclude.path(),
            &exclude_args,
            "pinned exclude-per-directory read-tree",
        );
        let zmin_exclude_output = raw_command_output(
            zmin_bin(),
            zmin_exclude.path(),
            &exclude_args,
            "zmin exclude-per-directory read-tree",
        );
        assert_eq!(zmin_exclude_output, stock_exclude_output, "sha256={sha256}");
        assert_eq!(stock_exclude_output.0, 0, "sha256={sha256}");
        for repo in [stock_exclude.path(), zmin_exclude.path()] {
            assert_eq!(
                fs::read(repo.join("keep/item.txt")).expect("read excluded target"),
                b"keep two\n"
            );
            assert_eq!(
                fs::read(repo.join("drop/item.txt")).expect("read excluded sibling"),
                b"drop two\n"
            );
            assert_eq!(
                fs::read(repo.join(".gitignore")).expect("read ignore file"),
                b"keep/item.txt\n"
            );
            assert!(
                command_any_output(
                    stock_git.to_str().expect("pinned Git path utf8"),
                    repo,
                    &["ls-files", "--error-unmatch", "keep/item.txt"],
                    "verify excluded target index",
                )
                .0 == 0
            );
        }
    }
}

#[test]
fn read_tree_invalid_submodule_recurse_config_matches_pinned_git_for_sha1_and_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let stock_repo = read_tree_branch_fixture(sha256);
        let zmin_repo = read_tree_branch_fixture(sha256);
        for repo in [stock_repo.path(), zmin_repo.path()] {
            git(repo, ["config", "submodule.recurse", "not-a-boolean"]);
        }
        let stock_before = snapshot_read_tree_nested(stock_repo.path());
        let zmin_before = snapshot_read_tree_nested(zmin_repo.path());
        let roots = ReadTreeFixtureRoots::from_paths(
            sha256,
            &[stock_repo.path(), zmin_repo.path()],
            stock_repo.path(),
            zmin_repo.path(),
        );
        let args = ["read-tree", "HEAD"];
        let stock_output = raw_command_output(
            stock_git.as_path(),
            stock_repo.path(),
            &args,
            "pinned invalid submodule.recurse read-tree",
        );
        let zmin_output = raw_command_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin invalid submodule.recurse read-tree",
        );
        assert_eq!(zmin_output, stock_output, "sha256={sha256}");
        assert_eq!(stock_output.0, 128, "sha256={sha256}");
        assert_eq!(
            snapshot_read_tree_nested(stock_repo.path()),
            stock_before,
            "pinned invalid config mutated state, sha256={sha256}"
        );
        assert_eq!(
            snapshot_read_tree_nested(zmin_repo.path()),
            zmin_before,
            "zmin invalid config mutated state, sha256={sha256}"
        );
        assert_read_tree_nested_semantics_equal(
            stock_repo.path(),
            zmin_repo.path(),
            &roots,
            "invalid submodule.recurse cross-side state",
        );
    }
}

#[test]
fn read_tree_documented_option_forms_match_stock_git() {
    let stock_git = required_pinned_stock_git();
    let tree_repo = two_commit_repo();
    let tree = git(tree_repo.path(), ["rev-parse", "HEAD~1^{tree}"]);

    for args in [
        ["read-tree", "--empty", "--prefix=import/"].as_slice(),
        ["read-tree", "--empty", &tree].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(tree_repo.path());
        let zmin_repo = clone_repo_fixture(tree_repo.path());
        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), args),
            git_failure_output(git_repo.path(), args),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(git_repo.path(), ["ls-files", "-s"]),
            "args: {args:?}"
        );
    }

    for args in [
        ["read-tree", "--empty"].as_slice(),
        ["read-tree", "--prefix", "import/", &tree].as_slice(),
        ["read-tree", "--prefix=import", &tree].as_slice(),
        ["read-tree", "-m", "-m", &tree].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(tree_repo.path());
        let zmin_repo = clone_repo_fixture(tree_repo.path());
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin"),
            command_any_output("git", git_repo.path(), args, "git"),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(git_repo.path(), ["ls-files", "-s"]),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["write-tree"]),
            git(git_repo.path(), ["write-tree"]),
            "args: {args:?}"
        );
    }

    for args in [
        ["read-tree", "--dry-run", &tree].as_slice(),
        ["read-tree", "--dry-run", "--dry-run", &tree].as_slice(),
        ["read-tree", "-n", &tree].as_slice(),
        ["read-tree", "-n", "-n", &tree].as_slice(),
        ["read-tree", "--dry-run", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "--dry-run", &tree].as_slice(),
        ["read-tree", "-n", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "-n", &tree].as_slice(),
        ["read-tree", "-v", &tree].as_slice(),
        ["read-tree", "-v", "-v", &tree].as_slice(),
        ["read-tree", "-v", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "-v", &tree].as_slice(),
        ["read-tree", "--trivial", &tree].as_slice(),
        ["read-tree", "--trivial", "--trivial", &tree].as_slice(),
        ["read-tree", "--trivial", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "--trivial", &tree].as_slice(),
        ["read-tree", "--aggressive", &tree].as_slice(),
        ["read-tree", "--aggressive", "--aggressive", &tree].as_slice(),
        ["read-tree", "--aggressive", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "--aggressive", &tree].as_slice(),
        ["read-tree", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "-q", &tree].as_slice(),
        ["read-tree", "-q", "--quiet", &tree].as_slice(),
        ["read-tree", "-q", &tree].as_slice(),
        ["read-tree", "-q", "-q", &tree].as_slice(),
        ["read-tree", "--reset", &tree].as_slice(),
        ["read-tree", "--reset", "--reset", &tree].as_slice(),
        ["read-tree", "--reset", "--index-output=alt.index", &tree].as_slice(),
        ["read-tree", "--index-output=alt.index", "--reset", &tree].as_slice(),
        ["read-tree", "--no-sparse-checkout", &tree].as_slice(),
        [
            "read-tree",
            "--no-sparse-checkout",
            "--no-sparse-checkout",
            &tree,
        ]
        .as_slice(),
        ["read-tree", "--no-sparse-checkout", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "--no-sparse-checkout", &tree].as_slice(),
        ["read-tree", "--recurse-submodules", &tree].as_slice(),
        [
            "read-tree",
            "--recurse-submodules",
            "--recurse-submodules",
            &tree,
        ]
        .as_slice(),
        [
            "read-tree",
            "--recurse-submodules",
            "--no-recurse-submodules",
            &tree,
        ]
        .as_slice(),
        [
            "read-tree",
            "--recurse-submodules",
            "--no-recurse-submodules",
            "--recurse-submodules",
            &tree,
        ]
        .as_slice(),
        ["read-tree", "--no-recurse-submodules", &tree].as_slice(),
        [
            "read-tree",
            "--no-recurse-submodules",
            "--no-recurse-submodules",
            &tree,
        ]
        .as_slice(),
        [
            "read-tree",
            "--no-recurse-submodules",
            "--recurse-submodules",
            &tree,
        ]
        .as_slice(),
        [
            "read-tree",
            "--no-recurse-submodules",
            "--recurse-submodules",
            "--no-recurse-submodules",
            &tree,
        ]
        .as_slice(),
        ["read-tree", "-i", "--prefix=import/", &tree].as_slice(),
        ["read-tree", "-i", "-i", "--prefix=import/", &tree].as_slice(),
        ["read-tree", "-i", "--reset", &tree].as_slice(),
        ["read-tree", "-i", "-i", "--reset", &tree].as_slice(),
        [
            "read-tree",
            "-i",
            "--prefix=import/",
            "--index-output=alt.index",
            &tree,
        ]
        .as_slice(),
        [
            "read-tree",
            "-i",
            "--prefix=import/",
            "--index-output",
            "alt.index",
            &tree,
        ]
        .as_slice(),
        [
            "read-tree",
            "-i",
            "--prefix=import/",
            "--index-output=first.index",
            "--index-output=alt.index",
            &tree,
        ]
        .as_slice(),
    ] {
        let git_repo = clone_repo_fixture(tree_repo.path());
        let zmin_repo = clone_repo_fixture(tree_repo.path());
        git(git_repo.path(), ["read-tree", "--empty"]);
        run_zmin(zmin_repo.path(), ["read-tree", "--empty"]);
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin"),
            command_any_output("git", git_repo.path(), args, "git"),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(git_repo.path(), ["ls-files", "-s"]),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["write-tree"]),
            git(git_repo.path(), ["write-tree"]),
            "args: {args:?}"
        );
        if args.contains(&"--index-output=alt.index") {
            assert_eq!(
                command_output_with_env(
                    "git",
                    zmin_repo.path(),
                    &["write-tree"],
                    &[("GIT_INDEX_FILE", "alt.index")],
                    "git write-tree zmin alt index",
                )
                .1,
                command_output_with_env(
                    "git",
                    git_repo.path(),
                    &["write-tree"],
                    &[("GIT_INDEX_FILE", "alt.index")],
                    "git write-tree git alt index",
                )
                .1,
                "args: {args:?}"
            );
        }
    }

    for args in [
        ["read-tree", "-m", "-u", &tree].as_slice(),
        ["read-tree", "-m", "-u", "-u", &tree].as_slice(),
        ["read-tree", "--reset", "-u", &tree].as_slice(),
        ["read-tree", "-u", "-u", "--reset", &tree].as_slice(),
        ["read-tree", "--reset", "-u", "-u", &tree].as_slice(),
        ["read-tree", "-u", "--quiet", "--reset", &tree].as_slice(),
        ["read-tree", "--quiet", "-u", "--reset", &tree].as_slice(),
        ["read-tree", "-u", "--trivial", "--reset", &tree].as_slice(),
        ["read-tree", "-u", "--aggressive", "--reset", &tree].as_slice(),
        ["read-tree", "--prefix=import/", "-u", &tree].as_slice(),
        ["read-tree", "-u", "-u", "--prefix=import/", &tree].as_slice(),
        ["read-tree", "--prefix=import/", "-u", "-u", &tree].as_slice(),
        ["read-tree", "-u", "--quiet", "--prefix=import/", &tree].as_slice(),
        ["read-tree", "--quiet", "-u", "--prefix=import/", &tree].as_slice(),
        ["read-tree", "-m", "--trivial", "-u", &tree].as_slice(),
        ["read-tree", "-m", "--aggressive", "-u", &tree].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(tree_repo.path());
        let zmin_repo = clone_repo_fixture(tree_repo.path());
        fs::remove_file(git_repo.path().join("a.txt")).expect("remove git worktree a");
        let _ = fs::remove_file(git_repo.path().join("b.txt"));
        fs::remove_file(zmin_repo.path().join("a.txt")).expect("remove zmin worktree a");
        let _ = fs::remove_file(zmin_repo.path().join("b.txt"));
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin"),
            command_any_output(
                stock_git.to_str().expect("pinned Git path utf8"),
                git_repo.path(),
                args,
                "pinned Git",
            ),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(git_repo.path(), ["ls-files", "-s"]),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["write-tree"]),
            git(git_repo.path(), ["write-tree"]),
            "args: {args:?}"
        );
        if args.contains(&"--prefix=import/") {
            assert_eq!(
                fs::read(zmin_repo.path().join("import/a.txt")).expect("read zmin imported file"),
                fs::read(git_repo.path().join("import/a.txt")).expect("read git imported file"),
                "args: {args:?}"
            );
        } else {
            assert_eq!(
                fs::read(zmin_repo.path().join("a.txt")).expect("read zmin worktree file"),
                fs::read(git_repo.path().join("a.txt")).expect("read git worktree file"),
                "args: {args:?}"
            );
        }
    }

    {
        for args in [
            ["read-tree", "--index-output=alt.index", &tree].as_slice(),
            [
                "read-tree",
                "-u",
                "--index-output=alt.index",
                "--reset",
                &tree,
            ]
            .as_slice(),
            [
                "read-tree",
                "--index-output=alt.index",
                "-u",
                "--reset",
                &tree,
            ]
            .as_slice(),
            [
                "read-tree",
                "-u",
                "--index-output=alt.index",
                "--prefix=import/",
                &tree,
            ]
            .as_slice(),
            [
                "read-tree",
                "--index-output=alt.index",
                "-u",
                "--prefix=import/",
                &tree,
            ]
            .as_slice(),
        ] {
            let git_repo = clone_repo_fixture(tree_repo.path());
            let zmin_repo = clone_repo_fixture(tree_repo.path());
            git(git_repo.path(), ["read-tree", "--empty"]);
            run_zmin(zmin_repo.path(), ["read-tree", "--empty"]);
            if args.contains(&"-u") {
                let _ = fs::remove_file(git_repo.path().join("a.txt"));
                let _ = fs::remove_file(git_repo.path().join("b.txt"));
                let _ = fs::remove_file(zmin_repo.path().join("a.txt"));
                let _ = fs::remove_file(zmin_repo.path().join("b.txt"));
            }
            assert_eq!(
                command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin"),
                command_any_output("git", git_repo.path(), args, "git"),
                "args: {args:?}"
            );
            assert_eq!(
                run_zmin(zmin_repo.path(), ["write-tree"]),
                git(git_repo.path(), ["write-tree"]),
                "args: {args:?}"
            );
            assert_eq!(
                command_output_with_env(
                    "git",
                    zmin_repo.path(),
                    &["ls-files", "-s"],
                    &[("GIT_INDEX_FILE", "alt.index")],
                    "git ls-files zmin alt index",
                )
                .1,
                command_output_with_env(
                    "git",
                    git_repo.path(),
                    &["ls-files", "-s"],
                    &[("GIT_INDEX_FILE", "alt.index")],
                    "git ls-files git alt index",
                )
                .1,
                "args: {args:?}"
            );
        }
    }

    {
        let git_repo = clone_repo_fixture(tree_repo.path());
        let zmin_repo = clone_repo_fixture(tree_repo.path());
        git(git_repo.path(), ["read-tree", "--empty"]);
        run_zmin(zmin_repo.path(), ["read-tree", "--empty"]);
        for args in [
            ["read-tree", "-i", &tree].as_slice(),
            ["read-tree", "-i", "--index-output=alt.index", &tree].as_slice(),
            ["read-tree", "-u", &tree].as_slice(),
            ["read-tree", "-i", "-u", "--prefix=import/", &tree].as_slice(),
        ] {
            assert_eq!(
                run_zmin_failure_output(zmin_repo.path(), args),
                git_failure_output(git_repo.path(), args),
                "args: {args:?}"
            );
            assert_eq!(
                run_zmin(zmin_repo.path(), ["write-tree"]),
                git(git_repo.path(), ["write-tree"]),
                "args: {args:?}"
            );
        }
    }
}

#[test]
fn read_tree_confusing_paths_respect_protect_hfs_and_ntfs_like_stock_git() {
    let stock_git = required_pinned_stock_git();
    let git_repo = git_init();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join("file"), b"content\n").expect("write file");
        git(repo, ["add", "file"]);
        git_with_env(repo, ["commit", "-m", "base"]);
        git(repo, ["config", "core.protectHFS", "true"]);
        git(repo, ["config", "core.protectNTFS", "true"]);
    }

    let blob = git(git_repo.path(), ["rev-parse", "HEAD:file"]);
    let tree = git(git_repo.path(), ["rev-parse", "HEAD^{tree}"]);
    let blob_bytes = hex_to_bytes(&blob);
    let tree_bytes = hex_to_bytes(&tree);

    for (path, mode, object_bytes) in [
        (".", "100644", blob_bytes.as_slice()),
        ("..", "100644", blob_bytes.as_slice()),
        (".git", "100644", blob_bytes.as_slice()),
        (".GIT", "100644", blob_bytes.as_slice()),
        ("\u{200c}.Git", "100644", blob_bytes.as_slice()),
        ("\u{200d}.Git", "100644", blob_bytes.as_slice()),
        ("\u{feff}.Git", "100644", blob_bytes.as_slice()),
        (".gI\u{200c}T", "100644", blob_bytes.as_slice()),
        (".GiT\u{200c}", "100644", blob_bytes.as_slice()),
        ("git~1", "100644", blob_bytes.as_slice()),
        (".git. ", "100644", blob_bytes.as_slice()),
        (".\\\\.GIT\\\\foobar", "100644", blob_bytes.as_slice()),
        (".git\\\\foobar", "100644", blob_bytes.as_slice()),
        (".git...:alternate-stream", "100644", blob_bytes.as_slice()),
        (".", "040000", tree_bytes.as_slice()),
        ("..", "040000", tree_bytes.as_slice()),
        (".git", "040000", tree_bytes.as_slice()),
        (".GIT", "040000", tree_bytes.as_slice()),
        ("\u{200c}.Git", "040000", tree_bytes.as_slice()),
        ("\u{200d}.Git", "040000", tree_bytes.as_slice()),
        ("\u{feff}.Git", "040000", tree_bytes.as_slice()),
        (".gI\u{200c}T", "040000", tree_bytes.as_slice()),
        (".GiT\u{200c}", "040000", tree_bytes.as_slice()),
        ("git~1", "040000", tree_bytes.as_slice()),
        (".git. ", "040000", tree_bytes.as_slice()),
        (".\\\\.GIT\\\\foobar", "040000", tree_bytes.as_slice()),
        (".git\\\\foobar", "040000", tree_bytes.as_slice()),
        (".git...:alternate-stream", "040000", tree_bytes.as_slice()),
    ] {
        let mut raw = Vec::new();
        raw.extend_from_slice(mode.as_bytes());
        raw.push(b' ');
        raw.extend_from_slice(path.as_bytes());
        raw.push(0);
        raw.extend_from_slice(object_bytes);

        let git_tree = git_with_stdin_bytes(
            git_repo.path(),
            ["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
            &raw,
        );
        let zmin_tree = git_with_stdin_bytes(
            zmin_repo.path(),
            ["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
            &raw,
        );

        let stock_output = raw_command_output(
            stock_git.as_path(),
            git_repo.path(),
            &["read-tree", &git_tree],
            "pinned confusing-path read-tree",
        );
        let zmin_output = raw_command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["read-tree", &zmin_tree],
            "zmin confusing-path read-tree",
        );
        assert_eq!(
            (
                zmin_output.0,
                zmin_output.1,
                normalize_read_tree_temp_path(&zmin_output.2, zmin_repo.path()),
            ),
            (
                stock_output.0,
                stock_output.1,
                normalize_read_tree_temp_path(&stock_output.2, git_repo.path()),
            ),
            "confusing path raw tuple diverged for path {path:?} mode {mode}"
        );
        assert_ne!(
            stock_output.0, 0,
            "pinned Git unexpectedly accepted path {path:?} mode {mode}"
        );
    }

    #[cfg(not(windows))]
    {
        let mut raw = b"100644 foo\\bar".to_vec();
        raw.push(0);
        raw.extend_from_slice(&blob_bytes);
        let stock_tree = raw_command_output_with_stdin(
            stock_git.as_path(),
            git_repo.path(),
            &["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
            &raw,
            "pinned ordinary backslash tree",
        );
        let zmin_tree = raw_command_output_with_stdin(
            stock_git.as_path(),
            zmin_repo.path(),
            &["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
            &raw,
            "pinned ordinary backslash zmin tree",
        );
        assert_eq!(stock_tree.0, 0);
        assert_eq!(zmin_tree.0, 0);
        let stock_tree_id = String::from_utf8(stock_tree.1)
            .expect("ordinary backslash stock tree id")
            .trim()
            .to_owned();
        let zmin_tree_id = String::from_utf8(zmin_tree.1)
            .expect("ordinary backslash zmin tree id")
            .trim()
            .to_owned();
        let stock_output = raw_command_output(
            stock_git.as_path(),
            git_repo.path(),
            &["read-tree", &stock_tree_id],
            "pinned ordinary backslash read-tree",
        );
        let zmin_output = raw_command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["read-tree", &zmin_tree_id],
            "zmin ordinary backslash read-tree",
        );
        assert_eq!(zmin_output, stock_output);
        assert_eq!(stock_output.0, 0);
    }
}

#[test]
fn read_tree_confusing_path_modes_match_pinned_git_for_sha1_and_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let stock_repo = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        configure_identity(stock_repo.path());
        fs::write(stock_repo.path().join("file"), b"content\n").expect("write mode matrix file");
        git(stock_repo.path(), ["add", "file"]);
        git_with_env(stock_repo.path(), ["commit", "-m", "base"]);
        let zmin_repo = clone_repo_fixture(stock_repo.path());
        let blob = git(stock_repo.path(), ["rev-parse", "HEAD:file"]);
        let blob_bytes = hex_to_bytes(&blob);

        for (protect_hfs, protect_ntfs) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            for repo in [stock_repo.path(), zmin_repo.path()] {
                git(
                    repo,
                    [
                        "config",
                        "core.protectHFS",
                        if protect_hfs { "true" } else { "false" },
                    ],
                );
                git(
                    repo,
                    [
                        "config",
                        "core.protectNTFS",
                        if protect_ntfs { "true" } else { "false" },
                    ],
                );
            }

            for path in [
                ".git",
                ".GIT",
                "git~1",
                ".git. ",
                ".gité",
                "\u{200d}.Git",
                "\u{feff}.Git",
                ".git\\foo",
            ] {
                let mut raw = b"100644 ".to_vec();
                raw.extend_from_slice(path.as_bytes());
                raw.push(0);
                raw.extend_from_slice(&blob_bytes);
                let stock_tree = git_with_stdin_bytes(
                    stock_repo.path(),
                    ["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
                    &raw,
                );
                let zmin_tree = git_with_stdin_bytes(
                    zmin_repo.path(),
                    ["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
                    &raw,
                );
                let stock_output = raw_command_output(
                    stock_git.as_path(),
                    stock_repo.path(),
                    &["read-tree", stock_tree.trim()],
                    "pinned read-tree protection mode",
                );
                let zmin_output = raw_command_output(
                    zmin_bin(),
                    zmin_repo.path(),
                    &["read-tree", zmin_tree.trim()],
                    "zmin read-tree protection mode",
                );
                assert_eq!(
                    zmin_output, stock_output,
                    "sha256={sha256}, protect_hfs={protect_hfs}, protect_ntfs={protect_ntfs}, path={path:?}"
                );
                let expected_rejected = match path {
                    ".git" | ".GIT" => true,
                    "git~1" | ".git. " => protect_ntfs,
                    ".gité" => false,
                    "\u{200d}.Git" | "\u{feff}.Git" => protect_hfs,
                    ".git\\foo" => {
                        protect_ntfs || (protect_hfs && cfg!(any(windows, target_os = "cygwin")))
                    }
                    _ => false,
                };
                assert_eq!(
                    stock_output.0 != 0,
                    expected_rejected,
                    "pinned protection mode expectation diverged for sha256={sha256}, protect_hfs={protect_hfs}, protect_ntfs={protect_ntfs}, path={path:?}"
                );
            }

            for raw_path in [b"foo\x80".as_slice(), b".git\x80".as_slice()] {
                let mut raw = b"100644 ".to_vec();
                raw.extend_from_slice(raw_path);
                raw.push(0);
                raw.extend_from_slice(&blob_bytes);
                let stock_tree = git_with_stdin_bytes(
                    stock_repo.path(),
                    ["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
                    &raw,
                );
                let zmin_tree = git_with_stdin_bytes(
                    zmin_repo.path(),
                    ["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
                    &raw,
                );
                let stock_output = raw_command_output(
                    stock_git.as_path(),
                    stock_repo.path(),
                    &["read-tree", stock_tree.trim()],
                    "pinned malformed-byte read-tree protection mode",
                );
                let zmin_output = raw_command_output(
                    zmin_bin(),
                    zmin_repo.path(),
                    &["read-tree", zmin_tree.trim()],
                    "zmin malformed-byte read-tree protection mode",
                );
                assert_eq!(
                    zmin_output, stock_output,
                    "sha256={sha256}, protect_hfs={protect_hfs}, protect_ntfs={protect_ntfs}, raw_path={raw_path:?}"
                );
                let expected_rejected = raw_path == b".git\x80" && protect_hfs;
                assert_eq!(
                    stock_output.0 != 0,
                    expected_rejected,
                    "pinned malformed-byte expectation diverged for sha256={sha256}, protect_hfs={protect_hfs}, protect_ntfs={protect_ntfs}, raw_path={raw_path:?}"
                );
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn read_tree_confusing_non_utf8_path_preserves_pinned_raw_diagnostic() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let stock_repo = TempDir::new().expect("stock raw path repo");
        let zmin_repo = TempDir::new().expect("zmin raw path repo");
        let init_args = if sha256 {
            ["init", "--object-format=sha256"]
        } else {
            ["init", "--object-format=sha1"]
        };
        for repo in [stock_repo.path(), zmin_repo.path()] {
            let init = raw_command_output(stock_git.as_path(), repo, &init_args, "pinned init");
            assert_eq!(init.0, 0, "sha256={sha256}");
            for (key, value) in [("core.protectHFS", "true"), ("core.protectNTFS", "true")] {
                let config = raw_command_output(
                    stock_git.as_path(),
                    repo,
                    &["config", key, value],
                    "pinned confusing-path config",
                );
                assert_eq!(config.0, 0, "sha256={sha256}, key={key}");
            }
        }
        let blob = raw_command_output_with_stdin(
            stock_git.as_path(),
            stock_repo.path(),
            &["hash-object", "-w", "--stdin"],
            b"content\n",
            "pinned raw-path blob",
        );
        assert_eq!(blob.0, 0, "sha256={sha256}");
        let blob_id = String::from_utf8(blob.1)
            .expect("pinned blob id utf8")
            .trim()
            .to_owned();
        let mut tree_input = b"100644 .git".to_vec();
        tree_input.push(0x80);
        tree_input.push(0);
        tree_input.extend_from_slice(&hex_to_bytes(&blob_id));
        let mut tree_ids = Vec::new();
        for repo in [stock_repo.path(), zmin_repo.path()] {
            let tree = raw_command_output_with_stdin(
                stock_git.as_path(),
                repo,
                &["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
                &tree_input,
                "pinned raw-path tree",
            );
            assert_eq!(tree.0, 0, "sha256={sha256}");
            tree_ids.push(
                String::from_utf8(tree.1)
                    .expect("pinned tree id utf8")
                    .trim()
                    .to_owned(),
            );
        }
        let stock_output = raw_command_output(
            stock_git.as_path(),
            stock_repo.path(),
            &["read-tree", &tree_ids[0]],
            "pinned raw confusing-path read-tree",
        );
        let zmin_output = raw_command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["read-tree", &tree_ids[1]],
            "zmin raw confusing-path read-tree",
        );
        assert_eq!(zmin_output, stock_output, "sha256={sha256}");
        assert_eq!(stock_output.0, 128, "sha256={sha256}");
        assert!(stock_output.1.is_empty(), "sha256={sha256}");
        assert!(!stock_output.2.is_empty(), "sha256={sha256}");
    }
}

#[test]
fn read_tree_update_worktree_collision_and_super_prefix_match_stock_git() {
    let setup_repo = |repo: &std::path::Path| {
        let _ = fs::remove_file(repo.join(".git/index"));
        fs::write(repo.join("a"), b"").expect("write fixture a");
        git(repo, ["update-index", "--add", "a"]);
        let tree_m = git(repo, ["write-tree"]);
        fs::remove_file(repo.join("a")).expect("remove fixture a");
        git(repo, ["update-index", "--remove", "a"]);
        fs::create_dir(repo.join("a")).expect("create fixture a dir");
        fs::write(repo.join("a/b"), b"").expect("write fixture a/b");
        let tree_h = git(repo, ["write-tree"]);
        (tree_h, tree_m)
    };

    for with_super_prefix in [false, true] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let (git_tree_h, git_tree_m) = setup_repo(git_repo.path());
        let (zmin_tree_h, zmin_tree_m) = setup_repo(zmin_repo.path());
        assert_eq!(git_tree_h, zmin_tree_h, "treeH fixture diverged");
        assert_eq!(git_tree_m, zmin_tree_m, "treeM fixture diverged");

        let owned_args = if with_super_prefix {
            vec![
                "read-tree".to_owned(),
                "--super-prefix".to_owned(),
                "fictional/".to_owned(),
                "-u".to_owned(),
                "-m".to_owned(),
                git_tree_h.clone(),
                git_tree_m.clone(),
            ]
        } else {
            vec![
                "read-tree".to_owned(),
                "-n".to_owned(),
                "-u".to_owned(),
                "-m".to_owned(),
                git_tree_h.clone(),
                git_tree_m.clone(),
            ]
        };
        let args = owned_args.iter().map(String::as_str).collect::<Vec<_>>();

        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), &args),
            git_failure_output(git_repo.path(), &args),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(git_repo.path(), ["ls-files", "-s"]),
            "args: {args:?}"
        );
        assert!(
            zmin_repo.path().join("a/b").is_file(),
            "zmin should preserve untracked a/b for args: {args:?}"
        );
        assert!(
            git_repo.path().join("a/b").is_file(),
            "git should preserve untracked a/b for args: {args:?}"
        );
    }
}

#[test]
fn read_tree_actual_super_prefix_collision_leaves_zero_partial_mutation_sha1_sha256() {
    let stock_git = required_pinned_stock_git();
    for sha256 in [false, true] {
        let setup = |repo: &Path| {
            let _ = fs::remove_file(repo.join(".git/index"));
            fs::write(repo.join("a"), b"file\n").expect("write collision file");
            git(repo, ["update-index", "--add", "a"]);
            let file_tree = git(repo, ["write-tree"]).trim().to_owned();
            fs::remove_file(repo.join("a")).expect("remove collision file");
            git(repo, ["update-index", "--remove", "a"]);
            fs::create_dir(repo.join("a")).expect("create collision directory");
            fs::write(repo.join("a/b"), b"untracked\n").expect("write collision child");
            let directory_tree = git(repo, ["write-tree"]).trim().to_owned();
            (file_tree, directory_tree)
        };
        let stock_repo = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        let zmin_repo = if sha256 {
            pinned_git_init_sha256()
        } else {
            git_init()
        };
        let (stock_file_tree, stock_directory_tree) = setup(stock_repo.path());
        let (zmin_file_tree, zmin_directory_tree) = setup(zmin_repo.path());
        assert_eq!(stock_file_tree, zmin_file_tree, "sha256={sha256}");
        assert_eq!(stock_directory_tree, zmin_directory_tree, "sha256={sha256}");
        let stock_before = (
            git(stock_repo.path(), ["ls-files", "--stage"]),
            fs::read(stock_repo.path().join("a/b")).expect("read stock collision child"),
        );
        let zmin_before = (
            git(zmin_repo.path(), ["ls-files", "--stage"]),
            fs::read(zmin_repo.path().join("a/b")).expect("read zmin collision child"),
        );
        let stock_output = raw_command_output(
            stock_git.as_path(),
            stock_repo.path(),
            &[
                "read-tree",
                "--super-prefix",
                "fictional/",
                "-u",
                "-m",
                stock_directory_tree.as_str(),
                stock_file_tree.as_str(),
            ],
            "pinned actual super-prefix collision",
        );
        let zmin_output = raw_command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "read-tree",
                "--super-prefix",
                "fictional/",
                "-u",
                "-m",
                zmin_directory_tree.as_str(),
                zmin_file_tree.as_str(),
            ],
            "zmin actual super-prefix collision",
        );
        assert_eq!(zmin_output, stock_output, "sha256={sha256}");
        assert_ne!(stock_output.0, 0, "collision unexpectedly succeeded");
        assert_eq!(
            (
                git(stock_repo.path(), ["ls-files", "--stage"]),
                fs::read(stock_repo.path().join("a/b")).expect("read stock post collision child")
            ),
            stock_before,
            "pinned collision partially mutated state"
        );
        assert_eq!(
            (
                git(zmin_repo.path(), ["ls-files", "--stage"]),
                fs::read(zmin_repo.path().join("a/b")).expect("read zmin post collision child")
            ),
            zmin_before,
            "zmin collision partially mutated state"
        );
    }
}

#[test]
fn read_tree_three_way_current_index_matrix_matches_stock_git() {
    let setup_repo = |repo: &std::path::Path| {
        let _ = fs::remove_file(repo.join(".git/index"));
        fs::write(repo.join("f"), b"base\n").expect("write base");
        git(repo, ["update-index", "--add", "f"]);
        let tree_o = git(repo, ["write-tree"]);
        let tree_a = tree_o.clone();
        fs::write(repo.join("f"), b"theirs\n").expect("write theirs");
        git(repo, ["update-index", "f"]);
        let tree_b = git(repo, ["write-tree"]);
        (tree_o, tree_a, tree_b)
    };

    {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let (git_tree_o, git_tree_a, git_tree_b) = setup_repo(git_repo.path());
        let (zmin_tree_o, zmin_tree_a, zmin_tree_b) = setup_repo(zmin_repo.path());
        assert_eq!(
            (git_tree_o.clone(), git_tree_a.clone(), git_tree_b.clone()),
            (
                zmin_tree_o.clone(),
                zmin_tree_a.clone(),
                zmin_tree_b.clone()
            )
        );

        for repo in [git_repo.path(), zmin_repo.path()] {
            let _ = fs::remove_file(repo.join(".git/index"));
            fs::write(repo.join("f"), b"theirs\n").expect("write current b");
            git(repo, ["update-index", "--add", "f"]);
            fs::write(repo.join("f"), b"theirs\nextra\n").expect("dirty worktree");
        }

        let args = [
            "read-tree",
            "-m",
            git_tree_o.as_str(),
            git_tree_a.as_str(),
            git_tree_b.as_str(),
        ];
        run_zmin(zmin_repo.path(), args);
        git(git_repo.path(), args);
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(git_repo.path(), ["ls-files", "-s"])
        );
        assert_eq!(
            fs::read(zmin_repo.path().join("f")).expect("read zmin worktree"),
            fs::read(git_repo.path().join("f")).expect("read git worktree")
        );
    }

    {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let (git_tree_o, git_tree_a, git_tree_b) = setup_repo(git_repo.path());
        let (zmin_tree_o, zmin_tree_a, zmin_tree_b) = setup_repo(zmin_repo.path());
        assert_eq!(
            (git_tree_o.clone(), git_tree_a.clone(), git_tree_b.clone()),
            (
                zmin_tree_o.clone(),
                zmin_tree_a.clone(),
                zmin_tree_b.clone()
            )
        );

        for repo in [git_repo.path(), zmin_repo.path()] {
            let _ = fs::remove_file(repo.join(".git/index"));
            fs::write(repo.join("f"), b"other\n").expect("write mismatched current");
            git(repo, ["update-index", "--add", "f"]);
        }

        let args = [
            "read-tree",
            "-m",
            git_tree_o.as_str(),
            git_tree_a.as_str(),
            git_tree_b.as_str(),
        ];
        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), &args),
            git_failure_output(git_repo.path(), &args)
        );
    }
}

#[test]
fn read_tree_three_way_directory_file_conflicts_match_stock_git() {
    let setup_repo = |repo: &std::path::Path| {
        let make_tree = |name: &str, items: &[&str]| {
            let _ = fs::remove_file(repo.join(".git/index"));
            let _ = fs::remove_file(repo.join(".git/index.lock"));
            git(repo, ["clean", "-d", "-f", "-f", "-q", "-x"]);
            for item in items {
                let path = item.split(':').next().expect("path component");
                if let Some(parent) = std::path::Path::new(path).parent() {
                    fs::create_dir_all(repo.join(parent)).expect("create parent dirs");
                }
                fs::write(repo.join(path), format!("{item}\n")).expect("write tree entry");
                git(repo, ["update-index", "--add", path]);
            }
            git(repo, ["tag", name, &git(repo, ["write-tree"])]);
        };

        make_tree("O-000", &["a/b-2/c/d", "a/b/c/d", "a/x"]);
        make_tree("A-000", &["a/b-2/c/d", "a/b/c/d", "a/x"]);
        make_tree("A-001", &["a/b-2/c/d", "a/b/c/d", "a/b/c/e", "a/x"]);
        make_tree("B-000", &["a/b-2/c/d", "a/b", "a/x"]);
        make_tree("O-010", &["t-0", "t/1", "t/2", "t=3"]);
        make_tree("A-010", &["t-0", "t", "t=3"]);
        make_tree("B-010", &["t/1:", "t=3:"]);
    };

    let set_tree = |repo: &std::path::Path, tree: &str| {
        let _ = fs::remove_file(repo.join(".git/index"));
        let _ = fs::remove_file(repo.join(".git/index.lock"));
        git(repo, ["clean", "-d", "-f", "-f", "-q", "-x"]);
        git(repo, ["read-tree", tree]);
        git(repo, ["checkout-index", "-f", "-q", "-u", "-a"]);
        git(repo, ["update-index", "--refresh"]);
    };

    let git_repo = git_init();
    let zmin_repo = git_init();
    setup_repo(git_repo.path());
    setup_repo(zmin_repo.path());

    for (tree, args) in [
        (
            "A-000",
            vec!["read-tree", "-m", "-u", "O-000", "A-000", "B-000"],
        ),
        (
            "A-001",
            vec!["read-tree", "-m", "-u", "O-000", "A-001", "B-000"],
        ),
        (
            "A-010",
            vec!["read-tree", "-m", "-u", "O-010", "A-010", "B-010"],
        ),
    ] {
        set_tree(git_repo.path(), tree);
        set_tree(zmin_repo.path(), tree);
        run_zmin_args(zmin_repo.path(), &args);
        let _ = command_any_output("git", git_repo.path(), &args, "read-tree");
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(git_repo.path(), ["ls-files", "-s"]),
            "args: {args:?}"
        );
    }
}

#[test]
fn mktree_matches_stock_git_for_text_nul_and_batch_input() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"a\n").expect("write a");
    fs::write(repo.path().join("b.txt"), b"b\n").expect("write b");
    let a = git(repo.path(), ["hash-object", "-w", "a.txt"]);
    let b = git(repo.path(), ["hash-object", "-w", "b.txt"]);

    let input = format!("100644 blob {b}\tb.txt\n100644 blob {a}\ta.txt\n");
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktree"], &input),
        git_with_stdin(repo.path(), ["mktree"], &input)
    );

    let nul_input = format!("100644 blob {a}\ta.txt\0");
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktree", "-z"], &nul_input),
        git_with_stdin(repo.path(), ["mktree", "-z"], &nul_input)
    );

    let batch_input = format!(
        "100644 blob {a}\ta.txt\n\n100644 blob {b}\tb.txt\n160000 commit 1111111111111111111111111111111111111111\tsub\n"
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktree", "--batch"], &batch_input),
        git_with_stdin(repo.path(), ["mktree", "--batch"], &batch_input)
    );
}

#[test]
fn mktag_matches_stock_git_for_valid_tag_object() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"a\n").expect("write a");
    let blob = git(repo.path(), ["hash-object", "-w", "a.txt"]);
    let input = format!(
        "object {blob}\ntype blob\ntag v1\ntagger Bench <bench@example.test> 1700000000 +0000\n\ntag message\n"
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktag"], &input),
        git_with_stdin(repo.path(), ["mktag"], &input)
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktag", "--strict"], &input),
        git_with_stdin(repo.path(), ["mktag", "--strict"], &input)
    );
}
