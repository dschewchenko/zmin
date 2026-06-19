mod common;

use std::fs;
use std::process::Command;

use common::{
    command_any_output, command_output, configure_identity, git, git_args, git_init, git_with_env,
    run_zmin, run_zmin_args, visible_worktree_files, write_file, zmin_bin,
};
use tempfile::TempDir;

#[test]
fn submodule_add_status_creates_stock_readable_gitlink() {
    let submodule = submodule_child_repo();
    let super_repo = git_init();
    configure_identity(super_repo.path());
    git(super_repo.path(), ["checkout", "-b", "main"]);

    run_zmin(
        super_repo.path(),
        [
            "submodule",
            "add",
            submodule.path().to_str().expect("submodule path"),
            "deps/sub",
        ],
    );
    assert_eq!(
        git(super_repo.path(), ["status", "--short"]),
        "A  .gitmodules\nA  deps/sub"
    );
    assert_eq!(
        git(super_repo.path(), ["submodule", "status"]),
        run_zmin(super_repo.path(), ["submodule", "status"])
    );
    assert!(
        fs::read_to_string(super_repo.path().join(".gitmodules"))
            .expect("read gitmodules")
            .contains("path = deps/sub")
    );
}

#[test]
fn add_submodule_path_updates_gitlink_without_staging_nested_files() {
    let dir = TempDir::new().expect("temp dir");
    let submodule = dir.path().join("submodule");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            submodule.to_str().expect("submodule path"),
        ],
    );
    configure_identity(&submodule);
    write_file(&submodule, "sub.txt", "one\n");
    git(&submodule, ["add", "-A"]);
    git_with_env(&submodule, ["commit", "-m", "one"]);

    let super_repo = dir.path().join("super");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            super_repo.to_str().expect("super path"),
        ],
    );
    configure_identity(&super_repo);
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "../submodule",
            "deps/sub",
        ])
        .current_dir(&super_repo)
        .output()
        .expect("git submodule add");
    assert!(
        output.status.success(),
        "git submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_with_env(&super_repo, ["commit", "-m", "submodule"]);

    git(&super_repo.join("deps/sub"), ["checkout", "-b", "feature"]);
    write_file(&super_repo.join("deps/sub"), "feature.txt", "feature\n");
    git(&super_repo.join("deps/sub"), ["add", "-A"]);
    git_with_env(&super_repo.join("deps/sub"), ["commit", "-m", "feature"]);
    run_zmin(&super_repo, ["add", "deps/sub"]);

    let staged = git(&super_repo, ["ls-files", "-s", "deps/sub"]);
    assert!(
        staged.starts_with("160000 "),
        "submodule path should remain a gitlink: {staged}"
    );
    assert!(
        !git(&super_repo, ["ls-files"]).contains("deps/sub/feature.txt"),
        "nested submodule files must not be staged in the superproject"
    );
}

