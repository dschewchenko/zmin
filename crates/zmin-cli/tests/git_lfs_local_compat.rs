mod common;

use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use common::{configure_identity, git, git_init, run_zmin, stock_git_bin, zmin_bin};
use zmin_git_core::{GitHashAlgorithm, GitObjectHash};

struct PinnedGitLfs {
    path_env: OsString,
    _alias_dir: tempfile::TempDir,
}

static PINNED_GIT_LFS: OnceLock<PinnedGitLfs> = OnceLock::new();

fn pinned_stock_git_lfs() -> &'static PinnedGitLfs {
    PINNED_GIT_LFS.get_or_init(|| {
        let (path, expected_version, expected_sha256) = if let Ok(manifest_path) =
            std::env::var("ZMIN_STOCK_GIT_LFS_MANIFEST")
        {
            let manifest_path = PathBuf::from(manifest_path);
            assert!(
                manifest_path.is_absolute(),
                "stock Git LFS manifest must be absolute"
            );
            let mut manifest_bytes = Vec::new();
            fs::File::open(&manifest_path)
                .expect("open stock Git LFS manifest")
                .take(64 * 1024 + 1)
                .read_to_end(&mut manifest_bytes)
                .expect("read stock Git LFS manifest");
            assert!(
                manifest_bytes.len() <= 64 * 1024,
                "stock Git LFS manifest is too large"
            );
            let manifest = String::from_utf8(manifest_bytes).expect("stock Git LFS manifest UTF-8");
            let mut values = std::collections::HashMap::new();
            for line in manifest.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    values.insert(key, value);
                }
            }
            let artifact = values.get("artifact").expect("manifest artifact");
            let path = std::env::var_os("ZMIN_STOCK_GIT_LFS")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    manifest_path
                        .parent()
                        .expect("manifest parent")
                        .join(artifact)
                });
            (
                path,
                std::env::var("ZMIN_STOCK_GIT_LFS_VERSION").unwrap_or_else(|_| {
                    values
                        .get("release_version")
                        .expect("manifest version")
                        .to_string()
                }),
                std::env::var("ZMIN_STOCK_GIT_LFS_SHA256").unwrap_or_else(|_| {
                    values
                        .get("binary_sha256")
                        .expect("manifest checksum")
                        .to_string()
                }),
            )
        } else {
            (
                PathBuf::from(
                    std::env::var("ZMIN_STOCK_GIT_LFS")
                        .expect("ZMIN_STOCK_GIT_LFS or ZMIN_STOCK_GIT_LFS_MANIFEST is required"),
                ),
                std::env::var("ZMIN_STOCK_GIT_LFS_VERSION")
                    .expect("ZMIN_STOCK_GIT_LFS_VERSION is required"),
                std::env::var("ZMIN_STOCK_GIT_LFS_SHA256")
                    .expect("ZMIN_STOCK_GIT_LFS_SHA256 is required"),
            )
        };
        assert!(
            path.is_absolute(),
            "stock Git LFS path must be absolute: {path:?}"
        );
        let metadata = fs::symlink_metadata(&path).expect("stat stock Git LFS");
        assert!(
            metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
            "stock Git LFS must be a regular non-symlink file: {path:?}"
        );
        let canonical = fs::canonicalize(&path).expect("canonicalize stock Git LFS");
        let mut file = fs::File::open(&canonical).expect("open stock Git LFS");
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha256);
        let mut buffer = [0_u8; 128 * 1024];
        loop {
            let read = file.read(&mut buffer).expect("read stock Git LFS");
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        assert_eq!(hasher.finalize().to_hex(), expected_sha256);
        let version = Command::new(&canonical)
            .arg("--version")
            .output()
            .expect("run stock Git LFS version");
        assert!(version.status.success());
        let version = String::from_utf8(version.stdout).expect("stock Git LFS version UTF-8");
        assert!(
            version
                .trim_start()
                .starts_with(&format!("git-lfs/{expected_version}")),
            "unexpected stock Git LFS version: {version:?}"
        );
        let alias_dir = tempfile::Builder::new()
            .prefix("zmin-pinned-git-lfs-")
            .tempdir()
            .expect("create pinned Git LFS alias directory");
        let alias_name = if cfg!(windows) {
            "git-lfs.exe"
        } else {
            "git-lfs"
        };
        let alias = alias_dir.path().join(alias_name);
        if fs::hard_link(&canonical, &alias).is_err() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&canonical, &alias)
                .expect("alias pinned Git LFS executable");
            #[cfg(windows)]
            fs::copy(&canonical, &alias).expect("copy pinned Git LFS executable");
        }
        let parent = canonical.parent().expect("stock Git LFS parent");
        let mut paths = vec![alias_dir.path().to_path_buf(), parent.to_path_buf()];
        for directory in std::env::var_os("PATH")
            .into_iter()
            .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        {
            if directory == parent {
                continue;
            }
            let lfs_name = if cfg!(windows) {
                "git-lfs.exe"
            } else {
                "git-lfs"
            };
            if directory.join(lfs_name).is_file() {
                continue;
            }
            paths.push(directory);
        }
        let path_env = std::env::join_paths(paths).expect("construct hermetic Git LFS PATH");
        PinnedGitLfs {
            path_env,
            _alias_dir: alias_dir,
        }
    })
}

fn test_command(command: &str, cwd: &Path, args: &[&str]) -> Command {
    let is_stock = command == "git"
        || Path::new(command).canonicalize().ok().as_deref() == Some(stock_git_bin());
    let program = if is_stock {
        stock_git_bin().as_os_str().to_owned()
    } else {
        OsString::from(command)
    };
    let mut process = Command::new(&program);
    process.args(args).current_dir(cwd);
    if is_stock {
        process.env("PATH", &pinned_stock_git_lfs().path_env);
    }
    process
}

fn command_any_output(
    command: &str,
    cwd: &Path,
    args: &[&str],
    label: &str,
) -> (i32, String, String) {
    let output = test_command(command, cwd, args)
        .output()
        .unwrap_or_else(|error| panic!("run {label}: {error}"));
    (
        output.status.code().expect("process exit code"),
        String::from_utf8(output.stdout)
            .expect("stdout UTF-8")
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .expect("stderr UTF-8")
            .trim_end_matches('\n')
            .to_owned(),
    )
}

fn command_any_output_with_stdin(
    command: &str,
    cwd: &Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> (i32, String, String) {
    let mut process = test_command(command, cwd, args);
    let mut child = process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn {label}: {error}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait for command");
    (
        output.status.code().expect("process exit code"),
        String::from_utf8(output.stdout)
            .expect("stdout UTF-8")
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .expect("stderr UTF-8")
            .trim_end_matches('\n')
            .to_owned(),
    )
}

fn command_stdout_bytes(command: &str, cwd: &Path, args: &[&str]) -> Vec<u8> {
    let output = test_command(command, cwd, args)
        .output()
        .expect("run command");
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn command_stdout_bytes_with_stdin(
    command: &str,
    cwd: &Path,
    args: &[&str],
    stdin: &[u8],
) -> Vec<u8> {
    let mut process = test_command(command, cwd, args);
    let mut child = process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn command");
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin)
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait command");
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn lfs_track_and_untrack_match_stock_git_for_basic_patterns() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let track_args = ["lfs", "track", "*.bin"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &track_args, "git lfs track"),
        command_any_output(zmin_bin(), zmin_repo.path(), &track_args, "zmin lfs track")
    );
    assert_eq!(
        fs::read_to_string(git_repo.path().join(".gitattributes")).expect("read git attributes"),
        fs::read_to_string(zmin_repo.path().join(".gitattributes")).expect("read zmin attributes")
    );

    assert_eq!(
        command_any_output(
            "git",
            git_repo.path(),
            &track_args,
            "git lfs track duplicate"
        ),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &track_args,
            "zmin lfs track duplicate"
        )
    );

    let untrack_args = ["lfs", "untrack", "*.bin"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &untrack_args, "git lfs untrack"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &untrack_args,
            "zmin lfs untrack"
        )
    );
    assert_eq!(
        fs::read_to_string(git_repo.path().join(".gitattributes")).expect("read git attributes"),
        fs::read_to_string(zmin_repo.path().join(".gitattributes")).expect("read zmin attributes")
    );
}

