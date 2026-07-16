mod common;

use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use common::{
    command_any_output, configure_identity, git, git_init, git_with_env, run_zmin,
    run_zmin_with_env, zmin_bin,
};

fn loose_object_path(repo: &Path, oid: &str) -> std::path::PathBuf {
    repo.join(".git/objects").join(&oid[..2]).join(&oid[2..])
}

fn corrupt_blob_payload(repo: &Path, oid: &str) {
    let path = loose_object_path(repo, oid);
    let mut permissions = fs::metadata(&path)
        .expect("loose blob metadata")
        .permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        permissions.set_mode(permissions.mode() | 0o200);
    }
    #[cfg(windows)]
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).expect("make loose blob writable");
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("open loose blob");
    file.seek(SeekFrom::Start(10))
        .expect("seek into loose blob");
    file.write_all(&[0]).expect("corrupt loose blob byte");
}

fn commit_content(repo: &Path, message: &str) -> String {
    configure_identity(repo);
    fs::write(repo.join("content.t"), b"content\n").expect("write shared content");
    git(repo, ["add", "content.t"]);
    git_with_env(repo, ["commit", "-m", message]);
    git(repo, ["rev-parse", "HEAD:content.t"])
}

#[test]
fn cat_file_size_rejects_a_corrupt_loose_blob_like_stock_git() {
    let repo = git_init();
    let blob = commit_content(repo.path(), "corrupt blob fixture");
    corrupt_blob_payload(repo.path(), &blob);

    let args = ["cat-file", "-s", blob.as_str()];
    let stock = command_any_output("git", repo.path(), &args, "git cat-file corrupt size");
    let zmin = command_any_output(zmin_bin(), repo.path(), &args, "zmin cat-file corrupt size");

    assert_eq!(zmin.0, stock.0, "exit code");
    assert_eq!(zmin.1, stock.1, "stdout");
    assert_ne!(stock.0, 0, "stock Git must reject corrupt loose data");
    assert!(
        zmin.2.contains("corrupt") || zmin.2.contains("inflate"),
        "zmin stderr should identify corruption: {}",
        zmin.2
    );
}

#[test]
fn implicit_empty_tree_is_available_to_commit_tree_and_rev_list_like_stock_git() {
    let stock_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(stock_repo.path());
    configure_identity(zmin_repo.path());

    let stock_tree = git(
        stock_repo.path(),
        ["hash-object", "-t", "tree", "/dev/null"],
    );
    let zmin_tree = run_zmin(zmin_repo.path(), ["hash-object", "-t", "tree", "/dev/null"]);
    assert_eq!(zmin_tree, stock_tree, "implicit empty-tree id");
    assert!(!loose_object_path(stock_repo.path(), &stock_tree).exists());
    assert!(!loose_object_path(zmin_repo.path(), &zmin_tree).exists());

    let stock_commit = git_with_env(
        stock_repo.path(),
        ["commit-tree", &stock_tree, "-m", "implicit tree"],
    );
    let zmin_commit = run_zmin_with_env(
        zmin_repo.path(),
        ["commit-tree", &zmin_tree, "-m", "implicit tree"],
    );
    assert_eq!(zmin_commit, stock_commit, "commit id");

    let stock = command_any_output(
        "git",
        stock_repo.path(),
        &["rev-list", "--objects", &stock_commit],
        "git rev-list implicit tree",
    );
    let zmin = command_any_output(
        zmin_bin(),
        zmin_repo.path(),
        &["rev-list", "--objects", &zmin_commit],
        "zmin rev-list implicit tree",
    );
    assert_eq!(zmin, stock);
}

#[test]
fn local_fetch_pack_rejects_a_corrupt_existing_loose_object_like_stock_git() {
    let source = git_init();
    let source_blob = commit_content(source.path(), "clean source");

    let stock_repo = git_init();
    let stock_blob = commit_content(stock_repo.path(), "corrupt destination");
    let zmin_repo = git_init();
    let zmin_blob = commit_content(zmin_repo.path(), "corrupt destination");
    assert_eq!(stock_blob, source_blob, "shared stock blob id");
    assert_eq!(zmin_blob, source_blob, "shared zmin blob id");
    corrupt_blob_payload(stock_repo.path(), &stock_blob);
    corrupt_blob_payload(zmin_repo.path(), &zmin_blob);

    let source_path = source.path().to_str().expect("source path utf8");
    let args = ["-c", "transfer.unpackLimit=1", "fetch", source_path];
    let stock = command_any_output("git", stock_repo.path(), &args, "git corrupt fetch");
    let zmin = command_any_output(zmin_bin(), zmin_repo.path(), &args, "zmin corrupt fetch");

    assert_eq!(zmin.0, stock.0, "exit code");
    assert_eq!(zmin.1, stock.1, "stdout");
    assert_ne!(stock.0, 0, "stock Git must reject the corrupt collision");
    assert!(!zmin.2.to_ascii_lowercase().contains("collision"));
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(stock_repo.path(), ["rev-parse", "HEAD"]),
        "failed fetch must preserve destination HEAD",
    );
}
