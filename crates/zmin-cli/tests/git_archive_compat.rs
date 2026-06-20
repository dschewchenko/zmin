mod common;

use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use common::{
    command_stdout_bytes, configure_identity, git, git_failure_output, git_status, git_with_env,
    git_with_stdin_bytes, run_zmin, run_zmin_failure_output, run_zmin_status,
    run_zmin_with_stdin_bytes, zmin_bin,
};

fn zmin_upload_archive_exec() -> String {
    let path = zmin_bin();
    #[cfg(windows)]
    let path = path.replace('\\', "/");
    #[cfg(not(windows))]
    let path = path.to_owned();
    format!("--exec={path} upload-archive")
}

#[test]
fn get_tar_commit_id_matches_stock_git_archive_metadata() {
    let repo = common::git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let archive = command_stdout_bytes("git", repo.path(), &["archive", "--format=tar", "HEAD"]);

    assert_eq!(
        run_zmin_with_stdin_bytes(repo.path(), ["get-tar-commit-id"], &archive),
        git_with_stdin_bytes(repo.path(), ["get-tar-commit-id"], &archive)
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["get-tar-commit-id"]),
        git_status(repo.path(), ["get-tar-commit-id"])
    );
}

#[test]
fn archive_matches_stock_git_for_tar_prefix_and_paths() {
    let repo = common::git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("dir/a.txt"), b"hello\n").expect("write file");
    fs::write(repo.path().join("exe.sh"), b"#!/bin/sh\n").expect("write exe");
    make_executable(&repo.path().join("exe.sh"));
    #[cfg(unix)]
    std::os::unix::fs::symlink("dir/a.txt", repo.path().join("link")).expect("create symlink");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "archive fixture"]);
    fs::write(repo.path().join("extra.txt"), b"extra\n").expect("write extra");

    run_zmin(
        repo.path(),
        [
            "archive",
            "--format=tar",
            "--prefix=pre/",
            "--add-file=extra.txt",
            "--add-virtual-file=virt/path.txt:virtual",
            "--mtime=2024-01-02 03:04:05 +0000",
            "-o",
            "zmin.tar",
            "HEAD",
        ],
    );
    git(
        repo.path(),
        [
            "archive",
            "--format=tar",
            "--prefix=pre/",
            "--add-file=extra.txt",
            "--add-virtual-file=virt/path.txt:virtual",
            "--mtime=2024-01-02 03:04:05 +0000",
            "-o",
            "git.tar",
            "HEAD",
        ],
    );
    let zmin_tar = fs::read(repo.path().join("zmin.tar")).expect("read zmin tar");
    let git_tar = fs::read(repo.path().join("git.tar")).expect("read git tar");
    assert_eq!(
        run_zmin_with_stdin_bytes(repo.path(), ["get-tar-commit-id"], &zmin_tar),
        git_with_stdin_bytes(repo.path(), ["get-tar-commit-id"], &git_tar)
    );
    assert_eq!(
        git(repo.path(), ["archive", "--list"]),
        run_zmin(repo.path(), ["archive", "--list"])
    );
    assert_eq!(
        tar_listing(repo.path(), "zmin.tar"),
        tar_listing(repo.path(), "git.tar")
    );
    fs::create_dir(repo.path().join("zmin-extract")).expect("create zmin extract");
    fs::create_dir(repo.path().join("git-extract")).expect("create git extract");
    extract_tar(repo.path(), "zmin.tar", "zmin-extract");
    extract_tar(repo.path(), "git.tar", "git-extract");
    assert_eq!(
        fs::read(repo.path().join("zmin-extract/pre/dir/a.txt")).expect("read zmin file"),
        fs::read(repo.path().join("git-extract/pre/dir/a.txt")).expect("read git file")
    );
    assert_eq!(
        fs::read(repo.path().join("zmin-extract/pre/extra.txt")).expect("read zmin extra"),
        fs::read(repo.path().join("git-extract/pre/extra.txt")).expect("read git extra")
    );
    assert_eq!(
        fs::read(repo.path().join("zmin-extract/virt/path.txt")).expect("read zmin virtual"),
        fs::read(repo.path().join("git-extract/virt/path.txt")).expect("read git virtual")
    );
    #[cfg(unix)]
    assert_eq!(
        fs::read_link(repo.path().join("zmin-extract/pre/link")).expect("read zmin link"),
        fs::read_link(repo.path().join("git-extract/pre/link")).expect("read git link")
    );

    run_zmin(
        repo.path(),
        [
            "archive",
            "--format=tar",
            "--prefix=pre/",
            "-o",
            "zmin-dir.tar",
            "HEAD",
            "dir",
        ],
    );
    git(
        repo.path(),
        [
            "archive",
            "--format=tar",
            "--prefix=pre/",
            "-o",
            "git-dir.tar",
            "HEAD",
            "dir",
        ],
    );
    assert_eq!(
        tar_listing(repo.path(), "zmin-dir.tar"),
        tar_listing(repo.path(), "git-dir.tar")
    );
}