#[test]
fn lfs_track_rejects_control_injection_and_oversized_attributes() {
    let repo = git_init();
    let injected = command_any_output(
        zmin_bin(),
        repo.path(),
        &["lfs", "track", "bad\nfilter=exec"],
        "control pattern",
    );
    assert_ne!(injected.0, 0);
    assert!(!repo.path().join(".gitattributes").exists());

    fs::write(
        repo.path().join(".gitattributes"),
        vec![b'a'; 1024 * 1024 + 1],
    )
    .expect("write oversized attributes");
    let oversized = command_any_output(
        zmin_bin(),
        repo.path(),
        &["lfs", "track", "*.bin"],
        "oversized attributes",
    );
    assert_ne!(oversized.0, 0);
}

#[cfg(unix)]
#[test]
fn lfs_track_rejects_symlinked_attributes_without_touching_target() {
    use std::os::unix::fs::symlink;

    let repo = git_init();
    let outside = tempfile::TempDir::new().expect("outside temp");
    let outside_attributes = outside.path().join("attributes");
    fs::write(&outside_attributes, b"outside\n").expect("write outside attributes");
    symlink(&outside_attributes, repo.path().join(".gitattributes"))
        .expect("create attributes symlink");

    let output = command_any_output(
        zmin_bin(),
        repo.path(),
        &["lfs", "track", "*.bin"],
        "track symlinked attributes",
    );
    assert_ne!(output.0, 0);
    assert_eq!(
        fs::read(&outside_attributes).expect("read outside attributes"),
        b"outside\n"
    );
}

#[test]
fn lfs_install_does_not_clobber_custom_hook_without_force() {
    let repo = git_init();
    let hook = repo.path().join(".git/hooks/pre-push");
    let custom = b"#!/bin/sh\necho custom-hook\n";
    fs::write(&hook, custom).expect("write custom hook");

    let output = command_any_output(
        zmin_bin(),
        repo.path(),
        &["lfs", "install", "--local"],
        "install over custom hook",
    );
    assert_ne!(output.0, 0);
    assert_eq!(fs::read(&hook).expect("read custom hook"), custom);
}

#[test]
fn lfs_checkout_rejects_nonportable_and_escaping_paths() {
    let repo = git_init();
    for path in [
        "../outside",
        "a//b",
        "a/./b",
        "a/../b",
        "a\\b",
        "C:asset.bin",
    ] {
        let output = command_any_output(
            zmin_bin(),
            repo.path(),
            &["lfs", "checkout", path],
            "checkout invalid path",
        );
        assert_ne!(output.0, 0, "path should be rejected: {path}");
    }
}

#[test]
fn lfs_install_local_skip_repo_matches_stock_git_filter_config() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let args = ["lfs", "install", "--local", "--skip-repo"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs install"),
        command_any_output(zmin_bin(), zmin_repo.path(), &args, "zmin lfs install")
    );
    assert_zmin_filter_config(&zmin_repo.path(), false);
    assert!(!zmin_repo.path().join(".git/hooks/pre-push").exists());
}

#[test]
fn lfs_install_default_writes_global_scope_and_update_leaves_filters_untouched() {
    let repo = git_init();
    let root = tempfile::TempDir::new().expect("temp global config");
    let global = root.path().join("gitconfig");
    let global_value = global.to_str().expect("global config path");
    let output = zmin_lfs_any_output_with_env(
        repo.path(),
        &["lfs", "install", "--skip-repo"],
        &[
            ("GIT_CONFIG_GLOBAL", global_value),
            ("GIT_CONFIG_NOSYSTEM", "1"),
        ],
    );
    assert_eq!(output.0, 0, "{output:?}");
    let contents = fs::read_to_string(&global).expect("read global config");
    assert!(contents.contains("[filter \"lfs\"]"));
    assert!(
        !repo
            .path()
            .join(".git/config")
            .to_string_lossy()
            .contains("filter.lfs")
    );

    run_zmin(
        repo.path(),
        ["config", "--local", "filter.lfs.clean", "custom-clean %f"],
    );
    let before = run_zmin(
        repo.path(),
        ["config", "--local", "--get", "filter.lfs.clean"],
    );
    let update = command_any_output(zmin_bin(), repo.path(), &["lfs", "update"], "zmin update");
    assert_eq!(update.0, 0, "{update:?}");
    assert_eq!(
        run_zmin(
            repo.path(),
            ["config", "--local", "--get", "filter.lfs.clean"]
        ),
        before
    );
}

#[cfg(unix)]
#[test]
fn lfs_install_force_replaces_hook_symlink_without_following_target() {
    use std::os::unix::fs::symlink;

    let repo = git_init();
    let outside = tempfile::TempDir::new().expect("outside temp");
    let outside_hook = outside.path().join("outside-hook");
    fs::write(&outside_hook, b"outside\n").expect("write outside hook");
    let hook = repo.path().join(".git/hooks/pre-push");
    symlink(&outside_hook, &hook).expect("create hook symlink");
    let output = command_any_output(
        zmin_bin(),
        repo.path(),
        &["lfs", "install", "--local", "--force"],
        "zmin install force symlink",
    );
    assert_eq!(output.0, 0, "{output:?}");
    assert!(
        !fs::symlink_metadata(&hook)
            .expect("hook metadata")
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&outside_hook).expect("outside hook"), b"outside\n");
}

#[test]
fn lfs_install_local_skip_smudge_matches_stock_git_hook_side_effects() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let args = ["lfs", "install", "--local", "--skip-smudge"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs install skip-smudge"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs install skip-smudge"
        )
    );
    assert_zmin_filter_config(&zmin_repo.path(), true);
    let hook = fs::read_to_string(zmin_repo.path().join(".git/hooks/pre-push"))
        .expect("read zmin pre-push hook");
    assert!(hook.contains("# zmin-lfs-pre-push"));
    assert!(hook.contains("lfs pre-push \"$@\""));
    for hook_name in ["post-checkout", "post-commit", "post-merge"] {
        assert_eq!(
            git_repo.path().join(".git/hooks").join(hook_name).exists(),
            zmin_repo.path().join(".git/hooks").join(hook_name).exists(),
            "{hook_name}"
        );
    }
}