#[test]
fn submodule_status_cached_and_changed_head_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let submodule = dir.path().join("submodule");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            submodule.to_str().expect("submodule path"),
        ],
    );
    configure_identity(&submodule);
    write_file(&submodule, "sub.txt", "one\n");
    git(&submodule, ["add", "-A"]);
    git_with_env(&submodule, ["commit", "-m", "one"]);
    let first = git(&submodule, ["rev-parse", "HEAD"]);
    write_file(&submodule, "sub.txt", "two\n");
    git(&submodule, ["add", "-A"]);
    git_with_env(&submodule, ["commit", "-m", "two"]);
    git(&submodule, ["checkout", &first]);

    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            submodule.to_str().expect("submodule path"),
            "deps/sub",
        ])
        .current_dir(&source)
        .output()
        .expect("git submodule add");
    assert!(
        output.status.success(),
        "git submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_with_env(&source, ["commit", "-m", "submodule"]);
    git(&submodule, ["checkout", "main"]);

    let git_output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "clone",
            "--recurse-submodules",
            source.to_str().expect("source path"),
            "git-status-clone",
        ])
        .current_dir(dir.path())
        .output()
        .expect("git recursive clone");
    assert!(
        git_output.status.success(),
        "git recursive clone failed: {}",
        String::from_utf8_lossy(&git_output.stderr)
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--recurse-submodules",
            source.to_str().expect("source path"),
            "zmin-status-clone",
        ],
    );
    let git_clone = dir.path().join("git-status-clone");
    let zmin_clone = dir.path().join("zmin-status-clone");

    for args in [
        ["submodule"].as_slice(),
        ["submodule", "status"].as_slice(),
        ["submodule", "--cached"].as_slice(),
        ["submodule", "status", "--cached"].as_slice(),
        ["submodule", "status", "--quiet"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&zmin_clone, args),
            git_args(&git_clone, args),
            "clean status mismatch for {args:?}"
        );
    }

    git(&git_clone.join("deps/sub"), ["checkout", "main"]);
    git(&zmin_clone.join("deps/sub"), ["checkout", "main"]);
    for args in [
        ["submodule"].as_slice(),
        ["submodule", "status"].as_slice(),
        ["submodule", "--cached"].as_slice(),
        ["submodule", "status", "--cached"].as_slice(),
        ["submodule", "status", "--quiet"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&zmin_clone, args),
            git_args(&git_clone, args),
            "changed status mismatch for {args:?}"
        );
    }
}

#[test]
fn clone_remote_submodules_checks_out_remote_head_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let submodule = dir.path().join("submodule");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            submodule.to_str().expect("submodule path"),
        ],
    );
    configure_identity(&submodule);
    write_file(&submodule, "sub.txt", "one\n");
    git(&submodule, ["add", "-A"]);
    git_with_env(&submodule, ["commit", "-m", "one"]);
    let first = git(&submodule, ["rev-parse", "HEAD"]);
    write_file(&submodule, "sub.txt", "two\n");
    git(&submodule, ["add", "-A"]);
    git_with_env(&submodule, ["commit", "-m", "two"]);
    let second = git(&submodule, ["rev-parse", "HEAD"]);
    git(&submodule, ["checkout", &first]);

    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            submodule.to_str().expect("submodule path"),
            "deps/sub",
        ])
        .current_dir(&source)
        .output()
        .expect("git submodule add");
    assert!(
        output.status.success(),
        "git submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_with_env(&source, ["commit", "-m", "submodule"]);
    git(&submodule, ["checkout", "main"]);

    let git_output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "clone",
            "--recurse-submodules",
            "--remote-submodules",
            source.to_str().expect("source path"),
            "git-remote-submodules",
        ])
        .current_dir(dir.path())
        .output()
        .expect("git remote-submodules clone");
    assert!(
        git_output.status.success(),
        "git remote-submodules clone failed: {}",
        String::from_utf8_lossy(&git_output.stderr)
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--recurse-submodules",
            "--remote-submodules",
            source.to_str().expect("source path"),
            "zmin-remote-submodules",
        ],
    );

    let git_clone = dir.path().join("git-remote-submodules");
    let zmin_clone = dir.path().join("zmin-remote-submodules");
    assert_eq!(
        git(&git_clone.join("deps/sub"), ["rev-parse", "HEAD"]),
        second
    );
    assert_eq!(
        git(&zmin_clone.join("deps/sub"), ["rev-parse", "HEAD"]),
        second
    );
    assert_eq!(
        visible_worktree_files(&zmin_clone),
        visible_worktree_files(&git_clone)
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["submodule", "status"]),
        git(&git_clone, ["submodule", "status"])
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["submodule", "status", "--cached"]),
        git(&git_clone, ["submodule", "status", "--cached"])
    );
}

