mod common;

use std::fs;

use common::{
    configure_identity, git, git_status, git_with_env, git_with_stdin, run_skron, run_skron_status,
    run_skron_with_stdin,
};

#[test]
fn check_ignore_matches_stock_git_for_common_modes() {
    let repo = common::git_init();
    configure_identity(repo.path());
    fs::write(
        repo.path().join(".gitignore"),
        b"target/\n*.log\n/build/*.tmp\n/dist\n",
    )
    .expect("write gitignore");
    fs::create_dir_all(repo.path().join("target")).expect("create target");
    fs::create_dir_all(repo.path().join("build")).expect("create build");
    fs::create_dir_all(repo.path().join("src")).expect("create src");
    fs::write(repo.path().join("target/a"), b"ignored\n").expect("write target");
    fs::write(repo.path().join("src/debug.log"), b"ignored\n").expect("write log");
    fs::write(repo.path().join("build/cache.tmp"), b"ignored\n").expect("write tmp");
    fs::write(repo.path().join("keep.txt"), b"keep\n").expect("write keep");

    assert_eq!(
        run_skron(
            repo.path(),
            ["check-ignore", "target/a", "src/debug.log", "keep.txt"]
        ),
        git(
            repo.path(),
            ["check-ignore", "target/a", "src/debug.log", "keep.txt"]
        )
    );
    assert_eq!(
        run_skron(
            repo.path(),
            [
                "check-ignore",
                "-v",
                "target/a",
                "src/debug.log",
                "keep.txt"
            ]
        ),
        git(
            repo.path(),
            [
                "check-ignore",
                "-v",
                "target/a",
                "src/debug.log",
                "keep.txt"
            ]
        )
    );
    assert_eq!(
        run_skron_with_stdin(
            repo.path(),
            ["check-ignore", "--stdin"],
            "target/a\nkeep.txt\nsrc/debug.log\n"
        ),
        git_with_stdin(
            repo.path(),
            ["check-ignore", "--stdin"],
            "target/a\nkeep.txt\nsrc/debug.log\n"
        )
    );
    assert_eq!(
        run_skron_status(repo.path(), ["check-ignore", "-q", "target/a"]),
        git_status(repo.path(), ["check-ignore", "-q", "target/a"])
    );
    assert_eq!(
        run_skron_status(repo.path(), ["check-ignore", "keep.txt"]),
        git_status(repo.path(), ["check-ignore", "keep.txt"])
    );

    fs::write(repo.path().join("tracked.log"), b"tracked\n").expect("write tracked");
    git(repo.path(), ["add", "-f", "tracked.log"]);
    git_with_env(repo.path(), ["commit", "-m", "tracked"]);
    assert_eq!(
        run_skron(repo.path(), ["check-ignore", "--no-index", "tracked.log"]),
        git(repo.path(), ["check-ignore", "--no-index", "tracked.log"])
    );
    assert_eq!(
        run_skron_status(repo.path(), ["check-ignore", "tracked.log"]),
        git_status(repo.path(), ["check-ignore", "tracked.log"])
    );
}
