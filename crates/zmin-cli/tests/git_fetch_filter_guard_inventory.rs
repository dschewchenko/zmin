mod common;

use std::{fs, path::Path};

use tempfile::TempDir;

use common::{command_any_output, configure_identity, git, git_with_env, zmin_bin};

#[test]
fn fetch_filter_local_file_and_shallow_depth_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_local_client = dir.path().join("git-local-client");
    let zmin_local_client = dir.path().join("zmin-local-client");
    let git_file_client = dir.path().join("git-file-client");
    let zmin_file_client = dir.path().join("zmin-file-client");
    let git_shallow_client = dir.path().join("git-shallow-client");
    let zmin_shallow_client = dir.path().join("zmin-shallow-client");

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
            git_shallow_client
                .to_str()
                .expect("git shallow client path"),
        ],
    );
    git(
        dir.path(),
        [
            "clone",
            "--depth=1",
            file_url.as_str(),
            zmin_shallow_client
                .to_str()
                .expect("zmin shallow client path"),
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

    assert_filter_fetch_matches_stock(
        &git_shallow_client,
        &zmin_shallow_client,
        &["fetch", "--filter=blob:none", "--depth=1", "origin", "main"],
    );
}

fn assert_filter_fetch_matches_stock(git_repo: &Path, zmin_repo: &Path, args: &[&str]) {
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
        optional_file_content(&zmin_repo.join(".git/shallow")),
        optional_file_content(&git_repo.join(".git/shallow"))
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

fn optional_file_content(path: &Path) -> Option<String> {
    path.exists()
        .then(|| fs::read_to_string(path).expect("optional file content"))
}