#[test]
fn lfs_install_worktree_matches_stock_git_current_side_effects() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let args = ["lfs", "install", "--worktree"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs install worktree"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs install worktree"
        )
    );
    assert_zmin_filter_config(&zmin_repo.path(), false);
    assert_eq!(
        git_repo.path().join(".git/config.worktree").exists(),
        zmin_repo.path().join(".git/config.worktree").exists()
    );
    let hook = fs::read_to_string(zmin_repo.path().join(".git/hooks/pre-push"))
        .expect("read zmin pre-push hook");
    assert!(hook.contains("# zmin-lfs-pre-push"));
    assert!(hook.contains("lfs pre-push \"$@\""));
    for hook_name in ["post-checkout", "post-commit", "post-merge"] {
        assert_eq!(
            git_repo.path().join(".git/hooks").join(hook_name).exists(),
            zmin_repo.path().join(".git/hooks").join(hook_name).exists(),
            "{hook_name}"
        );
    }
}

#[test]
fn lfs_install_worktree_skip_smudge_matches_stock_git_current_side_effects() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let args = ["lfs", "install", "--worktree", "--skip-smudge"];
    assert_eq!(
        command_any_output(
            "git",
            git_repo.path(),
            &args,
            "git lfs install worktree skip-smudge"
        ),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs install worktree skip-smudge"
        )
    );
    assert_zmin_filter_config(&zmin_repo.path(), true);
    let hook = fs::read_to_string(zmin_repo.path().join(".git/hooks/pre-push"))
        .expect("read zmin pre-push hook");
    assert!(hook.contains("# zmin-lfs-pre-push"));
    assert!(hook.contains("lfs pre-push \"$@\""));
    for hook_name in ["post-checkout", "post-commit", "post-merge"] {
        assert_eq!(
            git_repo.path().join(".git/hooks").join(hook_name).exists(),
            zmin_repo.path().join(".git/hooks").join(hook_name).exists(),
            "{hook_name}"
        );
    }
}

#[test]
fn lfs_install_respects_relative_core_hookspath_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    git(git_repo.path(), ["config", "core.hooksPath", ".githooks"]);
    run_zmin(zmin_repo.path(), ["config", "core.hooksPath", ".githooks"]);

    let args = ["lfs", "install", "--local", "--skip-smudge"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs install hookspath"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs install hookspath"
        )
    );

    for hook_name in ["pre-push", "post-checkout", "post-commit", "post-merge"] {
        assert_eq!(
            git_repo.path().join(".githooks").join(hook_name).exists(),
            zmin_repo.path().join(".githooks").join(hook_name).exists(),
            "{hook_name}"
        );
        assert_eq!(
            git_repo.path().join(".git/hooks").join(hook_name).exists(),
            zmin_repo.path().join(".git/hooks").join(hook_name).exists(),
            "{hook_name} .git/hooks"
        );
    }
    let hook = fs::read_to_string(zmin_repo.path().join(".githooks/pre-push"))
        .expect("read zmin hookspath pre-push hook");
    assert!(hook.contains("# zmin-lfs-pre-push"));
    assert!(hook.contains("lfs pre-push \"$@\""));
}

#[test]
fn lfs_install_manual_modes_match_stock_git() {
    let zmin_repo = git_init();

    for args in [
        ["lfs", "install", "--manual"].as_slice(),
        ["lfs", "install", "--manual", "--skip-smudge"].as_slice(),
        ["lfs", "install", "--manual", "--local"].as_slice(),
    ] {
        let output = command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            args,
            "zmin lfs install manual",
        );
        assert_eq!(output.0, 0, "{args:?}: {output:?}");
        assert!(output.1.contains(zmin_bin()));
        assert!(!output.1.contains("git-lfs"));
    }
}

#[test]
fn lfs_install_manual_respects_relative_core_hookspath_like_stock_git() {
    let zmin_repo = git_init();
    run_zmin(zmin_repo.path(), ["config", "core.hooksPath", ".githooks"]);

    let args = ["lfs", "install", "--manual"];
    let output = command_any_output(
        zmin_bin(),
        zmin_repo.path(),
        &args,
        "zmin lfs install manual hookspath",
    );
    assert_eq!(output.0, 0);
    assert!(output.1.contains(".githooks/pre-push"));
    assert!(output.1.contains(zmin_bin()));
    assert!(!output.1.contains("git-lfs"));
}

#[test]
fn lfs_install_invalid_flag_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let args = ["lfs", "install", "--bogus"];
    let stock = command_any_output("git", git_repo.path(), &args, "git lfs install invalid");
    let zmin = command_any_output(
        zmin_bin(),
        zmin_repo.path(),
        &args,
        "zmin lfs install invalid",
    );
    assert_eq!((stock.0, stock.2), (zmin.0, zmin.2));
    assert!(zmin.1.contains("--global:"));
}

#[test]
fn lfs_install_ignores_extra_positional_argument_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let args = ["lfs", "install", "oops"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs install extra arg"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs install extra arg"
        )
    );
}

#[test]
fn lfs_install_manual_modes_do_not_depend_on_stock_git_runtime() {
    let repo = git_init();
    let poisoned_envs = &[
        ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
        ("GIT_BIN", "/definitely/missing/git"),
    ];

    let manual =
        zmin_lfs_any_output_with_env(repo.path(), &["lfs", "install", "--manual"], poisoned_envs);
    assert_eq!(manual.0, 0);
    assert!(
        manual
            .1
            .contains("Add the following to '.git/hooks/pre-push':")
    );
    assert!(manual.1.contains(zmin_bin()));
    assert!(manual.1.contains("lfs pre-push \"$@\""));
    assert!(!manual.1.contains("git-lfs"));
    assert!(manual.1.ends_with("Git LFS initialized."));
    assert_eq!(manual.2, "");
}

#[test]
fn lfs_update_and_manual_modes_match_stock_git() {
    let zmin_repo = git_init();

    for args in [
        ["lfs", "update"].as_slice(),
        ["lfs", "update", "--force"].as_slice(),
        ["lfs", "update", "--manual"].as_slice(),
        ["lfs", "update", "-m"].as_slice(),
        ["lfs", "update", "-f"].as_slice(),
    ] {
        let output = command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin lfs update");
        assert_eq!(output.0, 0, "{args:?}: {output:?}");
        if args.contains(&"--manual") || args.contains(&"-m") {
            assert!(output.1.contains(zmin_bin()));
            assert!(!output.1.contains("git-lfs"));
        }
    }
}

#[test]
fn lfs_update_respects_relative_core_hookspath_and_matches_stock_git() {
    let zmin_repo = git_init();
    run_zmin(zmin_repo.path(), ["config", "core.hooksPath", ".githooks"]);

    for args in [
        ["lfs", "update"].as_slice(),
        ["lfs", "update", "--manual"].as_slice(),
    ] {
        let output = command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            args,
            "zmin lfs update hookspath",
        );
        assert_eq!(output.0, 0, "{args:?}: {output:?}");
        if args.contains(&"--manual") {
            assert!(output.1.contains(".githooks/pre-push"));
            assert!(output.1.contains(zmin_bin()));
            assert!(!output.1.contains("git-lfs"));
        }
    }
}

#[test]
fn lfs_update_invalid_flag_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let args = ["lfs", "update", "--bogus"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs update invalid"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs update invalid"
        )
    );
}

#[test]
fn lfs_ls_files_default_name_only_and_long_match_stock_git_for_pointer_entries() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        fs::write(
            repo.join("a.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:1111111111111111111111111111111111111111111111111111111111111111\nsize 3\n",
        )
        .expect("write pointer");
        git(repo, ["add", ".gitattributes", "a.bin"]);
        git(repo, ["commit", "-m", "base"]);
    }

    for args in [
        ["lfs", "ls-files"].as_slice(),
        ["lfs", "ls-files", "--name-only"].as_slice(),
        ["lfs", "ls-files", "--long"].as_slice(),
        ["lfs", "ls-files", "--size"].as_slice(),
    ] {
        assert_eq!(
            command_stdout_bytes("git", git_repo.path(), args),
            command_stdout_bytes(zmin_bin(), zmin_repo.path(), args),
            "{args:?}"
        );
    }
}

