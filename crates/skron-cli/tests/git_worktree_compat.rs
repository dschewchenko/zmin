mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    command_failure_output, command_output, configure_identity, git, git_failure_output, git_init,
    git_with_env, run_skron, run_skron_failure_output, skron_bin, write_file,
};

fn worktree_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    repo
}

fn single_worktree_admin(repo: &std::path::Path) -> std::path::PathBuf {
    let entries = fs::read_dir(repo.join(".git/worktrees"))
        .expect("read worktree admin dir")
        .map(|entry| entry.expect("read worktree admin entry").path())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    entries.into_iter().next().expect("worktree admin path")
}

fn gitdir_file_target(worktree: &std::path::Path) -> String {
    fs::read_to_string(worktree.join(".git"))
        .expect("read worktree .git file")
        .trim()
        .strip_prefix("gitdir: ")
        .expect("gitdir prefix")
        .to_owned()
}

#[test]
fn worktree_add_list_remove_creates_stock_readable_linked_worktree() {
    let repo = worktree_fixture_repo();
    let worktree = repo.path().with_file_name(format!(
        "{}-linked",
        repo.path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let target = git(repo.path(), ["rev-parse", "HEAD~1"]);

    run_skron(
        repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            worktree.to_str().expect("worktree path"),
            "HEAD~1",
        ],
    );
    assert_eq!(git(&worktree, ["rev-parse", "HEAD"]), target);
    assert_eq!(
        fs::read_to_string(worktree.join("a.txt")).expect("read worktree file"),
        "one\n"
    );
    let list = run_skron(repo.path(), ["worktree", "list", "--porcelain"]);
    assert!(list.contains(&format!("worktree {}", worktree.display())));
    assert!(list.contains(&format!("HEAD {target}")));
    assert!(list.contains("detached"));

    run_skron(
        repo.path(),
        [
            "worktree",
            "remove",
            worktree.to_str().expect("worktree path"),
        ],
    );
    assert!(!worktree.exists());
}

#[test]
fn worktree_move_relocates_linked_worktree_like_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-move-source",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_worktree = skron_repo.path().with_file_name(format!(
        "{}-skron-move-source",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    git(
        git_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            git_worktree.to_str().expect("git worktree path"),
        ],
    );
    run_skron(
        skron_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            skron_worktree.to_str().expect("skron worktree path"),
        ],
    );

    let git_parent = git_repo.path().with_file_name(format!(
        "{}-git-move-target-parent",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_parent = skron_repo.path().with_file_name(format!(
        "{}-skron-move-target-parent",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    fs::create_dir_all(&git_parent).expect("create git move target parent");
    fs::create_dir_all(&skron_parent).expect("create skron move target parent");
    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &[
                "worktree",
                "move",
                skron_worktree.to_str().expect("skron source worktree"),
                skron_parent.to_str().expect("skron target parent"),
            ],
            "skron",
        ),
        command_output(
            "git",
            git_repo.path(),
            &[
                "worktree",
                "move",
                git_worktree.to_str().expect("git source worktree"),
                git_parent.to_str().expect("git target parent"),
            ],
            "git",
        )
    );

    let moved_git = git_parent.join(git_worktree.file_name().expect("git worktree name"));
    let moved_skron = skron_parent.join(skron_worktree.file_name().expect("skron worktree name"));
    assert!(!git_worktree.exists());
    assert!(!skron_worktree.exists());
    assert_eq!(
        git(&moved_skron, ["rev-parse", "HEAD"]),
        git(&moved_git, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&moved_skron, ["rev-parse", "--abbrev-ref", "HEAD"]),
        git(&moved_git, ["rev-parse", "--abbrev-ref", "HEAD"])
    );
    assert_eq!(
        fs::read_to_string(moved_skron.join("a.txt")).expect("read moved skron worktree"),
        fs::read_to_string(moved_git.join("a.txt")).expect("read moved git worktree")
    );
    assert!(
        run_skron(skron_repo.path(), ["worktree", "list", "--porcelain"])
            .contains(&format!("worktree {}", moved_skron.display()))
    );
    assert_eq!(
        fs::read_to_string(
            fs::read_to_string(moved_skron.join(".git"))
                .expect("read moved skron gitfile")
                .trim()
                .strip_prefix("gitdir: ")
                .expect("gitdir prefix")
                .trim()
                .to_owned()
                + "/gitdir"
        )
        .expect("read moved skron admin gitdir")
        .trim(),
        moved_skron.join(".git").to_string_lossy()
    );
}

#[test]
fn worktree_lock_unlock_and_locked_remove_match_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-lock-source",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_worktree = skron_repo.path().with_file_name(format!(
        "{}-skron-lock-source",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    git(
        git_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            git_worktree.to_str().expect("git worktree path"),
        ],
    );
    run_skron(
        skron_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            skron_worktree.to_str().expect("skron worktree path"),
        ],
    );

    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &[
                "worktree",
                "lock",
                "--reason",
                "why now",
                skron_worktree.to_str().expect("skron worktree path"),
            ],
            "skron",
        ),
        command_output(
            "git",
            git_repo.path(),
            &[
                "worktree",
                "lock",
                "--reason",
                "why now",
                git_worktree.to_str().expect("git worktree path"),
            ],
            "git",
        )
    );
    let list = run_skron(skron_repo.path(), ["worktree", "list", "--porcelain"]);
    assert!(list.contains("locked why now"));
    assert_eq!(
        run_skron_failure_output(
            skron_repo.path(),
            &[
                "worktree",
                "remove",
                skron_worktree.to_str().expect("skron worktree path"),
            ],
        ),
        git_failure_output(
            git_repo.path(),
            &[
                "worktree",
                "remove",
                git_worktree.to_str().expect("git worktree path"),
            ],
        )
    );

    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &[
                "worktree",
                "unlock",
                skron_worktree.to_str().expect("skron worktree path"),
            ],
            "skron",
        ),
        command_output(
            "git",
            git_repo.path(),
            &[
                "worktree",
                "unlock",
                git_worktree.to_str().expect("git worktree path"),
            ],
            "git",
        )
    );
    let list = run_skron(skron_repo.path(), ["worktree", "list", "--porcelain"]);
    assert!(!list.contains("locked why now"));
    run_skron(
        skron_repo.path(),
        [
            "worktree",
            "remove",
            skron_worktree.to_str().expect("skron worktree path"),
        ],
    );
    git(
        git_repo.path(),
        [
            "worktree",
            "remove",
            git_worktree.to_str().expect("git worktree path"),
        ],
    );
    assert!(!skron_worktree.exists());
    assert!(!git_worktree.exists());
}