#[test]
fn archive_matches_stock_git_for_zip_and_tgz_formats() {
    let repo = common::git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("dir/a.txt"), b"hello\n").expect("write file");
    fs::write(repo.path().join("root.txt"), b"root\n").expect("write root");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "archive zip fixture"]);
    fs::write(repo.path().join("extra.txt"), b"extra\n").expect("write extra");

    for (format, zmin_name, git_name) in [
        ("zip", "zmin.zip", "git.zip"),
        ("tgz", "zmin.tgz", "git.tgz"),
        ("tar.gz", "zmin.tar.gz", "git.tar.gz"),
    ] {
        run_zmin(
            repo.path(),
            [
                "archive",
                &format!("--format={format}"),
                "--prefix=pre/",
                "--add-file=extra.txt",
                "--add-virtual-file=virt/path.txt:virtual",
                "--mtime=2024-01-02 03:04:05 +0000",
                "-o",
                zmin_name,
                "HEAD",
            ],
        );
        git(
            repo.path(),
            [
                "archive",
                &format!("--format={format}"),
                "--prefix=pre/",
                "--add-file=extra.txt",
                "--add-virtual-file=virt/path.txt:virtual",
                "--mtime=2024-01-02 03:04:05 +0000",
                "-o",
                git_name,
                "HEAD",
            ],
        );

        if format == "zip" {
            assert_eq!(
                zip_listing(repo.path(), zmin_name),
                zip_listing(repo.path(), git_name)
            );
            for path in [
                "pre/dir/a.txt",
                "pre/root.txt",
                "pre/extra.txt",
                "virt/path.txt",
            ] {
                assert_eq!(
                    command_stdout_bytes("unzip", repo.path(), &["-p", zmin_name, path]),
                    command_stdout_bytes("unzip", repo.path(), &["-p", git_name, path]),
                    "zip path: {path}"
                );
            }
        } else {
            assert_eq!(
                tar_listing(repo.path(), zmin_name),
                tar_listing(repo.path(), git_name)
            );
        }
    }

    assert_eq!(
        run_zmin_failure_output(repo.path(), &["archive", "--format=bad", "HEAD"]),
        git_failure_output(repo.path(), &["archive", "--format=bad", "HEAD"])
    );
}

