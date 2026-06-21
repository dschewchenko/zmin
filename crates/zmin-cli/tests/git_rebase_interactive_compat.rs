mod common;

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use common::{
    clone_repo_fixture, command_failure_output_with_env, configure_identity, git, git_init,
    git_with_env, run_zmin, write_file, zmin_bin,
};

fn rebase_interactive_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "a.txt", "one\ntwo\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    repo
}

fn write_bad_sequence_editor(repo: &Path) -> PathBuf {
    let editor = repo.join(".git/bad-sequence-editor.sh");
    fs::write(
        &editor,
        "#!/bin/sh\nperl -0pi -e 's/^pick /bogus /m' \"$1\"\n",
    )
    .expect("write bad sequence editor");
    chmod_executable(&editor);
    editor
}

fn first_rebase_todo_line(repo: &Path) -> String {
    fs::read_to_string(repo.join(".git/rebase-merge/git-rebase-todo"))
        .expect("read rebase todo")
        .lines()
        .next()
        .expect("first rebase todo line")
        .to_owned()
}

#[cfg(unix)]
fn chmod_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = fs::metadata(path).expect("script metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).expect("chmod script");
}

#[cfg(not(unix))]
fn chmod_executable(_path: &Path) {}

#[test]
fn rebase_interactive_invalid_todo_command_matches_stock_git() {
    let source = rebase_interactive_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    let git_editor = write_bad_sequence_editor(git_repo.path());
    let zmin_editor = write_bad_sequence_editor(zmin_repo.path());
    let git_editor = git_editor.to_string_lossy().to_string();
    let zmin_editor = zmin_editor.to_string_lossy().to_string();
    let git_head = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let zmin_head = git(zmin_repo.path(), ["rev-parse", "HEAD"]);
    assert_eq!(zmin_head, git_head);

    let git_output = command_failure_output_with_env(
        "git",
        git_repo.path(),
        &["rebase", "-i", "HEAD~1"],
        &[("GIT_SEQUENCE_EDITOR", git_editor.as_str())],
        "git",
    );
    let zmin_output = command_failure_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &["rebase", "-i", "HEAD~1"],
        &[("GIT_SEQUENCE_EDITOR", zmin_editor.as_str())],
        "zmin",
    );
    assert_eq!(zmin_output, git_output);
    assert!(git_repo.path().join(".git/rebase-merge").is_dir());
    assert!(zmin_repo.path().join(".git/rebase-merge").is_dir());
    assert_eq!(
        first_rebase_todo_line(zmin_repo.path()),
        first_rebase_todo_line(git_repo.path())
    );
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );

    git(git_repo.path(), ["rebase", "--abort"]);
    run_zmin(zmin_repo.path(), ["rebase", "--abort"]);
    assert!(!git_repo.path().join(".git/rebase-merge").exists());
    assert!(!zmin_repo.path().join(".git/rebase-merge").exists());
    assert_eq!(git(git_repo.path(), ["rev-parse", "HEAD"]), git_head);
    assert_eq!(git(zmin_repo.path(), ["rev-parse", "HEAD"]), zmin_head);
    assert_eq!(git(git_repo.path(), ["status", "--short"]), "");
    assert_eq!(git(zmin_repo.path(), ["status", "--short"]), "");
}
