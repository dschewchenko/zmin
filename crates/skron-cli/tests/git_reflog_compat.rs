mod common;

use common::{
    configure_identity, git, git_args, git_init, git_status, git_with_env, run_skron_args,
    run_skron_status,
};

#[test]
fn reflog_show_list_and_exists_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "one"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "two"]);

    for args in [
        ["reflog"].as_slice(),
        ["reflog", "show"].as_slice(),
        ["reflog", "--date=iso"].as_slice(),
        ["reflog", "--date=unix"].as_slice(),
        ["reflog", "--date=raw"].as_slice(),
        ["reflog", "show", "main"].as_slice(),
        ["reflog", "list"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    assert_eq!(
        run_skron_status(repo.path(), ["reflog", "exists", "HEAD"]),
        git_status(repo.path(), ["reflog", "exists", "HEAD"])
    );
    assert_eq!(
        run_skron_status(repo.path(), ["reflog", "exists", "refs/heads/missing"]),
        git_status(repo.path(), ["reflog", "exists", "refs/heads/missing"])
    );
}