#[test]
fn worktree_locked_double_force_matches_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-locked-force-source",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_worktree = skron_repo.path().with_file_name(format!(
        "{}-skron-locked-force-source",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    git(
        git_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            git_worktree.to_str().expect("git worktree path"),
        ],
    );
    run_skron(
        skron_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            skron_worktree.to_str().expect("skron worktree path"),
        ],
    );
    git(
        git_repo.path(),
        [
            "worktree",
            "lock",
            "--reason",
            "why now",
            git_worktree.to_str().expect("git worktree path"),
        ],
    );
    run_skron(
        skron_repo.path(),
        [
            "worktree",
            "lock",
            "--reason",
            "why now",
            skron_worktree.to_str().expect("skron worktree path"),
        ],
    );

    let git_moved = git_repo.path().with_file_name(format!(
        "{}-git-locked-force-moved",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_moved = skron_repo.path().with_file_name(format!(
        "{}-skron-locked-force-moved",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));

    for flag in [None, Some("-f")] {
        let mut skron_args = vec!["worktree", "move"];
        let mut git_args = vec!["worktree", "move"];
        if let Some(flag) = flag {
            skron_args.push(flag);
            git_args.push(flag);
        }
        skron_args.extend([
            skron_worktree.to_str().expect("skron worktree path"),
            skron_moved.to_str().expect("skron moved path"),
        ]);
        git_args.extend([
            git_worktree.to_str().expect("git worktree path"),
            git_moved.to_str().expect("git moved path"),
        ]);
        assert_eq!(
            run_skron_failure_output(skron_repo.path(), &skron_args),
            git_failure_output(git_repo.path(), &git_args),
            "{skron_args:?}"
        );
    }

    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &[
                "worktree",
                "move",
                "-f",
                "-f",
                skron_worktree.to_str().expect("skron worktree path"),
                skron_moved.to_str().expect("skron moved path"),
            ],
            "skron",
        ),
        command_output(
            "git",
            git_repo.path(),
            &[
                "worktree",
                "move",
                "-f",
                "-f",
                git_worktree.to_str().expect("git worktree path"),
                git_moved.to_str().expect("git moved path"),
            ],
            "git",
        )
    );
    let git_admin = single_worktree_admin(git_repo.path());
    let skron_admin = single_worktree_admin(skron_repo.path());
    assert!(!git_worktree.exists());
    assert!(!skron_worktree.exists());
    assert!(git_moved.exists());
    assert!(skron_moved.exists());

    assert_eq!(
        run_skron_failure_output(
            skron_repo.path(),
            &[
                "worktree",
                "remove",
                "-f",
                skron_moved.to_str().expect("skron moved path"),
            ],
        ),
        git_failure_output(
            git_repo.path(),
            &[
                "worktree",
                "remove",
                "-f",
                git_moved.to_str().expect("git moved path"),
            ],
        )
    );
    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &[
                "worktree",
                "remove",
                "-f",
                "-f",
                skron_moved.to_str().expect("skron moved path"),
            ],
            "skron",
        ),
        command_output(
            "git",
            git_repo.path(),
            &[
                "worktree",
                "remove",
                "-f",
                "-f",
                git_moved.to_str().expect("git moved path"),
            ],
            "git",
        )
    );
    assert!(!git_moved.exists());
    assert!(!skron_moved.exists());
    assert!(!git_admin.exists());
    assert!(!skron_admin.exists());
}

