mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    command_any_output, configure_identity, git, git_args, git_init, git_status, git_with_env,
    run_zmin, run_zmin_args, run_zmin_status, zmin_bin,
};

fn clean_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("tracked_dir")).expect("create tracked dir");
    fs::write(repo.path().join("tracked.txt"), b"tracked\n").expect("write tracked");
    fs::write(repo.path().join("tracked_dir/tracked.txt"), b"tracked\n")
        .expect("write nested tracked");
    fs::write(
        repo.path().join(".gitignore"),
        b"ignored.log\nignored_dir/\n",
    )
    .expect("write ignore");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    fs::create_dir_all(repo.path().join("dir/sub")).expect("create untracked dir");
    fs::write(repo.path().join("untracked.txt"), b"untracked\n").expect("write untracked");
    fs::write(repo.path().join("dir/a.txt"), b"dir\n").expect("write dir file");
    fs::write(repo.path().join("tracked_dir/untracked.txt"), b"nested\n")
        .expect("write nested untracked");
    fs::write(repo.path().join("ignored.log"), b"ignored\n").expect("write ignored");
    fs::create_dir_all(repo.path().join("ignored_dir")).expect("create ignored dir");
    fs::write(repo.path().join("ignored_dir/ignored.txt"), b"ignored\n")
        .expect("write ignored dir file");
    repo
}

