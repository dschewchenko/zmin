mod common;

use common::{git_failure_output, git_init, run_zmin_failure_output};

#[test]
fn worktree_unknown_subcommand_matches_stock_git_usage() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["worktree", "bogus"]),
        git_failure_output(git_repo.path(), &["worktree", "bogus"]),
    );
}