#[test]
fn lfs_ls_files_ref_argument_matches_stock_git_for_historical_pointer_tree() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        fs::write(
            repo.join("a.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:1111111111111111111111111111111111111111111111111111111111111111\nsize 3\n",
        )
        .expect("write base pointer");
        git(repo, ["add", ".gitattributes", "a.bin"]);
        git(repo, ["commit", "-m", "base"]);

        fs::write(
            repo.join("b.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:2222222222222222222222222222222222222222222222222222222222222222\nsize 4\n",
        )
        .expect("write second pointer");
        git(repo, ["add", "b.bin"]);
        git(repo, ["commit", "-m", "second"]);
    }

    for args in [
        ["lfs", "ls-files", "HEAD~1"].as_slice(),
        ["lfs", "ls-files", "--name-only", "HEAD~1"].as_slice(),
        ["lfs", "ls-files", "--long", "HEAD"].as_slice(),
        ["lfs", "ls-files", "--size", "HEAD~1"].as_slice(),
    ] {
        assert_eq!(
            command_stdout_bytes("git", git_repo.path(), args),
            command_stdout_bytes(zmin_bin(), zmin_repo.path(), args),
            "{args:?}"
        );
    }
}

#[test]
fn lfs_ls_files_two_ref_diff_matches_stock_git_for_changed_lfs_entries() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        fs::write(
            repo.join("a.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:1111111111111111111111111111111111111111111111111111111111111111\nsize 3\n",
        )
        .expect("write base pointer");
        git(repo, ["add", ".gitattributes", "a.bin"]);
        git(repo, ["commit", "-m", "base"]);

        fs::write(
            repo.join("a.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:2222222222222222222222222222222222222222222222222222222222222222\nsize 4\n",
        )
        .expect("write modified pointer");
        fs::write(
            repo.join("b.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:3333333333333333333333333333333333333333333333333333333333333333\nsize 5\n",
        )
        .expect("write added pointer");
        git(repo, ["add", "a.bin", "b.bin"]);
        git(repo, ["commit", "-m", "second"]);

        fs::remove_file(repo.join("b.bin")).expect("remove pointer");
        git(repo, ["rm", "b.bin"]);
        git(repo, ["commit", "-m", "third"]);
    }

    for args in [
        ["lfs", "ls-files", "HEAD~1", "HEAD"].as_slice(),
        ["lfs", "ls-files", "HEAD~2", "HEAD~1"].as_slice(),
        ["lfs", "ls-files", "HEAD~2", "HEAD"].as_slice(),
        ["lfs", "ls-files", "--name-only", "HEAD~2", "HEAD"].as_slice(),
        ["lfs", "ls-files", "--long", "HEAD~2", "HEAD"].as_slice(),
        ["lfs", "ls-files", "--size", "HEAD~2", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_stdout_bytes("git", git_repo.path(), args),
            command_stdout_bytes(zmin_bin(), zmin_repo.path(), args),
            "{args:?}"
        );
    }
}

#[test]
fn lfs_ls_files_history_modes_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        fs::write(
            repo.join("a.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:1111111111111111111111111111111111111111111111111111111111111111\nsize 3\n",
        )
        .expect("write base pointer");
        git(repo, ["add", ".gitattributes", "a.bin"]);
        git(repo, ["commit", "-m", "base"]);

        fs::write(
            repo.join("a.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:2222222222222222222222222222222222222222222222222222222222222222\nsize 4\n",
        )
        .expect("write modified pointer");
        fs::write(
            repo.join("b.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:3333333333333333333333333333333333333333333333333333333333333333\nsize 5\n",
        )
        .expect("write added pointer");
        git(repo, ["add", "a.bin", "b.bin"]);
        git(repo, ["commit", "-m", "second"]);

        fs::remove_file(repo.join("b.bin")).expect("remove pointer");
        git(repo, ["rm", "b.bin"]);
        git(repo, ["commit", "-m", "third"]);
    }

    for args in [
        ["lfs", "ls-files", "--all"].as_slice(),
        ["lfs", "ls-files", "--deleted"].as_slice(),
        ["lfs", "ls-files", "--deleted", "HEAD"].as_slice(),
        ["lfs", "ls-files", "--deleted", "HEAD~1"].as_slice(),
        ["lfs", "ls-files", "--debug"].as_slice(),
        ["lfs", "ls-files", "--json"].as_slice(),
        ["lfs", "ls-files", "--all", "--json"].as_slice(),
        ["lfs", "ls-files", "--deleted", "--json"].as_slice(),
        ["lfs", "ls-files", "--all", "--name-only"].as_slice(),
        ["lfs", "ls-files", "--deleted", "--name-only"].as_slice(),
    ] {
        assert_eq!(
            command_stdout_bytes("git", git_repo.path(), args),
            command_stdout_bytes(zmin_bin(), zmin_repo.path(), args),
            "{args:?}"
        );
    }
}

#[test]
fn lfs_ls_files_invalid_flag_and_history_conflicts_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for args in [
        ["lfs", "ls-files", "--cached"].as_slice(),
        ["lfs", "ls-files", "--modified"].as_slice(),
        ["lfs", "ls-files", "--all", "HEAD"].as_slice(),
        ["lfs", "ls-files", "--deleted", "HEAD~1", "HEAD"].as_slice(),
    ] {
        let stock = command_any_output(
            "git",
            git_repo.path(),
            args,
            "git lfs ls-files invalid/history",
        );
        let zmin = command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            args,
            "zmin lfs ls-files invalid/history",
        );
        assert_eq!(
            (stock.0, &stock.2),
            (zmin.0, &zmin.2),
            "{args:?}: stock={stock:?} zmin={zmin:?}"
        );
    }
}

#[test]
fn lfs_ls_files_invalid_ref_matches_stock_git_error_shape() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let stock_git = stock_git_bin().to_str().expect("stock git path");
    let args = ["lfs", "ls-files", "nosuch"];
    assert_eq!(
        command_any_output(
            stock_git,
            git_repo.path(),
            &args,
            "git lfs ls-files invalid ref"
        ),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs ls-files invalid ref"
        )
    );
}

#[test]
fn lfs_version_and_env_report_builtin_local_foundation_state() {
    let repo = git_init();

    let version = run_zmin(repo.path(), ["lfs", "version"]);
    assert!(version.starts_with("git-lfs/zmin (zmin "));
    assert!(version.contains("built-in local foundation"));

    run_zmin(repo.path(), ["lfs", "install", "--local", "--skip-smudge"]);
    let env = run_zmin(repo.path(), ["lfs", "env"]);
    let repo_path = fs::canonicalize(repo.path()).expect("canonicalize repo path");

    assert!(env.contains("git-lfs/zmin (zmin "));
    assert!(env.contains("git version "));
    assert!(env.contains(&format!("LocalWorkingDir={}", repo_path.display())));
    assert!(env.contains(&format!("LocalGitDir={}", repo_path.join(".git").display())));
    assert!(env.contains(&format!(
        "LocalMediaDir={}",
        repo_path.join(".git/lfs/objects").display()
    )));
    assert!(env.contains("ConcurrentTransfers=8"));
    assert!(env.contains("git config lfs.repositoryformatversion = 0"));
    assert!(env.contains("git config filter.lfs.process = "));
    assert!(env.contains(zmin_bin()));
    assert!(env.contains(" lfs filter-process --skip"));
    assert!(env.contains(" lfs smudge --skip -- %f"));
    assert!(env.contains(" lfs clean -- %f"));
    assert!(env.contains("git config filter.lfs.required = true"));

    run_zmin(repo.path(), ["config", "lfs.concurrentTransfers", "02"]);
    let env = run_zmin(repo.path(), ["lfs", "env"]);
    assert!(env.contains("ConcurrentTransfers=2"));
}

