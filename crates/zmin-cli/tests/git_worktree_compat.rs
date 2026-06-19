mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    command_failure_output, command_output, configure_identity, git, git_failure_output, git_init,
    git_with_env, run_zmin, run_zmin_failure_output, zmin_bin, write_file,
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

    run_zmin(
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
    let list = run_zmin(repo.path(), ["worktree", "list", "--porcelain"]);
    assert!(list.contains(&format!("worktree {}", worktree.display())));
    assert!(list.contains(&format!("HEAD {target}")));
    assert!(list.contains("detached"));

    run_zmin(
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
    let zmin_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-move-source",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_worktree = zmin_repo.path().with_file_name(format!(
        "{}-zmin-move-source",
        zmin_repo
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
    run_zmin(
        zmin_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            zmin_worktree.to_str().expect("zmin worktree path"),
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
    let zmin_parent = zmin_repo.path().with_file_name(format!(
        "{}-zmin-move-target-parent",
        zmin_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    fs::create_dir_all(&git_parent).expect("create git move target parent");
    fs::create_dir_all(&zmin_parent).expect("create zmin move target parent");
    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "worktree",
                "move",
                zmin_worktree.to_str().expect("zmin source worktree"),
                zmin_parent.to_str().expect("zmin target parent"),
            ],
            "zmin",
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
    let moved_zmin = zmin_parent.join(zmin_worktree.file_name().expect("zmin worktree name"));
    assert!(!git_worktree.exists());
    assert!(!zmin_worktree.exists());
    assert_eq!(
        git(&moved_zmin, ["rev-parse", "HEAD"]),
        git(&moved_git, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&moved_zmin, ["rev-parse", "--abbrev-ref", "HEAD"]),
        git(&moved_git, ["rev-parse", "--abbrev-ref", "HEAD"])
    );
    assert_eq!(
        fs::read_to_string(moved_zmin.join("a.txt")).expect("read moved zmin worktree"),
        fs::read_to_string(moved_git.join("a.txt")).expect("read moved git worktree")
    );
    assert!(
        run_zmin(zmin_repo.path(), ["worktree", "list", "--porcelain"])
            .contains(&format!("worktree {}", moved_zmin.display()))
    );
    assert_eq!(
        fs::read_to_string(
            fs::read_to_string(moved_zmin.join(".git"))
                .expect("read moved zmin gitfile")
                .trim()
                .strip_prefix("gitdir: ")
                .expect("gitdir prefix")
                .trim()
                .to_owned()
                + "/gitdir"
        )
        .expect("read moved zmin admin gitdir")
        .trim(),
        moved_zmin.join(".git").to_string_lossy()
    );
}

#[test]
fn worktree_lock_unlock_and_locked_remove_match_stock_git() {
    let git_repo = worktree_fixture_repo();
    let zmin_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-lock-source",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_worktree = zmin_repo.path().with_file_name(format!(
        "{}-zmin-lock-source",
        zmin_repo
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
    run_zmin(
        zmin_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            zmin_worktree.to_str().expect("zmin worktree path"),
        ],
    );

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "worktree",
                "lock",
                "--reason",
                "why now",
                zmin_worktree.to_str().expect("zmin worktree path"),
            ],
            "zmin",
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
    let list = run_zmin(zmin_repo.path(), ["worktree", "list", "--porcelain"]);
    assert!(list.contains("locked why now"));
    assert_eq!(
        run_zmin_failure_output(
            zmin_repo.path(),
            &[
                "worktree",
                "remove",
                zmin_worktree.to_str().expect("zmin worktree path"),
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
            zmin_bin(),
            zmin_repo.path(),
            &[
                "worktree",
                "unlock",
                zmin_worktree.to_str().expect("zmin worktree path"),
            ],
            "zmin",
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
    let list = run_zmin(zmin_repo.path(), ["worktree", "list", "--porcelain"]);
    assert!(!list.contains("locked why now"));
    run_zmin(
        zmin_repo.path(),
        [
            "worktree",
            "remove",
            zmin_worktree.to_str().expect("zmin worktree path"),
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
    assert!(!zmin_worktree.exists());
    assert!(!git_worktree.exists());
}

#[test]
fn worktree_locked_double_force_matches_stock_git() {
    let git_repo = worktree_fixture_repo();
    let zmin_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-locked-force-source",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_worktree = zmin_repo.path().with_file_name(format!(
        "{}-zmin-locked-force-source",
        zmin_repo
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
    run_zmin(
        zmin_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            zmin_worktree.to_str().expect("zmin worktree path"),
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
    run_zmin(
        zmin_repo.path(),
        [
            "worktree",
            "lock",
            "--reason",
            "why now",
            zmin_worktree.to_str().expect("zmin worktree path"),
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
    let zmin_moved = zmin_repo.path().with_file_name(format!(
        "{}-zmin-locked-force-moved",
        zmin_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));

    for flag in [None, Some("-f")] {
        let mut zmin_args = vec!["worktree", "move"];
        let mut git_args = vec!["worktree", "move"];
        if let Some(flag) = flag {
            zmin_args.push(flag);
            git_args.push(flag);
        }
        zmin_args.extend([
            zmin_worktree.to_str().expect("zmin worktree path"),
            zmin_moved.to_str().expect("zmin moved path"),
        ]);
        git_args.extend([
            git_worktree.to_str().expect("git worktree path"),
            git_moved.to_str().expect("git moved path"),
        ]);
        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), &zmin_args),
            git_failure_output(git_repo.path(), &git_args),
            "{zmin_args:?}"
        );
    }

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "worktree",
                "move",
                "-f",
                "-f",
                zmin_worktree.to_str().expect("zmin worktree path"),
                zmin_moved.to_str().expect("zmin moved path"),
            ],
            "zmin",
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
    let zmin_admin = single_worktree_admin(zmin_repo.path());
    assert!(!git_worktree.exists());
    assert!(!zmin_worktree.exists());
    assert!(git_moved.exists());
    assert!(zmin_moved.exists());

    assert_eq!(
        run_zmin_failure_output(
            zmin_repo.path(),
            &[
                "worktree",
                "remove",
                "-f",
                zmin_moved.to_str().expect("zmin moved path"),
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
            zmin_bin(),
            zmin_repo.path(),
            &[
                "worktree",
                "remove",
                "-f",
                "-f",
                zmin_moved.to_str().expect("zmin moved path"),
            ],
            "zmin",
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
    assert!(!zmin_moved.exists());
    assert!(!git_admin.exists());
    assert!(!zmin_admin.exists());
}

#[test]
fn worktree_move_existing_target_fails_even_with_force_like_stock_git() {
    let git_repo = worktree_fixture_repo();
    let zmin_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-force-move-source",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_worktree = zmin_repo.path().with_file_name(format!(
        "{}-zmin-force-move-source",
        zmin_repo
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
    run_zmin(
        zmin_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            zmin_worktree.to_str().expect("zmin worktree path"),
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
    let zmin_target = zmin_repo.path().with_file_name(format!(
        "{}-zmin-force-move-target",
        zmin_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    fs::write(&git_target, b"occupied").expect("write git move target");
    fs::write(&zmin_target, b"occupied").expect("write zmin move target");

    let zmin_failure = run_zmin_failure_output(
        zmin_repo.path(),
        &[
            "worktree",
            "move",
            "-f",
            zmin_worktree.to_str().expect("zmin worktree path"),
            zmin_target.to_str().expect("zmin target path"),
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
    assert_eq!(zmin_failure.0, git_failure.0);
    assert_eq!(zmin_failure.1, git_failure.1);
    assert!(zmin_failure.2.ends_with("already exists"));
    assert!(git_failure.2.ends_with("already exists"));
    assert!(zmin_worktree.exists());
    assert!(git_worktree.exists());
    assert_eq!(
        fs::read_to_string(&zmin_target).expect("read zmin target"),
        fs::read_to_string(&git_target).expect("read git target")
    );
}

#[test]
fn worktree_prune_removes_missing_unlocked_metadata_like_stock_git() {
    let git_repo = worktree_fixture_repo();
    let zmin_repo = worktree_fixture_repo();
    let git_parent = git_repo.path().with_file_name(format!(
        "{}-git-prune-parent",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_parent = zmin_repo.path().with_file_name(format!(
        "{}-zmin-prune-parent",
        zmin_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    fs::create_dir_all(&git_parent).expect("create git prune parent");
    fs::create_dir_all(&zmin_parent).expect("create zmin prune parent");
    let git_worktree = git_parent.join("linked-prune");
    let zmin_worktree = zmin_parent.join("linked-prune");
    git(
        git_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            git_worktree.to_str().expect("git worktree path"),
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            zmin_worktree.to_str().expect("zmin worktree path"),
        ],
    );
    let git_admin = single_worktree_admin(git_repo.path());
    let zmin_admin = single_worktree_admin(zmin_repo.path());
    fs::remove_dir_all(&git_worktree).expect("remove git worktree");
    fs::remove_dir_all(&zmin_worktree).expect("remove zmin worktree");

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "worktree",
                "prune",
                "--dry-run",
                "--verbose",
                "--expire",
                "now"
            ],
            "zmin",
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
    assert!(zmin_admin.exists());
    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["worktree", "prune", "--expire", "now"],
            "zmin",
        ),
        command_output(
            "git",
            git_repo.path(),
            &["worktree", "prune", "--expire", "now"],
            "git",
        )
    );
    assert!(!git_admin.exists());
    assert!(!zmin_admin.exists());
}

#[test]
fn worktree_repair_updates_moved_worktree_gitdir_like_stock_git() {
    let git_repo = worktree_fixture_repo();
    let zmin_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-repair-source",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_worktree = zmin_repo.path().with_file_name(format!(
        "{}-repair-source",
        zmin_repo
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
    run_zmin(
        zmin_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            zmin_worktree.to_str().expect("zmin worktree path"),
        ],
    );
    let git_admin = single_worktree_admin(git_repo.path());
    let zmin_admin = single_worktree_admin(zmin_repo.path());
    let git_moved = git_repo.path().with_file_name(format!(
        "{}-repair-moved",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_moved = zmin_repo.path().with_file_name(format!(
        "{}-repair-moved",
        zmin_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    fs::rename(&git_worktree, &git_moved).expect("move git worktree outside git");
    fs::rename(&zmin_worktree, &zmin_moved).expect("move zmin worktree outside zmin");

    let zmin_repair = command_output(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "worktree",
            "repair",
            zmin_moved.to_str().expect("zmin moved worktree"),
        ],
        "zmin",
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
    assert_eq!(zmin_repair.0, git_repair.0);
    assert_eq!(zmin_repair.1, git_repair.1);
    assert!(zmin_repair.2.contains("repair: gitdir incorrect:"));
    assert!(git_repair.2.contains("repair: gitdir incorrect:"));
    assert_eq!(
        std::path::PathBuf::from(
            fs::read_to_string(zmin_admin.join("gitdir"))
                .expect("read repaired zmin admin gitdir")
                .trim()
        )
        .canonicalize()
        .expect("canonicalize repaired zmin gitdir"),
        zmin_moved
            .join(".git")
            .canonicalize()
            .expect("canonicalize zmin moved gitfile")
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
        git(&zmin_moved, ["rev-parse", "HEAD"]),
        git(&git_moved, ["rev-parse", "HEAD"])
    );
    assert!(
        run_zmin(zmin_repo.path(), ["worktree", "list", "--porcelain"])
            .contains(&format!("worktree {}", zmin_moved.display()))
    );
}

#[test]
fn worktree_repair_relative_path_modes_match_stock_git() {
    let git_repo = worktree_fixture_repo();
    let zmin_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-relative-repair-source",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_worktree = zmin_repo.path().with_file_name(format!(
        "{}-zmin-relative-repair-source",
        zmin_repo
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
    run_zmin(
        zmin_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            zmin_worktree.to_str().expect("zmin worktree path"),
        ],
    );
    let git_admin = single_worktree_admin(git_repo.path());
    let zmin_admin = single_worktree_admin(zmin_repo.path());
    let git_moved = git_repo.path().with_file_name(format!(
        "{}-git-relative-repair-moved",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_moved = zmin_repo.path().with_file_name(format!(
        "{}-zmin-relative-repair-moved",
        zmin_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    fs::rename(&git_worktree, &git_moved).expect("move git worktree");
    fs::rename(&zmin_worktree, &zmin_moved).expect("move zmin worktree");

    let zmin_relative = command_output(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "worktree",
            "repair",
            "--relative-paths",
            zmin_moved.to_str().expect("zmin moved worktree"),
        ],
        "zmin",
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
    assert_eq!(zmin_relative.0, git_relative.0);
    assert_eq!(zmin_relative.1, git_relative.1);
    assert!(
        zmin_relative
            .2
            .contains("repair: gitdir absolute/relative path mismatch:")
    );
    assert!(
        zmin_relative
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

    let zmin_dotgit = gitdir_file_target(&zmin_moved);
    let git_dotgit = gitdir_file_target(&git_moved);
    assert!(!std::path::Path::new(&zmin_dotgit).is_absolute());
    assert!(!std::path::Path::new(&git_dotgit).is_absolute());
    assert!(
        !std::path::Path::new(
            fs::read_to_string(zmin_admin.join("gitdir"))
                .expect("read zmin relative admin gitdir")
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
        git(&zmin_moved, ["rev-parse", "HEAD"]),
        git(&git_moved, ["rev-parse", "HEAD"])
    );

    let zmin_absolute = command_output(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "worktree",
            "repair",
            "--no-relative-paths",
            zmin_moved.to_str().expect("zmin moved worktree"),
        ],
        "zmin",
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
    assert_eq!(zmin_absolute.0, git_absolute.0);
    assert_eq!(zmin_absolute.1, git_absolute.1);
    assert!(std::path::Path::new(&gitdir_file_target(&zmin_moved)).is_absolute());
    assert!(std::path::Path::new(&gitdir_file_target(&git_moved)).is_absolute());
    assert!(
        std::path::Path::new(
            fs::read_to_string(zmin_admin.join("gitdir"))
                .expect("read zmin absolute admin gitdir")
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
    let zmin_repo = worktree_fixture_repo();
    git(
        git_repo.path(),
        ["config", "extensions.worktreeConfig", "true"],
    );
    run_zmin(
        zmin_repo.path(),
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
    let zmin_worktree = zmin_repo.path().with_file_name(format!(
        "{}-zmin-config-worktree",
        zmin_repo
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
    run_zmin(
        zmin_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            zmin_worktree.to_str().expect("zmin worktree path"),
        ],
    );

    assert_eq!(
        command_output(
            zmin_bin(),
            &zmin_worktree,
            &["config", "--worktree", "demo.value", "linked"],
            "zmin",
        ),
        command_output(
            "git",
            &git_worktree,
            &["config", "--worktree", "demo.value", "linked"],
            "git",
        )
    );
    assert_eq!(
        run_zmin(&zmin_worktree, ["config", "--get", "demo.value"]),
        git(&git_worktree, ["config", "--get", "demo.value"])
    );
    assert_eq!(
        run_zmin(&zmin_worktree, ["config", "--worktree", "--list"]),
        git(&git_worktree, ["config", "--worktree", "--list"])
    );
    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["config", "--get", "demo.value"]),
        git_failure_output(git_repo.path(), &["config", "--get", "demo.value"])
    );
    assert_eq!(
        fs::read_to_string(single_worktree_admin(zmin_repo.path()).join("config.worktree"))
            .expect("read zmin worktree config"),
        fs::read_to_string(single_worktree_admin(git_repo.path()).join("config.worktree"))
            .expect("read git worktree config")
    );

    assert_eq!(
        command_output(
            zmin_bin(),
            &zmin_worktree,
            &["config", "--worktree", "--unset", "demo.value"],
            "zmin",
        ),
        command_output(
            "git",
            &git_worktree,
            &["config", "--worktree", "--unset", "demo.value"],
            "git",
        )
    );
    assert_eq!(
        run_zmin_failure_output(
            &zmin_worktree,
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
    let zmin_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-config-denied",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_worktree = zmin_repo.path().with_file_name(format!(
        "{}-zmin-config-denied",
        zmin_repo
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
    run_zmin(
        zmin_repo.path(),
        [
            "worktree",
            "add",
            "--detach",
            zmin_worktree.to_str().expect("zmin worktree path"),
        ],
    );

    assert_eq!(
        run_zmin_failure_output(
            &zmin_worktree,
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
    let zmin_repo = worktree_fixture_repo();
    git(git_repo.path(), ["branch", "feature"]);
    git(zmin_repo.path(), ["branch", "feature"]);
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-feature",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_worktree = zmin_repo.path().with_file_name(format!(
        "{}-zmin-feature",
        zmin_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "worktree",
                "add",
                zmin_worktree.to_str().expect("zmin worktree path"),
                "feature",
            ],
            "zmin",
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
        git(&zmin_worktree, ["symbolic-ref", "HEAD"]),
        git(&git_worktree, ["symbolic-ref", "HEAD"])
    );
    assert!(
        run_zmin(zmin_repo.path(), ["worktree", "list", "--porcelain"])
            .contains("branch refs/heads/feature")
    );
    assert_eq!(
        git(&zmin_worktree, ["rev-parse", "HEAD"]),
        git(&git_worktree, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        fs::read_to_string(zmin_worktree.join("a.txt")).expect("read zmin worktree"),
        fs::read_to_string(git_worktree.join("a.txt")).expect("read git worktree")
    );
}

#[test]
fn worktree_add_force_allows_checked_out_branch_like_stock_git() {
    let git_repo = worktree_fixture_repo();
    let zmin_repo = worktree_fixture_repo();
    git(git_repo.path(), ["branch", "feature"]);
    git(zmin_repo.path(), ["branch", "feature"]);

    let git_first = git_repo.path().with_file_name(format!(
        "{}-git-feature-first",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_first = zmin_repo.path().with_file_name(format!(
        "{}-zmin-feature-first",
        zmin_repo
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
    run_zmin(
        zmin_repo.path(),
        [
            "worktree",
            "add",
            zmin_first.to_str().expect("zmin first worktree"),
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
    let zmin_forced = zmin_repo.path().with_file_name(format!(
        "{}-zmin-feature-forced",
        zmin_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "worktree",
                "add",
                "-f",
                zmin_forced.to_str().expect("zmin forced worktree"),
                "feature",
            ],
            "zmin",
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
        git(&zmin_forced, ["symbolic-ref", "HEAD"]),
        git(&git_forced, ["symbolic-ref", "HEAD"])
    );
    assert_eq!(
        git(&zmin_forced, ["rev-parse", "HEAD"]),
        git(&git_forced, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        fs::read_to_string(zmin_forced.join("a.txt")).expect("read zmin forced worktree"),
        fs::read_to_string(git_forced.join("a.txt")).expect("read git forced worktree")
    );
}

#[test]
fn worktree_add_commitish_detaches_like_stock_git() {
    let git_repo = worktree_fixture_repo();
    let zmin_repo = worktree_fixture_repo();
    let git_worktree = git_repo.path().with_file_name(format!(
        "{}-git-detached",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_worktree = zmin_repo.path().with_file_name(format!(
        "{}-zmin-detached",
        zmin_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "worktree",
                "add",
                zmin_worktree.to_str().expect("zmin worktree path"),
                "HEAD~1",
            ],
            "zmin",
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
        &zmin_worktree,
        &["symbolic-ref", "-q", "HEAD"],
        "git",
    );
    assert_eq!(symbolic_status, 1);
    assert_eq!(symbolic_stdout, "");
    assert_eq!(
        git(&zmin_worktree, ["rev-parse", "HEAD"]),
        git(&git_worktree, ["rev-parse", "HEAD"])
    );
    assert!(run_zmin(zmin_repo.path(), ["worktree", "list", "--porcelain"]).contains("detached"));
    assert_eq!(
        fs::read_to_string(zmin_worktree.join("a.txt")).expect("read zmin worktree"),
        fs::read_to_string(git_worktree.join("a.txt")).expect("read git worktree")
    );
}

#[test]
fn worktree_add_branch_options_match_stock_git() {
    let git_repo = worktree_fixture_repo();
    let zmin_repo = worktree_fixture_repo();
    let git_branch_worktree = git_repo.path().with_file_name(format!(
        "{}-git-new-branch",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_branch_worktree = zmin_repo.path().with_file_name(format!(
        "{}-zmin-new-branch",
        zmin_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "topic",
                zmin_branch_worktree.to_str().expect("zmin worktree path"),
                "HEAD~1",
            ],
            "zmin",
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
        git(&zmin_branch_worktree, ["symbolic-ref", "HEAD"]),
        git(&git_branch_worktree, ["symbolic-ref", "HEAD"])
    );
    assert_eq!(
        git(&zmin_branch_worktree, ["rev-parse", "HEAD"]),
        git(&git_branch_worktree, ["rev-parse", "HEAD"])
    );

    git(git_repo.path(), ["branch", "reset-me", "HEAD~1"]);
    git(zmin_repo.path(), ["branch", "reset-me", "HEAD~1"]);
    let git_reset_worktree = git_repo.path().with_file_name(format!(
        "{}-git-reset-branch",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_reset_worktree = zmin_repo.path().with_file_name(format!(
        "{}-zmin-reset-branch",
        zmin_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "worktree",
                "add",
                "-B",
                "reset-me",
                zmin_reset_worktree.to_str().expect("zmin worktree path"),
                "HEAD",
            ],
            "zmin",
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
        git(zmin_repo.path(), ["rev-parse", "reset-me"]),
        git(git_repo.path(), ["rev-parse", "reset-me"])
    );
    assert_eq!(
        git(&zmin_reset_worktree, ["symbolic-ref", "HEAD"]),
        git(&git_reset_worktree, ["symbolic-ref", "HEAD"])
    );
}

#[test]
fn worktree_add_path_only_creates_branch_like_stock_git() {
    let git_repo = worktree_fixture_repo();
    let zmin_repo = worktree_fixture_repo();
    let git_suffix = git_repo
        .path()
        .file_name()
        .and_then(|value| value.to_str())
        .expect("git temp dir name");
    let zmin_suffix = zmin_repo
        .path()
        .file_name()
        .and_then(|value| value.to_str())
        .expect("zmin temp dir name");
    let git_worktree = git_repo
        .path()
        .with_file_name(format!("git-auto-{git_suffix}"));
    let zmin_worktree = zmin_repo
        .path()
        .with_file_name(format!("zmin-auto-{zmin_suffix}"));

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "worktree",
                "add",
                zmin_worktree.to_str().expect("zmin worktree path"),
            ],
            "zmin",
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
        git(&zmin_worktree, ["symbolic-ref", "HEAD"]),
        format!(
            "refs/heads/{}",
            zmin_worktree
                .file_name()
                .and_then(|value| value.to_str())
                .expect("zmin worktree dirname")
        )
    );
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(&zmin_worktree, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        fs::read_to_string(zmin_worktree.join("a.txt")).expect("read zmin worktree"),
        fs::read_to_string(git_worktree.join("a.txt")).expect("read git worktree")
    );
}

#[test]
fn worktree_add_missing_ref_failure_matches_stock_git() {
    let git_repo = worktree_fixture_repo();
    let zmin_repo = worktree_fixture_repo();
    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["worktree", "add", "../wt", "missing"]),
        git_failure_output(git_repo.path(), &["worktree", "add", "../wt", "missing"])
    );
}

#[test]
fn worktree_main_worktree_failures_match_stock_git() {
    let git_repo = worktree_fixture_repo();
    let zmin_repo = worktree_fixture_repo();
    let git_move_target = git_repo.path().with_file_name(format!(
        "{}-git-main-move-target",
        git_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));
    let zmin_move_target = zmin_repo.path().with_file_name(format!(
        "{}-zmin-main-move-target",
        zmin_repo
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .expect("temp dir name")
    ));

    for (zmin_args, git_args) in [
        (
            vec![
                "worktree",
                "remove",
                zmin_repo.path().to_str().expect("zmin repo"),
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
                zmin_repo.path().to_str().expect("zmin repo"),
                zmin_move_target.to_str().expect("zmin move target"),
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
                zmin_repo.path().to_str().expect("zmin repo"),
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
                zmin_repo.path().to_str().expect("zmin repo"),
            ],
            vec![
                "worktree",
                "unlock",
                git_repo.path().to_str().expect("git repo"),
            ],
        ),
    ] {
        let zmin = run_zmin_failure_output(zmin_repo.path(), &zmin_args);
        let git = git_failure_output(git_repo.path(), &git_args);
        assert_eq!(zmin.0, git.0, "{zmin_args:?} exit mismatch");
        assert_eq!(zmin.1, git.1, "{zmin_args:?} stdout mismatch");
        if zmin_args[1] == "lock" || zmin_args[1] == "unlock" {
            assert_eq!(zmin.2, git.2, "{zmin_args:?} stderr mismatch");
        } else {
            assert!(zmin.2.contains("is a main working tree"));
            assert!(git.2.contains("is a main working tree"));
        }
    }
}

#[test]
fn worktree_missing_path_failures_match_stock_git() {
    let git_repo = worktree_fixture_repo();
    let zmin_repo = worktree_fixture_repo();
    let scratch = TempDir::new().expect("shared missing path root");
    let missing = scratch.path().join("missing-worktree");
    let move_target = scratch.path().join("move-target");

    for (zmin_args, git_args) in [
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
            run_zmin_failure_output(zmin_repo.path(), &zmin_args),
            git_failure_output(git_repo.path(), &git_args),
            "{zmin_args:?}"
        );
    }
}
