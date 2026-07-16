mod common;

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{
    command_any_output, command_failure_output_with_env, command_output_with_env, git, git_init,
    stock_git_bin, zmin_bin,
};
use tempfile::TempDir;

const HELP_ENVS: &[(&str, &str)] = &[
    ("GIT_PAGER", "cat"),
    ("PAGER", "cat"),
    ("MANPAGER", "cat"),
    ("GIT_MAN_VIEWER", "cat"),
    ("GIT_EDITOR", "true"),
];

fn normalize_man_footer(text: &str) -> String {
    text.lines()
        .filter(|line| !line.starts_with("Git "))
        .collect::<Vec<_>>()
        .join("\n")
}

fn write_poison_man_script(dir: &Path, trap_log: &Path) -> PathBuf {
    #[cfg(windows)]
    let script_path = dir.join("man.cmd");
    #[cfg(not(windows))]
    let script_path = dir.join("man");

    #[cfg(windows)]
    let script = format!(
        "@echo off\r\n\
        echo man %*>>\"{}\"\r\n\
        exit /b 97\r\n",
        trap_log.display()
    );
    #[cfg(not(windows))]
    let script = format!(
        "#!/bin/sh\n\
        printf '%s\\n' \"man $*\" >> \"{}\"\n\
        exit 97\n",
        trap_log.display()
    );

    fs::write(&script_path, script).expect("write poison man script");
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path)
            .expect("poison man metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).expect("chmod poison man");
    }
    script_path
}

fn write_test_browser_script(dir: &Path, browser_log: &Path) -> PathBuf {
    #[cfg(windows)]
    let script_path = dir.join("test-browser.cmd");
    #[cfg(not(windows))]
    let script_path = dir.join("test-browser");

    #[cfg(windows)]
    let script = format!(
        "@echo off\r\n\
        echo %* >\"{}\"\r\n",
        browser_log.display()
    );
    #[cfg(not(windows))]
    let script = format!(
        "#!/bin/sh\n\
        printf '%s\\n' \"$*\" > \"{}\"\n",
        browser_log.display()
    );

    fs::write(&script_path, script).expect("write test browser script");
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script_path)
            .expect("test browser metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).expect("chmod test browser");
    }
    script_path
}

#[test]
fn help_documented_option_family_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let stock = stock_git_bin().to_str().expect("stock git path");
    let cases: &[&[&str]] = &[
        &["help", "-a"],
        &["help", "--all"],
        &["help", "-g"],
        &["help", "--guides"],
        &["help", "-c"],
        &["help", "--config"],
        &["help", "-m"],
        &["help", "--man"],
        &["help", "-i"],
        &["help", "--info"],
        &["help", "-w"],
        &["help", "--web"],
        &["help", "--user-interfaces"],
        &["help", "--developer-interfaces"],
        &["help", "--config-for-completion"],
        &["help", "--config-sections-for-completion"],
        &["help", "--all", "--no-aliases"],
        &["help", "--all", "--no-external-commands"],
        &["help", "--all", "--verbose"],
    ];

    for args in cases {
        let git = command_output_with_env(stock, dir.path(), args, HELP_ENVS, "stock git help");
        let zmin = command_output_with_env(zmin_bin(), dir.path(), args, HELP_ENVS, "zmin help");
        assert_eq!(zmin, git, "args: {:?}", args);
    }
}

#[test]
fn root_help_and_builtin_listing_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let stock = stock_git_bin().to_str().expect("stock git path");
    let cases: &[&[&str]] = &[&[], &["--list-cmds=builtins"]];

    for args in cases {
        let git = command_any_output(stock, dir.path(), args, "stock git root");
        let zmin = command_any_output(zmin_bin(), dir.path(), args, "zmin root");
        assert_eq!(zmin, git, "args: {:?}", args);
    }
}