#[test]
fn worktree_move_existing_target_fails_even_with_force_like_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-force-move-source",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_worktree = skron_repo.path().with_file_name(format!(
        "{}-skron-force-move-source",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    git(
        git_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            git_worktree.to_str().expect("git worktree path"),
        ],
    );
    run_skron(
        skron_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            skron_worktree.to_str().expect("skron worktree path"),
        ],
    );

    let git_target = git_repo.path().with_file_name(format!(
        "{}-git-force-move-target",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_target = skron_repo.path().with_file_name(format!(
        "{}-skron-force-move-target",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    fs::write(&git_target, b"occupied").expect("write git move target");
    fs::write(&skron_target, b"occupied").expect("write skron move target");

    let skron_failure = run_skron_failure_output(
        skron_repo.path(),
        &[
            "worktree",
            "move",
            "-f",
            skron_worktree.to_str().expect("skron worktree path"),
            skron_target.to_str().expect("skron target path"),
        ],
    );
    let git_failure = git_failure_output(
        git_repo.path(),
        &[
            "worktree",
            "move",
            "-f",
            git_worktree.to_str().expect("git worktree path"),
            git_target.to_str().expect("git target path"),
        ],
    );
    assert_eq!(skron_failure.0, git_failure.0);
    assert_eq!(skron_failure.1, git_failure.1);
    assert!(skron_failure.2.ends_with("already exists"));
    assert!(git_failure.2.ends_with("already exists"));
    assert!(skron_worktree.exists());
    assert!(git_worktree.exists());
    assert_eq!(
        fs::read_to_string(&skron_target).expect("read skron target"),
        fs::read_to_string(&git_target).expect("read git target")
    );
}

#[test]
fn worktree_prune_removes_missing_unlocked_metadata_like_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    let git_parent = git_repo.path().with_file_name(format!(
        "{}-git-prune-parent",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_parent = skron_repo.path().with_file_name(format!(
        "{}-skron-prune-parent",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    fs::create_dir_all(&git_parent).expect("create git prune parent");
    fs::create_dir_all(&skron_parent).expect("create skron prune parent");
    let git_worktree = git_parent.join("linked-prune");
    let skron_worktree = skron_parent.join("linked-prune");
    git(
        git_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            git_worktree.to_str().expect("git worktree path"),
        ],
    );
    run_skron(
        skron_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            skron_worktree.to_str().expect("skron worktree path"),
        ],
    );
    let git_admin = single_worktree_admin(git_repo.path());
    let skron_admin = single_worktree_admin(skron_repo.path());
    fs::remove_dir_all(&git_worktree).expect("remove git worktree");
    fs::remove_dir_all(&skron_worktree).expect("remove skron worktree");

    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &[
                "worktree",
                "prune",
                "--dry-run",
                "--verbose",
                "--expire",
                "now"
            ],
            "skron",
        ),
        command_output(
            "git",
            git_repo.path(),
            &[
                "worktree",
                "prune",
                "--dry-run",
                "--verbose",
                "--expire",
                "now"
            ],
            "git",
        )
    );
    assert!(git_admin.exists());
    assert!(skron_admin.exists());
    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &["worktree", "prune", "--expire", "now"],
            "skron",
        ),
        command_output(
            "git",
            git_repo.path(),
            &["worktree", "prune", "--expire", "now"],
            "git",
        )
    );
    assert!(!git_admin.exists());
    assert!(!skron_admin.exists());
}

