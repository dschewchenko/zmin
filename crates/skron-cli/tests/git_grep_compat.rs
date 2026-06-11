mod common;

use std::fs;

use common::{
    configure_identity, git, git_init, git_status, git_with_env, run_skron, run_skron_status,
};

#[test]
fn grep_matches_stock_git_for_tracked_text_files() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("a.txt"), b"hello\nworld\nhello world\n").expect("write a");
    fs::write(repo.path().join("dir/b.txt"), b"nested hello\n").expect("write b");
    fs::write(repo.path().join("literal.txt"), b"hello.world\n").expect("write literal");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    assert_eq!(
        run_skron(repo.path(), ["grep", "hello"]),
        git(repo.path(), ["grep", "hello"])
    );
    assert_eq!(
        run_skron(repo.path(), ["grep", "-n", "hello"]),
        git(repo.path(), ["grep", "-n", "hello"])
    );
    assert_eq!(
        run_skron(repo.path(), ["grep", "-l", "hello"]),
        git(repo.path(), ["grep", "-l", "hello"])
    );
    assert_eq!(
        run_skron(repo.path(), ["grep", "hello", "dir"]),
        git(repo.path(), ["grep", "hello", "dir"])
    );
    assert_eq!(
        run_skron(repo.path(), ["grep", "-F", "hello.world"]),
        git(repo.path(), ["grep", "-F", "hello.world"])
    );

    fs::write(repo.path().join("a.txt"), b"cached hello\n").expect("write cached grep");
    git(repo.path(), ["add", "a.txt"]);
    fs::write(repo.path().join("a.txt"), b"worktree hello\n").expect("write worktree grep");
    assert_eq!(
        run_skron(repo.path(), ["grep", "hello", "a.txt"]),
        git(repo.path(), ["grep", "hello", "a.txt"])
    );
    assert_eq!(
        run_skron(repo.path(), ["grep", "--cached", "hello", "a.txt"]),
        git(repo.path(), ["grep", "--cached", "hello", "a.txt"])
    );
    assert_eq!(
        run_skron(repo.path(), ["grep", "hello", "HEAD", "--", "a.txt"]),
        git(repo.path(), ["grep", "hello", "HEAD", "--", "a.txt"])
    );
    assert_eq!(
        run_skron(repo.path(), ["grep", "-n", "hello", "HEAD"]),
        git(repo.path(), ["grep", "-n", "hello", "HEAD"])
    );
    assert_eq!(
        run_skron(repo.path(), ["grep", "-l", "hello", "HEAD"]),
        git(repo.path(), ["grep", "-l", "hello", "HEAD"])
    );
    assert_eq!(
        run_skron(repo.path(), ["grep", "hello", "HEAD", "--", "dir"]),
        git(repo.path(), ["grep", "hello", "HEAD", "--", "dir"])
    );
    assert_eq!(
        run_skron_status(repo.path(), ["grep", "absent"]),
        git_status(repo.path(), ["grep", "absent"])
    );
}
