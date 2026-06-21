mod common;

use common::{
    command_any_output, command_failure_output_with_env, configure_identity, git, git_with_env,
    write_file, zmin_bin,
};
use tempfile::TempDir;

#[test]
fn filter_branch_parent_filter_bad_token_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_repo = create_repo(dir.path(), "git-rewrite");
    let zmin_repo = create_repo(dir.path(), "zmin-rewrite");
    let git_head = git(&git_repo, ["rev-parse", "HEAD"]);
    let zmin_head = git(&zmin_repo, ["rev-parse", "HEAD"]);
    let args = [
        "filter-branch",
        "-f",
        "--parent-filter",
        "printf bad",
        "HEAD",
    ];
    let env = [("FILTER_BRANCH_SQUELCH_WARNING", "1")];

    let stock = command_failure_output_with_env(
        "git",
        &git_repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "filter-branch",
            "-f",
            "--parent-filter",
            "printf bad",
            "HEAD",
        ],
        &env,
        "git filter-branch bad parent-filter",
    );
    assert_eq!(stock.0, 1);
    assert!(stock.1.contains("Rewrite "));
    assert!(stock.2.contains("fatal: must give exactly one tree"));
    assert!(stock.2.contains("could not write rewritten commit"));
    assert_eq!(git(&git_repo, ["rev-parse", "HEAD"]), git_head);
    assert_ne!(
        command_any_output(
            "git",
            &git_repo,
            &["show-ref", "--verify", "refs/original/refs/heads/main"],
            "git missing original ref"
        )
        .0,
        0
    );

    let zmin = command_failure_output_with_env(
        zmin_bin(),
        &zmin_repo,
        &args,
        &env,
        "zmin filter-branch bad parent-filter",
    );
    assert_eq!(zmin.0, 1);
    assert!(zmin.1.contains("Rewrite "));
    assert!(zmin.2.contains("fatal: must give exactly one tree"));
    assert!(zmin.2.contains("could not write rewritten commit"));
    assert_eq!(git(&zmin_repo, ["rev-parse", "HEAD"]), zmin_head);
    assert_ne!(
        command_any_output(
            "git",
            &zmin_repo,
            &["show-ref", "--verify", "refs/original/refs/heads/main"],
            "zmin missing original ref"
        )
        .0,
        0
    );
}

fn create_repo(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    let repo = root.join(name);
    git(
        root,
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    write_file(&repo, "a.txt", "one\n");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "one"]);
    write_file(&repo, "a.txt", "two\n");
    git(&repo, ["add", "-A"]);
    git_with_env(&repo, ["commit", "-m", "two"]);
    repo
}