#[test]
fn help_surface_does_not_depend_on_stock_git_runtime() {
    let dir = TempDir::new().expect("temp dir");
    let poisoned_envs = &[
        ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
        ("GIT_BIN", "/definitely/missing/git"),
        ("GIT_PAGER", "cat"),
        ("PAGER", "cat"),
        ("MANPAGER", "cat"),
        ("GIT_MAN_VIEWER", "cat"),
        ("GIT_EDITOR", "true"),
    ];
    let root = command_failure_output_with_env(
        zmin_bin(),
        dir.path(),
        &[],
        poisoned_envs,
        "zmin poisoned root help",
    );
    assert_eq!(root.0, 1);
    assert!(root.2.is_empty(), "expected empty stderr, got: {}", root.2);
    assert!(!root.1.is_empty(), "expected root help stdout");

    let success_cases: &[&[&str]] = &[
        &["--list-cmds=builtins"],
        &["help"],
        &["help", "--all"],
        &["help", "--guides"],
        &["help", "--config"],
        &["help", "--config-for-completion"],
        &["help", "--config-sections-for-completion"],
        &["help", "--user-interfaces"],
        &["help", "--developer-interfaces"],
        &["help", "branch"],
    ];

    for args in success_cases {
        let output = command_output_with_env(
            zmin_bin(),
            dir.path(),
            args,
            poisoned_envs,
            "zmin poisoned help",
        );
        assert_eq!(output.0, 0, "args: {:?}", args);
        assert!(
            output.2.is_empty(),
            "expected empty stderr for args {:?}, got: {}",
            args,
            output.2
        );
        assert!(!output.1.is_empty(), "expected stdout for args {:?}", args);
    }
}

#[test]
fn help_command_topics_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let stock = stock_git_bin().to_str().expect("stock git path");
    let help_envs = &[
        ("PAGER", "cat"),
        ("MANPAGER", "cat"),
        ("GIT_EDITOR", "true"),
    ];
    let cases: &[&[&str]] = &[&["help", "branch"], &["help", "revisions"]];

    for args in cases {
        let git =
            command_output_with_env(stock, dir.path(), args, help_envs, "stock git help topic");
        let zmin =
            command_output_with_env(zmin_bin(), dir.path(), args, help_envs, "zmin help topic");
        assert_eq!(zmin.0, git.0, "args: {:?}", args);
        assert_eq!(
            normalize_man_footer(&zmin.1),
            normalize_man_footer(&git.1),
            "args: {:?}",
            args
        );
        assert_eq!(zmin.2, git.2, "args: {:?}", args);
    }
}

#[test]
fn help_topics_do_not_invoke_system_man_runtime() {
    let dir = TempDir::new().expect("temp dir");
    let trap_dir = dir.path().join("trap-bin");
    fs::create_dir_all(&trap_dir).expect("create trap dir");
    let trap_log = dir.path().join("poison-man.log");
    let fake_man = write_poison_man_script(&trap_dir, &trap_log);
    let mut poisoned_paths = vec![trap_dir.clone()];
    poisoned_paths.extend(
        std::env::var_os("PATH")
            .iter()
            .flat_map(std::env::split_paths),
    );
    let poisoned_path = std::env::join_paths(poisoned_paths).expect("join poisoned path");
    let stock = stock_git_bin().to_str().expect("stock git path");
    let help_envs = &[
        ("PAGER", "cat"),
        ("MANPAGER", "cat"),
        ("GIT_EDITOR", "true"),
    ];

    for args in [
        ["help", "branch"].as_slice(),
        ["help", "revisions"].as_slice(),
        ["help", "git"].as_slice(),
        ["help", "everyday"].as_slice(),
        ["help", "tutorial"].as_slice(),
        ["help", "tutorial-2"].as_slice(),
        ["help", "workflows"].as_slice(),
    ] {
        let git =
            command_output_with_env(stock, dir.path(), args, help_envs, "stock git help topic");
        let output = std::process::Command::new(zmin_bin())
            .args(args)
            .current_dir(dir.path())
            .env("PATH", &poisoned_path)
            .env("MAN", &fake_man)
            .env("GIT_PAGER", "cat")
            .env("PAGER", "cat")
            .env("MANPAGER", "cat")
            .env("GIT_MAN_VIEWER", "man")
            .env("GIT_EDITOR", "true")
            .output()
            .expect("run zmin poisoned help topic");
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

        assert_eq!(zmin.0, git.0, "args: {:?}", args);
        assert_eq!(
            normalize_man_footer(&zmin.1),
            normalize_man_footer(&git.1),
            "args: {:?}",
            args
        );
        assert_eq!(zmin.2, git.2, "args: {:?}", args);
        let trap = fs::read_to_string(&trap_log).unwrap_or_default();
        assert!(
            trap.trim().is_empty(),
            "zmin unexpectedly invoked system man for args {:?}: {}",
            args,
            trap
        );
        let _ = fs::remove_file(&trap_log);
    }
}

#[test]
fn help_all_rejects_non_option_arguments_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let stock = stock_git_bin().to_str().expect("stock git path");
    let args = ["help", "-a", "branch"];

    let git = command_any_output(stock, dir.path(), &args, "stock git help invalid");
    let zmin = command_any_output(zmin_bin(), dir.path(), &args, "zmin help invalid");
    assert_eq!(zmin, git);
}

