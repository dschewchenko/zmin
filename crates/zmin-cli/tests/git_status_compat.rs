mod common;

use std::fs;

#[cfg(not(windows))]
use common::write_file;
use common::{
    configure_identity, git, git_args, git_failure_output, git_init, git_with_env, run_zmin,
    run_zmin_args, run_zmin_failure_output, run_zmin_with_env,
};
use tempfile::TempDir;

#[test]
fn status_porcelain_matches_stock_git_for_clean_dirty_and_ignored_worktrees() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write tracked");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "-sb"]),
        git(repo.path(), ["status", "-sb"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--short"]),
        git(repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain"]),
        git(repo.path(), ["status", "--porcelain"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "-z"]),
        git(repo.path(), ["status", "-z"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--null"]),
        git(repo.path(), ["status", "--null"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v2", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v2", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v2", "-z", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v2", "-z", "--branch"])
    );

    fs::write(repo.path().join("a.txt"), b"changed\n").expect("modify tracked");
    fs::write(repo.path().join("b.txt"), b"new\n").expect("write untracked");
    fs::create_dir_all(repo.path().join("dir")).expect("create untracked dir");
    fs::write(repo.path().join("dir/nested.txt"), b"nested\n").expect("write nested untracked");
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v1", "-z", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v1", "-z", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "-z"]),
        git(repo.path(), ["status", "-z"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--null"]),
        git(repo.path(), ["status", "--null"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v2", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v2", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v2", "-z", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v2", "-z", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v2", "--short"]),
        git(repo.path(), ["status", "--porcelain=v2", "--short"])
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            ["status", "--porcelain=v2", "--short", "--branch"]
        ),
        git(
            repo.path(),
            ["status", "--porcelain=v2", "--short", "--branch"]
        )
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            ["status", "--porcelain=v1", "--branch", "-uno"]
        ),
        git(
            repo.path(),
            ["status", "--porcelain=v1", "--branch", "-uno"]
        )
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            ["status", "--porcelain=v1", "--branch", "-uall"]
        ),
        git(
            repo.path(),
            ["status", "--porcelain=v1", "--branch", "-uall"]
        )
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--untracked-files=no"]),
        git(repo.path(), ["status", "--untracked-files=no"])
    );
    for args in [
        ["status", "--porcelain=v1", "-u"].as_slice(),
        ["status", "--porcelain=v1", "-unormal"].as_slice(),
        ["status", "--porcelain=v1", "--untracked-files"].as_slice(),
        ["status", "--porcelain=v1", "--untracked-files=normal"].as_slice(),
        ["status", "--porcelain=v1", "--untracked-files=all"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    fs::write(repo.path().join(".gitignore"), b"*.log\nignored-dir/\n").expect("write gitignore");
    fs::write(repo.path().join("debug.log"), b"ignored\n").expect("write ignored file");
    fs::create_dir_all(repo.path().join("ignored-dir")).expect("create ignored dir");
    fs::write(repo.path().join("ignored-dir/file.txt"), b"ignored\n")
        .expect("write ignored dir file");
    for args in [
        ["status", "--porcelain=v1", "--ignored"].as_slice(),
        ["status", "--porcelain=v1", "--ignored=traditional"].as_slice(),
        ["status", "--porcelain=v1", "--ignored=matching"].as_slice(),
        ["status", "--porcelain=v1", "--ignored=no"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args)
        );
    }
}

#[test]
fn status_porcelain_v2_matches_stock_git_for_staged_states() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("modified.txt"), b"old\n").expect("write modified");
    fs::write(repo.path().join("deleted.txt"), b"old\n").expect("write deleted");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    fs::write(repo.path().join("modified.txt"), b"new\n").expect("modify tracked");
    fs::write(repo.path().join("added.txt"), b"added\n").expect("write added");
    git(repo.path(), ["add", "modified.txt", "added.txt"]);
    git(repo.path(), ["rm", "-q", "deleted.txt"]);

    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v2", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v2", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v2", "-z", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v2", "-z", "--branch"])
    );
}