#[test]
fn clone_shallow_submodules_matches_stock_git_for_file_urls() {
    let dir = TempDir::new().expect("temp dir");
    let submodule = dir.path().join("submodule");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            submodule.to_str().expect("submodule path"),
        ],
    );
    configure_identity(&submodule);
    for number in 1..=3 {
        write_file(&submodule, "sub.txt", &format!("{number}\n"));
        git(&submodule, ["add", "-A"]);
        git_with_env(&submodule, ["commit", "-m", &format!("sub{number}")]);
    }

    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    let submodule_url = format!("file://{}", submodule.display());
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            &submodule_url,
            "deps/sub",
        ])
        .current_dir(&source)
        .output()
        .expect("git submodule add");
    assert!(
        output.status.success(),
        "git submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_with_env(&source, ["commit", "-m", "submodule"]);

    let git_output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "clone",
            "--recurse-submodules",
            "--shallow-submodules",
            source.to_str().expect("source path"),
            "git-shallow-submodules",
        ])
        .current_dir(dir.path())
        .output()
        .expect("git shallow-submodules clone");
    assert!(
        git_output.status.success(),
        "git shallow-submodules clone failed: {}",
        String::from_utf8_lossy(&git_output.stderr)
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--recurse-submodules",
            "--shallow-submodules",
            source.to_str().expect("source path"),
            "zmin-shallow-submodules",
        ],
    );

    let git_clone = dir.path().join("git-shallow-submodules");
    let zmin_clone = dir.path().join("zmin-shallow-submodules");
    assert_eq!(
        git(
            &zmin_clone.join("deps/sub"),
            ["rev-parse", "--is-shallow-repository"]
        ),
        git(
            &git_clone.join("deps/sub"),
            ["rev-parse", "--is-shallow-repository"]
        )
    );
    assert_eq!(
        git(
            &zmin_clone.join("deps/sub"),
            ["rev-list", "--count", "HEAD"]
        ),
        git(&git_clone.join("deps/sub"), ["rev-list", "--count", "HEAD"])
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["submodule", "status"]),
        git(&git_clone, ["submodule", "status"])
    );
}

#[test]
fn clone_checks_out_uninitialized_submodule_gitlinks_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let submodule = submodule_child_repo();
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            submodule.path().to_str().expect("submodule path"),
            "deps/sub",
        ])
        .current_dir(&source)
        .output()
        .expect("git submodule add");
    assert!(
        output.status.success(),
        "git submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_with_env(&source, ["commit", "-m", "submodule"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-clone"],
    );
    run_zmin(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "zmin-clone"],
    );
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");

    assert!(git_clone.join("deps/sub").is_dir());
    assert!(zmin_clone.join("deps/sub").is_dir());
    assert_eq!(
        visible_worktree_files(&zmin_clone),
        visible_worktree_files(&git_clone)
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["submodule", "status"]),
        git(&git_clone, ["submodule", "status"])
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["ls-files", "-s"]),
        git(&git_clone, ["ls-files", "-s"])
    );
}

#[test]
fn clone_recurse_submodules_initializes_local_submodules_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let submodule = submodule_child_repo();
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            submodule.path().to_str().expect("submodule path"),
            "deps/sub",
        ])
        .current_dir(&source)
        .output()
        .expect("git submodule add");
    assert!(
        output.status.success(),
        "git submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_with_env(&source, ["commit", "-m", "submodule"]);

    let git_output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "clone",
            "--recurse-submodules",
            source.to_str().expect("source path"),
            "git-recursive-clone",
        ])
        .current_dir(dir.path())
        .output()
        .expect("git recursive clone");
    assert!(
        git_output.status.success(),
        "git recursive clone failed: {}",
        String::from_utf8_lossy(&git_output.stderr)
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--recurse-submodules",
            source.to_str().expect("source path"),
            "zmin-recursive-clone",
        ],
    );
    let git_clone = dir.path().join("git-recursive-clone");
    let zmin_clone = dir.path().join("zmin-recursive-clone");

    assert_eq!(
        visible_worktree_files(&zmin_clone),
        visible_worktree_files(&git_clone)
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["submodule", "status"]),
        git(&git_clone, ["submodule", "status"])
    );
    for key in ["submodule.active", "submodule.deps/sub.url"] {
        assert_eq!(
            run_zmin(&zmin_clone, ["config", "--get", key]),
            git(&git_clone, ["config", "--get", key]),
            "recursive clone config mismatch for {key}"
        );
    }
}

