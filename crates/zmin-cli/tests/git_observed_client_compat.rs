mod common;

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{
    command_any_output, command_any_output_with_stdin, configure_identity, git, git_with_env,
    run_zmin, write_file, zmin_bin,
};
use tempfile::TempDir;

fn assert_observed_command_matches_stock_git(repo: &Path, args: &[&str]) {
    let stock = command_any_output("git", repo, args, "stock git");
    let zmin = command_any_output(zmin_bin(), repo, args, "zmin");
    assert_eq!(zmin, stock, "observed args: {args:?}");
}

fn assert_split_observed_command_matches_stock_git(
    git_repo: &Path,
    zmin_repo: &Path,
    args: &[&str],
) {
    let stock = command_any_output("git", git_repo, args, "stock git");
    let zmin = command_any_output(zmin_bin(), zmin_repo, args, "zmin");
    assert_eq!(zmin, stock, "observed args: {args:?}");
}

fn assert_observed_command_with_stdin_matches_stock_git(repo: &Path, args: &[&str], stdin: &str) {
    let stock = command_any_output_with_stdin("git", repo, args, stdin, "stock git");
    let zmin = command_any_output_with_stdin(zmin_bin(), repo, args, stdin, "zmin");
    assert_eq!(zmin, stock, "observed args: {args:?}");
}

fn corrupt_pack_index_checksum(repo: &Path) {
    let index = fs::read_dir(repo.join(".git/objects/pack"))
        .expect("read pack directory")
        .map(|entry| entry.expect("pack directory entry").path())
        .find(|path| path.extension().and_then(|extension| extension.to_str()) == Some("idx"))
        .expect("packed index");
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&index)
            .expect("pack index metadata")
            .permissions();
        permissions.set_mode(0o644);
        fs::set_permissions(&index, permissions).expect("make pack index writable");
    }
    let mut bytes = fs::read(&index).expect("read pack index");
    *bytes.last_mut().expect("pack index checksum") ^= 0xff;
    fs::write(index, bytes).expect("corrupt pack index checksum");
}

fn write_poison_git_script(dir: &Path, trap_log: &Path) -> PathBuf {
    #[cfg(windows)]
    let script_path = dir.join("git.cmd");
    #[cfg(not(windows))]
    let script_path = dir.join("git");

    #[cfg(windows)]
    let script = format!(
        "@echo off\r\n\
        echo git %*>>\"{}\"\r\n\
        exit /b 97\r\n",
        trap_log.display()
    );
    #[cfg(not(windows))]
    let script = format!(
        "#!/bin/sh\n\
        printf '%s\\n' \"git $*\" >> \"{}\"\n\
        exit 97\n",
        trap_log.display()
    );

    fs::write(&script_path, script).expect("write poison git script");
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path)
            .expect("poison git metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).expect("chmod poison git");
    }
    script_path
}