#[test]
#[cfg(not(windows))]
fn status_detects_same_size_same_mtime_content_change_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "aaaa\n");
    write_file(zmin_repo.path(), "a.txt", "aaaa\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "initial"]);

    let git_path = git_repo.path().join("a.txt");
    let zmin_path = zmin_repo.path().join("a.txt");
    let git_mtime = fs::metadata(&git_path)
        .expect("git metadata")
        .modified()
        .expect("git modified time");
    let zmin_mtime = fs::metadata(&zmin_path)
        .expect("zmin metadata")
        .modified()
        .expect("zmin modified time");

    fs::write(&git_path, b"bbbb\n").expect("modify git file");
    fs::write(&zmin_path, b"bbbb\n").expect("modify zmin file");
    fs::OpenOptions::new()
        .write(true)
        .open(&git_path)
        .expect("open git file")
        .set_modified(git_mtime)
        .expect("restore git mtime");
    fs::OpenOptions::new()
        .write(true)
        .open(&zmin_path)
        .expect("open zmin file")
        .set_modified(zmin_mtime)
        .expect("restore zmin mtime");

    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
}

#[test]
fn status_human_matches_stock_git_for_common_states() {
    let unborn = git_init();
    assert_eq!(
        run_zmin(unborn.path(), ["status"]),
        git(unborn.path(), ["status"])
    );
    fs::write(unborn.path().join("new.txt"), b"new\n").expect("write unborn untracked");
    assert_eq!(
        run_zmin(unborn.path(), ["status"]),
        git(unborn.path(), ["status"])
    );
    git(unborn.path(), ["add", "new.txt"]);
    assert_eq!(
        run_zmin(unborn.path(), ["status"]),
        git(unborn.path(), ["status"])
    );

    let repo = committed_repo();
    assert_eq!(
        run_zmin(repo.path(), ["status"]),
        git(repo.path(), ["status"])
    );
    fs::write(repo.path().join("a.txt"), b"changed\n").expect("modify tracked");
    assert_eq!(
        run_zmin(repo.path(), ["status"]),
        git(repo.path(), ["status"])
    );
    fs::write(repo.path().join("staged.txt"), b"staged\n").expect("write staged");
    git(repo.path(), ["add", "staged.txt"]);
    fs::write(repo.path().join("untracked.txt"), b"untracked\n").expect("write untracked");
    assert_eq!(
        run_zmin(repo.path(), ["status"]),
        git(repo.path(), ["status"])
    );
    fs::remove_file(repo.path().join("a.txt")).expect("delete tracked");
    assert_eq!(
        run_zmin(repo.path(), ["status"]),
        git(repo.path(), ["status"])
    );
}

#[test]
fn status_branch_reports_upstream_ahead_count() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(
        dir.path(),
        ["clone", remote.to_str().expect("remote path"), "work"],
    );
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["push", "-u", "origin", "HEAD"]);

    fs::write(work.join("b.txt"), b"local\n").expect("write local");
    run_zmin(&work, ["add", "-A"]);
    run_zmin_with_env(&work, ["commit", "-m", "local"]);

    assert_eq!(
        run_zmin(&work, ["status", "--porcelain=v1", "--branch"]),
        git(&work, ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        run_zmin(
            &work,
            ["status", "--porcelain=v2", "--branch", "--ahead-behind"]
        ),
        git(
            &work,
            ["status", "--porcelain=v2", "--branch", "--ahead-behind"]
        )
    );
    assert_eq!(
        run_zmin(
            &work,
            ["status", "--porcelain=v2", "--branch", "--no-ahead-behind"]
        ),
        git(
            &work,
            ["status", "--porcelain=v2", "--branch", "--no-ahead-behind"]
        )
    );
    assert_eq!(
        run_zmin(
            &work,
            ["status", "--porcelain=v1", "--branch", "--no-ahead-behind"]
        ),
        git(
            &work,
            ["status", "--porcelain=v1", "--branch", "--no-ahead-behind"]
        )
    );
    assert_eq!(
        run_zmin(
            &work,
            ["status", "--short", "--branch", "--no-ahead-behind"]
        ),
        git(
            &work,
            ["status", "--short", "--branch", "--no-ahead-behind"]
        )
    );
}

