mod common;

use common::{git_failure_output, git_init, run_zmin_failure_output};

#[test]
fn submodule_subcommand_unknown_options_match_stock_git_usage() {
    for subcommand in ["add", "status", "update", "deinit", "set-branch", "summary"] {
        let git_repo = git_init();
        let zmin_repo = git_init();

        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), &["submodule", subcommand, "--bad"]),
            git_failure_output(git_repo.path(), &["submodule", subcommand, "--bad"]),
            "submodule {subcommand} --bad"
        );
    }
}