#[test]
fn clone_recurse_submodules_pathspec_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let one = submodule_child_repo_named("one.txt", "one\n");
    let two = submodule_child_repo_named("two.txt", "two\n");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    for (repo, path) in [(&one, "deps/one"), (&two, "deps/two")] {
        let output = Command::new("git")
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                repo.path().to_str().expect("submodule path"),
                path,
            ])
            .current_dir(&source)
            .output()
            .expect("git submodule add");
        assert!(
            output.status.success(),
            "git submodule add failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    git_with_env(&source, ["commit", "-m", "submodules"]);

    let git_output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "clone",
            "--recurse-submodules=deps/one",
            source.to_str().expect("source path"),
            "git-recursive-one",
        ])
        .current_dir(dir.path())
        .output()
        .expect("git recursive pathspec clone");
    assert!(
        git_output.status.success(),
        "git recursive pathspec clone failed: {}",
        String::from_utf8_lossy(&git_output.stderr)
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--recurse-submodules=deps/one",
            source.to_str().expect("source path"),
            "zmin-recursive-one",
        ],
    );
    let git_clone = dir.path().join("git-recursive-one");
    let zmin_clone = dir.path().join("zmin-recursive-one");

    assert_eq!(
        visible_worktree_files(&zmin_clone),
        visible_worktree_files(&git_clone)
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["submodule", "status"]),
        git(&git_clone, ["submodule", "status"])
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "submodule.active"]),
        git(&git_clone, ["config", "--get", "submodule.active"])
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            &zmin_clone,
            &["config", "--get", "submodule.deps/two.url"],
            "zmin"
        ),
        command_any_output(
            "git",
            &git_clone,
            &["config", "--get", "submodule.deps/two.url"],
            "git"
        )
    );
}

#[test]
fn clone_jobs_option_validation_matches_stock_git() {
    let source = submodule_child_repo_named("a.txt", "a\n");
    let dir = TempDir::new().expect("temp dir");
    let source_arg = source.path().to_str().expect("source path");

    for args in [
        ["clone", "-j", "bad", source_arg, "zmin-bad-short"].as_slice(),
        ["clone", "--jobs=bad", source_arg, "zmin-bad-long"].as_slice(),
        ["clone", "--jobs", "bad", source_arg, "zmin-bad-long-space"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), dir.path(), args, "zmin"),
            command_any_output("git", dir.path(), args, "git"),
            "jobs validation mismatch for {args:?}"
        );
    }

    for (args, git_name, zmin_name) in [
        (
            ["clone", "-j", "-1"].as_slice(),
            "git-jobs-negative",
            "zmin-jobs-negative",
        ),
        (
            ["clone", "--jobs=2"].as_slice(),
            "git-jobs-positive",
            "zmin-jobs-positive",
        ),
    ] {
        let mut git_args = args.to_vec();
        git_args.extend_from_slice(&[source_arg, git_name]);
        let mut zmin_args = args.to_vec();
        zmin_args.extend_from_slice(&[source_arg, zmin_name]);
        assert_eq!(command_output("git", dir.path(), &git_args, "git").0, 0);
        assert_eq!(
            command_output(zmin_bin(), dir.path(), &zmin_args, "zmin").0,
            0
        );
        assert_eq!(
            visible_worktree_files(&dir.path().join(zmin_name)),
            visible_worktree_files(&dir.path().join(git_name)),
            "jobs success worktree mismatch for {args:?}"
        );
    }
}