#[test]
fn status_human_branch_modes_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(
        dir.path(),
        ["clone", remote.to_str().expect("remote path"), "work"],
    );
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["push", "-u", "origin", "HEAD"]);

    fs::write(work.join("a.txt"), b"changed\n").expect("modify tracked");
    fs::write(work.join("b.txt"), b"untracked\n").expect("write untracked");
    for args in [
        ["status", "-b"].as_slice(),
        ["status", "--branch"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&work, args),
            git_args(&work, args),
            "dirty human branch args: {args:?}"
        );
    }

    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "local"]);
    for args in [
        ["status", "-b"].as_slice(),
        ["status", "--branch"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&work, args),
            git_args(&work, args),
            "ahead human branch args: {args:?}"
        );
    }
}

#[test]
fn status_pathspec_modes_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::create_dir_all(repo.path().join("other")).expect("create other dir");
    for (path, content) in [
        ("a.txt", b"base\n".as_slice()),
        ("a*b.txt", b"base\n".as_slice()),
        ("dir/one.txt", b"base\n".as_slice()),
        ("dir/two.log", b"base\n".as_slice()),
        ("other/ABC.TXT", b"base\n".as_slice()),
    ] {
        fs::write(repo.path().join(path), content).expect("write tracked pathspec fixture");
    }
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "pathspec base"]);

    for path in [
        "a.txt",
        "a*b.txt",
        "dir/one.txt",
        "dir/two.log",
        "other/ABC.TXT",
    ] {
        fs::write(repo.path().join(path), b"changed\n").expect("modify pathspec fixture");
    }
    fs::write(repo.path().join("dir/new.txt"), b"new\n").expect("write nested untracked");
    fs::write(repo.path().join("root-new.txt"), b"new\n").expect("write root untracked");

    for args in [
        ["status", "--porcelain=v1", "--", "a.txt"].as_slice(),
        ["status", "--porcelain=v1", "--", "dir"].as_slice(),
        ["status", "--porcelain=v1", "--", "dir/"].as_slice(),
        ["status", "--porcelain=v1", "--", "*.txt"].as_slice(),
        ["status", "--porcelain=v1", "--", ":(glob)dir/*.txt"].as_slice(),
        ["status", "--porcelain=v1", "--", ":(literal)a*b.txt"].as_slice(),
        ["status", "--porcelain=v1", "--", ":(icase)other/abc.txt"].as_slice(),
        ["status", "--porcelain=v1", "--", "*.txt", ":(exclude)a.txt"].as_slice(),
        ["status", "--short", "--", "*.txt", ":(exclude)a.txt"].as_slice(),
        ["status", "--", "dir"].as_slice(),
        [
            "--literal-pathspecs",
            "status",
            "--porcelain=v1",
            "--",
            "a*b.txt",
        ]
        .as_slice(),
        [
            "--glob-pathspecs",
            "status",
            "--porcelain=v1",
            "--",
            "a*.txt",
        ]
        .as_slice(),
        [
            "--icase-pathspecs",
            "status",
            "--porcelain=v1",
            "--",
            "other/abc.txt",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "status pathspec args: {args:?}"
        );
    }
}

#[test]
fn status_branch_no_ahead_behind_reports_equal_upstream_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(
        dir.path(),
        ["clone", remote.to_str().expect("remote path"), "work"],
    );
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["push", "-u", "origin", "HEAD"]);

    assert_eq!(
        run_zmin(
            &work,
            ["status", "--porcelain=v2", "--branch", "--no-ahead-behind"]
        ),
        git(
            &work,
            ["status", "--porcelain=v2", "--branch", "--no-ahead-behind"]
        )
    );
    assert_eq!(
        run_zmin(
            &work,
            ["status", "--porcelain=v1", "--branch", "--no-ahead-behind"]
        ),
        git(
            &work,
            ["status", "--porcelain=v1", "--branch", "--no-ahead-behind"]
        )
    );
}

