mod common;

use std::fs;

use tempfile::TempDir;

use common::{configure_identity, git, git_status, git_with_env, run_zmin_failure_output};

#[test]
fn fetch_filter_local_file_and_shallow_guards_are_git_supported_gaps() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let local_client = dir.path().join("local-client");
    let file_client = dir.path().join("file-client");
    let shallow_client = dir.path().join("shallow-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"one\n").expect("write one");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "one"]);
    fs::write(source.join("file"), b"two\n").expect("write two");
    git(&source, ["commit", "-am", "two"]);

    git(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            local_client.to_str().expect("local client path"),
        ],
    );
    let file_url = format!("file://{}", source.display());
    git(
        dir.path(),
        [
            "clone",
            file_url.as_str(),
            file_client.to_str().expect("file client path"),
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

    assert_eq!(
        git_status(&local_client, ["fetch", "--filter=blob:none", "origin", "main"]),
        0
    );
    assert_zmin_filter_gap(
        &local_client,
        &["fetch", "--filter=blob:none", "origin", "main"],
        "fetch --filter currently supports network remotes",
    );

    assert_eq!(
        git_status(&file_client, ["fetch", "--filter=blob:none", "origin", "main"]),
        0
    );
    assert_zmin_filter_gap(
        &file_client,
        &["fetch", "--filter=blob:none", "origin", "main"],
        "fetch --filter currently supports network remotes",
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

fn assert_zmin_filter_gap(repo: &std::path::Path, args: &[&str], expected: &str) {
    let (status, _stdout, stderr) = run_zmin_failure_output(repo, args);
    assert_eq!(status, 128, "{stderr}");
    assert!(stderr.contains(expected), "{stderr}");
}