#[test]
fn clone_recurse_submodules_order_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let submodule = submodule_child_repo();
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            submodule.path().to_str().expect("submodule path"),
            "deps/sub",
        ])
        .current_dir(&source)
        .output()
        .expect("git submodule add");
    assert!(
        output.status.success(),
        "git submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_with_env(&source, ["commit", "-m", "submodule"]);

    let source_arg = source.to_str().expect("source path");
    for (flags, git_name, zmin_name) in [
        (
            ["--recurse-submodules=.", "--no-recurse-submodules"].as_slice(),
            "git-no-recurse-last",
            "zmin-no-recurse-last",
        ),
        (
            ["--no-recurse-submodules", "--recurse-submodules=."].as_slice(),
            "git-recurse-last",
            "zmin-recurse-last",
        ),
        (
            [
                "--recurse-submodules=.",
                "--no-recurse-submodules",
                "--recurse-submodules=.",
            ]
            .as_slice(),
            "git-recurse-last-again",
            "zmin-recurse-last-again",
        ),
    ] {
        let mut git_args = vec!["-c", "protocol.file.allow=always", "clone"];
        git_args.extend_from_slice(flags);
        git_args.extend_from_slice(&[source_arg, git_name]);
        assert_eq!(
            command_output("git", dir.path(), &git_args, "git").0,
            0,
            "git clone failed for {flags:?}"
        );

        let mut zmin_args = vec!["clone"];
        zmin_args.extend_from_slice(flags);
        zmin_args.extend_from_slice(&[source_arg, zmin_name]);
        assert_eq!(
            command_output(zmin_bin(), dir.path(), &zmin_args, "zmin").0,
            0,
            "zmin clone failed for {flags:?}"
        );

        let git_clone = dir.path().join(git_name);
        let zmin_clone = dir.path().join(zmin_name);
        assert_eq!(
            visible_worktree_files(&zmin_clone),
            visible_worktree_files(&git_clone),
            "ordered recurse worktree mismatch for {flags:?}"
        );
        assert_eq!(
            command_any_output(
                zmin_bin(),
                &zmin_clone,
                &["config", "--get", "submodule.active"],
                "zmin"
            ),
            command_any_output(
                "git",
                &git_clone,
                &["config", "--get", "submodule.active"],
                "git"
            ),
            "ordered recurse config mismatch for {flags:?}"
        );
    }
}

#[test]
fn clone_recurse_submodules_handles_nested_pathspecs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let grandchild = submodule_child_repo_named("grand.txt", "grand\n");
    let child = dir.path().join("child");
    git(
        dir.path(),
        ["init", "-b", "main", child.to_str().expect("child path")],
    );
    configure_identity(&child);
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            grandchild.path().to_str().expect("grandchild path"),
            "nested/grand",
        ])
        .current_dir(&child)
        .output()
        .expect("git nested submodule add");
    assert!(
        output.status.success(),
        "git nested submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_with_env(&child, ["commit", "-m", "nested submodule"]);

    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            child.to_str().expect("child path"),
            "deps/child",
        ])
        .current_dir(&source)
        .output()
        .expect("git parent submodule add");
    assert!(
        output.status.success(),
        "git parent submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_with_env(&source, ["commit", "-m", "parent submodule"]);

    let source_arg = source.to_str().expect("source path");
    for (flag, git_name, zmin_name) in [
        (
            "--recurse-submodules",
            "git-nested-full",
            "zmin-nested-full",
        ),
        (
            "--recurse-submodules=deps/child",
            "git-nested-child",
            "zmin-nested-child",
        ),
        (
            "--recurse-submodules=deps/child/nested/grand",
            "git-nested-grand",
            "zmin-nested-grand",
        ),
    ] {
        let git_output = Command::new("git")
            .args([
                "-c",
                "protocol.file.allow=always",
                "clone",
                flag,
                source_arg,
                git_name,
            ])
            .current_dir(dir.path())
            .output()
            .expect("git nested recursive clone");
        assert!(
            git_output.status.success(),
            "git nested recursive clone failed: {}",
            String::from_utf8_lossy(&git_output.stderr)
        );
        run_zmin_args(dir.path(), &["clone", flag, source_arg, zmin_name]);

        let git_clone = dir.path().join(git_name);
        let zmin_clone = dir.path().join(zmin_name);
        assert_eq!(
            visible_worktree_files(&zmin_clone),
            visible_worktree_files(&git_clone),
            "nested recursive worktree mismatch for {flag}"
        );
        assert_eq!(
            run_zmin(&zmin_clone, ["submodule", "status", "--recursive"]),
            git(&git_clone, ["submodule", "status", "--recursive"]),
            "nested recursive status mismatch for {flag}"
        );
        assert_eq!(
            run_zmin(&zmin_clone, ["config", "--get", "submodule.active"]),
            git(&git_clone, ["config", "--get", "submodule.active"]),
            "nested recursive active config mismatch for {flag}"
        );
    }
}