#[test]
fn worktree_repair_updates_moved_worktree_gitdir_like_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-repair-source",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_worktree = skron_repo.path().with_file_name(format!(
        "{}-repair-source",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    git(
        git_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            git_worktree.to_str().expect("git worktree path"),
        ],
    );
    run_skron(
        skron_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            skron_worktree.to_str().expect("skron worktree path"),
        ],
    );
    let git_admin = single_worktree_admin(git_repo.path());
    let skron_admin = single_worktree_admin(skron_repo.path());
    let git_moved = git_repo.path().with_file_name(format!(
        "{}-repair-moved",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_moved = skron_repo.path().with_file_name(format!(
        "{}-repair-moved",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    fs::rename(&git_worktree, &git_moved).expect("move git worktree outside git");
    fs::rename(&skron_worktree, &skron_moved).expect("move skron worktree outside skron");

    let skron_repair = command_output(
        skron_bin(),
        skron_repo.path(),
        &[
            "worktree",
            "repair",
            skron_moved.to_str().expect("skron moved worktree"),
        ],
        "skron",
    );
    let git_repair = command_output(
        "git",
        git_repo.path(),
        &[
            "worktree",
            "repair",
            git_moved.to_str().expect("git moved worktree"),
        ],
        "git",
    );
    assert_eq!(skron_repair.0, git_repair.0);
    assert_eq!(skron_repair.1, git_repair.1);
    assert!(skron_repair.2.contains("repair: gitdir incorrect:"));
    assert!(git_repair.2.contains("repair: gitdir incorrect:"));
    assert_eq!(
        std::path::PathBuf::from(
            fs::read_to_string(skron_admin.join("gitdir"))
                .expect("read repaired skron admin gitdir")
                .trim()
        )
        .canonicalize()
        .expect("canonicalize repaired skron gitdir"),
        skron_moved
            .join(".git")
            .canonicalize()
            .expect("canonicalize skron moved gitfile")
    );
    assert_eq!(
        std::path::PathBuf::from(
            fs::read_to_string(git_admin.join("gitdir"))
                .expect("read repaired git admin gitdir")
                .trim()
        )
        .canonicalize()
        .expect("canonicalize repaired git gitdir"),
        git_moved
            .join(".git")
            .canonicalize()
            .expect("canonicalize git moved gitfile")
    );
    assert_eq!(
        git(&skron_moved, ["rev-parse", "HEAD"]),
        git(&git_moved, ["rev-parse", "HEAD"])
    );
    assert!(
        run_skron(skron_repo.path(), ["worktree", "list", "--porcelain"])
            .contains(&format!("worktree {}", skron_moved.display()))
    );
}

#[test]
fn worktree_repair_relative_path_modes_match_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-relative-repair-source",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_worktree = skron_repo.path().with_file_name(format!(
        "{}-skron-relative-repair-source",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    git(
        git_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            git_worktree.to_str().expect("git worktree path"),
        ],
    );
    run_skron(
        skron_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            skron_worktree.to_str().expect("skron worktree path"),
        ],
    );
    let git_admin = single_worktree_admin(git_repo.path());
    let skron_admin = single_worktree_admin(skron_repo.path());
    let git_moved = git_repo.path().with_file_name(format!(
        "{}-git-relative-repair-moved",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_moved = skron_repo.path().with_file_name(format!(
        "{}-skron-relative-repair-moved",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    fs::rename(&git_worktree, &git_moved).expect("move git worktree");
    fs::rename(&skron_worktree, &skron_moved).expect("move skron worktree");

    let skron_relative = command_output(
        skron_bin(),
        skron_repo.path(),
        &[
            "worktree",
            "repair",
            "--relative-paths",
            skron_moved.to_str().expect("skron moved worktree"),
        ],
        "skron",
    );
    let git_relative = command_output(
        "git",
        git_repo.path(),
        &[
            "worktree",
            "repair",
            "--relative-paths",
            git_moved.to_str().expect("git moved worktree"),
        ],
        "git",
    );
    assert_eq!(skron_relative.0, git_relative.0);
    assert_eq!(skron_relative.1, git_relative.1);
    assert!(
        skron_relative
            .2
            .contains("repair: gitdir absolute/relative path mismatch:")
    );
    assert!(
        skron_relative
            .2
            .contains("repair: .git file absolute/relative path mismatch:")
    );
    assert!(
        git_relative
            .2
            .contains("repair: gitdir absolute/relative path mismatch:")
    );
    assert!(
        git_relative
            .2
            .contains("repair: .git file absolute/relative path mismatch:")
    );

    let skron_dotgit = gitdir_file_target(&skron_moved);
    let git_dotgit = gitdir_file_target(&git_moved);
    assert!(!std::path::Path::new(&skron_dotgit).is_absolute());
    assert!(!std::path::Path::new(&git_dotgit).is_absolute());
    assert!(
        !std::path::Path::new(
            fs::read_to_string(skron_admin.join("gitdir"))
                .expect("read skron relative admin gitdir")
                .trim()
        )
        .is_absolute()
    );
    assert!(
        !std::path::Path::new(
            fs::read_to_string(git_admin.join("gitdir"))
                .expect("read git relative admin gitdir")
                .trim()
        )
        .is_absolute()
    );
    assert_eq!(
        git(&skron_moved, ["rev-parse", "HEAD"]),
        git(&git_moved, ["rev-parse", "HEAD"])
    );

    let skron_absolute = command_output(
        skron_bin(),
        skron_repo.path(),
        &[
            "worktree",
            "repair",
            "--no-relative-paths",
            skron_moved.to_str().expect("skron moved worktree"),
        ],
        "skron",
    );
    let git_absolute = command_output(
        "git",
        git_repo.path(),
        &[
            "worktree",
            "repair",
            "--no-relative-paths",
            git_moved.to_str().expect("git moved worktree"),
        ],
        "git",
    );
    assert_eq!(skron_absolute.0, git_absolute.0);
    assert_eq!(skron_absolute.1, git_absolute.1);
    assert!(std::path::Path::new(&gitdir_file_target(&skron_moved)).is_absolute());
    assert!(std::path::Path::new(&gitdir_file_target(&git_moved)).is_absolute());
    assert!(
        std::path::Path::new(
            fs::read_to_string(skron_admin.join("gitdir"))
                .expect("read skron absolute admin gitdir")
                .trim()
        )
        .is_absolute()
    );
    assert!(
        std::path::Path::new(
            fs::read_to_string(git_admin.join("gitdir"))
                .expect("read git absolute admin gitdir")
                .trim()
        )
        .is_absolute()
    );
}

#[test]
fn worktree_config_scope_matches_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    git(
        git_repo.path(),
        ["config", "extensions.worktreeConfig", "true"],
    );
    run_skron(
        skron_repo.path(),
        ["config", "extensions.worktreeConfig", "true"],
    );
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-config-worktree",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_worktree = skron_repo.path().with_file_name(format!(
        "{}-skron-config-worktree",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    git(
        git_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            git_worktree.to_str().expect("git worktree path"),
        ],
    );
    run_skron(
        skron_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            skron_worktree.to_str().expect("skron worktree path"),
        ],
    );

    assert_eq!(
        command_output(
            skron_bin(),
            &skron_worktree,
            &["config", "--worktree", "demo.value", "linked"],
            "skron",
        ),
        command_output(
            "git",
            &git_worktree,
            &["config", "--worktree", "demo.value", "linked"],
            "git",
        )
    );
    assert_eq!(
        run_skron(&skron_worktree, ["config", "--get", "demo.value"]),
        git(&git_worktree, ["config", "--get", "demo.value"])
    );
    assert_eq!(
        run_skron(&skron_worktree, ["config", "--worktree", "--list"]),
        git(&git_worktree, ["config", "--worktree", "--list"])
    );
    assert_eq!(
        run_skron_failure_output(skron_repo.path(), &["config", "--get", "demo.value"]),
        git_failure_output(git_repo.path(), &["config", "--get", "demo.value"])
    );
    assert_eq!(
        fs::read_to_string(single_worktree_admin(skron_repo.path()).join("config.worktree"))
            .expect("read skron worktree config"),
        fs::read_to_string(single_worktree_admin(git_repo.path()).join("config.worktree"))
            .expect("read git worktree config")
    );

    assert_eq!(
        command_output(
            skron_bin(),
            &skron_worktree,
            &["config", "--worktree", "--unset", "demo.value"],
            "skron",
        ),
        command_output(
            "git",
            &git_worktree,
            &["config", "--worktree", "--unset", "demo.value"],
            "git",
        )
    );
    assert_eq!(
        run_skron_failure_output(
            &skron_worktree,
            &["config", "--worktree", "--get", "demo.value"]
        ),
        git_failure_output(
            &git_worktree,
            &["config", "--worktree", "--get", "demo.value"]
        )
    );
}

#[test]
fn worktree_config_requires_extension_with_multiple_worktrees_like_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-config-denied",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_worktree = skron_repo.path().with_file_name(format!(
        "{}-skron-config-denied",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    git(
        git_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            git_worktree.to_str().expect("git worktree path"),
        ],
    );
    run_skron(
        skron_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            skron_worktree.to_str().expect("skron worktree path"),
        ],
    );

    assert_eq!(
        run_skron_failure_output(
            &skron_worktree,
            &["config", "--worktree", "demo.value", "blocked"],
        ),
        git_failure_output(
            &git_worktree,
            &["config", "--worktree", "demo.value", "blocked"],
        )
    );
}