fn observed_client_fixture() -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");

    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(
        dir.path(),
        ["clone", remote.to_str().expect("remote path"), "work"],
    );
    configure_identity(&work);

    git(&work, ["checkout", "-b", "main"]);
    write_file(&work, "README.md", "hello\n");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["push", "-u", "origin", "main"]);

    git(&work, ["checkout", "-b", "compat/status-pathspec-matrix"]);
    write_file(&work, "README.md", "hello\nbranch\n");
    git_with_env(&work, ["commit", "-am", "branch update"]);
    git(
        &work,
        ["push", "-u", "origin", "compat/status-pathspec-matrix"],
    );

    git(&work, ["checkout", "main"]);
    git(&work, ["checkout", "compat/status-pathspec-matrix"]);
    git(
        &work,
        ["config", "filter.lfs.process", "git-lfs filter-process"],
    );
    git(&work, ["config", "filter.lfs.required", "true"]);
    git(&work, ["config", "filter.lfs.clean", "git-lfs clean -- %f"]);
    git(
        &work,
        ["config", "filter.lfs.smudge", "git-lfs smudge -- %f"],
    );
    git_with_env(&work, ["tag", "-a", "observed-tag", "-m", "observed tag"]);
    git(&work, ["tag", "observed-lightweight"]);
    git(
        &work,
        [
            "update-ref",
            "refs/codex/turn-diffs/checkpoints/fixture-a/session-a/1700000000000/a",
            "HEAD~1",
        ],
    );
    git(
        &work,
        [
            "update-ref",
            "refs/codex/turn-diffs/checkpoints/fixture-a/session-a/1700000001000/b",
            "HEAD",
        ],
    );
    git(
        &work,
        [
            "update-ref",
            "refs/codex/turn-diffs/captures/1700000002000/sample/base",
            "HEAD~1",
        ],
    );
    git(
        &work,
        [
            "update-ref",
            "refs/codex/turn-diffs/captures/1700000002000/sample/head",
            "HEAD",
        ],
    );

    write_file(
        &work,
        "crates/zmin-cli/tests/git_ls_files_compat.rs",
        "observed\n",
    );
    write_file(&work, "untracked.txt", "untracked\n");
    write_file(&work, ".idea/.gitignore", "workspace.xml\n");
    write_file(&work, ".idea/modules.xml", "<modules />\n");
    write_file(&work, ".idea/workspace.xml", "<workspace />\n");
    write_file(&work, ".idea/workspace.xml~", "<workspace backup />\n");
    git(&work, ["stash", "push", "-m", "observed stash"]);

    write_file(
        &work,
        "crates/zmin-cli/tests/git_ls_files_compat.rs",
        "changed\n",
    );
    write_file(&work, "untracked.txt", "untracked\n");

    dir
}

fn observed_merge_commit_fixture() -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path();
    git(repo, ["init", "-b", "main"]);
    configure_identity(repo);

    write_file(repo, "base.txt", "base\n");
    git(repo, ["add", "-A"]);
    git_with_env(repo, ["commit", "-m", "base"]);

    git(repo, ["checkout", "-b", "side"]);
    write_file(repo, "side.txt", "side\n");
    git(repo, ["add", "-A"]);
    git_with_env(repo, ["commit", "-m", "side"]);

    git(repo, ["checkout", "main"]);
    write_file(repo, "main.txt", "main\n");
    git(repo, ["add", "-A"]);
    git_with_env(repo, ["commit", "-m", "main"]);

    git(repo, ["merge", "--no-ff", "side", "-m", "merge"]);
    dir
}

#[test]
fn observed_client_history_queries_match_stock_git() {
    let dir = observed_client_fixture();
    let work = dir.path().join("work");
    let head = git(&work, ["rev-parse", "HEAD"]);
    let parent = git(&work, ["rev-parse", "HEAD~1"]);

    for args in [
        [
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "for-each-ref",
            "refs/tags/**",
            "--no-color",
            "--format=%(refname)\t%(*objectname)\t%(objectname)",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "log",
            "--pretty=format:%x01%x01%H%x02%x02%P%x02%x02%ct%x03%x03",
            "--encoding=UTF-8",
            "--decorate=full",
            "compat/status-pathspec-matrix",
            "--extended-regexp",
            "--regexp-ignore-case",
            "--max-count=100",
            "--",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "log",
            "HEAD",
            "--branches",
            "--remotes",
            "--max-count=200",
            "--pretty=format:%x01%x01%H%x02%x02%P%x02%x02%ct%x02%x02%cn%x02%x02%ce%x02%x02%an%x02%x02%at%x02%x02%ae%x02%x02%s%x02%x02%b%x02%x02%B%x03%x03",
            "--encoding=UTF-8",
            "--",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "log",
            "--pretty=format:%x01%x01%H%x02%x02%P%x02%x02%ct%x02%x02%an%x02%x02%ae%x02%x02%d%x03%x03",
            "--encoding=UTF-8",
            "--decorate=full",
            "HEAD",
            "--branches",
            "--remotes",
            "--tags",
            "--date-order",
            "--",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "log",
            "-n1",
            "--pretty=format:%x01%x01%H%x02%x02%ct%x03%x03",
            "HEAD",
            "--",
            "README.md",
        ]
        .as_slice(),
        [
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "blame",
            "--porcelain",
            "-l",
            "-t",
            "--encoding=UTF-8",
            "-w",
            head.as_str(),
            "--",
            "README.md",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "rev-list",
            "--count",
            "main..compat/status-pathspec-matrix",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "rev-list",
            "--count",
            "compat/status-pathspec-matrix..compat/status-pathspec-matrix@{u}",
        ]
        .as_slice(),
        [
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "reflog",
            "--max-count",
            "50",
            "--grep-reflog",
            "checkout:",
            "--",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "stash",
            "list",
            "--pretty=format:%H:%P:%at:%gd:%s",
        ]
        .as_slice(),
    ] {
        assert_observed_command_matches_stock_git(&work, args);
    }

    assert_observed_command_with_stdin_matches_stock_git(
        &work,
        &[
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "log",
            "--no-walk=unsorted",
            "--pretty=format:%x01%x01%H%x02%x02%P%x02%x02%ct%x02%x02%cn%x02%x02%ce%x02%x02%an%x02%x02%at%x02%x02%ae%x02%x02%s%x02%x02%b%x02%x02%B%x03%x03",
            "--encoding=UTF-8",
            "--stdin",
            "--",
        ],
        &format!("{head}\n{parent}\n{head}\n"),
    );
}