#[test]
fn status_show_stash_matches_stock_git() {
    let repo = committed_repo();
    fs::write(repo.path().join("a.txt"), b"stash one\n").expect("modify first stash");
    run_zmin(repo.path(), ["stash", "push", "-m", "one"]);
    fs::write(repo.path().join("a.txt"), b"stash two\n").expect("modify second stash");
    run_zmin(repo.path(), ["stash", "push", "-m", "two"]);

    for args in [
        ["status", "--show-stash"].as_slice(),
        ["status", "--no-show-stash"].as_slice(),
        ["status", "--show-stash", "--no-show-stash"].as_slice(),
        ["status", "--no-show-stash", "--show-stash"].as_slice(),
        ["status", "--porcelain=v2", "--show-stash"].as_slice(),
        ["status", "--porcelain=v2", "--branch", "--show-stash"].as_slice(),
        ["status", "--porcelain=v1", "--branch", "--show-stash"].as_slice(),
        ["status", "--short", "--show-stash"].as_slice(),
        ["status", "-z", "--show-stash"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn status_long_mode_toggles_match_stock_git() {
    let repo = committed_repo();
    fs::write(repo.path().join("a.txt"), b"changed\n").expect("modify tracked");
    fs::write(repo.path().join("staged.txt"), b"staged\n").expect("write staged");
    git(repo.path(), ["add", "staged.txt"]);

    for args in [
        ["status", "--long"].as_slice(),
        ["status", "--no-long"].as_slice(),
        ["status", "--short", "--long"].as_slice(),
        ["status", "--long", "--short"].as_slice(),
        ["status", "--short", "--no-long"].as_slice(),
        ["status", "--no-long", "--short"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn status_verbose_modes_match_stock_git() {
    let repo = committed_repo();
    fs::write(repo.path().join("a.txt"), b"hello\nchanged\n").expect("modify tracked");
    fs::write(repo.path().join("staged.txt"), b"staged\n").expect("write staged");
    git(repo.path(), ["add", "staged.txt"]);

    for args in [
        ["status", "--verbose"].as_slice(),
        ["status", "-v"].as_slice(),
        ["status", "-vv"].as_slice(),
        ["status", "--verbose", "--no-verbose"].as_slice(),
        ["status", "--no-verbose", "--verbose"].as_slice(),
        ["status", "-vv", "--no-verbose"].as_slice(),
        ["status", "--no-verbose", "-vv"].as_slice(),
        ["status", "--short", "--verbose"].as_slice(),
        ["status", "--porcelain=v1", "--verbose"].as_slice(),
        ["status", "--porcelain=v2", "--verbose"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn status_column_modes_match_stock_git() {
    let repo = committed_repo();
    for index in 1..=12 {
        fs::write(
            repo.path().join(format!("untracked-{index:02}.txt")),
            b"untracked\n",
        )
        .expect("write untracked fixture");
    }

    for args in [
        ["status", "--column"].as_slice(),
        ["status", "--no-column"].as_slice(),
        ["status", "--column", "--no-column"].as_slice(),
        ["status", "--no-column", "--column"].as_slice(),
        ["status", "--column=never"].as_slice(),
        ["status", "--column=always"].as_slice(),
        ["status", "--short", "--column"].as_slice(),
        ["status", "--porcelain=v1", "--column"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    git(repo.path(), ["config", "column.status", "always"]);
    run_zmin(repo.path(), ["config", "column.status", "always"]);
    for args in [
        ["status"].as_slice(),
        ["status", "--no-column"].as_slice(),
        ["status", "--column=never"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args with column.status=always: {args:?}"
        );
    }
}

#[test]
fn status_cache_index_toggles_are_not_stock_git_options() {
    let repo = committed_repo();
    for args in [
        ["status", "--untracked-cache"].as_slice(),
        ["status", "--no-untracked-cache"].as_slice(),
        ["status", "--split-index"].as_slice(),
        ["status", "--no-split-index"].as_slice(),
    ] {
        let git_output = git_failure_output(repo.path(), args);
        let zmin_output = run_zmin_failure_output(repo.path(), args);
        assert_eq!(git_output.0, 129, "stock Git args: {args:?}");
        assert_eq!(zmin_output.0, 129, "Zmin args: {args:?}");
    }
}

#[test]
fn status_invalid_porcelain_version_matches_stock_git() {
    let repo = committed_repo();
    let args = ["status", "--porcelain=v3"];

    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );
}

#[test]
fn status_invalid_untracked_files_mode_matches_stock_git() {
    let repo = committed_repo();
    let args = ["status", "--untracked-files=bogus"];

    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );
}

#[test]
fn status_invalid_ignore_submodules_mode_matches_stock_git() {
    let repo = committed_repo();
    let args = ["status", "--ignore-submodules=bogus"];

    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );
}

#[test]
fn status_rename_modes_match_stock_git() {
    let repo = committed_repo();
    run_zmin(repo.path(), ["mv", "a.txt", "renamed.txt"]);

    for args in [
        ["status", "--porcelain=v1"].as_slice(),
        ["status", "--porcelain=v1", "--renames"].as_slice(),
        ["status", "--porcelain=v1", "--no-renames"].as_slice(),
        ["status", "--porcelain=v1", "--find-renames"].as_slice(),
        ["status", "--porcelain=v1", "--find-renames=50%"].as_slice(),
        ["status", "--porcelain=v2", "--renames"].as_slice(),
        ["status", "--short", "--renames"].as_slice(),
        ["status", "--renames"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn status_ignore_submodules_modes_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let sub_src = dir.path().join("sub-src");
    let super_repo = dir.path().join("super");
    git(dir.path(), ["init", "sub-src"]);
    configure_identity(&sub_src);
    fs::write(sub_src.join("file.txt"), b"base\n").expect("write submodule source");
    git(&sub_src, ["add", "-A"]);
    git_with_env(&sub_src, ["commit", "-m", "sub init"]);

    git(dir.path(), ["init", "super"]);
    configure_identity(&super_repo);
    git(
        &super_repo,
        [
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "../sub-src",
            "sub",
        ],
    );
    git_with_env(&super_repo, ["commit", "-m", "add submodule"]);

    fs::write(super_repo.join("sub/file.txt"), b"base\ndirty\n").expect("dirty submodule");
    for args in [
        ["status", "--porcelain=v1"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules=all"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules=dirty"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules=untracked"].as_slice(),
        ["status", "--porcelain=v2"].as_slice(),
        ["status", "--short", "--ignore-submodules=untracked"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&super_repo, args),
            git_args(&super_repo, args),
            "dirty submodule args: {args:?}"
        );
    }

    fs::write(super_repo.join("sub/new.txt"), b"untracked\n").expect("untracked submodule");
    for args in [
        ["status", "--porcelain=v1"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules=dirty"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules=untracked"].as_slice(),
        ["status", "--short", "--ignore-submodules=untracked"].as_slice(),
        ["status", "--ignore-submodules=untracked"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&super_repo, args),
            git_args(&super_repo, args),
            "dirty and untracked submodule args: {args:?}"
        );
    }

    git(&super_repo.join("sub"), ["add", "-A"]);
    git_with_env(&super_repo.join("sub"), ["commit", "-m", "sub change"]);
    for args in [
        ["status", "--porcelain=v1"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules=dirty"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules=all"].as_slice(),
        ["status", "--porcelain=v2", "--ignore-submodules=dirty"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&super_repo, args),
            git_args(&super_repo, args),
            "new submodule commit args: {args:?}"
        );
    }
}

fn committed_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    run_zmin(repo.path(), ["add", "-A"]);
    run_zmin_with_env(repo.path(), ["commit", "-m", "initial"]);
    repo
}
