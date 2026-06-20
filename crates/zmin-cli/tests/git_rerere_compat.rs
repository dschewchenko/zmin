mod common;

use common::{git, git_failure_output, run_zmin_failure_output};
use tempfile::TempDir;

#[test]
fn rerere_invalid_operation_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path();
    git(repo, ["init"]);

    assert_eq!(
        run_zmin_failure_output(repo, &["rerere", "bogus"]),
        git_failure_output(repo, &["rerere", "bogus"])
    );
}