#[test]
fn observed_client_workspace_queries_match_stock_git() {
    let dir = observed_client_fixture();
    let work = dir.path().join("work");
    write_file(&work, ".gitignore", "ignored.log\n");
    write_file(&work, "ignored.log", "ignored\n");

    for args in [
        ["status", "--short"].as_slice(),
        ["status", "--short", "--branch"].as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "status",
            "--porcelain",
            "-z",
            "--untracked-files=no",
            "--ignored=no",
            "--",
            ".",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "status",
            "--porcelain",
            "-z",
            "--no-renames",
            "--untracked-files=all",
            "--ignored=matching",
            "--",
        ]
        .as_slice(),
        ["status", "--ignored", "--porcelain=v1", "-z"].as_slice(),
        ["status", "--ignored", "--porcelain=v2", "-z", "--branch"].as_slice(),
        ["config", "--null", "-l"].as_slice(),
        ["config", "--null", "--get", "core.fsmonitor"].as_slice(),
        ["config", "--null", "--get", "i18n.logoutputencoding"].as_slice(),
        ["config", "--null", "--get", "i18n.commitencoding"].as_slice(),
        ["config", "--null", "--get", "commit.gpgSign"].as_slice(),
        ["config", "--null", "--get", "user.signingkey"].as_slice(),
        [
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "config",
            "--name-only",
            "--get-regexp",
            "^filter\\..*\\.(clean|smudge|process|required)$",
        ]
        .as_slice(),
        [
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "for-each-ref",
            "refs/heads/**",
            "refs/remotes/**",
            "--no-color",
            "--format=%(refname)\t%(objectname)\t%(HEAD)",
        ]
        .as_slice(),
        ["rev-parse", "--verify", "--quiet", "origin/main"].as_slice(),
        [
            "rev-parse",
            "--verify",
            "--quiet",
            "refs/remotes/origin/main",
        ]
        .as_slice(),
        [
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
        ]
        .as_slice(),
        ["ls-files", "--stage", "-z"].as_slice(),
        ["ls-files", "-z", "--deleted", "--modified"].as_slice(),
        [
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "ls-files",
            "-t",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".idea/workspace.xml",
            ".idea/workspace.xml~",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "ls-files",
            "-s",
            "--",
            ".idea",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "ls-files",
            "--exclude-standard",
            "--others",
            "-z",
            "--",
            ".idea/.gitignore",
        ]
        .as_slice(),
        [
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "rev-parse",
            "--shared-index-path",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "diff",
            "--name-status",
            "--no-renames",
            "HEAD",
            "--",
            "crates/zmin-cli/tests/git_ls_files_compat.rs",
        ]
        .as_slice(),
        ["diff", "--cached", "--name-status", "-z"].as_slice(),
        ["diff", "--cached", "--name-only", "-z"].as_slice(),
        ["diff", "--cached", "--raw", "-z"].as_slice(),
        [
            "-c",
            "diff.mnemonicPrefix=false",
            "-c",
            "diff.noprefix=false",
            "-c",
            "core.quotePath=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--color=never",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "HEAD~1",
            "HEAD",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--format=raw",
            "--show-notes",
            "--stat",
            "-p",
            "-M",
            "HEAD",
            "--",
            "README.md",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "worktree",
            "list",
            "--porcelain",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--numstat",
            "--format=%H",
            "-M",
            "HEAD",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--summary",
            "--format=%H",
            "-M",
            "HEAD",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--name-only",
            "--format=%H",
            "-M",
            "HEAD",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--name-status",
            "--format=%H",
            "-M",
            "HEAD",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--raw",
            "--format=%H",
            "-M",
            "HEAD",
        ]
        .as_slice(),
        ["log", "-z", "--format=%H%x00%P%x00%D%x00%s", "-1"].as_slice(),
        [
            "log",
            "-z",
            "--date=iso-strict",
            "--format=%H%x00%ad%x00%cd",
            "-1",
        ]
        .as_slice(),
        [
            "-c",
            "attr.tree=",
            "-c",
            "core.attributesFile=",
            "-c",
            "filter.lfs.clean=",
            "-c",
            "filter.lfs.smudge=",
            "-c",
            "filter.lfs.process=",
            "-c",
            "filter.lfs.required=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "status",
            "--porcelain=1",
            "-z",
            "--untracked-files=no",
        ]
        .as_slice(),
        [
            "-c",
            "attr.tree=",
            "-c",
            "core.attributesFile=",
            "-c",
            "filter.lfs.clean=",
            "-c",
            "filter.lfs.smudge=",
            "-c",
            "filter.lfs.process=",
            "-c",
            "filter.lfs.required=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "status",
            "--porcelain=1",
            "-z",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--format=fuller",
            "--stat",
            "-p",
            "HEAD",
            "--",
            "README.md",
        ]
        .as_slice(),
        [
            "-c",
            "diff.mnemonicPrefix=false",
            "-c",
            "diff.noprefix=false",
            "-c",
            "core.quotePath=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--color=never",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "HEAD",
            "HEAD",
            "--find-renames",
            "--numstat",
            "-z",
        ]
        .as_slice(),
        [
            "-c",
            "diff.mnemonicPrefix=false",
            "-c",
            "diff.noprefix=false",
            "-c",
            "core.quotePath=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--color=never",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "HEAD",
            "HEAD",
            "--find-renames",
            "--name-status",
            "-z",
        ]
        .as_slice(),
        [
            "-c",
            "diff.mnemonicPrefix=false",
            "-c",
            "diff.noprefix=false",
            "-c",
            "core.quotePath=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--color=never",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "HEAD",
            "HEAD",
            "--find-renames",
            "--raw",
            "-z",
        ]
        .as_slice(),
        [
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "for-each-ref",
            "--format=%(refname)",
            "refs/codex/turn-diffs/captures",
        ]
        .as_slice(),
        [
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "for-each-ref",
            "--format=%(refname)",
            "refs/codex/turn-diffs/checkpoints",
        ]
        .as_slice(),
        [
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "for-each-ref",
            "--sort=-refname",
            "--format=%(refname) %(objectname)",
            "refs/codex/turn-diffs/checkpoints/fixture-a/session-a",
        ]
        .as_slice(),
    ] {
        assert_observed_command_matches_stock_git(&work, args);
    }

    assert_observed_command_with_stdin_matches_stock_git(
        &work,
        ["check-ignore", "-v", "--stdin"].as_slice(),
        "ignored.log\ntracked.txt\n",
    );
    assert_observed_command_with_stdin_matches_stock_git(
        &work,
        [
            "cat-file",
            "--batch-check=%(objectname) %(objecttype) %(objectsize)",
        ]
        .as_slice(),
        "HEAD\nHEAD^{tree}\n",
    );

    for args in [
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "log",
            "--name-status",
            "--format=fuller",
            "--max-count=1000",
            "--date-order",
            "--decorate=full",
            "--parents",
            "--stdin",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--format=raw",
            "--show-notes",
            "--stat",
            "-p",
            "-M",
            "--no-walk=unsorted",
            "--stdin",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--format=raw",
            "--show-notes",
            "--stat",
            "-p",
            "-M",
            "--diff-merges=separate",
            "--stdin",
        ]
        .as_slice(),
    ] {
        assert_observed_command_with_stdin_matches_stock_git(&work, args, "HEAD\nHEAD~1\n");
    }

    assert_eq!(
        run_zmin(&work, ["worktree", "list", "--porcelain"]),
        git(&work, ["worktree", "list", "--porcelain"])
    );
}