#[test]
fn submodule_update_init_sync_foreach_deinit_match_stock_git_state() {
    let dir = TempDir::new().expect("temp dir");
    let submodule = submodule_child_repo();
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            submodule.path().to_str().expect("submodule path"),
            "deps/sub",
        ])
        .current_dir(&source)
        .output()
        .expect("git submodule add");
    assert!(
        output.status.success(),
        "git submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_with_env(&source, ["commit", "-m", "submodule"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-clone"],
    );
    run_zmin(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "zmin-clone"],
    );
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");

    assert_eq!(
        command_output(
            "git",
            &git_clone,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "update",
                "--init",
                "--quiet",
                "--depth",
                "1",
                "--single-branch",
                "--progress",
            ],
            "git",
        )
        .0,
        0
    );
    run_zmin(
        &zmin_clone,
        [
            "submodule",
            "update",
            "--init",
            "--quiet",
            "--depth",
            "1",
            "--single-branch",
            "--progress",
        ],
    );
    assert_eq!(
        visible_worktree_files(&zmin_clone),
        visible_worktree_files(&git_clone)
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["submodule", "status"]),
        git(&git_clone, ["submodule", "status"])
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "submodule.deps/sub.url"]),
        git(&git_clone, ["config", "--get", "submodule.deps/sub.url"])
    );

    git(&git_clone, ["config", "submodule.deps/sub.url", "wrong"]);
    run_zmin(&zmin_clone, ["config", "submodule.deps/sub.url", "wrong"]);
    git(&git_clone, ["submodule", "sync"]);
    run_zmin(&zmin_clone, ["submodule", "sync"]);
    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "submodule.deps/sub.url"]),
        git(&git_clone, ["config", "--get", "submodule.deps/sub.url"])
    );

    let foreach = r#"printf "%s|%s|%s|%s\n" "$name" "$path" "$displaypath" "$sha1""#;
    assert_eq!(
        run_zmin_args(&zmin_clone, &["submodule", "foreach", foreach]),
        git_args(&git_clone, &["submodule", "foreach", foreach])
    );

    git(&git_clone, ["submodule", "deinit", "-f", "deps/sub"]);
    run_zmin(&zmin_clone, ["submodule", "deinit", "-f", "deps/sub"]);
    assert_eq!(
        visible_worktree_files(&zmin_clone),
        visible_worktree_files(&git_clone)
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            &zmin_clone,
            &["config", "--get", "submodule.deps/sub.url"],
            "zmin",
        )
        .0,
        command_any_output(
            "git",
            &git_clone,
            &["config", "--get", "submodule.deps/sub.url"],
            "git",
        )
        .0
    );
}

#[test]
fn submodule_absorbgitdirs_moves_embedded_git_dir_like_stock_git() {
    let submodule = submodule_child_repo();
    let super_repo = git_init();
    configure_identity(super_repo.path());
    git(super_repo.path(), ["checkout", "-b", "main"]);

    run_zmin(
        super_repo.path(),
        [
            "submodule",
            "add",
            submodule.path().to_str().expect("submodule path"),
            "deps/sub",
        ],
    );
    assert!(super_repo.path().join("deps/sub/.git").is_dir());
    run_zmin(super_repo.path(), ["submodule", "absorbgitdirs"]);
    assert!(super_repo.path().join("deps/sub/.git").is_file());
    assert!(super_repo.path().join(".git/modules/deps/sub").is_dir());
    assert_eq!(
        run_zmin(super_repo.path(), ["submodule", "status"]),
        git(super_repo.path(), ["submodule", "status"])
    );
}

fn submodule_child_repo() -> TempDir {
    submodule_child_repo_named("sub.txt", "sub\n")
}

fn submodule_child_repo_named(path: &str, content: &str) -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), path, content);
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "sub"]);
    repo
}