#[test]
fn worktree_add_existing_branch_matches_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    git(git_repo.path(), ["branch", "feature"]);
    git(skron_repo.path(), ["branch", "feature"]);
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-feature",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_worktree = skron_repo.path().with_file_name(format!(
        "{}-skron-feature",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));

    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &[
                "worktree",
                "add",
                skron_worktree.to_str().expect("skron worktree path"),
                "feature",
            ],
            "skron",
        )
        .0,
        command_output(
            "git",
            git_repo.path(),
            &[
                "worktree",
                "add",
                git_worktree.to_str().expect("git worktree path"),
                "feature",
            ],
            "git",
        )
        .0
    );
    assert_eq!(
        git(&skron_worktree, ["symbolic-ref", "HEAD"]),
        git(&git_worktree, ["symbolic-ref", "HEAD"])
    );
    assert!(
        run_skron(skron_repo.path(), ["worktree", "list", "--porcelain"])
            .contains("branch refs/heads/feature")
    );
    assert_eq!(
        git(&skron_worktree, ["rev-parse", "HEAD"]),
        git(&git_worktree, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        fs::read_to_string(skron_worktree.join("a.txt")).expect("read skron worktree"),
        fs::read_to_string(git_worktree.join("a.txt")).expect("read git worktree")
    );
}