#[test]
fn observed_client_show_stdin_keeps_all_requested_commits() {
    let dir = observed_client_fixture();
    let work = dir.path().join("work");
    let stdin = "HEAD\nHEAD~1\n";
    let args = [
        "-c",
        "credential.helper=",
        "-c",
        "core.quotepath=false",
        "-c",
        "log.showSignature=false",
        "show",
        "--format=raw",
        "--show-notes",
        "--stat",
        "-p",
        "-M",
        "--no-walk=unsorted",
        "--stdin",
    ];

    let stock = command_any_output_with_stdin("git", &work, &args, stdin, "stock git");
    let zmin = command_any_output_with_stdin(zmin_bin(), &work, &args, stdin, "zmin");

    assert_eq!(zmin, stock, "observed args: {args:?}");
    assert_eq!(stock.1.matches("\ncommit ").count(), 1);
    assert_eq!(zmin.1.matches("\ncommit ").count(), 1);
    assert!(stock.1.starts_with("commit "));
    assert!(zmin.1.starts_with("commit "));
}

#[test]
fn observed_show_reads_objects_when_only_pack_index_checksum_is_corrupt_like_stock_git() {
    let repo = TempDir::new().expect("temp repo");
    git(repo.path(), ["init"]);
    configure_identity(repo.path());
    write_file(repo.path(), "tracked.txt", "content\n");
    git(repo.path(), ["add", "tracked.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "packed"]);
    git(repo.path(), ["repack", "-adq"]);
    corrupt_pack_index_checksum(repo.path());

    assert_observed_command_matches_stock_git(
        repo.path(),
        &["show", "--name-status", "--format=%H", "HEAD"],
    );
}

#[test]
fn observed_client_show_pathspec_omits_commits_without_matching_paths() {
    let dir = observed_client_fixture();
    let work = dir.path().join("work");
    let args = [
        "show",
        "HEAD",
        "--",
        "crates/zmin-cli/tests/git_ls_files_compat.rs",
    ];

    let stock = command_any_output("git", &work, &args, "stock git");
    let zmin = command_any_output(zmin_bin(), &work, &args, "zmin");

    assert_eq!(zmin, stock, "observed args: {args:?}");
    assert_eq!(stock.1, "");
    assert_eq!(zmin.1, "");
}

#[test]
fn observed_client_show_raw_parents_pathspec_matches_stock_git() {
    let dir = observed_client_fixture();
    let work = dir.path().join("work");
    let args = [
        "show",
        "--format=raw",
        "--show-notes",
        "--parents",
        "HEAD",
        "--",
        "crates/zmin-cli/tests/git_admin_tools_compat.rs",
    ];

    assert_observed_command_matches_stock_git(&work, &args);
}

#[test]
fn observed_client_show_path_limited_numstat_and_name_status_match_stock_git() {
    let dir = observed_client_fixture();
    let work = dir.path().join("work");

    for args in [
        [
            "show",
            "--numstat",
            "--format=%H",
            "HEAD",
            "--",
            "README.md",
        ]
        .as_slice(),
        [
            "show",
            "--name-status",
            "--format=%H %P",
            "HEAD",
            "--",
            "README.md",
        ]
        .as_slice(),
        [
            "show",
            "--numstat",
            "--format=%H",
            "HEAD",
            "--",
            "crates/zmin-cli/tests/git_ls_files_compat.rs",
        ]
        .as_slice(),
        [
            "show",
            "--name-status",
            "--format=%H %P",
            "HEAD",
            "--",
            "crates/zmin-cli/tests/git_ls_files_compat.rs",
        ]
        .as_slice(),
    ] {
        assert_observed_command_matches_stock_git(&work, args);
    }
}

#[test]
fn observed_client_show_accepts_interspersed_options_after_revision() {
    let dir = observed_client_fixture();
    let work = dir.path().join("work");

    for args in [
        [
            "show",
            "HEAD",
            "--format=raw",
            "--show-notes",
            "--stat",
            "-p",
            "-M",
            "--diff-merges=separate",
        ]
        .as_slice(),
        [
            "show",
            "HEAD",
            "--format=raw",
            "--show-notes",
            "--stat",
            "-p",
            "-M",
            "--diff-merges=separate",
            "--",
            "README.md",
        ]
        .as_slice(),
        ["show", "HEAD", "--format=fuller", "--stat", "-p"].as_slice(),
    ] {
        assert_observed_command_matches_stock_git(&work, args);
    }
}

#[test]
fn observed_client_show_stdin_commit_details_match_stock_git() {
    let dir = observed_client_fixture();
    let work = dir.path().join("work");

    for args in [
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--format=raw",
            "--show-notes",
            "--parents",
            "--stdin",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--format=fuller",
            "--stat",
            "--parents",
            "--stdin",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--numstat",
            "--format=%H %P",
            "--stdin",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--summary",
            "--format=%H %P",
            "--stdin",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--name-only",
            "--format=%H %P",
            "--stdin",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--name-status",
            "--format=%H %P",
            "--stdin",
        ]
        .as_slice(),
    ] {
        assert_observed_command_with_stdin_matches_stock_git(&work, args, "HEAD\nHEAD~1\n");
    }
}

#[test]
fn observed_client_show_name_only_stdin_rename_detection_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path();
    git(repo, ["init"]);
    configure_identity(repo);

    write_file(repo, "old.txt", "base\n");
    git(repo, ["add", "-A"]);
    git_with_env(repo, ["commit", "-m", "base"]);

    git(repo, ["mv", "old.txt", "new.txt"]);
    git_with_env(repo, ["commit", "-m", "rename"]);

    let args = [
        "-c",
        "credential.helper=",
        "-c",
        "core.quotepath=false",
        "-c",
        "log.showSignature=false",
        "show",
        "--name-only",
        "--format=%H %P",
        "-M",
        "--stdin",
    ];

    assert_observed_command_with_stdin_matches_stock_git(repo, &args, "HEAD\nHEAD~1\n");
}