#[test]
fn help_html_topic_routing_matches_stock_git() {
    let repo = git_init();
    let browser_log = repo.path().join("test-browser.log");
    let _browser = write_test_browser_script(repo.path(), &browser_log);
    git(repo.path(), ["config", "help.format", "html"]);
    git(repo.path(), ["config", "help.htmlpath", "test://html"]);
    git(repo.path(), ["config", "help.browser", "test"]);
    git(
        repo.path(),
        ["config", "browser.test.cmd", "./test-browser"],
    );

    for args in [
        ["help", "status"].as_slice(),
        ["help", "revisions"].as_slice(),
        ["help", "--web", "status"].as_slice(),
    ] {
        let stock = command_any_output(
            stock_git_bin().to_str().expect("stock git path"),
            repo.path(),
            args,
            "stock git help html",
        );
        let zmin = command_any_output(zmin_bin(), repo.path(), args, "zmin help html");
        assert_eq!(zmin, stock, "args: {:?}", args);
    }
}

#[test]
fn help_exclude_guides_and_missing_html_docs_match_stock_git() {
    let repo = git_init();
    let browser_log = repo.path().join("test-browser.log");
    let _browser = write_test_browser_script(repo.path(), &browser_log);
    git(repo.path(), ["config", "help.format", "html"]);
    git(repo.path(), ["config", "help.browser", "test"]);
    git(
        repo.path(),
        ["config", "browser.test.cmd", "./test-browser"],
    );

    let exclude_args = ["help", "--exclude-guides", "revisions"];
    let stock_exclude = command_any_output(
        stock_git_bin().to_str().expect("stock git path"),
        repo.path(),
        &exclude_args,
        "stock git help exclude-guides",
    );
    let zmin_exclude = command_any_output(
        zmin_bin(),
        repo.path(),
        &exclude_args,
        "zmin help exclude-guides",
    );
    assert_eq!(zmin_exclude, stock_exclude);

    git(repo.path(), ["config", "help.htmlpath", "html-empty"]);
    fs::create_dir_all(repo.path().join("html-empty")).expect("create empty html dir");
    let missing_args = ["help", "status"];
    let stock_missing = command_any_output(
        stock_git_bin().to_str().expect("stock git path"),
        repo.path(),
        &missing_args,
        "stock git help missing html",
    );
    let zmin_missing = command_any_output(
        zmin_bin(),
        repo.path(),
        &missing_args,
        "zmin help missing html",
    );
    assert_eq!(zmin_missing, stock_missing);
}

#[test]
fn help_htmlpath_page_without_git_index_matches_stock_git() {
    let repo = git_init();
    let browser_log = repo.path().join("test-browser.log");
    let _browser = write_test_browser_script(repo.path(), &browser_log);
    let docs_dir = repo.path().join("html-with-docs");
    fs::create_dir_all(&docs_dir).expect("create docs dir");
    fs::write(docs_dir.join("git-status.html"), "").expect("write git-status html");
    git(repo.path(), ["config", "help.format", "html"]);
    git(repo.path(), ["config", "help.browser", "test"]);
    git(
        repo.path(),
        ["config", "browser.test.cmd", "./test-browser"],
    );

    let args = ["-c", "help.htmlpath=html-with-docs", "help", "status"];
    let stock = command_any_output(
        stock_git_bin().to_str().expect("stock git path"),
        repo.path(),
        &args,
        "stock git help html docs",
    );
    let zmin = command_any_output(zmin_bin(), repo.path(), &args, "zmin help html docs");
    assert_eq!(zmin, stock);
}

#[test]
fn help_no_external_commands_with_topic_matches_stock_git() {
    let repo = git_init();
    let args = ["help", "--no-external-commands", "status"];
    let stock = command_any_output(
        stock_git_bin().to_str().expect("stock git path"),
        repo.path(),
        &args,
        "stock git help no-external topic",
    );
    let zmin = command_any_output(
        zmin_bin(),
        repo.path(),
        &args,
        "zmin help no-external topic",
    );
    assert_eq!(zmin, stock);
}