#[test]
fn worktree_add_force_allows_checked_out_branch_like_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    git(git_repo.path(), ["branch", "feature"]);
    git(skron_repo.path(), ["branch", "feature"]);

    let git_first = git_repo.path().with_file_name(format!(
        "{}-git-feature-first",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_first = skron_repo.path().with_file_name(format!(
        "{}-skron-feature-first",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    git(
        git_repo.path(),
        [
            "worktree",
            "add",
            git_first.to_str().expect("git first worktree"),
            "feature",
        ],
    );
    run_skron(
        skron_repo.path(),
        [
            "worktree",
            "add",
            skron_first.to_str().expect("skron first worktree"),
            "feature",
        ],
    );

    let git_forced = git_repo.path().with_file_name(format!(
        "{}-git-feature-forced",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_forced = skron_repo.path().with_file_name(format!(
        "{}-skron-feature-forced",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));

    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &[
                "worktree",
                "add",
                "-f",
                skron_forced.to_str().expect("skron forced worktree"),
                "feature",
            ],
            "skron",
        ),
        command_output(
            "git",
            git_repo.path(),
            &[
                "worktree",
                "add",
                "-f",
                git_forced.to_str().expect("git forced worktree"),
                "feature",
            ],
            "git",
        )
    );
    assert_eq!(
        git(&skron_forced, ["symbolic-ref", "HEAD"]),
        git(&git_forced, ["symbolic-ref", "HEAD"])
    );
    assert_eq!(
        git(&skron_forced, ["rev-parse", "HEAD"]),
        git(&git_forced, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        fs::read_to_string(skron_forced.join("a.txt")).expect("read skron forced worktree"),
        fs::read_to_string(git_forced.join("a.txt")).expect("read git forced worktree")
    );
}

#[test]
fn worktree_add_commitish_detaches_like_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-detached",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_worktree = skron_repo.path().with_file_name(format!(
        "{}-skron-detached",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));

    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &[
                "worktree",
                "add",
                skron_worktree.to_str().expect("skron worktree path"),
                "HEAD~1",
            ],
            "skron",
        )
        .0,
        command_output(
            "git",
            git_repo.path(),
            &[
                "worktree",
                "add",
                git_worktree.to_str().expect("git worktree path"),
                "HEAD~1",
            ],
            "git",
        )
        .0
    );
    let (symbolic_status, symbolic_stdout, _) = command_failure_output(
        "git",
        &skron_worktree,
        &["symbolic-ref", "-q", "HEAD"],
        "git",
    );
    assert_eq!(symbolic_status, 1);
    assert_eq!(symbolic_stdout, "");
    assert_eq!(
        git(&skron_worktree, ["rev-parse", "HEAD"]),
        git(&git_worktree, ["rev-parse", "HEAD"])
    );
    assert!(run_skron(skron_repo.path(), ["worktree", "list", "--porcelain"]).contains("detached"));
    assert_eq!(
        fs::read_to_string(skron_worktree.join("a.txt")).expect("read skron worktree"),
        fs::read_to_string(git_worktree.join("a.txt")).expect("read git worktree")
    );
}