#[test]
fn observed_client_show_name_only_stdin_copy_detection_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path();
    git(repo, ["init"]);
    configure_identity(repo);

    write_file(repo, "old.txt", "base\n");
    git(repo, ["add", "-A"]);
    git_with_env(repo, ["commit", "-m", "base"]);

    write_file(repo, "new.txt", "base\n");
    git(repo, ["add", "-A"]);
    git_with_env(repo, ["commit", "-m", "copy"]);

    let args = [
        "-c",
        "credential.helper=",
        "-c",
        "core.quotepath=false",
        "-c",
        "log.showSignature=false",
        "show",
        "--name-only",
        "--format=%H %P",
        "-C",
        "--find-copies-harder",
        "--stdin",
    ];

    assert_observed_command_with_stdin_matches_stock_git(repo, &args, "HEAD\nHEAD~1\n");
}

#[test]
fn observed_client_show_name_status_stdin_rename_detection_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path();
    git(repo, ["init"]);
    configure_identity(repo);

    write_file(repo, "old.txt", "base\n");
    git(repo, ["add", "-A"]);
    git_with_env(repo, ["commit", "-m", "base"]);

    git(repo, ["mv", "old.txt", "new.txt"]);
    git_with_env(repo, ["commit", "-m", "rename"]);

    let args = [
        "-c",
        "credential.helper=",
        "-c",
        "core.quotepath=false",
        "-c",
        "log.showSignature=false",
        "show",
        "--name-status",
        "--format=%H %P",
        "-M",
        "--stdin",
    ];

    assert_observed_command_with_stdin_matches_stock_git(repo, &args, "HEAD\nHEAD~1\n");
}