#[test]
fn clean_matches_stock_git_for_dry_run_force_and_paths() {
    let git_repo = clean_fixture_repo();
    let zmin_repo = clean_fixture_repo();

    assert_eq!(
        run_zmin(zmin_repo.path(), ["clean", "-n"]),
        git(git_repo.path(), ["clean", "-n"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["clean", "-n", "-d"]),
        git(git_repo.path(), ["clean", "-n", "-d"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["clean", "-n", "-d", "dir"]),
        git(git_repo.path(), ["clean", "-n", "-d", "dir"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["clean", "-X", "-n"]),
        git(git_repo.path(), ["clean", "-X", "-n"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["clean", "-X", "-n", "-d"]),
        git(git_repo.path(), ["clean", "-X", "-n", "-d"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["clean", "-x", "-n", "-d"]),
        git(git_repo.path(), ["clean", "-x", "-n", "-d"])
    );
    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["clean"]),
        git_status(git_repo.path(), ["clean"])
    );

    assert_eq!(
        run_zmin(zmin_repo.path(), ["clean", "-f", "-d"]),
        git(git_repo.path(), ["clean", "-f", "-d"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert!(zmin_repo.path().join("ignored.log").exists());
    assert!(zmin_repo.path().join("ignored_dir/ignored.txt").exists());
    assert!(zmin_repo.path().join("tracked_dir/tracked.txt").exists());
    assert!(!zmin_repo.path().join("dir").exists());
    assert!(!zmin_repo.path().join("tracked_dir/untracked.txt").exists());
    assert!(!zmin_repo.path().join("untracked.txt").exists());
}

#[test]
fn clean_quiet_matches_stock_git_output_and_state() {
    let git_repo = clean_fixture_repo();
    let zmin_repo = clean_fixture_repo();

    assert_eq!(
        run_zmin(zmin_repo.path(), ["clean", "-f", "-q", "-d"]),
        git(git_repo.path(), ["clean", "-f", "-q", "-d"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert!(!zmin_repo.path().join("dir").exists());
    assert!(!zmin_repo.path().join("untracked.txt").exists());
    assert!(zmin_repo.path().join("ignored.log").exists());
}

#[test]
fn clean_exclude_patterns_match_stock_git_modes() {
    for args in [
        vec!["clean", "-n", "-e", "keep.tmp"],
        vec!["clean", "-n", "-x", "-e", "keep.tmp"],
        vec!["clean", "-n", "-X", "-e", "keep.tmp"],
    ] {
        let git_repo = clean_fixture_repo();
        let zmin_repo = clean_fixture_repo();
        fs::write(git_repo.path().join("keep.tmp"), b"keep\n").expect("write git keep");
        fs::write(zmin_repo.path().join("keep.tmp"), b"keep\n").expect("write zmin keep");

        assert_eq!(
            run_zmin_args(zmin_repo.path(), &args),
            git_args(git_repo.path(), &args)
        );
    }

    let git_repo = clean_fixture_repo();
    let zmin_repo = clean_fixture_repo();
    fs::write(git_repo.path().join("keep.tmp"), b"keep\n").expect("write git keep");
    fs::write(zmin_repo.path().join("keep.tmp"), b"keep\n").expect("write zmin keep");
    assert_eq!(
        run_zmin(zmin_repo.path(), ["clean", "-f", "-x", "-e", "keep.tmp"]),
        git(git_repo.path(), ["clean", "-f", "-x", "-e", "keep.tmp"])
    );
    assert!(zmin_repo.path().join("keep.tmp").exists());
    assert!(!zmin_repo.path().join("ignored.log").exists());
}

#[test]
fn clean_no_option_toggles_match_stock_git_order() {
    for args in [
        vec!["clean", "-n", "--no-dry-run"],
        vec!["clean", "--no-dry-run", "--dry-run"],
        vec!["clean", "-f", "--no-force"],
        vec!["clean", "--no-force", "-f"],
        vec!["clean", "-f", "-q", "--no-quiet"],
        vec!["clean", "-n", "-q"],
        vec!["clean", "--dry-run", "--quiet", "--no-quiet"],
        vec!["clean", "--no-interactive", "-n"],
        vec!["clean", "-n", "--no-interactive"],
        vec!["clean", "--interactive", "--no-interactive", "-n"],
        vec!["clean", "-fd"],
        vec!["clean", "-fx", "-ekeep.tmp"],
    ] {
        let git_repo = clean_fixture_repo();
        let zmin_repo = clean_fixture_repo();
        fs::write(git_repo.path().join("keep.tmp"), b"keep\n").expect("write git keep");
        fs::write(zmin_repo.path().join("keep.tmp"), b"keep\n").expect("write zmin keep");

        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), &args, "zmin"),
            command_any_output("git", git_repo.path(), &args, "git"),
            "clean output should match for {args:?}"
        );
        assert_eq!(
            zmin_repo.path().join("keep.tmp").exists(),
            git_repo.path().join("keep.tmp").exists(),
            "keep.tmp existence should match for {args:?}"
        );
        assert_eq!(
            zmin_repo.path().join("untracked.txt").exists(),
            git_repo.path().join("untracked.txt").exists(),
            "fixture untracked.txt existence should match for {args:?}"
        );
    }
}

#[test]
fn clean_nested_git_directories_require_double_force_like_stock_git() {
    for args in [
        vec!["clean", "-n", "-d"],
        vec!["clean", "-f", "-d"],
        vec!["clean", "-ff", "-d"],
    ] {
        let git_repo = clean_fixture_repo();
        let zmin_repo = clean_fixture_repo();
        fs::create_dir_all(git_repo.path().join("nested")).expect("create git nested");
        fs::create_dir_all(zmin_repo.path().join("nested")).expect("create zmin nested");
        git(git_repo.path().join("nested").as_path(), ["init"]);
        git(zmin_repo.path().join("nested").as_path(), ["init"]);
        fs::write(git_repo.path().join("nested/file.txt"), b"nested\n").expect("write git nested");
        fs::write(zmin_repo.path().join("nested/file.txt"), b"nested\n")
            .expect("write zmin nested");

        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), &args, "zmin"),
            command_any_output("git", git_repo.path(), &args, "git"),
            "clean nested repo output should match for {args:?}"
        );
        assert_eq!(
            zmin_repo.path().join("nested/.git").exists(),
            git_repo.path().join("nested/.git").exists(),
            "nested repo existence should match for {args:?}"
        );
    }
}

#[test]
fn clean_require_force_false_matches_stock_git() {
    let git_repo = clean_fixture_repo();
    let zmin_repo = clean_fixture_repo();
    git(git_repo.path(), ["config", "clean.requireForce", "false"]);
    git(zmin_repo.path(), ["config", "clean.requireForce", "false"]);

    assert_eq!(
        command_any_output(zmin_bin(), zmin_repo.path(), &["clean"], "zmin"),
        command_any_output("git", git_repo.path(), &["clean"], "git")
    );
    assert_eq!(
        zmin_repo.path().join("untracked.txt").exists(),
        git_repo.path().join("untracked.txt").exists()
    );
    assert_eq!(
        zmin_repo.path().join("dir").exists(),
        git_repo.path().join("dir").exists()
    );
}

#[test]
fn clean_invalid_require_force_bool_matches_stock_git() {
    let git_repo = clean_fixture_repo();
    let zmin_repo = clean_fixture_repo();
    git(git_repo.path(), ["config", "clean.requireForce", "maybe"]);
    git(zmin_repo.path(), ["config", "clean.requireForce", "maybe"]);

    assert_eq!(
        command_any_output(zmin_bin(), zmin_repo.path(), &["clean"], "zmin"),
        command_any_output("git", git_repo.path(), &["clean"], "git")
    );
}
