mod common;

use tempfile::TempDir;

use common::{
    clone_repo_fixture, configure_identity, git, git_failure_output, git_init, git_with_env,
    run_zmin, run_zmin_failure_output, write_file,
};

fn bisect_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    for idx in 0..5 {
        write_file(repo.path(), "a.txt", &format!("{idx}\n"));
        git(repo.path(), ["add", "-A"]);
        git_with_env(repo.path(), ["commit", "-m", &format!("c{idx}")]);
    }
    repo
}

#[test]
fn bisect_visualize_unknown_option_matches_stock_git() {
    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());

    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_zmin(zmin_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["bisect", "visualize", "--bad"]),
        git_failure_output(git_repo.path(), &["bisect", "visualize", "--bad"])
    );
}