#[test]
fn builtin_help_flags_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("sub");
    std::process::Command::new(stock_git_bin())
        .args(["init", "-q", repo.to_str().expect("repo path")])
        .status()
        .expect("init repo");
    let stock = stock_git_bin().to_str().expect("stock git path");
    let cases: &[&[&str]] = &[
        &["-C", "sub", "add", "-h"],
        &["-C", "sub", "restore", "-h"],
        &["-C", "sub", "rm", "-h"],
        &["-C", "sub", "branch", "-h"],
        &["-C", "sub", "status", "-h"],
        &["-C", "sub", "submodule", "-h"],
        &["-C", "sub", "stash", "-h"],
        &["-C", "sub", "stash", "list", "-h"],
        &["-C", "sub", "stash", "push", "-h"],
        &["-C", "sub", "stash", "apply", "-h"],
        &["-C", "sub", "stash", "pop", "-h"],
        &["-C", "sub", "stash", "drop", "-h"],
        &["-C", "sub", "sparse-checkout", "-h"],
        &["-C", "sub", "stage", "-h"],
        &["-C", "sub", "stripspace", "-h"],
        &["-C", "sub", "submodule--helper", "-h"],
        &["-C", "sub", "switch", "-h"],
        &["-C", "sub", "symbolic-ref", "-h"],
        &["-C", "sub", "send-pack", "-h"],
        &["-C", "sub", "rev-list", "-h"],
        &["-C", "sub", "rev-parse", "-h"],
        &["-C", "sub", "revert", "-h"],
        &["-C", "sub", "replay", "-h"],
        &["-C", "sub", "rerere", "-h"],
        &["-C", "sub", "reset", "-h"],
        &["-C", "sub", "remote-fd", "-h"],
        &["-C", "sub", "remote", "-h"],
        &["-C", "sub", "remote-ext", "-h"],
        &["-C", "sub", "rebase", "-h"],
        &["-C", "sub", "receive-pack", "-h"],
        &["-C", "sub", "push", "-h"],
        &["-C", "sub", "multi-pack-index", "-h"],
        &["-C", "sub", "fmt-merge-msg", "-h"],
        &["-C", "sub", "for-each-ref", "-h"],
        &["-C", "sub", "for-each-repo", "-h"],
        &["-C", "sub", "format-patch", "-h"],
        &["-C", "sub", "fsmonitor--daemon", "-h"],
        &["-C", "sub", "gc", "-h"],
        &["-C", "sub", "get-tar-commit-id", "-h"],
        &["-C", "sub", "grep", "-h"],
        &["-C", "sub", "hash-object", "-h"],
        &["-C", "sub", "help", "-h"],
        &["-C", "sub", "hook", "-h"],
        &["-C", "sub", "index-pack", "-h"],
        &["-C", "sub", "init", "-h"],
        &["-C", "sub", "init-db", "-h"],
        &["-C", "sub", "interpret-trailers", "-h"],
        &["-C", "sub", "log", "-h"],
        &["-C", "sub", "ls-files", "-h"],
        &["-C", "sub", "ls-remote", "-h"],
        &["-C", "sub", "ls-tree", "-h"],
        &["-C", "sub", "mailinfo", "-h"],
        &["-C", "sub", "mailsplit", "-h"],
        &["-C", "sub", "maintenance", "-h"],
        &["-C", "sub", "merge", "-h"],
        &["-C", "sub", "merge-base", "-h"],
        &["-C", "sub", "merge-file", "-h"],
        &["-C", "sub", "merge-index", "-h"],
        &["-C", "sub", "merge-ours", "-h"],
        &["-C", "sub", "merge-recursive", "-h"],
        &["-C", "sub", "merge-recursive-ours", "-h"],
        &["-C", "sub", "merge-recursive-theirs", "-h"],
        &["-C", "sub", "merge-subtree", "-h"],
        &["-C", "sub", "merge-tree", "-h"],
        &["-C", "sub", "mktag", "-h"],
        &["-C", "sub", "mktree", "-h"],
        &["-C", "sub", "name-rev", "-h"],
        &["-C", "sub", "prune", "-h"],
        &["-C", "sub", "prune-packed", "-h"],
        &["-C", "sub", "pull", "-h"],
        &["-C", "sub", "notes", "-h"],
        &["-C", "sub", "pack-objects", "-h"],
        &["-C", "sub", "pack-refs", "-h"],
        &["-C", "sub", "pack-redundant", "-h"],
        &["-C", "sub", "patch-id", "-h"],
        &["-C", "sub", "pickaxe", "-h"],
        &["-C", "sub", "range-diff", "-h"],
        &["-C", "sub", "read-tree", "-h"],
        &["-C", "sub", "repack", "-h"],
        &["-C", "sub", "replace", "-h"],
        &["-C", "sub", "shortlog", "-h"],
        &["-C", "sub", "show", "-h"],
        &["-C", "sub", "show-branch", "-h"],
        &["-C", "sub", "show-ref", "-h"],
        &["-C", "sub", "tag", "-h"],
        &["-C", "sub", "clean", "-h"],
        &["-C", "sub", "am", "-h"],
        &["-C", "sub", "apply", "-h"],
        &["-C", "sub", "archive", "-h"],
        &["-C", "sub", "backfill", "-h"],
        &["-C", "sub", "bugreport", "-h"],
        &["-C", "sub", "bundle", "-h"],
        &["-C", "sub", "check-attr", "-h"],
        &["-C", "sub", "check-ignore", "-h"],
        &["-C", "sub", "check-mailmap", "-h"],
        &["-C", "sub", "check-ref-format", "-h"],
        &["-C", "sub", "checkout", "-h"],
        &["-C", "sub", "checkout--worker", "-h"],
        &["-C", "sub", "checkout-index", "-h"],
        &["-C", "sub", "cherry", "-h"],
        &["-C", "sub", "cherry-pick", "-h"],
        &["-C", "sub", "clone", "-h"],
        &["-C", "sub", "column", "-h"],
        &["-C", "sub", "commit-graph", "-h"],
        &["-C", "sub", "commit-tree", "-h"],
        &["-C", "sub", "config", "-h"],
        &["-C", "sub", "count-objects", "-h"],
        &["-C", "sub", "credential", "-h"],
        &["-C", "sub", "credential-cache", "-h"],
        &["-C", "sub", "credential-cache--daemon", "-h"],
        &["-C", "sub", "credential-store", "-h"],
        &["-C", "sub", "describe", "-h"],
        &["-C", "sub", "diagnose", "-h"],
        &["-C", "sub", "diff-files", "-h"],
        &["-C", "sub", "diff-index", "-h"],
        &["-C", "sub", "diff-pairs", "-h"],
        &["-C", "sub", "diff-tree", "-h"],
        &["-C", "sub", "difftool", "-h"],
        &["-C", "sub", "unpack-file", "-h"],
        &["-C", "sub", "unpack-objects", "-h"],
        &["-C", "sub", "fast-export", "-h"],
        &["-C", "sub", "fast-import", "-h"],
        &["-C", "sub", "fetch", "-h"],
        &["-C", "sub", "fetch-pack", "-h"],
        &["-C", "sub", "update-index", "-h"],
        &["-C", "sub", "update-ref", "-h"],
        &["-C", "sub", "update-server-info", "-h"],
        &["-C", "sub", "upload-archive", "-h"],
        &["-C", "sub", "upload-archive--writer", "-h"],
        &["-C", "sub", "upload-pack", "-h"],
        &["-C", "sub", "worktree", "-h"],
        &["-C", "sub", "write-tree", "-h"],
        &["-C", "sub", "var", "-h"],
        &["-C", "sub", "version", "-h"],
        &["-C", "sub", "verify-commit", "-h"],
        &["-C", "sub", "verify-pack", "-h"],
        &["-C", "sub", "verify-tag", "-h"],
        &["-C", "sub", "whatchanged", "-h"],
    ];

    for args in cases {
        let git = command_any_output(stock, dir.path(), args, "stock git builtin help");
        let zmin = command_any_output(zmin_bin(), dir.path(), args, "zmin builtin help");
        assert_eq!(zmin, git, "args: {:?}", args);
    }
}

