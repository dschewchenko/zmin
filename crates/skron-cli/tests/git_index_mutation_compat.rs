mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    command_output, configure_identity, git, git_failure_output, git_init, git_with_env,
    git_with_stdin, run_skron, run_skron_failure_output, run_skron_with_env, run_skron_with_stdin,
    skron_bin, write_file,
};

fn committed_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    run_skron(repo.path(), ["add", "-A"]);
    run_skron_with_env(repo.path(), ["commit", "-m", "initial"]);
    repo
}

fn rm_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("a.txt"), b"a\n").expect("write a");
    fs::write(repo.path().join("cached.txt"), b"cached\n").expect("write cached");
    fs::write(repo.path().join("dir/tracked.txt"), b"tracked\n").expect("write tracked");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    fs::write(repo.path().join("dir/untracked.txt"), b"untracked\n").expect("write untracked");
    repo
}

fn mv_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("a.txt"), b"a\n").expect("write a");
    fs::write(repo.path().join("dir/tracked.txt"), b"tracked\n").expect("write tracked");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    fs::write(repo.path().join("dir/untracked.txt"), b"untracked\n").expect("write untracked");
    repo
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path).expect("read mode").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("set executable");
}

#[cfg(not(unix))]
fn make_executable(_path: &std::path::Path) {}

#[test]
fn add_all_pathspec_limits_tracked_deletes_like_stock_git() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::create_dir_all(repo.join("dir")).expect("mkdir dir");
        write_file(repo, "dir/inside.txt", "inside\n");
        write_file(repo, "outside.txt", "outside\n");
    }
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "initial"]);

    fs::remove_file(git_repo.path().join("dir/inside.txt")).expect("remove git inside");
    fs::remove_file(skron_repo.path().join("dir/inside.txt")).expect("remove skron inside");
    fs::remove_file(git_repo.path().join("outside.txt")).expect("remove git outside");
    fs::remove_file(skron_repo.path().join("outside.txt")).expect("remove skron outside");

    git(git_repo.path(), ["add", "-A", "dir"]);
    run_skron(skron_repo.path(), ["add", "-A", "dir"]);

    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
    assert_eq!(
        git(skron_repo.path(), ["diff", "--cached", "--name-status"]),
        git(git_repo.path(), ["diff", "--cached", "--name-status"])
    );
}

#[test]
fn add_all_stages_mode_change_with_unchanged_content_like_stock_git() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    write_file(git_repo.path(), "script.sh", "#!/bin/sh\n");
    write_file(skron_repo.path(), "script.sh", "#!/bin/sh\n");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "initial"]);

    make_executable(&git_repo.path().join("script.sh"));
    make_executable(&skron_repo.path().join("script.sh"));
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);

    assert_eq!(
        git(skron_repo.path(), ["diff", "--cached", "--summary"]),
        git(git_repo.path(), ["diff", "--cached", "--summary"])
    );
    assert_eq!(
        git(skron_repo.path(), ["ls-files", "--stage", "script.sh"]),
        git(git_repo.path(), ["ls-files", "--stage", "script.sh"])
    );
}

