mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use common::{
    clone_repo_fixture, command_failure_output_with_env, command_output_with_env,
    configure_identity, git, git_init, git_with_env, run_zmin, test_command_program, write_file,
    zmin_bin,
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

fn rebase_interactive_three_commit_repo() -> TempDir {
    let repo = rebase_interactive_fixture_repo();
    write_file(repo.path(), "a.txt", "one\ntwo\nthree\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "three"]);
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

fn command_any_output_with_env(
    command: &str,
    cwd: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
    label: &str,
) -> (i32, String, String) {
    let output = Command::new(test_command_program(command))
        .args(args)
        .envs(envs.iter().copied())
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|err| panic!("run {label}: {err}"));
    (
        output.status.code().expect("process exit code"),
        String::from_utf8(output.stdout)
            .expect("stdout utf8")
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .expect("stderr utf8")
            .trim_end_matches('\n')
            .to_owned(),
    )
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

#[test]
fn rebase_interactive_two_commit_todo_order_matches_stock_git() {
    let source = rebase_interactive_three_commit_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    let git_editor = write_bad_sequence_editor(git_repo.path());
    let zmin_editor = write_bad_sequence_editor(zmin_repo.path());
    let git_editor = git_editor.to_string_lossy().to_string();
    let zmin_editor = zmin_editor.to_string_lossy().to_string();

    let git_output = command_failure_output_with_env(
        "git",
        git_repo.path(),
        &["rebase", "-i", "HEAD~2"],
        &[("GIT_SEQUENCE_EDITOR", git_editor.as_str())],
        "git",
    );
    let zmin_output = command_failure_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &["rebase", "-i", "HEAD~2"],
        &[("GIT_SEQUENCE_EDITOR", zmin_editor.as_str())],
        "zmin",
    );
    assert_eq!(zmin_output, git_output);
    assert_eq!(
        first_rebase_todo_line(zmin_repo.path()),
        first_rebase_todo_line(git_repo.path())
    );
    assert!(
        first_rebase_todo_line(git_repo.path()).contains("# two"),
        "unexpected stock todo line: {}",
        first_rebase_todo_line(git_repo.path())
    );
    assert_eq!(
        command_any_output_with_env(
            "git",
            git_repo.path(),
            &["symbolic-ref", "-q", "HEAD"],
            &[],
            "git symbolic-ref"
        ),
        command_any_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["symbolic-ref", "-q", "HEAD"],
            &[],
            "zmin symbolic-ref"
        )
    );
    assert_eq!(
        git(git_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(zmin_repo.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(git_repo.path(), ["ls-files", "--stage"]),
        git(zmin_repo.path(), ["ls-files", "--stage"])
    );
}

fn write_sequence_editor_replace_pick(
    root: &Path,
    name: &str,
    action: &str,
    occurrence: usize,
) -> PathBuf {
    let editor = root.join(name);
    fs::write(
        &editor,
        format!(
            r#"#!/bin/sh
awk -v action='{}' -v occurrence='{}' '
  /^pick / {{
    seen += 1
    if (seen == occurrence) {{
      sub(/^pick /, action " ")
    }}
  }}
  {{ print }}
' "$1" > "$1.tmp" && mv "$1.tmp" "$1"
"#,
            action, occurrence
        ),
    )
    .expect("write sequence editor");
    chmod_executable(&editor);
    editor
}

#[test]
fn branch_list_during_real_interactive_rebase_matches_stock_git() {
    let source = rebase_interactive_three_commit_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    let git_editor =
        write_sequence_editor_replace_pick(git_repo.path(), "edit-first.sh", "edit", 1);
    let zmin_editor =
        write_sequence_editor_replace_pick(zmin_repo.path(), "edit-first.sh", "edit", 1);
    let git_editor = git_editor.to_string_lossy().to_string();
    let zmin_editor = zmin_editor.to_string_lossy().to_string();
    let git_env = [("GIT_SEQUENCE_EDITOR", git_editor.as_str())];
    let zmin_env = [("GIT_SEQUENCE_EDITOR", zmin_editor.as_str())];

    let git_rebase = command_output_with_env(
        "git",
        git_repo.path(),
        &["rebase", "-i", "HEAD~2"],
        &git_env,
        "git rebase -i",
    );
    let zmin_rebase = command_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &["rebase", "-i", "HEAD~2"],
        &zmin_env,
        "zmin rebase -i",
    );
    assert_eq!(zmin_rebase, git_rebase);

    assert_eq!(
        command_any_output_with_env(
            "git",
            git_repo.path(),
            &["branch", "--list"],
            &[],
            "git branch"
        ),
        command_any_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["branch", "--list"],
            &[],
            "zmin branch"
        )
    );
    assert_eq!(
        command_any_output_with_env(
            "git",
            git_repo.path(),
            &["symbolic-ref", "-q", "HEAD"],
            &[],
            "git symbolic-ref"
        ),
        command_any_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["symbolic-ref", "-q", "HEAD"],
            &[],
            "zmin symbolic-ref"
        )
    );

    run_zmin(zmin_repo.path(), ["rebase", "--abort"]);
    git(git_repo.path(), ["rebase", "--abort"]);
}