#[test]
fn builtin_help_synopses_match_pinned_v2_47() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("sub");
    std::process::Command::new(stock_git_bin())
        .args(["init", "-q", repo.to_str().expect("repo path")])
        .status()
        .expect("init repo");
    let cases: &[(&[&str], &str)] = &[
        (
            &["-C", "sub", "cat-file", "-h"],
            "usage: git cat-file <type> <object>\n   or: git cat-file (-e | -p) <object>\n   or: git cat-file (-t | -s) [--allow-unknown-type] <object>",
        ),
        (
            &["-C", "sub", "commit", "-h"],
            "usage: git commit [-a | --interactive | --patch] [-s] [-v] [-u<mode>] [--amend]",
        ),
        (
            &["-C", "sub", "fsck", "-h"],
            "usage: git fsck [--tags] [--root] [--unreachable] [--cache] [--no-reflogs]\n                [--[no-]full] [--strict] [--verbose] [--lost-found]\n                [--[no-]dangling] [--[no-]progress] [--connectivity-only]\n                [--[no-]name-objects] [<object>...]",
        ),
        (
            &["-C", "sub", "mv", "-h"],
            "usage: git mv [<options>] <source>... <destination>",
        ),
        (
            &["-C", "sub", "reflog", "-h"],
            "usage: git reflog [show] [<log-options>] [<ref>]\n   or: git reflog list\n   or: git reflog expire [--expire=<time>] [--expire-unreachable=<time>]\n                         [--rewrite] [--updateref] [--stale-fix]\n                         [--dry-run | -n] [--verbose] [--all [--single-worktree] | <refs>...]\n   or: git reflog delete [--rewrite] [--updateref]\n                         [--dry-run | -n] [--verbose] <ref>@{<specifier>}...\n   or: git reflog exists <ref>",
        ),
        (
            &["-C", "sub", "refs", "-h"],
            "usage: git refs migrate --ref-format=<format> [--dry-run]\n   or: git refs verify [--strict] [--verbose]",
        ),
        (
            &["-C", "sub", "show-index", "-h"],
            "usage: git show-index [--object-format=<hash-algorithm>]",
        ),
    ];

    for (args, synopsis) in cases {
        let output = command_any_output(zmin_bin(), dir.path(), args, "zmin v2.47 builtin help");
        assert_eq!(output.0, 129, "args: {args:?}");
        assert!(
            output.1.starts_with(synopsis),
            "args: {args:?}\n{}",
            output.1
        );
        assert!(output.2.is_empty(), "args: {args:?}\n{}", output.2);
    }
}