#[test]
fn worktree_add_branch_options_match_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    let git_branch_worktree = git_repo.path().with_file_name(format!(
        "{}-git-new-branch",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_branch_worktree = skron_repo.path().with_file_name(format!(
        "{}-skron-new-branch",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));

    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "topic",
                skron_branch_worktree.to_str().expect("skron worktree path"),
                "HEAD~1",
            ],
            "skron",
        )
        .0,
        command_output(
            "git",
            git_repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "topic",
                git_branch_worktree.to_str().expect("git worktree path"),
                "HEAD~1",
            ],
            "git",
        )
        .0
    );
    assert_eq!(
        git(&skron_branch_worktree, ["symbolic-ref", "HEAD"]),
        git(&git_branch_worktree, ["symbolic-ref", "HEAD"])
    );
    assert_eq!(
        git(&skron_branch_worktree, ["rev-parse", "HEAD"]),
        git(&git_branch_worktree, ["rev-parse", "HEAD"])
    );

    git(git_repo.path(), ["branch", "reset-me", "HEAD~1"]);
    git(skron_repo.path(), ["branch", "reset-me", "HEAD~1"]);
    let git_reset_worktree = git_repo.path().with_file_name(format!(
        "{}-git-reset-branch",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_reset_worktree = skron_repo.path().with_file_name(format!(
        "{}-skron-reset-branch",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));

    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &[
                "worktree",
                "add",
                "-B",
                "reset-me",
                skron_reset_worktree.to_str().expect("skron worktree path"),
                "HEAD",
            ],
            "skron",
        )
        .0,
        command_output(
            "git",
            git_repo.path(),
            &[
                "worktree",
                "add",
                "-B",
                "reset-me",
                git_reset_worktree.to_str().expect("git worktree path"),
                "HEAD",
            ],
            "git",
        )
        .0
    );
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "reset-me"]),
        git(git_repo.path(), ["rev-parse", "reset-me"])
    );
    assert_eq!(
        git(&skron_reset_worktree, ["symbolic-ref", "HEAD"]),
        git(&git_reset_worktree, ["symbolic-ref", "HEAD"])
    );
}

#[test]
fn worktree_add_path_only_creates_branch_like_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    let git_suffix = git_repo
        .path()
        .file_name()
        .and_then(|value| value.to_str())
        .expect("git temp dir name");
    let skron_suffix = skron_repo
        .path()
        .file_name()
        .and_then(|value| value.to_str())
        .expect("skron temp dir name");
    let git_worktree = git_repo
        .path()
        .with_file_name(format!("git-auto-{git_suffix}"));
    let skron_worktree = skron_repo
        .path()
        .with_file_name(format!("skron-auto-{skron_suffix}"));

    assert_eq!(
        command_output(
            skron_bin(),
            skron_repo.path(),
            &[
                "worktree",
                "add",
                skron_worktree.to_str().expect("skron worktree path"),
            ],
            "skron",
        )
        .0,
        command_output(
            "git",
            git_repo.path(),
            &[
                "worktree",
                "add",
                git_worktree.to_str().expect("git worktree path"),
            ],
            "git",
        )
        .0
    );
    assert_eq!(
        git(&skron_worktree, ["symbolic-ref", "HEAD"]),
        format!(
            "refs/heads/{}",
            skron_worktree
                .file_name()
                .and_then(|value| value.to_str())
                .expect("skron worktree dirname")
        )
    );
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        git(&skron_worktree, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        fs::read_to_string(skron_worktree.join("a.txt")).expect("read skron worktree"),
        fs::read_to_string(git_worktree.join("a.txt")).expect("read git worktree")
    );
}