#[test]
fn add_update_matches_stock_git_state() {
    let git_repo = committed_repo();
    let skron_repo = committed_repo();

    fs::create_dir_all(git_repo.path().join("dir")).expect("mkdir git dir");
    fs::create_dir_all(skron_repo.path().join("dir")).expect("mkdir skron dir");
    fs::write(git_repo.path().join("dir/nested.txt"), b"nested\n").expect("write git nested");
    fs::write(skron_repo.path().join("dir/nested.txt"), b"nested\n").expect("write skron nested");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "nested"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "nested"]);

    fs::write(git_repo.path().join("a.txt"), b"tracked change\n").expect("modify git a");
    fs::write(skron_repo.path().join("a.txt"), b"tracked change\n").expect("modify skron a");
    fs::write(git_repo.path().join("dir/nested.txt"), b"nested change\n")
        .expect("modify git nested");
    fs::write(skron_repo.path().join("dir/nested.txt"), b"nested change\n")
        .expect("modify skron nested");
    fs::write(git_repo.path().join("new.txt"), b"new\n").expect("write git new");
    fs::write(skron_repo.path().join("new.txt"), b"new\n").expect("write skron new");

    git(git_repo.path(), ["add", "-u", "dir"]);
    run_skron(skron_repo.path(), ["add", "-u", "dir"]);
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );

    fs::remove_file(git_repo.path().join("dir/nested.txt")).expect("remove git nested");
    fs::remove_file(skron_repo.path().join("dir/nested.txt")).expect("remove skron nested");
    git(git_repo.path(), ["stage", "-u"]);
    run_skron(skron_repo.path(), ["stage", "-u"]);
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );

    git_with_env(git_repo.path(), ["commit", "-m", "update tracked"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "update tracked"]);
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn update_index_matches_stock_git_for_core_index_mutations() {
    let git_repo = git_init();
    let skron_repo = git_init();
    for repo in [git_repo.path(), skron_repo.path()] {
        configure_identity(repo);
        write_file(repo, "a.txt", "one\n");
        write_file(repo, "c.txt", "cee\n");
        write_file(repo, "d.txt", "dee\n");
    }

    git(git_repo.path(), ["update-index", "--add", "a.txt"]);
    run_skron(skron_repo.path(), ["update-index", "--add", "a.txt"]);
    assert_eq!(
        git(skron_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );

    write_file(git_repo.path(), "a.txt", "two\n");
    write_file(skron_repo.path(), "a.txt", "two\n");
    git(git_repo.path(), ["update-index", "a.txt"]);
    run_skron(skron_repo.path(), ["update-index", "a.txt"]);
    assert_eq!(
        git(skron_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );

    fs::remove_file(git_repo.path().join("a.txt")).expect("remove git a");
    fs::remove_file(skron_repo.path().join("a.txt")).expect("remove skron a");
    git(git_repo.path(), ["update-index", "--remove", "a.txt"]);
    run_skron(skron_repo.path(), ["update-index", "--remove", "a.txt"]);
    assert_eq!(
        git(skron_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );

    let git_blob = git_with_stdin(git_repo.path(), ["hash-object", "-w", "--stdin"], "blob\n");
    let skron_blob = git_with_stdin(
        skron_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "blob\n",
    );
    assert_eq!(skron_blob, git_blob);
    let cacheinfo = format!("100644,{git_blob},b.txt");
    git(
        git_repo.path(),
        ["update-index", "--add", "--cacheinfo", &cacheinfo],
    );
    run_skron(
        skron_repo.path(),
        ["update-index", "--add", "--cacheinfo", &cacheinfo],
    );
    assert_eq!(
        git(skron_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );

    write_file(git_repo.path(), "b.txt", "blob\n");
    write_file(skron_repo.path(), "b.txt", "blob\n");
    git(git_repo.path(), ["update-index", "--chmod=+x", "b.txt"]);
    run_skron(skron_repo.path(), ["update-index", "--chmod=+x", "b.txt"]);
    assert_eq!(
        git(skron_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );

    git_with_stdin(
        git_repo.path(),
        ["update-index", "--add", "-z", "--stdin"],
        "c.txt\0d.txt\0",
    );
    run_skron_with_stdin(
        skron_repo.path(),
        ["update-index", "--add", "-z", "--stdin"],
        "c.txt\0d.txt\0",
    );
    assert_eq!(
        git(skron_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );
}

#[test]
fn rm_file_dir_and_cached_match_stock_git_state() {
    let git_repo = rm_fixture_repo();
    let skron_repo = rm_fixture_repo();

    git(git_repo.path(), ["rm", "a.txt"]);
    run_skron(skron_repo.path(), ["rm", "a.txt"]);
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert!(!skron_repo.path().join("a.txt").exists());

    git(git_repo.path(), ["rm", "-r", "dir"]);
    run_skron(skron_repo.path(), ["rm", "-r", "dir"]);
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert!(!skron_repo.path().join("dir/tracked.txt").exists());
    assert!(skron_repo.path().join("dir/untracked.txt").exists());

    git(git_repo.path(), ["rm", "--cached", "cached.txt"]);
    run_skron(skron_repo.path(), ["rm", "--cached", "cached.txt"]);
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert!(skron_repo.path().join("cached.txt").exists());
}

#[test]
fn rm_common_options_match_stock_git() {
    let git_repo = rm_fixture_repo();
    let skron_repo = rm_fixture_repo();
    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &["rm", "-n", "a.txt"],
            "skron",
        ),
        command_output("git", git_repo.path(), &["rm", "-n", "a.txt"], "git")
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
    assert!(skron_repo.path().join("a.txt").exists());

    let git_repo = rm_fixture_repo();
    let skron_repo = rm_fixture_repo();
    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &["rm", "-q", "a.txt"],
            "skron",
        ),
        command_output("git", git_repo.path(), &["rm", "-q", "a.txt"], "git")
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
    assert!(!skron_repo.path().join("a.txt").exists());

    let git_repo = rm_fixture_repo();
    let skron_repo = rm_fixture_repo();
    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &["rm", "--ignore-unmatch", "missing.txt"],
            "skron",
        ),
        command_output(
            "git",
            git_repo.path(),
            &["rm", "--ignore-unmatch", "missing.txt"],
            "git",
        )
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );

    let git_repo = rm_fixture_repo();
    let skron_repo = rm_fixture_repo();
    fs::write(git_repo.path().join("paths.nul"), b"a.txt\0cached.txt\0").expect("git paths");
    fs::write(skron_repo.path().join("paths.nul"), b"a.txt\0cached.txt\0").expect("skron paths");
    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &[
                "rm",
                "--cached",
                "--pathspec-from-file",
                "paths.nul",
                "--pathspec-file-nul",
            ],
            "skron",
        ),
        command_output(
            "git",
            git_repo.path(),
            &[
                "rm",
                "--cached",
                "--pathspec-from-file",
                "paths.nul",
                "--pathspec-file-nul",
            ],
            "git",
        )
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );

    let repo = rm_fixture_repo();
    assert_eq!(
        run_skron_failure_output(repo.path(), &["rm", "--pathspec-file-nul"]),
        git_failure_output(repo.path(), &["rm", "--pathspec-file-nul"])
    );
}

#[test]
fn mv_file_and_directory_match_stock_git_tree_after_commit() {
    let git_repo = mv_fixture_repo();
    let skron_repo = mv_fixture_repo();

    git(git_repo.path(), ["mv", "a.txt", "renamed.txt"]);
    run_skron(skron_repo.path(), ["mv", "a.txt", "renamed.txt"]);
    git(git_repo.path(), ["mv", "dir", "renamed-dir"]);
    run_skron(skron_repo.path(), ["mv", "dir", "renamed-dir"]);

    assert!(!skron_repo.path().join("a.txt").exists());
    assert!(skron_repo.path().join("renamed.txt").exists());
    assert!(!skron_repo.path().join("dir/tracked.txt").exists());
    assert!(skron_repo.path().join("renamed-dir/tracked.txt").exists());
    assert!(skron_repo.path().join("renamed-dir/untracked.txt").exists());

    assert_eq!(
        git(
            skron_repo.path(),
            ["diff", "--cached", "--name-status", "--no-renames"],
        ),
        git(
            git_repo.path(),
            ["diff", "--cached", "--name-status", "--no-renames"],
        )
    );

    git_with_env(git_repo.path(), ["commit", "-m", "move"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "move"]);
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
}