#[test]
fn diff_help_flag_matches_upstream_builtin_stdout_shape() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("sub");
    std::process::Command::new(stock_git_bin())
        .args(["init", "-q", repo.to_str().expect("repo path")])
        .status()
        .expect("init repo");

    let output = command_failure_output_with_env(
        zmin_bin(),
        dir.path(),
        &["-C", "sub", "diff", "-h"],
        HELP_ENVS,
        "zmin diff help",
    );
    assert_eq!(output.0, 129);
    assert!(
        output.2.is_empty(),
        "expected empty stderr, got: {}",
        output.2
    );
    assert!(
        output.1.starts_with("usage: git diff "),
        "expected diff usage on stdout, got: {}",
        output.1
    );
}

#[test]
fn builtin_help_flags_do_not_depend_on_stock_git_runtime() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("sub");
    std::process::Command::new(stock_git_bin())
        .args(["init", "-q", repo.to_str().expect("repo path")])
        .status()
        .expect("init repo");

    let poisoned_envs = &[
        ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
        ("GIT_BIN", "/definitely/missing/git"),
        ("GIT_PAGER", "cat"),
        ("PAGER", "cat"),
        ("MANPAGER", "cat"),
        ("GIT_MAN_VIEWER", "cat"),
        ("GIT_EDITOR", "true"),
    ];
    let cases: &[(&[&str], i32)] = &[
        (&["-C", "sub", "add", "-h"], 129),
        (&["-C", "sub", "restore", "-h"], 129),
        (&["-C", "sub", "rm", "-h"], 129),
        (&["-C", "sub", "branch", "-h"], 129),
        (&["-C", "sub", "status", "-h"], 129),
        (&["-C", "sub", "submodule", "-h"], 0),
        (&["-C", "sub", "stash", "-h"], 129),
        (&["-C", "sub", "stash", "list", "-h"], 129),
        (&["-C", "sub", "stash", "push", "-h"], 129),
        (&["-C", "sub", "stash", "apply", "-h"], 129),
        (&["-C", "sub", "stash", "pop", "-h"], 129),
        (&["-C", "sub", "stash", "drop", "-h"], 129),
        (&["-C", "sub", "sparse-checkout", "-h"], 129),
        (&["-C", "sub", "stage", "-h"], 129),
        (&["-C", "sub", "stripspace", "-h"], 129),
        (&["-C", "sub", "submodule--helper", "-h"], 129),
        (&["-C", "sub", "switch", "-h"], 129),
        (&["-C", "sub", "symbolic-ref", "-h"], 129),
        (&["-C", "sub", "send-pack", "-h"], 129),
        (&["-C", "sub", "rev-list", "-h"], 129),
        (&["-C", "sub", "rev-parse", "-h"], 129),
        (&["-C", "sub", "revert", "-h"], 129),
        (&["-C", "sub", "replay", "-h"], 129),
        (&["-C", "sub", "rerere", "-h"], 129),
        (&["-C", "sub", "reset", "-h"], 129),
        (&["-C", "sub", "remote-fd", "-h"], 129),
        (&["-C", "sub", "refs", "-h"], 129),
        (&["-C", "sub", "remote", "-h"], 129),
        (&["-C", "sub", "remote-ext", "-h"], 129),
        (&["-C", "sub", "rebase", "-h"], 129),
        (&["-C", "sub", "receive-pack", "-h"], 129),
        (&["-C", "sub", "reflog", "-h"], 129),
        (&["-C", "sub", "push", "-h"], 129),
        (&["-C", "sub", "multi-pack-index", "-h"], 129),
        (&["-C", "sub", "fmt-merge-msg", "-h"], 129),
        (&["-C", "sub", "for-each-ref", "-h"], 129),
        (&["-C", "sub", "for-each-repo", "-h"], 129),
        (&["-C", "sub", "clean", "-h"], 129),
        (&["-C", "sub", "am", "-h"], 129),
        (&["-C", "sub", "apply", "-h"], 129),
        (&["-C", "sub", "archive", "-h"], 129),
        (&["-C", "sub", "backfill", "-h"], 129),
        (&["-C", "sub", "bugreport", "-h"], 129),
        (&["-C", "sub", "bundle", "-h"], 129),
        (&["-C", "sub", "cat-file", "-h"], 129),
        (&["-C", "sub", "check-attr", "-h"], 129),
        (&["-C", "sub", "check-ignore", "-h"], 129),
        (&["-C", "sub", "check-mailmap", "-h"], 129),
        (&["-C", "sub", "check-ref-format", "-h"], 129),
        (&["-C", "sub", "checkout", "-h"], 129),
        (&["-C", "sub", "checkout--worker", "-h"], 129),
        (&["-C", "sub", "checkout-index", "-h"], 129),
        (&["-C", "sub", "cherry", "-h"], 129),
        (&["-C", "sub", "cherry-pick", "-h"], 129),
        (&["-C", "sub", "clone", "-h"], 129),
        (&["-C", "sub", "column", "-h"], 129),
        (&["-C", "sub", "commit", "-h"], 129),
        (&["-C", "sub", "commit-graph", "-h"], 129),
        (&["-C", "sub", "commit-tree", "-h"], 129),
        (&["-C", "sub", "config", "-h"], 129),
        (&["-C", "sub", "count-objects", "-h"], 129),
        (&["-C", "sub", "credential", "-h"], 129),
        (&["-C", "sub", "credential-cache", "-h"], 129),
        (&["-C", "sub", "credential-cache--daemon", "-h"], 129),
        (&["-C", "sub", "credential-store", "-h"], 129),
        (&["-C", "sub", "describe", "-h"], 129),
        (&["-C", "sub", "diagnose", "-h"], 129),
        (&["-C", "sub", "diff", "-h"], 129),
        (&["-C", "sub", "diff-files", "-h"], 129),
        (&["-C", "sub", "diff-index", "-h"], 129),
        (&["-C", "sub", "diff-pairs", "-h"], 129),
        (&["-C", "sub", "diff-tree", "-h"], 129),
        (&["-C", "sub", "difftool", "-h"], 129),
        (&["-C", "sub", "fast-export", "-h"], 129),
        (&["-C", "sub", "fast-import", "-h"], 129),
        (&["-C", "sub", "fetch", "-h"], 129),
        (&["-C", "sub", "fetch-pack", "-h"], 129),
        (&["-C", "sub", "format-patch", "-h"], 129),
        (&["-C", "sub", "fsck", "-h"], 129),
        (&["-C", "sub", "fsck-objects", "-h"], 129),
        (&["-C", "sub", "fsmonitor--daemon", "-h"], 129),
        (&["-C", "sub", "gc", "-h"], 129),
        (&["-C", "sub", "get-tar-commit-id", "-h"], 129),
        (&["-C", "sub", "grep", "-h"], 129),
        (&["-C", "sub", "hash-object", "-h"], 129),
        (&["-C", "sub", "help", "-h"], 129),
        (&["-C", "sub", "hook", "-h"], 129),
        (&["-C", "sub", "index-pack", "-h"], 129),
        (&["-C", "sub", "init", "-h"], 129),
        (&["-C", "sub", "init-db", "-h"], 129),
        (&["-C", "sub", "interpret-trailers", "-h"], 129),
        (&["-C", "sub", "log", "-h"], 129),
        (&["-C", "sub", "ls-files", "-h"], 129),
        (&["-C", "sub", "ls-remote", "-h"], 129),
        (&["-C", "sub", "ls-tree", "-h"], 129),
        (&["-C", "sub", "mailinfo", "-h"], 129),
        (&["-C", "sub", "mailsplit", "-h"], 129),
        (&["-C", "sub", "maintenance", "-h"], 129),
        (&["-C", "sub", "merge", "-h"], 129),
        (&["-C", "sub", "merge-base", "-h"], 129),
        (&["-C", "sub", "merge-file", "-h"], 129),
        (&["-C", "sub", "merge-index", "-h"], 129),
        (&["-C", "sub", "merge-ours", "-h"], 129),
        (&["-C", "sub", "merge-recursive", "-h"], 129),
        (&["-C", "sub", "merge-recursive-ours", "-h"], 129),
        (&["-C", "sub", "merge-recursive-theirs", "-h"], 129),
        (&["-C", "sub", "merge-subtree", "-h"], 129),
        (&["-C", "sub", "merge-tree", "-h"], 129),
        (&["-C", "sub", "mktag", "-h"], 129),
        (&["-C", "sub", "mktree", "-h"], 129),
        (&["-C", "sub", "mv", "-h"], 129),
        (&["-C", "sub", "name-rev", "-h"], 129),
        (&["-C", "sub", "prune", "-h"], 129),
        (&["-C", "sub", "prune-packed", "-h"], 129),
        (&["-C", "sub", "pull", "-h"], 129),
        (&["-C", "sub", "notes", "-h"], 129),
        (&["-C", "sub", "pack-objects", "-h"], 129),
        (&["-C", "sub", "pack-refs", "-h"], 129),
        (&["-C", "sub", "pack-redundant", "-h"], 129),
        (&["-C", "sub", "patch-id", "-h"], 129),
        (&["-C", "sub", "pickaxe", "-h"], 129),
        (&["-C", "sub", "range-diff", "-h"], 129),
        (&["-C", "sub", "read-tree", "-h"], 129),
        (&["-C", "sub", "repack", "-h"], 129),
        (&["-C", "sub", "replace", "-h"], 129),
        (&["-C", "sub", "shortlog", "-h"], 129),
        (&["-C", "sub", "show", "-h"], 129),
        (&["-C", "sub", "show-branch", "-h"], 129),
        (&["-C", "sub", "show-index", "-h"], 129),
        (&["-C", "sub", "show-ref", "-h"], 129),
        (&["-C", "sub", "tag", "-h"], 129),
        (&["-C", "sub", "unpack-file", "-h"], 129),
        (&["-C", "sub", "unpack-objects", "-h"], 129),
        (&["-C", "sub", "update-index", "-h"], 129),
        (&["-C", "sub", "update-ref", "-h"], 129),
        (&["-C", "sub", "update-server-info", "-h"], 129),
        (&["-C", "sub", "upload-archive", "-h"], 129),
        (&["-C", "sub", "upload-archive--writer", "-h"], 129),
        (&["-C", "sub", "upload-pack", "-h"], 129),
        (&["-C", "sub", "worktree", "-h"], 129),
        (&["-C", "sub", "write-tree", "-h"], 129),
        (&["-C", "sub", "var", "-h"], 129),
        (&["-C", "sub", "version", "-h"], 129),
        (&["-C", "sub", "verify-commit", "-h"], 129),
        (&["-C", "sub", "verify-pack", "-h"], 129),
        (&["-C", "sub", "verify-tag", "-h"], 129),
        (&["-C", "sub", "whatchanged", "-h"], 129),
    ];

    for (args, expected_code) in cases {
        let output = if *expected_code == 0 {
            let output = std::process::Command::new(zmin_bin())
                .args(*args)
                .current_dir(dir.path())
                .env("ZMIN_STOCK_GIT", "/definitely/missing/git")
                .env("GIT_BIN", "/definitely/missing/git")
                .env("GIT_PAGER", "cat")
                .env("PAGER", "cat")
                .env("MANPAGER", "cat")
                .env("GIT_MAN_VIEWER", "cat")
                .env("GIT_EDITOR", "true")
                .output()
                .expect("run zmin poisoned builtin help");
            assert!(output.status.success(), "args: {:?}", args);
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
        } else {
            command_failure_output_with_env(
                zmin_bin(),
                dir.path(),
                args,
                poisoned_envs,
                "zmin poisoned builtin help",
            )
        };
        assert_eq!(output.0, *expected_code, "args: {:?}", args);
        assert!(!output.1.is_empty(), "expected stdout for args {:?}", args);
        assert!(
            output.2.is_empty(),
            "expected empty stderr for args {:?}, got: {}",
            args,
            output.2
        );
    }
}
