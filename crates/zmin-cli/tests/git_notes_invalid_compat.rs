mod common;

use common::{git_failure_output, git_init, run_zmin_failure_output};

#[test]
fn notes_copy_unknown_option_matches_stock_git_usage() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["notes", "copy", "--bad"]),
        git_failure_output(git_repo.path(), &["notes", "copy", "--bad"])
    );
}

#[test]
fn notes_subcommand_unknown_options_match_stock_git_usage() {
    for subcommand in ["add", "edit", "remove", "prune", "merge"] {
        let git_repo = git_init();
        let zmin_repo = git_init();

        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), &["notes", subcommand, "--bad"]),
            git_failure_output(git_repo.path(), &["notes", subcommand, "--bad"]),
            "notes {subcommand} --bad"
        );
    }
}