#[test]
fn worktree_add_missing_ref_failure_matches_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    assert_eq!(
        run_skron_failure_output(skron_repo.path(), &["worktree", "add", "../wt", "missing"]),
        git_failure_output(git_repo.path(), &["worktree", "add", "../wt", "missing"])
    );
}

#[test]
fn worktree_main_worktree_failures_match_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    let git_move_target = git_repo.path().with_file_name(format!(
        "{}-git-main-move-target",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let skron_move_target = skron_repo.path().with_file_name(format!(
        "{}-skron-main-move-target",
        skron_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));

    for (skron_args, git_args) in [
        (
            vec![
                "worktree",
                "remove",
                skron_repo.path().to_str().expect("skron repo"),
            ],
            vec![
                "worktree",
                "remove",
                git_repo.path().to_str().expect("git repo"),
            ],
        ),
        (
            vec![
                "worktree",
                "move",
                skron_repo.path().to_str().expect("skron repo"),
                skron_move_target.to_str().expect("skron move target"),
            ],
            vec![
                "worktree",
                "move",
                git_repo.path().to_str().expect("git repo"),
                git_move_target.to_str().expect("git move target"),
            ],
        ),
        (
            vec![
                "worktree",
                "lock",
                skron_repo.path().to_str().expect("skron repo"),
            ],
            vec![
                "worktree",
                "lock",
                git_repo.path().to_str().expect("git repo"),
            ],
        ),
        (
            vec![
                "worktree",
                "unlock",
                skron_repo.path().to_str().expect("skron repo"),
            ],
            vec![
                "worktree",
                "unlock",
                git_repo.path().to_str().expect("git repo"),
            ],
        ),
    ] {
        let skron = run_skron_failure_output(skron_repo.path(), &skron_args);
        let git = git_failure_output(git_repo.path(), &git_args);
        assert_eq!(skron.0, git.0, "{skron_args:?} exit mismatch");
        assert_eq!(skron.1, git.1, "{skron_args:?} stdout mismatch");
        if skron_args[1] == "lock" || skron_args[1] == "unlock" {
            assert_eq!(skron.2, git.2, "{skron_args:?} stderr mismatch");
        } else {
            assert!(skron.2.contains("is a main working tree"));
            assert!(git.2.contains("is a main working tree"));
        }
    }
}

#[test]
fn worktree_missing_path_failures_match_stock_git() {
    let git_repo = worktree_fixture_repo();
    let skron_repo = worktree_fixture_repo();
    let scratch = TempDir::new().expect("shared missing path root");
    let missing = scratch.path().join("missing-worktree");
    let move_target = scratch.path().join("move-target");

    for (skron_args, git_args) in [
        (
            vec![
                "worktree",
                "remove",
                missing.to_str().expect("missing path"),
            ],
            vec![
                "worktree",
                "remove",
                missing.to_str().expect("missing path"),
            ],
        ),
        (
            vec![
                "worktree",
                "move",
                missing.to_str().expect("missing path"),
                move_target.to_str().expect("move target"),
            ],
            vec![
                "worktree",
                "move",
                missing.to_str().expect("missing path"),
                move_target.to_str().expect("move target"),
            ],
        ),
        (
            vec!["worktree", "lock", missing.to_str().expect("missing path")],
            vec!["worktree", "lock", missing.to_str().expect("missing path")],
        ),
        (
            vec![
                "worktree",
                "unlock",
                missing.to_str().expect("missing path"),
            ],
            vec![
                "worktree",
                "unlock",
                missing.to_str().expect("missing path"),
            ],
        ),
    ] {
        assert_eq!(
            run_skron_failure_output(skron_repo.path(), &skron_args),
            git_failure_output(git_repo.path(), &git_args),
            "{skron_args:?}"
        );
    }
}