#[test]
fn observed_client_show_name_status_stdin_copy_detection_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path();
    git(repo, ["init"]);
    configure_identity(repo);

    write_file(repo, "old.txt", "base\n");
    git(repo, ["add", "-A"]);
    git_with_env(repo, ["commit", "-m", "base"]);

    write_file(repo, "new.txt", "base\n");
    git(repo, ["add", "-A"]);
    git_with_env(repo, ["commit", "-m", "copy"]);

    let args = [
        "-c",
        "credential.helper=",
        "-c",
        "core.quotepath=false",
        "-c",
        "log.showSignature=false",
        "show",
        "--name-status",
        "--format=%H %P",
        "-C",
        "--find-copies-harder",
        "--stdin",
    ];

    assert_observed_command_with_stdin_matches_stock_git(repo, &args, "HEAD\nHEAD~1\n");
}

#[test]
fn observed_client_show_stdin_merge_commit_detail_families_match_stock_git() {
    let dir = observed_merge_commit_fixture();
    let repo = dir.path();

    for args in [
        ["show", "--name-only", "--format=%H %P", "--stdin"].as_slice(),
        ["show", "--name-status", "--format=%H %P", "--stdin"].as_slice(),
        ["show", "--numstat", "--format=%H %P", "--stdin"].as_slice(),
        ["show", "--raw", "--format=%H %P", "--stdin"].as_slice(),
    ] {
        assert_observed_command_with_stdin_matches_stock_git(repo, args, "HEAD\n");
    }
}