#[test]
fn lfs_pre_push_validates_update_stream_shape() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["commit", "--allow-empty", "-m", "base"]);
    git(repo.path(), ["remote", "add", "origin", "."]);

    let head = String::from_utf8(command_stdout_bytes(
        "git",
        repo.path(),
        &["rev-parse", "HEAD"],
    ))
    .expect("HEAD UTF-8");
    let valid = format!(
        "refs/heads/main {} refs/heads/main 0000000000000000000000000000000000000000\n",
        head.trim()
    );
    let (code, stdout, stderr) = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "pre-push", "origin", "."],
        &valid,
        "zmin lfs pre-push valid",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "");

    let (code, stdout, stderr) = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "pre-push"],
        "",
        "zmin lfs pre-push usage",
    );
    assert_eq!(code, 1);
    assert_eq!(
        stdout,
        "This should be run through Git's pre-push hook.  Run `git lfs update` to install it."
    );
    assert_eq!(stderr, "");
}

#[test]
fn lfs_pre_push_rejects_nonexistent_and_malformed_updates() {
    let repo = git_init();
    git(repo.path(), ["remote", "add", "origin", "."]);

    let nonexistent = "refs/heads/main 1111111111111111111111111111111111111111 refs/heads/main 0000000000000000000000000000000000000000\n";
    let (code, stdout, stderr) = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "pre-push", "origin", "."],
        nonexistent,
        "zmin lfs pre-push nonexistent object",
    );
    assert_eq!(code, 2);
    assert_eq!(stdout, "");
    assert!(stderr.starts_with("error:"));
    assert!(!stderr.contains("1111111111111111111111111111111111111111"));

    let (code, stdout, stderr) = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "pre-push", "origin", "."],
        "oops\n",
        "zmin lfs pre-push invalid line",
    );
    assert_eq!(code, 2);
    assert_eq!(stdout, "");
    assert!(stderr.starts_with("error:"));
    assert!(!stderr.contains("oops"));
}

#[test]
fn lfs_pre_push_uses_the_repository_sha256_object_format() {
    let repo = tempfile::TempDir::new().expect("SHA-256 repo");
    git(repo.path(), ["init", "--object-format=sha256"]);
    configure_identity(repo.path());
    git(repo.path(), ["commit", "--allow-empty", "-m", "base"]);
    git(repo.path(), ["remote", "add", "origin", "."]);
    let head = String::from_utf8(command_stdout_bytes(
        "git",
        repo.path(),
        &["rev-parse", "HEAD"],
    ))
    .expect("HEAD UTF-8");
    assert_eq!(head.trim().len(), 64);
    let update = format!(
        "refs/heads/main {} refs/heads/main {}\n",
        head.trim(),
        "0".repeat(64)
    );
    let (code, stdout, stderr) = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "pre-push", "origin", "."],
        &update,
        "zmin lfs pre-push SHA-256",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "");
}

#[test]
fn lfs_skip_push_only_short_circuits_pre_push_hook() {
    let repo = git_init();
    let env = &[("GIT_LFS_SKIP_PUSH", "true")];
    git(repo.path(), ["remote", "add", "origin", "."]);
    let push = zmin_lfs_any_output_with_env(repo.path(), &["lfs", "push", "origin", "HEAD"], env);
    assert_eq!(push.0, 2);
    assert!(push.1.is_empty());
    assert!(!push.2.is_empty());
    assert!(!push.2.contains("usage: git lfs push"));
    let pre_push = zmin_lfs_any_output_with_env_and_stdin(
        repo.path(),
        &[
            "lfs",
            "pre-push",
            "origin",
            "https://example.invalid/repo.git",
        ],
        env,
        "not parsed when pushing is disabled\n",
    );
    assert_eq!(pre_push, (0, String::new(), String::new()));
}

#[test]
fn lfs_pre_push_missing_remote_matches_stock_git_status_shape() {
    let repo = git_init();

    let stock = command_any_output_with_stdin(
        stock_git_bin().to_str().expect("stock git bin path"),
        repo.path(),
        &["lfs", "pre-push", "origin", "."],
        "oops\n",
        "stock git lfs pre-push missing remote",
    );
    let zmin = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "pre-push", "origin", "."],
        "oops\n",
        "zmin git lfs pre-push missing remote",
    );
    assert_eq!(zmin.0, stock.0);
}

#[test]
fn lfs_standard_hook_callbacks_match_stock_git_local_foundation_shape() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for args in [
        ["lfs", "post-commit"].as_slice(),
        ["lfs", "post-commit", "1"].as_slice(),
        ["lfs", "post-merge"].as_slice(),
        ["lfs", "post-merge", "1"].as_slice(),
        [
            "lfs",
            "post-checkout",
            "1111111111111111111111111111111111111111",
            "2222222222222222222222222222222222222222",
            "1",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_any_output("git", git_repo.path(), args, "git lfs hook callback"),
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin lfs hook callback"),
            "{args:?}"
        );
    }

    for args in [
        ["lfs", "post-checkout"].as_slice(),
        ["lfs", "post-checkout", "a", "b"].as_slice(),
        ["lfs", "post-checkout", "a", "b", "c", "d"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(
                "git",
                git_repo.path(),
                args,
                "git lfs hook callback invalid"
            ),
            command_any_output(
                zmin_bin(),
                zmin_repo.path(),
                args,
                "zmin lfs hook callback invalid"
            ),
            "{args:?}"
        );
    }
}

fn write_lfs_pointer(path: &std::path::Path, oid: &str, size: u64) {
    fs::write(
        path,
        format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n"),
    )
    .expect("write lfs pointer");
}

#[test]
fn lfs_checkout_without_args_materializes_local_objects_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    let oid = "2320bb89b3d79a57fe63ff8d2072dcc184c6d8df1869b975279b759e5845e009";
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        for name in ["a.bin", "b.bin"] {
            write_lfs_pointer(&repo.join(name), oid, 9);
        }
        git(repo, ["add", ".gitattributes", "a.bin", "b.bin"]);
        git(repo, ["commit", "-m", "base"]);
        fs::create_dir_all(repo.join(format!(".git/lfs/objects/{}/{}/", &oid[..2], &oid[2..4])))
            .expect("create lfs dir");
        fs::write(
            repo.join(format!(
                ".git/lfs/objects/{}/{}/{}",
                &oid[..2],
                &oid[2..4],
                oid
            )),
            b"REALDATA\n",
        )
        .expect("write local lfs object");
        for name in ["a.bin", "b.bin"] {
            write_lfs_pointer(&repo.join(name), oid, 9);
        }
    }

    let args = ["lfs", "checkout"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs checkout"),
        command_any_output(zmin_bin(), zmin_repo.path(), &args, "zmin lfs checkout")
    );

    assert_eq!(
        fs::read(git_repo.path().join("a.bin")).expect("read git a.bin"),
        fs::read(zmin_repo.path().join("a.bin")).expect("read zmin a.bin")
    );
    assert_eq!(
        fs::read(git_repo.path().join("b.bin")).expect("read git b.bin"),
        fs::read(zmin_repo.path().join("b.bin")).expect("read zmin b.bin")
    );
}