#[test]
fn branch_list_during_real_interactive_rebase_from_detached_head_matches_stock_git() {
    let source = rebase_interactive_three_commit_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    git(git_repo.path(), ["checkout", "main^0"]);
    git(zmin_repo.path(), ["checkout", "main^0"]);

    let git_editor =
        write_sequence_editor_replace_pick(git_repo.path(), "edit-first-detached.sh", "edit", 1);
    let zmin_editor =
        write_sequence_editor_replace_pick(zmin_repo.path(), "edit-first-detached.sh", "edit", 1);
    let git_editor = git_editor.to_string_lossy().to_string();
    let zmin_editor = zmin_editor.to_string_lossy().to_string();
    let git_env = [("GIT_SEQUENCE_EDITOR", git_editor.as_str())];
    let zmin_env = [("GIT_SEQUENCE_EDITOR", zmin_editor.as_str())];

    let git_rebase = command_output_with_env(
        "git",
        git_repo.path(),
        &["rebase", "-i", "HEAD~2"],
        &git_env,
        "git detached rebase -i",
    );
    let zmin_rebase = command_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &["rebase", "-i", "HEAD~2"],
        &zmin_env,
        "zmin detached rebase -i",
    );
    assert_eq!(zmin_rebase, git_rebase);

    assert_eq!(
        command_any_output_with_env(
            "git",
            git_repo.path(),
            &["branch", "--list"],
            &[],
            "git branch"
        ),
        command_any_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["branch", "--list"],
            &[],
            "zmin branch"
        )
    );
    assert_eq!(
        command_any_output_with_env(
            "git",
            git_repo.path(),
            &["symbolic-ref", "-q", "HEAD"],
            &[],
            "git symbolic-ref"
        ),
        command_any_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["symbolic-ref", "-q", "HEAD"],
            &[],
            "zmin symbolic-ref"
        )
    );

    run_zmin(zmin_repo.path(), ["rebase", "--abort"]);
    git(git_repo.path(), ["rebase", "--abort"]);
}

#[test]
fn detached_cherry_pick_before_rebase_matches_stock_git() {
    let source = rebase_interactive_three_commit_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    let picked = git(git_repo.path(), ["rev-parse", "HEAD~1"]);
    let env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
    ];

    git(git_repo.path(), ["checkout", "HEAD~2"]);
    git(zmin_repo.path(), ["checkout", "HEAD~2"]);

    let git_output = command_output_with_env(
        "git",
        git_repo.path(),
        &["cherry-pick", picked.as_str()],
        &env,
        "git cherry-pick detached",
    );
    let zmin_output = command_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &["cherry-pick", picked.as_str()],
        &env,
        "zmin cherry-pick detached",
    );
    assert_eq!(zmin_output, git_output);
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_repo.path(), ["rev-parse", "HEAD^{tree}"])
    );
}