#[test]
fn observed_client_show_stdin_raw_numstat_shortstat_families_match_stock_git() {
    let dir = observed_merge_commit_fixture();
    let repo = dir.path();

    for args in [
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "--raw",
            "--numstat",
            "--shortstat",
            "--format=fuller",
            "--decorate=full",
            "--date=default",
            "--parents",
            "--stdin",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "show",
            "-c",
            "-r",
            "--raw",
            "--numstat",
            "--shortstat",
            "--format=fuller",
            "--decorate=full",
            "--date=default",
            "--diff-merges=first-parent",
            "--stdin",
        ]
        .as_slice(),
    ] {
        assert_observed_command_with_stdin_matches_stock_git(repo, args, "HEAD\n");
    }
}

#[test]
fn observed_client_hot_paths_do_not_invoke_external_git_runtime() {
    let dir = observed_client_fixture();
    let work = dir.path().join("work");
    let trap_dir = dir.path().join("trap-bin");
    fs::create_dir_all(&trap_dir).expect("create trap dir");
    let trap_log = dir.path().join("poison-git.log");
    let fake_git = write_poison_git_script(&trap_dir, &trap_log);
    let mut poisoned_paths = vec![trap_dir.clone()];
    poisoned_paths.extend(
        std::env::var_os("PATH")
            .iter()
            .flat_map(std::env::split_paths),
    );
    let poisoned_path = std::env::join_paths(poisoned_paths).expect("join poisoned path");

    for args in [
        ["status", "--ignored", "--porcelain=v2", "-z", "--branch"].as_slice(),
        [
            "log",
            "--pretty=format:%x01%x01%H%x02%x02%P%x02%x02%ct%x02%x02%an%x02%x02%ae%x02%x02%d%x03%x03",
            "--encoding=UTF-8",
            "--decorate=full",
            "HEAD",
            "--branches",
            "--remotes",
            "--tags",
            "--date-order",
            "--",
        ]
        .as_slice(),
        [
            "show",
            "HEAD",
            "--format=raw",
            "--show-notes",
            "--stat",
            "-p",
            "-M",
            "--diff-merges=separate",
        ]
        .as_slice(),
        [
            "show",
            "--format=%H %P",
            "--name-status",
            "--stdin",
        ]
        .as_slice(),
        [
            "show",
            "--numstat",
            "--format=%H",
            "HEAD",
            "--",
            "README.md",
        ]
        .as_slice(),
        [
            "show",
            "--name-status",
            "--format=%H %P",
            "HEAD",
            "--",
            "README.md",
        ]
        .as_slice(),
        [
            "show",
            "--raw",
            "--numstat",
            "--shortstat",
            "--format=fuller",
            "--decorate=full",
            "--date=default",
            "--parents",
            "--stdin",
        ]
        .as_slice(),
        ["ls-files", "--stage", "-z"].as_slice(),
        [
            "ls-files",
            "-t",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".idea/workspace.xml",
            ".idea/workspace.xml~",
        ]
        .as_slice(),
    ] {
        let stock = if args.contains(&"--stdin") {
            command_any_output_with_stdin("git", &work, args, "HEAD\nHEAD~1\n", "stock git")
        } else {
            command_any_output("git", &work, args, "stock git")
        };
        let mut command = Command::new(zmin_bin());
        command
            .args(args)
            .current_dir(&work)
            .env("PATH", &poisoned_path)
            .env("ZMIN_STOCK_GIT", &fake_git)
            .env("GIT_BIN", &fake_git);
        let output = if args.contains(&"--stdin") {
            use std::io::Write;

            let mut child = command
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("spawn zmin under poisoned git");
            child
                .stdin
                .as_mut()
                .expect("zmin stdin")
                .write_all(b"HEAD\nHEAD~1\n")
                .expect("write zmin stdin");
            child.wait_with_output().expect("wait for zmin")
        } else {
            command.output().expect("run zmin under poisoned git")
        };
        let zmin = (
            output.status.code().expect("process exit code"),
            String::from_utf8(output.stdout)
                .expect("stdout utf8")
                .trim_end_matches('\n')
                .to_owned(),
            String::from_utf8(output.stderr)
                .expect("stderr utf8")
                .trim_end_matches('\n')
                .to_owned(),
        );

        assert_eq!(zmin, stock, "observed args: {args:?}");
        let trap = fs::read_to_string(&trap_log).unwrap_or_default();
        assert!(
            trap.trim().is_empty(),
            "zmin unexpectedly invoked external git for args {args:?}: {trap}"
        );
        let _ = fs::remove_file(&trap_log);
    }
}

