mod common;

use common::{git_args, git_status_args, run_skron_args, run_skron_status_args};

#[test]
fn check_ref_format_matches_stock_git_for_common_modes() {
    let repo = common::git_init();

    for args in [
        ["check-ref-format", "refs/heads/main"].as_slice(),
        ["check-ref-format", "--allow-onelevel", "main"].as_slice(),
        ["check-ref-format", "--normalize", "/refs//heads/main"].as_slice(),
        ["check-ref-format", "--branch", "main"].as_slice(),
        ["check-ref-format", "--branch", "refs/heads/main"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            git_args(repo.path(), args)
        );
    }

    for args in [
        ["check-ref-format", "main"].as_slice(),
        ["check-ref-format", "refs/heads/bad..name"].as_slice(),
        ["check-ref-format", "refs/heads/.hidden"].as_slice(),
        ["check-ref-format", "refs/heads/trailing."].as_slice(),
        ["check-ref-format", "--normalize", "refs/heads/main/"].as_slice(),
        ["check-ref-format", "--branch", "bad..name"].as_slice(),
    ] {
        assert_eq!(
            run_skron_status_args(repo.path(), args),
            git_status_args(repo.path(), args)
        );
    }
}