#[test]
fn lfs_checkout_path_argument_materializes_only_requested_file_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    let oid = "2320bb89b3d79a57fe63ff8d2072dcc184c6d8df1869b975279b759e5845e009";
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        write_lfs_pointer(&repo.join("a.bin"), oid, 9);
        write_lfs_pointer(&repo.join("b.bin"), oid, 9);
        git(repo, ["add", ".gitattributes", "a.bin", "b.bin"]);
        git(repo, ["commit", "-m", "base"]);
        fs::create_dir_all(repo.join(format!(".git/lfs/objects/{}/{}/", &oid[..2], &oid[2..4])))
            .expect("create lfs dir");
        fs::write(
            repo.join(format!(
                ".git/lfs/objects/{}/{}/{}",
                &oid[..2],
                &oid[2..4],
                oid
            )),
            b"REALDATA\n",
        )
        .expect("write local lfs object");
        write_lfs_pointer(&repo.join("a.bin"), oid, 9);
        write_lfs_pointer(&repo.join("b.bin"), oid, 9);
    }

    let args = ["lfs", "checkout", "a.bin"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs checkout path"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs checkout path"
        )
    );

    assert_eq!(
        fs::read(git_repo.path().join("a.bin")).expect("read git a.bin"),
        fs::read(zmin_repo.path().join("a.bin")).expect("read zmin a.bin")
    );
    assert_eq!(
        fs::read(git_repo.path().join("b.bin")).expect("read git b.bin"),
        fs::read(zmin_repo.path().join("b.bin")).expect("read zmin b.bin")
    );
}

#[test]
fn lfs_checkout_missing_local_object_matches_stock_git_shape() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    let oid = "1111111111111111111111111111111111111111111111111111111111111111";
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        write_lfs_pointer(&repo.join("a.bin"), oid, 9);
        git(repo, ["add", ".gitattributes", "a.bin"]);
        git(repo, ["commit", "-m", "base"]);
        write_lfs_pointer(&repo.join("a.bin"), oid, 9);
    }

    let args = ["lfs", "checkout"];
    assert_eq!(
        command_any_output(
            "git",
            git_repo.path(),
            &args,
            "git lfs checkout missing local"
        ),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs checkout missing local"
        )
    );
}

#[test]
fn lfs_checkout_rejects_corrupt_object_without_replacing_pointer() {
    let repo = git_init();
    configure_identity(repo.path());
    let oid = "2320bb89b3d79a57fe63ff8d2072dcc184c6d8df1869b975279b759e5845e009";
    let pointer = format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize 9\n");
    fs::write(repo.path().join("a.bin"), &pointer).expect("write pointer");
    git(repo.path(), ["add", "a.bin"]);
    git(repo.path(), ["commit", "-m", "pointer"]);
    let media = repo.path().join(format!(".git/lfs/objects/23/20/{oid}"));
    fs::create_dir_all(media.parent().expect("media parent")).expect("create media dir");
    fs::write(media, b"corrupt").expect("write corrupt object");

    let output = command_any_output(
        zmin_bin(),
        repo.path(),
        &["lfs", "checkout"],
        "corrupt checkout",
    );
    assert_ne!(output.0, 0);
    assert_eq!(
        fs::read(repo.path().join("a.bin")).expect("read pointer"),
        pointer.as_bytes()
    );
    assert!(!repo.path().join("a.bin.zmin-lfs-tmp").exists());
}

#[test]
fn lfs_pull_materializes_local_objects_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    let oid = "2320bb89b3d79a57fe63ff8d2072dcc184c6d8df1869b975279b759e5845e009";
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        write_lfs_pointer(&repo.join("a.bin"), oid, 9);
        git(repo, ["add", ".gitattributes", "a.bin"]);
        git(repo, ["commit", "-m", "base"]);
        fs::create_dir_all(repo.join(format!(".git/lfs/objects/{}/{}/", &oid[..2], &oid[2..4])))
            .expect("create lfs dir");
        fs::write(
            repo.join(format!(
                ".git/lfs/objects/{}/{}/{}",
                &oid[..2],
                &oid[2..4],
                oid
            )),
            b"REALDATA\n",
        )
        .expect("write local lfs object");
        write_lfs_pointer(&repo.join("a.bin"), oid, 9);
    }

    let args = ["lfs", "pull"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs pull"),
        command_any_output(zmin_bin(), zmin_repo.path(), &args, "zmin lfs pull")
    );
    assert_eq!(
        fs::read(git_repo.path().join("a.bin")).expect("read git a.bin"),
        fs::read(zmin_repo.path().join("a.bin")).expect("read zmin a.bin")
    );
}