#[test]
fn archive_mtime_parsing_matches_stock_git_for_common_formats() {
    let repo = common::git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write file");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "archive mtime fixture"]);

    for (idx, value) in [
        "1704164645",
        "@1704164645",
        "2024-01-02 03:04:05 +0000",
        "2024-01-02T03:04:05Z",
        "Jan 2 2024 03:04:05 +0000",
        "never",
    ]
    .iter()
    .enumerate()
    {
        let zmin_name = format!("zmin-mtime-{idx}.tar");
        let git_name = format!("git-mtime-{idx}.tar");
        let mtime_arg = format!("--mtime={value}");
        run_zmin(
            repo.path(),
            [
                "archive",
                "--format=tar",
                &mtime_arg,
                "-o",
                &zmin_name,
                "HEAD",
            ],
        );
        git(
            repo.path(),
            [
                "archive",
                "--format=tar",
                &mtime_arg,
                "-o",
                &git_name,
                "HEAD",
            ],
        );
        assert_eq!(
            tar_entry_mtime(
                &fs::read(repo.path().join(&zmin_name)).expect("read zmin tar"),
                "a.txt"
            ),
            tar_entry_mtime(
                &fs::read(repo.path().join(&git_name)).expect("read git tar"),
                "a.txt"
            ),
            "mtime value: {value}"
        );
    }
}

#[test]
fn archive_mtime_invalid_text_falls_back_to_current_time_like_stock_git() {
    let repo = common::git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write file");
    git(repo.path(), ["add", "-A"]);
    git_with_env(
        repo.path(),
        ["commit", "-m", "archive mtime invalid fixture"],
    );

    let before = unix_now().saturating_sub(2);
    run_zmin(
        repo.path(),
        [
            "archive",
            "--format=tar",
            "--mtime=bad",
            "-o",
            "zmin-bad-mtime.tar",
            "HEAD",
        ],
    );
    git(
        repo.path(),
        [
            "archive",
            "--format=tar",
            "--mtime=bad",
            "-o",
            "git-bad-mtime.tar",
            "HEAD",
        ],
    );
    let after = unix_now().saturating_add(2);
    for archive in ["zmin-bad-mtime.tar", "git-bad-mtime.tar"] {
        let mtime = tar_entry_mtime(
            &fs::read(repo.path().join(archive)).expect("read archive"),
            "a.txt",
        );
        assert!(
            (before..=after).contains(&mtime),
            "{archive} mtime {mtime} not in current-time window {before}..={after}"
        );
    }
}

#[test]
fn upload_archive_serves_stock_git_archive_remote_tar() {
    let repo = common::git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("dir/a.txt"), b"hello\n").expect("write file");
    fs::write(repo.path().join("root.txt"), b"root\n").expect("write root file");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "archive fixture"]);

    let remote = format!("--remote={}", repo.path().display());
    let exec = zmin_upload_archive_exec();
    let zmin_tar = command_stdout_bytes(
        "git",
        repo.path(),
        &[
            "archive",
            remote.as_str(),
            exec.as_str(),
            "--format=tar",
            "--prefix=pre/",
            "HEAD",
            "--",
            "dir",
        ],
    );
    let git_tar = command_stdout_bytes(
        "git",
        repo.path(),
        &[
            "archive",
            "--format=tar",
            "--prefix=pre/",
            "HEAD",
            "--",
            "dir",
        ],
    );
    fs::write(repo.path().join("zmin-remote.tar"), &zmin_tar).expect("write zmin tar");
    fs::write(repo.path().join("git-local.tar"), &git_tar).expect("write git tar");
    assert_eq!(
        tar_listing(repo.path(), "zmin-remote.tar"),
        tar_listing(repo.path(), "git-local.tar")
    );
    fs::create_dir(repo.path().join("zmin-remote-extract")).expect("create zmin extract");
    fs::create_dir(repo.path().join("git-local-extract")).expect("create git extract");
    extract_tar(repo.path(), "zmin-remote.tar", "zmin-remote-extract");
    extract_tar(repo.path(), "git-local.tar", "git-local-extract");
    assert_eq!(
        fs::read(repo.path().join("zmin-remote-extract/pre/dir/a.txt"))
            .expect("read zmin remote file"),
        fs::read(repo.path().join("git-local-extract/pre/dir/a.txt")).expect("read git local file")
    );
}