#[test]
fn observed_client_fuller_name_status_trims_trailing_blank_message_tail() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path();
    git(repo, ["init"]);
    configure_identity(repo);

    write_file(repo, "tracked.txt", "one\n");
    git(repo, ["add", "tracked.txt"]);
    git(repo, ["commit", "-m", "base"]);

    write_file(repo, "msg.txt", "subject only\n\n");
    write_file(repo, "tracked.txt", "two\n");
    git(repo, ["add", "tracked.txt"]);
    git(repo, ["commit", "-F", "msg.txt"]);

    let args = [
        "log",
        "--name-status",
        "--format=fuller",
        "--max-count=1",
        "--date-order",
        "--decorate=full",
        "--parents",
        "HEAD",
    ];
    assert_observed_command_matches_stock_git(repo, &args);
}

#[test]
fn observed_client_mutation_flow_matches_stock_git() {
    let git_dir = observed_client_fixture();
    let zmin_dir = observed_client_fixture();
    let git_work = git_dir.path().join("work");
    let zmin_work = zmin_dir.path().join("work");

    for args in [
        [
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "add",
            "-u",
        ]
        .as_slice(),
        [
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "write-tree",
        ]
        .as_slice(),
        [
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "update-ref",
            "refs/codex/turn-diffs/captures/1700000003000/fixture/base",
            "HEAD~1",
        ]
        .as_slice(),
        [
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=",
            "update-ref",
            "-d",
            "refs/codex/turn-diffs/captures/1700000002000/sample/base",
        ]
        .as_slice(),
    ] {
        assert_split_observed_command_matches_stock_git(&git_work, &zmin_work, args);
    }

    for args in [
        ["status", "--porcelain=1", "-z"].as_slice(),
        ["status", "--porcelain=2", "-z"].as_slice(),
        [
            "for-each-ref",
            "--sort=-refname",
            "--format=%(refname) %(objectname)",
            "refs/codex/turn-diffs/captures",
        ]
        .as_slice(),
        ["write-tree"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), &zmin_work, args, "zmin"),
            command_any_output("git", &git_work, args, "stock git"),
            "post-mutation args: {args:?}"
        );
    }
}