#[test]
fn lfs_pull_fetches_from_local_remote_like_stock_git() {
    let root = tempfile::TempDir::new().expect("temp root");
    let remote = root.path().join("remote");
    let git_clone = root.path().join("git-clone");
    let zmin_clone = root.path().join("zmin-clone");
    let oid = "2320bb89b3d79a57fe63ff8d2072dcc184c6d8df1869b975279b759e5845e009";

    git(root.path(), ["init", "remote"]);
    configure_identity(&remote);
    fs::write(
        remote.join(".gitattributes"),
        b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("write remote attributes");
    write_lfs_pointer(&remote.join("a.bin"), oid, 9);
    git(&remote, ["add", ".gitattributes", "a.bin"]);
    git(&remote, ["commit", "-m", "base"]);
    fs::create_dir_all(remote.join(format!(".git/lfs/objects/{}/{}/", &oid[..2], &oid[2..4])))
        .expect("create remote lfs dir");
    fs::write(
        remote.join(format!(
            ".git/lfs/objects/{}/{}/{}",
            &oid[..2],
            &oid[2..4],
            oid
        )),
        b"REALDATA\n",
    )
    .expect("write remote lfs object");

    git(
        root.path(),
        [
            "clone",
            remote.to_str().expect("remote path"),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    let clone_out = command_any_output(
        zmin_bin(),
        root.path(),
        &[
            "clone",
            remote.to_str().expect("remote path"),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
        "zmin clone",
    );
    assert_eq!(clone_out.0, 0, "zmin clone stderr: {}", clone_out.2);

    for repo in [&git_clone, &zmin_clone] {
        configure_identity(repo);
        write_lfs_pointer(&repo.join("a.bin"), oid, 9);
        let local_media = repo.join(format!(
            ".git/lfs/objects/{}/{}/{}",
            &oid[..2],
            &oid[2..4],
            oid
        ));
        if local_media.exists() {
            fs::remove_file(local_media).expect("remove local lfs media");
        }
    }

    let args = ["lfs", "pull", "origin"];
    assert_eq!(
        command_any_output("git", &git_clone, &args, "git lfs pull origin"),
        command_any_output(zmin_bin(), &zmin_clone, &args, "zmin lfs pull origin")
    );
    assert_eq!(
        fs::read(git_clone.join("a.bin")).expect("read git pulled file"),
        fs::read(zmin_clone.join("a.bin")).expect("read zmin pulled file")
    );
}

fn zmin_lfs_any_output_with_env(
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> (i32, String, String) {
    let output = Command::new(zmin_bin())
        .args(args)
        .current_dir(cwd)
        .envs(envs.iter().copied())
        .output()
        .expect("run zmin lfs command");
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

fn zmin_lfs_any_output_with_env_and_stdin(
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
    stdin: &str,
) -> (i32, String, String) {
    let mut child = Command::new(zmin_bin())
        .args(args)
        .current_dir(cwd)
        .envs(envs.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zmin lfs command");
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait for zmin lfs command");
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

fn assert_zmin_filter_config(repo: &std::path::Path, skip_smudge: bool) {
    assert_eq!(
        run_zmin(
            repo,
            ["config", "--local", "--get", "lfs.repositoryformatversion"]
        ),
        "0"
    );
    assert_eq!(
        run_zmin(repo, ["config", "--local", "--get", "filter.lfs.required"]),
        "true"
    );
    let clean = run_zmin(repo, ["config", "--local", "--get", "filter.lfs.clean"]);
    assert!(clean.contains(zmin_bin()));
    assert!(clean.ends_with(" lfs clean -- %f"));
    let smudge = run_zmin(repo, ["config", "--local", "--get", "filter.lfs.smudge"]);
    assert!(smudge.contains(zmin_bin()));
    assert_eq!(smudge.ends_with(" lfs smudge --skip -- %f"), skip_smudge);
    if !skip_smudge {
        assert!(smudge.ends_with(" lfs smudge -- %f"));
    }
    let process = run_zmin(repo, ["config", "--local", "--get", "filter.lfs.process"]);
    assert!(process.contains(zmin_bin()));
    assert_eq!(process.ends_with(" lfs filter-process --skip"), skip_smudge);
    if !skip_smudge {
        assert!(process.ends_with(" lfs filter-process"));
    }
}

#[test]
fn lfs_local_foundation_commands_do_not_depend_on_stock_git_runtime() {
    let repo = git_init();
    configure_identity(repo.path());
    let poisoned_envs = &[
        ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
        ("GIT_BIN", "/definitely/missing/git"),
    ];

    fs::write(
        repo.path().join(".gitattributes"),
        b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("write attributes");
    fs::write(
        repo.path().join("a.bin"),
        b"version https://git-lfs.github.com/spec/v1\noid sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\nsize 3\n",
    )
    .expect("write pointer");
    git(repo.path(), ["add", ".gitattributes", "a.bin"]);
    git(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["remote", "add", "origin", "."]);

    let install = zmin_lfs_any_output_with_env(
        repo.path(),
        &["lfs", "install", "--local", "--skip-smudge"],
        poisoned_envs,
    );
    assert_eq!(install.0, 0);
    assert_eq!(install.2, "");

    let version = zmin_lfs_any_output_with_env(repo.path(), &["lfs", "version"], poisoned_envs);
    assert_eq!(version.0, 0);
    assert!(version.1.starts_with("git-lfs/zmin (zmin "));
    assert_eq!(version.2, "");

    let env = zmin_lfs_any_output_with_env(repo.path(), &["lfs", "env"], poisoned_envs);
    assert_eq!(env.0, 0);
    assert!(env.1.contains("git-lfs/zmin (zmin "));
    assert!(env.1.contains("git version 2.47.1.zmin"));
    assert!(env.1.contains("LocalWorkingDir="));
    assert_eq!(env.2, "");

    let ls_files = zmin_lfs_any_output_with_env(
        repo.path(),
        &["lfs", "ls-files", "--name-only"],
        poisoned_envs,
    );
    assert_eq!(ls_files.0, 0);
    assert_eq!(ls_files.1, "a.bin");
    assert_eq!(ls_files.2, "");

    fs::create_dir_all(repo.path().join(".git/lfs/objects/ba/78")).expect("create lfs object dir");
    fs::write(
        repo.path()
            .join(".git/lfs/objects/ba/78/ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        b"abc",
    )
    .expect("write local lfs media");
    let checkout = zmin_lfs_any_output_with_env(repo.path(), &["lfs", "checkout"], poisoned_envs);
    assert_eq!(checkout.0, 0);
    assert!(checkout.1.contains("Checking out LFS objects: 100%"));
    assert_eq!(checkout.2, "");
    assert_eq!(
        fs::read(repo.path().join("a.bin")).expect("read checked out file"),
        b"abc"
    );

    write_lfs_pointer(
        &repo.path().join("a.bin"),
        "2320bb89b3d79a57fe63ff8d2072dcc184c6d8df1869b975279b759e5845e009",
        9,
    );
    git(repo.path(), ["add", "a.bin"]);
    git(repo.path(), ["commit", "-m", "second pointer"]);
    fs::create_dir_all(repo.path().join(".git/lfs/objects/23/20"))
        .expect("create second lfs object dir");
    fs::write(
        repo.path()
            .join(".git/lfs/objects/23/20/2320bb89b3d79a57fe63ff8d2072dcc184c6d8df1869b975279b759e5845e009"),
        b"REALDATA\n",
    )
    .expect("write second local lfs media");
    let pull = zmin_lfs_any_output_with_env(repo.path(), &["lfs", "pull"], poisoned_envs);
    assert_eq!(pull.0, 0);
    assert_eq!(pull.1, "");
    assert_eq!(pull.2, "");
    assert_eq!(
        fs::read(repo.path().join("a.bin")).expect("read pulled file"),
        b"REALDATA\n"
    );

    for args in [
        ["lfs", "post-commit"].as_slice(),
        [
            "lfs",
            "post-checkout",
            "1111111111111111111111111111111111111111",
            "2222222222222222222222222222222222222222",
            "1",
        ]
        .as_slice(),
        ["lfs", "post-merge", "1"].as_slice(),
    ] {
        let output = zmin_lfs_any_output_with_env(repo.path(), args, poisoned_envs);
        assert_eq!(output.0, 0, "{args:?}");
        assert_eq!(output.2, "", "{args:?}");
    }

    let head = String::from_utf8(command_stdout_bytes(
        "git",
        repo.path(),
        &["rev-parse", "HEAD"],
    ))
    .expect("HEAD UTF-8");
    let update = format!(
        "refs/heads/main {} refs/heads/main 0000000000000000000000000000000000000000\n",
        head.trim()
    );
    let pre_push = zmin_lfs_any_output_with_env_and_stdin(
        repo.path(),
        &["lfs", "pre-push", "origin", "."],
        poisoned_envs,
        &update,
    );
    assert_eq!(pre_push.0, 0);
    assert_eq!(pre_push.1, "");
    assert_eq!(pre_push.2, "");
}

#[test]
fn lfs_install_reuses_existing_stock_style_lfs_hooks_without_stock_git_runtime() {
    let repo = git_init();
    configure_identity(repo.path());
    let poisoned_envs = &[
        ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
        ("GIT_BIN", "/definitely/missing/git"),
    ];

    let hooks_dir = repo.path().join(".git/hooks");
    fs::create_dir_all(&hooks_dir).expect("create hooks dir");
    for (hook_name, subcommand) in [
        ("pre-push", "pre-push"),
        ("post-checkout", "post-checkout"),
        ("post-commit", "post-commit"),
        ("post-merge", "post-merge"),
    ] {
        fs::write(
            hooks_dir.join(hook_name),
            format!(
                "#!/bin/sh\ncommand -v git-lfs >/dev/null 2>&1 || {{ printf >&2 \"\\n%s\\n\\n\" \"This repository is configured for Git LFS but 'git-lfs' was not found on your path. If you no longer wish to use Git LFS, remove this hook by deleting the '{hook_name}' file in the hooks directory (set by 'core.hookspath'; usually '.git/hooks').\"; exit 2; }}\ngit lfs {subcommand} \"$@\"\n"
            ),
        )
        .expect("write stock-style lfs hook");
    }

    let install = zmin_lfs_any_output_with_env(
        repo.path(),
        &["lfs", "install", "--local", "--skip-smudge"],
        poisoned_envs,
    );
    assert_eq!(install.0, 0);
    assert_eq!(install.1, "Updated Git hooks.\nGit LFS initialized.");
    assert_eq!(install.2, "");

    let pre_push_hook = fs::read_to_string(hooks_dir.join("pre-push")).expect("read pre-push hook");
    assert!(pre_push_hook.contains("# zmin-lfs-pre-push"));
    assert!(pre_push_hook.contains("lfs pre-push \"$@\""));
}

#[test]
fn lfs_install_reuses_existing_stock_style_lfs_hooks_in_custom_hookspath_without_stock_git_runtime()
{
    let repo = git_init();
    configure_identity(repo.path());
    run_zmin(repo.path(), ["config", "core.hooksPath", ".githooks"]);
    let poisoned_envs = &[
        ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
        ("GIT_BIN", "/definitely/missing/git"),
    ];

    let hooks_dir = repo.path().join(".githooks");
    fs::create_dir_all(&hooks_dir).expect("create custom hooks dir");
    for (hook_name, subcommand) in [
        ("pre-push", "pre-push"),
        ("post-checkout", "post-checkout"),
        ("post-commit", "post-commit"),
        ("post-merge", "post-merge"),
    ] {
        fs::write(
            hooks_dir.join(hook_name),
            format!(
                "#!/bin/sh\ncommand -v git-lfs >/dev/null 2>&1 || {{ printf >&2 \"\\n%s\\n\\n\" \"This repository is configured for Git LFS but 'git-lfs' was not found on your path. If you no longer wish to use Git LFS, remove this hook by deleting the '{hook_name}' file in the hooks directory (set by 'core.hookspath'; usually '.git/hooks').\"; exit 2; }}\ngit lfs {subcommand} \"$@\"\n"
            ),
        )
        .expect("write stock-style custom lfs hook");
    }

    let install = zmin_lfs_any_output_with_env(
        repo.path(),
        &["lfs", "install", "--local", "--skip-smudge"],
        poisoned_envs,
    );
    assert_eq!(install.0, 0);
    assert_eq!(install.1, "Updated Git hooks.\nGit LFS initialized.");
    assert_eq!(install.2, "");

    let pre_push_hook =
        fs::read_to_string(hooks_dir.join("pre-push")).expect("read custom pre-push hook");
    assert!(pre_push_hook.contains("# zmin-lfs-pre-push"));
    assert!(pre_push_hook.contains("lfs pre-push \"$@\""));
    assert!(!repo.path().join(".git/hooks/pre-push").exists());
}

#[test]
fn lfs_update_modes_do_not_depend_on_stock_git_runtime() {
    let repo = git_init();
    let poisoned_envs = &[
        ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
        ("GIT_BIN", "/definitely/missing/git"),
    ];

    let update = zmin_lfs_any_output_with_env(repo.path(), &["lfs", "update"], poisoned_envs);
    assert_eq!(update.0, 0);
    assert_eq!(update.1, "Updated Git hooks.");
    assert_eq!(update.2, "");

    let manual =
        zmin_lfs_any_output_with_env(repo.path(), &["lfs", "update", "--manual"], poisoned_envs);
    assert_eq!(manual.0, 0);
    assert!(
        manual
            .1
            .contains("Add the following to '.git/hooks/pre-push':")
    );
    assert!(manual.1.contains(zmin_bin()));
    assert!(manual.1.contains("lfs pre-push \"$@\""));
    assert!(!manual.1.contains("git-lfs"));
    assert_eq!(manual.2, "");
}

#[test]
fn lfs_local_clean_smudge_and_pointer_check_are_streaming_commands() {
    let repo = git_init();
    let clean = command_stdout_bytes_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "clean", "--", "asset.bin"],
        b"abc",
    );
    let pointer = b"version https://git-lfs.github.com/spec/v1\n\
oid sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n\
size 3\n";
    assert_eq!(clean, pointer);
    assert_eq!(
        fs::read(
            repo.path()
                .join(".git/lfs/objects/ba/78/ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        )
        .expect("read clean object"),
        b"abc"
    );

    let smudged = command_stdout_bytes_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "smudge", "--", "asset.bin"],
        pointer,
    );
    assert_eq!(smudged, b"abc");

    let (code, stdout, stderr) = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "pointer", "--check", "--stdin"],
        std::str::from_utf8(pointer).expect("pointer utf8"),
        "pointer check",
    );
    assert_eq!((code, stdout, stderr), (0, String::new(), String::new()));
    let noncanonical = std::str::from_utf8(pointer)
        .expect("pointer utf8")
        .trim_end_matches('\n');
    let (code, stdout, stderr) = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "pointer", "--check", "--strict", "--stdin"],
        noncanonical,
        "strict pointer check",
    );
    assert_eq!(code, 1);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "");

    let oversized = "x".repeat(1025);
    let (code, stdout, stderr) = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "pointer", "--check", "--stdin"],
        &oversized,
        "oversized pointer check",
    );
    assert_eq!((code, stdout, stderr), (1, String::new(), String::new()));

    let pointer_path = repo.path().join("asset.pointer");
    fs::write(&pointer_path, pointer).expect("write pointer fixture");
    let output = Command::new(zmin_bin())
        .args(["lfs", "pointer", "--check", "--strict", "--file"])
        .arg(&pointer_path)
        .current_dir(repo.path())
        .output()
        .expect("run file pointer check");
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn stock_git_uses_absolute_zmin_filter_process_for_binary_add_and_checkout() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(
        repo.path().join(".gitattributes"),
        b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("write attributes");
    run_zmin(repo.path(), ["lfs", "install", "--local"]);
    let clean = run_zmin(
        repo.path(),
        ["config", "--local", "--get", "filter.lfs.clean"],
    );
    assert!(clean.contains(zmin_bin()));
    assert!(!clean.contains("git-lfs"));

    let content = b"\0binary\npayload\0";
    fs::write(repo.path().join("asset.bin"), content).expect("write asset");
    git(repo.path(), ["add", ".gitattributes", "asset.bin"]);
    let stored_pointer = git(repo.path(), ["cat-file", "-p", ":asset.bin"]);
    assert_eq!(
        stored_pointer,
        "version https://git-lfs.github.com/spec/v1\n\
oid sha256:e35c2abde08f488dae76e13889d839c210f65bf61ce35d5be3b1e762cf3504d5\n\
size 16"
    );
    git(repo.path(), ["commit", "-m", "lfs binary"]);
    fs::remove_file(repo.path().join("asset.bin")).expect("remove worktree asset");
    git(repo.path(), ["checkout", "--", "asset.bin"]);
    assert_eq!(
        fs::read(repo.path().join("asset.bin")).expect("read checked out binary"),
        content
    );
}
