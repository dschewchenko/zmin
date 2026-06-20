mod common;

use common::{
    configure_identity, git, git_failure_output, git_init, git_with_env, run_zmin_failure_output,
    write_file,
};

#[test]
fn log_invalid_diff_merges_value_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "a\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);

    assert_eq!(
        run_zmin_failure_output(repo.path(), &["log", "--diff-merges=bogus", "-1"]),
        git_failure_output(repo.path(), &["log", "--diff-merges=bogus", "-1"])
    );
}

#[test]
fn log_invalid_decorate_value_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "a\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);

    assert_eq!(
        run_zmin_failure_output(repo.path(), &["log", "--decorate=bogus", "-1"]),
        git_failure_output(repo.path(), &["log", "--decorate=bogus", "-1"])
    );
}
