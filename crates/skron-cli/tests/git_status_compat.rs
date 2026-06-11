mod common;

use std::fs;

use common::{
    configure_identity, git, git_args, git_init, git_with_env, run_skron, run_skron_args,
    run_skron_with_env, write_file,
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
        run_skron(repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        run_skron(repo.path(), ["status", "-sb"]),
        git(repo.path(), ["status", "-sb"])
    );
    assert_eq!(
        run_skron(repo.path(), ["status", "--porcelain"]),
        git(repo.path(), ["status", "--porcelain"])
    );

    fs::write(repo.path().join("a.txt"), b"changed\n").expect("modify tracked");
    fs::write(repo.path().join("b.txt"), b"new\n").expect("write untracked");
    fs::create_dir_all(repo.path().join("dir")).expect("create untracked dir");
    fs::write(repo.path().join("dir/nested.txt"), b"nested\n").expect("write nested untracked");
    assert_eq!(
        run_skron(repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        run_skron(
            repo.path(),
            ["status", "--porcelain=v1", "--branch", "-uno"]
        ),
        git(
            repo.path(),
            ["status", "--porcelain=v1", "--branch", "-uno"]
        )
    );
    assert_eq!(
        run_skron(
            repo.path(),
            ["status", "--porcelain=v1", "--branch", "-uall"]
        ),
        git(
            repo.path(),
            ["status", "--porcelain=v1", "--branch", "-uall"]
        )
    );
    assert_eq!(
        run_skron(repo.path(), ["status", "--untracked-files=no"]),
        git(repo.path(), ["status", "--untracked-files=no"])
    );
    for args in [
        ["status", "--porcelain=v1", "-u"].as_slice(),
        ["status", "--porcelain=v1", "--untracked-files=normal"].as_slice(),
        ["status", "--porcelain=v1", "--untracked-files=all"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
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
        ["status", "--porcelain=v1", "--ignored=matching"].as_slice(),
        ["status", "--porcelain=v1", "--ignored=no"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            git_args(repo.path(), args)
        );
    }
}

#[test]
fn status_detects_same_size_same_mtime_content_change_like_stock_git() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "a.txt", "aaaa\n");
    write_file(skron_repo.path(), "a.txt", "aaaa\n");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "initial"]);

    let git_path = git_repo.path().join("a.txt");
    let skron_path = skron_repo.path().join("a.txt");
    let git_mtime = fs::metadata(&git_path)
        .expect("git metadata")
        .modified()
        .expect("git modified time");
    let skron_mtime = fs::metadata(&skron_path)
        .expect("skron metadata")
        .modified()
        .expect("skron modified time");

    fs::write(&git_path, b"bbbb\n").expect("modify git file");
    fs::write(&skron_path, b"bbbb\n").expect("modify skron file");
    fs::File::open(&git_path)
        .expect("open git file")
        .set_modified(git_mtime)
        .expect("restore git mtime");
    fs::File::open(&skron_path)
        .expect("open skron file")
        .set_modified(skron_mtime)
        .expect("restore skron mtime");

    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
}

#[test]
fn status_human_matches_stock_git_for_common_states() {
    let unborn = git_init();
    assert_eq!(
        run_skron(unborn.path(), ["status"]),
        git(unborn.path(), ["status"])
    );
    fs::write(unborn.path().join("new.txt"), b"new\n").expect("write unborn untracked");
    assert_eq!(
        run_skron(unborn.path(), ["status"]),
        git(unborn.path(), ["status"])
    );
    git(unborn.path(), ["add", "new.txt"]);
    assert_eq!(
        run_skron(unborn.path(), ["status"]),
        git(unborn.path(), ["status"])
    );

    let repo = committed_repo();
    assert_eq!(
        run_skron(repo.path(), ["status"]),
        git(repo.path(), ["status"])
    );
    fs::write(repo.path().join("a.txt"), b"changed\n").expect("modify tracked");
    assert_eq!(
        run_skron(repo.path(), ["status"]),
        git(repo.path(), ["status"])
    );
    fs::write(repo.path().join("staged.txt"), b"staged\n").expect("write staged");
    git(repo.path(), ["add", "staged.txt"]);
    fs::write(repo.path().join("untracked.txt"), b"untracked\n").expect("write untracked");
    assert_eq!(
        run_skron(repo.path(), ["status"]),
        git(repo.path(), ["status"])
    );
    fs::remove_file(repo.path().join("a.txt")).expect("delete tracked");
    assert_eq!(
        run_skron(repo.path(), ["status"]),
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
    run_skron(&work, ["add", "-A"]);
    run_skron_with_env(&work, ["commit", "-m", "local"]);

    assert_eq!(
        run_skron(&work, ["status", "--porcelain=v1", "--branch"]),
        git(&work, ["status", "--porcelain=v1", "--branch"])
    );
}

fn committed_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    run_skron(repo.path(), ["add", "-A"]);
    run_skron_with_env(repo.path(), ["commit", "-m", "initial"]);
    repo
}
