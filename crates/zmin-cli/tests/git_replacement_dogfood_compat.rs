mod common;

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use common::{
    command_any_output, command_output_with_env, configure_identity, git, git_status,
    stock_git_bin, write_file, zmin_bin,
};
use tempfile::TempDir;

fn write_git_shim(dir: &Path) -> std::path::PathBuf {
    #[cfg(windows)]
    let script_path = dir.join("git.cmd");
    #[cfg(not(windows))]
    let script_path = dir.join("git");

    #[cfg(windows)]
    let script = format!("@echo off\r\n\"{}\" %*\r\n", zmin_bin().replace('\\', "/"));
    #[cfg(not(windows))]
    let script = format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", zmin_bin());

    fs::write(&script_path, script).expect("write git shim");
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path)
            .expect("git shim metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).expect("chmod git shim");
    }
    script_path
}

fn sync_alias_verify_fixture() -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(dir.path(), ["init", "-b", "main", "repo"]);
    configure_identity(&repo);
    write_file(&repo, "tracked.txt", "one\n");
    git(&repo, ["add", "tracked.txt"]);
    git(&repo, ["commit", "-m", "initial"]);
    write_file(&repo, "tracked.txt", "two\n");
    git(&repo, ["commit", "-am", "second"]);
    write_file(&repo, "tracked.txt", "worktree\n");
    write_file(&repo, "untracked.txt", "new\n");
    dir
}

fn shim_command_output(cwd: &Path, shim_dir: &Path, args: &[&str]) -> (i32, String, String) {
    shim_command_output_with_env(cwd, shim_dir, args, &[])
}