#[test]
fn upload_archive_accepts_remote_archive_options_like_stock_git() {
    let repo = common::git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("dir/a.txt"), b"hello\n").expect("write file");
    fs::write(repo.path().join("extra.txt"), b"extra\n").expect("write extra file");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "archive fixture"]);

    let remote = format!("--remote={}", repo.path().display());
    let exec = zmin_upload_archive_exec();
    let zmin_tar = command_stdout_bytes(
        "git",
        repo.path(),
        &[
            "archive",
            remote.as_str(),
            exec.as_str(),
            "--format=tar",
            "--prefix=pre/",
            "--add-file=extra.txt",
            "--add-virtual-file=virt/path.txt:virtual",
            "--mtime=2024-01-02 03:04:05 +0000",
            "HEAD",
            "--",
            "dir",
        ],
    );
    let git_tar = command_stdout_bytes(
        "git",
        repo.path(),
        &[
            "archive",
            "--format=tar",
            "--prefix=pre/",
            "--add-file=extra.txt",
            "--add-virtual-file=virt/path.txt:virtual",
            "--mtime=2024-01-02 03:04:05 +0000",
            "HEAD",
            "--",
            "dir",
        ],
    );
    fs::write(repo.path().join("zmin-remote-options.tar"), &zmin_tar).expect("write zmin tar");
    fs::write(repo.path().join("git-local-options.tar"), &git_tar).expect("write git tar");
    assert_eq!(
        tar_listing(repo.path(), "zmin-remote-options.tar"),
        tar_listing(repo.path(), "git-local-options.tar")
    );
    for archive in ["zmin-remote-options.tar", "git-local-options.tar"] {
        let destination = format!("{archive}.extract");
        fs::create_dir(repo.path().join(&destination)).expect("create extract dir");
        extract_tar(repo.path(), archive, &destination);
    }
    assert_eq!(
        fs::read(
            repo.path()
                .join("zmin-remote-options.tar.extract/pre/extra.txt")
        )
        .expect("read zmin add-file"),
        fs::read(
            repo.path()
                .join("git-local-options.tar.extract/pre/extra.txt")
        )
        .expect("read git add-file")
    );
    assert_eq!(
        fs::read(
            repo.path()
                .join("zmin-remote-options.tar.extract/virt/path.txt")
        )
        .expect("read zmin virtual"),
        fs::read(
            repo.path()
                .join("git-local-options.tar.extract/virt/path.txt")
        )
        .expect("read git virtual")
    );
}

fn tar_listing(cwd: &std::path::Path, path: &str) -> String {
    command_string("tar", cwd, &["-tf", path])
}

fn zip_listing(cwd: &std::path::Path, path: &str) -> String {
    command_string("unzip", cwd, &["-Z1", path])
}

fn extract_tar(cwd: &std::path::Path, archive: &str, destination: &str) {
    let output = Command::new("tar")
        .args(["-xf", archive, "-C", destination])
        .current_dir(cwd)
        .output()
        .expect("extract tar");
    assert!(
        output.status.success(),
        "tar failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn tar_entry_mtime(archive: &[u8], wanted: &str) -> u64 {
    let mut offset = 0usize;
    while offset + 512 <= archive.len() {
        let header = &archive[offset..offset + 512];
        if header.iter().all(|byte| *byte == 0) {
            break;
        }
        let name = tar_header_string(&header[..100]);
        let size = tar_octal(&header[124..136]) as usize;
        if name == wanted {
            return tar_octal(&header[136..148]);
        }
        let blocks = size.div_ceil(512);
        offset += 512 + blocks * 512;
    }
    panic!("tar entry {wanted} not found");
}

fn tar_header_string(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

fn tar_octal(bytes: &[u8]) -> u64 {
    let text = bytes
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    u64::from_str_radix(std::str::from_utf8(&text).expect("tar octal utf8"), 8).expect("tar octal")
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_secs()
}

fn command_string(command: &str, cwd: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{command} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path).expect("read mode").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("set executable");
}

#[cfg(not(unix))]
fn make_executable(_path: &std::path::Path) {}
