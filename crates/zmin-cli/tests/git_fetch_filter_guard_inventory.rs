mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    command_any_output, configure_identity, git, git_status, git_with_env, run_zmin_failure_output,
    zmin_bin,
};

#[test]
fn fetch_filter_local_file_and_shallow_guards_are_git_supported_gaps() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_local_client = dir.path().join("git-local-client");
    let zmin_local_client = dir.path().join("zmin-local-client");
    let git_file_client = dir.path().join("git-file-client");
    let zmin_file_client = dir.path().join("zmin-file-client");
    let shallow_client = dir.path().join("shallow-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"one\n").expect("write one");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "one"]);

    git(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            git_local_client.to_str().expect("git local client path"),
        ],
    );
    git(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            zmin_local_client.to_str().expect("zmin local client path"),
        ],
    );
    let file_url = format!("file://{}", source.display());
    git(
        dir.path(),
        [
            "clone",
            file_url.as_str(),
            git_file_client.to_str().expect("git file client path"),
        ],
    );
    git(
        dir.path(),
        [
            "clone",
            file_url.as_str(),
            zmin_file_client.to_str().expect("zmin file client path"),
        ],
    );
    git(
        dir.path(),
        [
            "clone",
            "--depth=1",
            file_url.as_str(),
            shallow_client.to_str().expect("shallow client path"),
        ],
    );
    fs::write(source.join("file"), b"two\n").expect("write two");
    git(&source, ["commit", "-am", "two"]);

    assert_filter_fetch_matches_stock(
        &git_local_client,
        &zmin_local_client,
        &["fetch", "--filter=blob:none", "origin", "main"],
    );

    assert_filter_fetch_matches_stock(
        &git_file_client,
        &zmin_file_client,
        &["fetch", "--filter=blob:none", "origin", "main"],
    );

    assert_eq!(
        git_status(
            &shallow_client,
            ["fetch", "--filter=blob:none", "--depth=1", "origin", "main"],
        ),
        0
    );
    assert_zmin_filter_gap(
        &shallow_client,
        &["fetch", "--filter=blob:none", "--depth=1", "origin", "main"],
        "fetch --filter currently supports non-shallow network fetches",
    );
}

fn assert_filter_fetch_matches_stock(
    git_repo: &std::path::Path,
    zmin_repo: &std::path::Path,
    args: &[&str],
) {
    let git_output = command_any_output("git", git_repo, args, "git");
    let zmin_output = command_any_output(zmin_bin(), zmin_repo, args, "zmin");
    assert_eq!(
        zmin_output.0, git_output.0,
        "zmin stderr: {}",
        zmin_output.2
    );
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(zmin_output.2, git_output.2);
    assert_eq!(
        fs::read_to_string(zmin_repo.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_repo.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert_eq!(
        git(zmin_repo, ["rev-parse", "refs/remotes/origin/main"]),
        git(git_repo, ["rev-parse", "refs/remotes/origin/main"])
    );
    assert_eq!(
        git(
            zmin_repo,
            ["cat-file", "-p", "refs/remotes/origin/main:file"]
        ),
        git(
            git_repo,
            ["cat-file", "-p", "refs/remotes/origin/main:file"]
        )
    );
    assert_eq!(
        git(zmin_repo, ["config", "--get", "remote.origin.promisor"]),
        git(git_repo, ["config", "--get", "remote.origin.promisor"])
    );
    assert_eq!(
        git(
            zmin_repo,
            ["config", "--get", "remote.origin.partialclonefilter"]
        ),
        git(
            git_repo,
            ["config", "--get", "remote.origin.partialclonefilter"]
        )
    );
}

fn assert_zmin_filter_gap(repo: &std::path::Path, args: &[&str], expected: &str) {
    let (status, _stdout, stderr) = run_zmin_failure_output(repo, args);
    assert_eq!(status, 128, "{stderr}");
    assert!(stderr.contains(expected), "{stderr}");
}