fn shim_command_output_with_env(
    cwd: &Path,
    shim_dir: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> (i32, String, String) {
    let mut paths = vec![shim_dir.to_path_buf()];
    paths.extend(
        std::env::var_os("PATH")
            .iter()
            .flat_map(std::env::split_paths),
    );
    let path = std::env::join_paths(paths).expect("join shim PATH");
    let mut command = Command::new("git");
    command.args(args).current_dir(cwd).env("PATH", path);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.output().expect("run git shim");
    (
        output.status.code().expect("git shim exit code"),
        String::from_utf8(output.stdout)
            .expect("git shim stdout utf8")
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .expect("git shim stderr utf8")
            .trim_end_matches('\n')
            .to_owned(),
    )
}

#[test]
fn replacement_dogfood_smoke_script_passes_with_current_zmin_binary() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let script = workspace_root.join("tools/git-replacement-dogfood-smoke.sh");

    let output = Command::new("bash")
        .arg(&script)
        .current_dir(workspace_root)
        .env("ZMIN_BIN", zmin_bin())
        .env("ZMIN_STOCK_GIT", stock_git_bin())
        .output()
        .expect("run git replacement dogfood smoke");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("git_replacement_dogfood_smoke=ok"),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn sync_local_git_alias_script_verifies_observed_client_flows() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let script = workspace_root.join("tools/sync-local-git-alias.sh");
    let fixture = sync_alias_verify_fixture();
    let alias_dir = fixture.path().join("alias-bin");
    fs::create_dir_all(&alias_dir).expect("create alias dir");
    let alias_bin = alias_dir.join("git.zmin-bin");

    let output = Command::new("bash")
        .arg(&script)
        .current_dir(workspace_root)
        .env("ZMIN_SOURCE_BIN", zmin_bin())
        .env("ZMIN_LOCAL_GIT_BIN", &alias_bin)
        .env("ZMIN_STOCK_GIT", stock_git_bin())
        .env("ZMIN_VERIFY_REPO", fixture.path().join("repo"))
        .output()
        .expect("run local alias sync");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("verified observed show/status on:"),
        "stdout: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        alias_bin.exists(),
        "stdout: {}\nstderr: {}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn visible_worktree_file_contents(path: &Path) -> Vec<(String, Vec<u8>)> {
    fn collect(root: &Path, path: &Path, files: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(path).expect("read worktree dir") {
            let entry = entry.expect("read worktree entry");
            if entry.file_name() == ".git" {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                collect(root, &path, files);
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

    let mut files = Vec::new();
    collect(path, path, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn normalize_clone_reflog_output(text: &str) -> String {
    text.lines()
        .map(|line| {
            let mut normalized = if let Some((prefix, _)) = line.split_once(" clone: from ") {
                format!("{prefix} clone: from <normalized-remote>")
            } else {
                line.to_owned()
            };

            if let Some(start) = normalized.find("@{")
                && let Some(end_rel) = normalized[start + 2..].find('}')
            {
                let end = start + 2 + end_rel;
                normalized.replace_range(start + 2..end, "<normalized-date>");
            }

            normalized
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn reflog_lines_grouped_by_ref(text: &str) -> Vec<(String, Vec<String>)> {
    let mut grouped = std::collections::BTreeMap::<String, Vec<String>>::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let mut parts = line.splitn(3, ' ');
        let _object_id = parts.next().expect("reflog object id");
        let selector = parts.next().expect("reflog selector");
        let ref_name = selector
            .split_once("@{")
            .map(|(ref_name, _)| ref_name)
            .expect("reflog selector ref");
        grouped
            .entry(ref_name.to_owned())
            .or_default()
            .push(line.to_owned());
    }
    grouped.into_iter().collect()
}

fn assert_repository_state_matches_with_normalized_clone_reflogs(left: &Path, right: &Path) {
    assert_eq!(
        visible_worktree_file_contents(left),
        visible_worktree_file_contents(right),
        "worktree contents diverged"
    );
    assert_eq!(
        git(left, ["status", "--porcelain=v2", "--branch"]),
        git(right, ["status", "--porcelain=v2", "--branch"]),
        "status diverged"
    );
    assert_eq!(
        git(left, ["ls-files", "--stage"]),
        git(right, ["ls-files", "--stage"]),
        "index entries diverged"
    );
    assert_eq!(
        git(left, ["show-ref", "--head", "--dereference"]),
        git(right, ["show-ref", "--head", "--dereference"]),
        "refs diverged"
    );
    assert_eq!(
        reflog_lines_grouped_by_ref(&normalize_clone_reflog_output(&git(
            left,
            [
                "reflog",
                "show",
                "--all",
                "--date=raw",
                "--format=%H %gD %gs"
            ],
        ))),
        reflog_lines_grouped_by_ref(&normalize_clone_reflog_output(&git(
            right,
            [
                "reflog",
                "show",
                "--all",
                "--date=raw",
                "--format=%H %gD %gs"
            ],
        ))),
        "reflogs diverged"
    );
    assert_eq!(
        git(
            left,
            [
                "cat-file",
                "--batch-all-objects",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)",
            ],
        ),
        git(
            right,
            [
                "cat-file",
                "--batch-all-objects",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)",
            ],
        ),
        "object inventories diverged"
    );
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
}

#[test]
fn replacement_dogfood_publish_and_pull_workflow_matches_stock_git_state() {
    let root = TempDir::new().expect("temp root");
    let source = root.path().join("source");
    let stock_remote = root.path().join("stock.git");
    let zmin_remote = root.path().join("zmin.git");
    let stock_publish = root.path().join("stock-publish");
    let zmin_publish = root.path().join("zmin-publish");
    let stock_pull = root.path().join("stock-pull");
    let zmin_pull = root.path().join("zmin-pull");

    git(root.path(), ["init", "source"]);
    configure_identity(&source);
    write_file(&source, "tracked.txt", "one\n");
    git(&source, ["add", "tracked.txt"]);
    git(&source, ["commit", "-m", "initial"]);
    git(
        root.path(),
        [
            "clone",
            "--bare",
            source.to_str().expect("source path"),
            stock_remote.to_str().expect("stock remote path"),
        ],
    );
    git(
        root.path(),
        [
            "clone",
            "--bare",
            source.to_str().expect("source path"),
            zmin_remote.to_str().expect("zmin remote path"),
        ],
    );

    for (label, command, remote, target) in [
        (
            "stock clone publish",
            "git",
            stock_remote.as_path(),
            stock_publish.as_path(),
        ),
        (
            "zmin clone publish",
            zmin_bin(),
            zmin_remote.as_path(),
            zmin_publish.as_path(),
        ),
        (
            "stock clone pull",
            "git",
            stock_remote.as_path(),
            stock_pull.as_path(),
        ),
        (
            "zmin clone pull",
            zmin_bin(),
            zmin_remote.as_path(),
            zmin_pull.as_path(),
        ),
    ] {
        let clone = command_any_output(
            command,
            root.path(),
            &[
                "clone",
                remote.to_str().expect("remote path"),
                target.to_str().expect("target path"),
                "--quiet",
            ],
            label,
        );
        assert_eq!(clone.0, 0, "{label} stderr: {}", clone.2);
    }

    for repo in [&stock_publish, &zmin_publish, &stock_pull, &zmin_pull] {
        configure_identity(repo);
    }

    write_file(&stock_publish, "tracked.txt", "workflow stock\n");
    write_file(&zmin_publish, "tracked.txt", "workflow stock\n");
    write_file(&stock_publish, "workflow.txt", "workflow-new\n");
    write_file(&zmin_publish, "workflow.txt", "workflow-new\n");
    git(&stock_publish, ["add", "tracked.txt", "workflow.txt"]);
    let zmin_add = command_any_output(
        zmin_bin(),
        &zmin_publish,
        &["add", "tracked.txt", "workflow.txt"],
        "zmin add workflow files",
    );
    assert_eq!(zmin_add.0, 0, "zmin add stderr: {}", zmin_add.2);

    let commit_env = [
        ("GIT_AUTHOR_NAME", "Zmin Dogfood"),
        ("GIT_AUTHOR_EMAIL", "zmin-dogfood@example.invalid"),
        ("GIT_COMMITTER_NAME", "Zmin Dogfood"),
        ("GIT_COMMITTER_EMAIL", "zmin-dogfood@example.invalid"),
        ("GIT_AUTHOR_DATE", "2000-01-02T03:04:05Z"),
        ("GIT_COMMITTER_DATE", "2000-01-02T03:04:05Z"),
    ];
    let stock_commit = command_output_with_env(
        "git",
        &stock_publish,
        &["commit", "-m", "workflow update"],
        &commit_env,
        "stock workflow commit",
    );
    let zmin_commit = command_output_with_env(
        zmin_bin(),
        &zmin_publish,
        &["commit", "-m", "workflow update"],
        &commit_env,
        "zmin workflow commit",
    );
    assert_eq!(zmin_commit, stock_commit);
    assert_eq!(
        git(&stock_publish, ["rev-parse", "HEAD"]),
        git(&zmin_publish, ["rev-parse", "HEAD"])
    );

    let stock_push = command_any_output(
        "git",
        &stock_publish,
        &["push", "origin", "main"],
        "stock workflow push",
    );
    let zmin_push = command_any_output(
        zmin_bin(),
        &zmin_publish,
        &["push", "origin", "main"],
        "zmin workflow push",
    );
    assert_eq!(stock_push.0, 0, "stock push stderr: {}", stock_push.2);
    assert_eq!(zmin_push.0, 0, "zmin push stderr: {}", zmin_push.2);

    let stock_pull_out = command_any_output(
        "git",
        &stock_pull,
        &["pull", "--ff-only"],
        "stock workflow pull",
    );
    let zmin_pull_out = command_any_output(
        zmin_bin(),
        &zmin_pull,
        &["pull", "--ff-only"],
        "zmin workflow pull",
    );
    assert_eq!(
        stock_pull_out.0, 0,
        "stock pull stderr: {}",
        stock_pull_out.2
    );
    assert_eq!(zmin_pull_out.0, 0, "zmin pull stderr: {}", zmin_pull_out.2);

    assert_repository_state_matches_with_normalized_clone_reflogs(&stock_publish, &zmin_publish);
    assert_repository_state_matches_with_normalized_clone_reflogs(&stock_pull, &zmin_pull);
    assert_eq!(
        std::fs::read_to_string(stock_pull.join("tracked.txt")).expect("read stock tracked"),
        "workflow stock\n"
    );
    assert_eq!(
        std::fs::read_to_string(zmin_pull.join("tracked.txt")).expect("read zmin tracked"),
        "workflow stock\n"
    );
    assert_eq!(
        std::fs::read_to_string(stock_pull.join("workflow.txt")).expect("read stock workflow"),
        "workflow-new\n"
    );
    assert_eq!(
        std::fs::read_to_string(zmin_pull.join("workflow.txt")).expect("read zmin workflow"),
        "workflow-new\n"
    );
}

#[test]
fn replacement_git_shim_exposes_builtin_lfs_discovery_commands() {
    let root = TempDir::new().expect("temp root");
    let shim_dir = root.path().join("shim-bin");
    fs::create_dir_all(&shim_dir).expect("create shim dir");
    write_git_shim(&shim_dir);

    let repo = root.path().join("repo");
    git(root.path(), ["init", "repo"]);

    let version = shim_command_output(&repo, &shim_dir, &["lfs", "version"]);
    assert_eq!(version.0, 0);
    assert!(version.1.starts_with("git-lfs/zmin (zmin "));
    assert!(version.1.contains("built-in local foundation"));
    assert_eq!(version.2, "");

    let install = shim_command_output(
        &repo,
        &shim_dir,
        &["lfs", "install", "--local", "--skip-smudge"],
    );
    assert_eq!(install.0, 0);
    assert_eq!(install.2, "");

    let env = shim_command_output(&repo, &shim_dir, &["lfs", "env"]);
    let repo_path = fs::canonicalize(&repo).expect("canonicalize repo path");
    assert_eq!(env.0, 0);
    assert!(env.1.contains("git-lfs/zmin (zmin "));
    assert!(env.1.contains("git version 2.47.1.zmin"));
    assert!(
        env.1
            .contains(&format!("LocalWorkingDir={}", repo_path.display()))
    );
    assert!(
        env.1
            .contains(&format!("LocalGitDir={}", repo_path.join(".git").display()))
    );
    assert!(env.1.contains(&format!(
        "LocalMediaDir={}",
        repo_path.join(".git/lfs/objects").display()
    )));
    assert_eq!(env.2, "");

    let ls_files = shim_command_output(&repo, &shim_dir, &["lfs", "ls-files"]);
    assert_eq!(ls_files.0, 0);
    assert_eq!(ls_files.1, "");
    assert_eq!(ls_files.2, "");

    let install_manual = shim_command_output(&repo, &shim_dir, &["lfs", "install", "--manual"]);
    assert_eq!(install_manual.0, 0);
    assert!(
        install_manual
            .1
            .contains("Add the following to '.git/hooks/pre-push':")
    );
    assert!(install_manual.1.contains("git lfs pre-push \"$@\""));
    assert_eq!(install_manual.2, "");

    let update_manual = shim_command_output(&repo, &shim_dir, &["lfs", "update", "--manual"]);
    assert_eq!(update_manual.0, 0);
    assert!(
        update_manual
            .1
            .contains("Add the following to '.git/hooks/pre-push':")
    );
    assert!(update_manual.1.contains("git lfs pre-push \"$@\""));
    assert_eq!(update_manual.2, "");
}

#[test]
fn replacement_git_shim_reuses_existing_stock_style_lfs_hooks() {
    let root = TempDir::new().expect("temp root");
    let shim_dir = root.path().join("shim-bin");
    fs::create_dir_all(&shim_dir).expect("create shim dir");
    write_git_shim(&shim_dir);

    let repo = root.path().join("repo");
    git(root.path(), ["init", "repo"]);
    configure_identity(&repo);

    let hooks_dir = repo.join(".git/hooks");
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

    let install = shim_command_output_with_env(
        &repo,
        &shim_dir,
        &["lfs", "install", "--local", "--skip-smudge"],
        &[
            ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
            ("GIT_BIN", "/definitely/missing/git"),
        ],
    );
    assert_eq!(install.0, 0);
    assert_eq!(install.1, "Updated Git hooks.\nGit LFS initialized.");
    assert_eq!(install.2, "");

    for hook_name in ["pre-push", "post-checkout", "post-commit", "post-merge"] {
        let hook = fs::read_to_string(hooks_dir.join(hook_name)).expect("read zmin hook");
        let marker = format!("# zmin-lfs-{hook_name}");
        assert!(hook.contains(&marker), "{hook_name} missing marker: {hook}");
        assert!(
            hook.contains(&format!("lfs {hook_name} \"$@\"")),
            "{hook_name} missing command: {hook}"
        );
    }
}

#[test]
fn replacement_git_shim_reuses_existing_stock_style_lfs_hooks_in_custom_hookspath() {
    let root = TempDir::new().expect("temp root");
    let shim_dir = root.path().join("shim-bin");
    fs::create_dir_all(&shim_dir).expect("create shim dir");
    write_git_shim(&shim_dir);

    let repo = root.path().join("repo");
    git(root.path(), ["init", "repo"]);
    configure_identity(&repo);
    git(&repo, ["config", "core.hooksPath", ".githooks"]);

    let hooks_dir = repo.join(".githooks");
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

    let install = shim_command_output_with_env(
        &repo,
        &shim_dir,
        &["lfs", "install", "--local", "--skip-smudge"],
        &[
            ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
            ("GIT_BIN", "/definitely/missing/git"),
        ],
    );
    assert_eq!(install.0, 0);
    assert_eq!(install.1, "Updated Git hooks.\nGit LFS initialized.");
    assert_eq!(install.2, "");

    for hook_name in ["pre-push", "post-checkout", "post-commit", "post-merge"] {
        let hook = fs::read_to_string(hooks_dir.join(hook_name)).expect("read zmin custom hook");
        let marker = format!("# zmin-lfs-{hook_name}");
        assert!(hook.contains(&marker), "{hook_name} missing marker: {hook}");
        assert!(
            hook.contains(&format!("lfs {hook_name} \"$@\"")),
            "{hook_name} missing command: {hook}"
        );
    }
    assert!(!repo.join(".git/hooks/pre-push").exists());
}
