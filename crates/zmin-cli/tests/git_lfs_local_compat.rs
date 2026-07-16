mod common;

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use common::{
    command_any_output, command_any_output_with_stdin, command_stdout_bytes, configure_identity,
    git, git_init, run_zmin, stock_git_bin, zmin_bin,
};

#[test]
fn lfs_track_and_untrack_match_stock_git_for_basic_patterns() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let track_args = ["lfs", "track", "*.bin"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &track_args, "git lfs track"),
        command_any_output(zmin_bin(), zmin_repo.path(), &track_args, "zmin lfs track")
    );
    assert_eq!(
        fs::read_to_string(git_repo.path().join(".gitattributes")).expect("read git attributes"),
        fs::read_to_string(zmin_repo.path().join(".gitattributes")).expect("read zmin attributes")
    );

    assert_eq!(
        command_any_output(
            "git",
            git_repo.path(),
            &track_args,
            "git lfs track duplicate"
        ),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &track_args,
            "zmin lfs track duplicate"
        )
    );

    let untrack_args = ["lfs", "untrack", "*.bin"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &untrack_args, "git lfs untrack"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &untrack_args,
            "zmin lfs untrack"
        )
    );
    assert_eq!(
        fs::read_to_string(git_repo.path().join(".gitattributes")).expect("read git attributes"),
        fs::read_to_string(zmin_repo.path().join(".gitattributes")).expect("read zmin attributes")
    );
}

#[test]
fn lfs_install_local_skip_repo_matches_stock_git_filter_config() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let args = ["lfs", "install", "--local", "--skip-repo"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs install"),
        command_any_output(zmin_bin(), zmin_repo.path(), &args, "zmin lfs install")
    );
    for key in [
        "lfs.repositoryformatversion",
        "filter.lfs.required",
        "filter.lfs.clean",
        "filter.lfs.smudge",
        "filter.lfs.process",
    ] {
        assert_eq!(
            git(git_repo.path(), ["config", "--local", "--get", key]),
            run_zmin(zmin_repo.path(), ["config", "--local", "--get", key]),
            "{key}"
        );
    }
    assert!(!zmin_repo.path().join(".git/hooks/pre-push").exists());
}

#[test]
fn lfs_install_local_skip_smudge_matches_stock_git_hook_side_effects() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let args = ["lfs", "install", "--local", "--skip-smudge"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs install skip-smudge"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs install skip-smudge"
        )
    );
    for key in [
        "lfs.repositoryformatversion",
        "filter.lfs.required",
        "filter.lfs.clean",
        "filter.lfs.smudge",
        "filter.lfs.process",
    ] {
        assert_eq!(
            git(git_repo.path(), ["config", "--local", "--get", key]),
            run_zmin(zmin_repo.path(), ["config", "--local", "--get", key]),
            "{key}"
        );
    }
    let hook = fs::read_to_string(zmin_repo.path().join(".git/hooks/pre-push"))
        .expect("read zmin pre-push hook");
    assert!(hook.contains("# zmin-lfs-pre-push"));
    assert!(hook.contains("lfs pre-push \"$@\""));
    for hook_name in ["post-checkout", "post-commit", "post-merge"] {
        assert_eq!(
            git_repo.path().join(".git/hooks").join(hook_name).exists(),
            zmin_repo.path().join(".git/hooks").join(hook_name).exists(),
            "{hook_name}"
        );
    }
}

#[test]
fn lfs_install_worktree_matches_stock_git_current_side_effects() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let args = ["lfs", "install", "--worktree"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs install worktree"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs install worktree"
        )
    );
    for key in [
        "lfs.repositoryformatversion",
        "filter.lfs.required",
        "filter.lfs.clean",
        "filter.lfs.smudge",
        "filter.lfs.process",
    ] {
        assert_eq!(
            git(git_repo.path(), ["config", "--local", "--get", key]),
            run_zmin(zmin_repo.path(), ["config", "--local", "--get", key]),
            "{key}"
        );
    }
    assert_eq!(
        git_repo.path().join(".git/config.worktree").exists(),
        zmin_repo.path().join(".git/config.worktree").exists()
    );
    let hook = fs::read_to_string(zmin_repo.path().join(".git/hooks/pre-push"))
        .expect("read zmin pre-push hook");
    assert!(hook.contains("# zmin-lfs-pre-push"));
    assert!(hook.contains("lfs pre-push \"$@\""));
    for hook_name in ["post-checkout", "post-commit", "post-merge"] {
        assert_eq!(
            git_repo.path().join(".git/hooks").join(hook_name).exists(),
            zmin_repo.path().join(".git/hooks").join(hook_name).exists(),
            "{hook_name}"
        );
    }
}

#[test]
fn lfs_install_worktree_skip_smudge_matches_stock_git_current_side_effects() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let args = ["lfs", "install", "--worktree", "--skip-smudge"];
    assert_eq!(
        command_any_output(
            "git",
            git_repo.path(),
            &args,
            "git lfs install worktree skip-smudge"
        ),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs install worktree skip-smudge"
        )
    );
    for key in [
        "lfs.repositoryformatversion",
        "filter.lfs.required",
        "filter.lfs.clean",
        "filter.lfs.smudge",
        "filter.lfs.process",
    ] {
        assert_eq!(
            git(git_repo.path(), ["config", "--local", "--get", key]),
            run_zmin(zmin_repo.path(), ["config", "--local", "--get", key]),
            "{key}"
        );
    }
    let hook = fs::read_to_string(zmin_repo.path().join(".git/hooks/pre-push"))
        .expect("read zmin pre-push hook");
    assert!(hook.contains("# zmin-lfs-pre-push"));
    assert!(hook.contains("lfs pre-push \"$@\""));
    for hook_name in ["post-checkout", "post-commit", "post-merge"] {
        assert_eq!(
            git_repo.path().join(".git/hooks").join(hook_name).exists(),
            zmin_repo.path().join(".git/hooks").join(hook_name).exists(),
            "{hook_name}"
        );
    }
}

#[test]
fn lfs_install_respects_relative_core_hookspath_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    git(git_repo.path(), ["config", "core.hooksPath", ".githooks"]);
    run_zmin(zmin_repo.path(), ["config", "core.hooksPath", ".githooks"]);

    let args = ["lfs", "install", "--local", "--skip-smudge"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs install hookspath"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs install hookspath"
        )
    );

    for hook_name in ["pre-push", "post-checkout", "post-commit", "post-merge"] {
        assert_eq!(
            git_repo.path().join(".githooks").join(hook_name).exists(),
            zmin_repo.path().join(".githooks").join(hook_name).exists(),
            "{hook_name}"
        );
        assert_eq!(
            git_repo.path().join(".git/hooks").join(hook_name).exists(),
            zmin_repo.path().join(".git/hooks").join(hook_name).exists(),
            "{hook_name} .git/hooks"
        );
    }
    let hook = fs::read_to_string(zmin_repo.path().join(".githooks/pre-push"))
        .expect("read zmin hookspath pre-push hook");
    assert!(hook.contains("# zmin-lfs-pre-push"));
    assert!(hook.contains("lfs pre-push \"$@\""));
}

#[test]
fn lfs_install_manual_modes_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for args in [
        ["lfs", "install", "--manual"].as_slice(),
        ["lfs", "install", "--manual", "--skip-smudge"].as_slice(),
        ["lfs", "install", "--manual", "--local"].as_slice(),
    ] {
        assert_eq!(
            command_any_output("git", git_repo.path(), args, "git lfs install manual"),
            command_any_output(
                zmin_bin(),
                zmin_repo.path(),
                args,
                "zmin lfs install manual"
            ),
            "{args:?}"
        );
    }
}

#[test]
fn lfs_install_manual_respects_relative_core_hookspath_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    git(git_repo.path(), ["config", "core.hooksPath", ".githooks"]);
    run_zmin(zmin_repo.path(), ["config", "core.hooksPath", ".githooks"]);

    let args = ["lfs", "install", "--manual"];
    assert_eq!(
        command_any_output(
            "git",
            git_repo.path(),
            &args,
            "git lfs install manual hookspath"
        ),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs install manual hookspath"
        )
    );
}

#[test]
fn lfs_install_invalid_flag_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let args = ["lfs", "install", "--bogus"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs install invalid"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs install invalid"
        )
    );
}

#[test]
fn lfs_install_ignores_extra_positional_argument_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let args = ["lfs", "install", "oops"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs install extra arg"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs install extra arg"
        )
    );
}

#[test]
fn lfs_install_manual_modes_do_not_depend_on_stock_git_runtime() {
    let repo = git_init();
    let poisoned_envs = &[
        ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
        ("GIT_BIN", "/definitely/missing/git"),
    ];

    let manual =
        zmin_lfs_any_output_with_env(repo.path(), &["lfs", "install", "--manual"], poisoned_envs);
    assert_eq!(manual.0, 0);
    assert!(
        manual
            .1
            .contains("Add the following to '.git/hooks/pre-push':")
    );
    assert!(manual.1.contains("git lfs pre-push \"$@\""));
    assert!(manual.1.ends_with("Git LFS initialized."));
    assert_eq!(manual.2, "");
}

#[test]
fn lfs_update_and_manual_modes_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for args in [
        ["lfs", "update"].as_slice(),
        ["lfs", "update", "--force"].as_slice(),
        ["lfs", "update", "--manual"].as_slice(),
        ["lfs", "update", "-m"].as_slice(),
        ["lfs", "update", "-f"].as_slice(),
    ] {
        assert_eq!(
            command_any_output("git", git_repo.path(), args, "git lfs update"),
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin lfs update"),
            "{args:?}"
        );
    }
}

#[test]
fn lfs_update_respects_relative_core_hookspath_and_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    git(git_repo.path(), ["config", "core.hooksPath", ".githooks"]);
    run_zmin(zmin_repo.path(), ["config", "core.hooksPath", ".githooks"]);

    for args in [
        ["lfs", "update"].as_slice(),
        ["lfs", "update", "--manual"].as_slice(),
    ] {
        assert_eq!(
            command_any_output("git", git_repo.path(), args, "git lfs update hookspath"),
            command_any_output(
                zmin_bin(),
                zmin_repo.path(),
                args,
                "zmin lfs update hookspath"
            ),
            "{args:?}"
        );
    }
}

#[test]
fn lfs_update_invalid_flag_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let args = ["lfs", "update", "--bogus"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs update invalid"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs update invalid"
        )
    );
}

#[test]
fn lfs_ls_files_default_name_only_and_long_match_stock_git_for_pointer_entries() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        fs::write(
            repo.join("a.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:1111111111111111111111111111111111111111111111111111111111111111\nsize 3\n",
        )
        .expect("write pointer");
        git(repo, ["add", ".gitattributes", "a.bin"]);
        git(repo, ["commit", "-m", "base"]);
    }

    for args in [
        ["lfs", "ls-files"].as_slice(),
        ["lfs", "ls-files", "--name-only"].as_slice(),
        ["lfs", "ls-files", "--long"].as_slice(),
        ["lfs", "ls-files", "--size"].as_slice(),
    ] {
        assert_eq!(
            command_stdout_bytes("git", git_repo.path(), args),
            command_stdout_bytes(zmin_bin(), zmin_repo.path(), args),
            "{args:?}"
        );
    }
}

#[test]
fn lfs_ls_files_ref_argument_matches_stock_git_for_historical_pointer_tree() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        fs::write(
            repo.join("a.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:1111111111111111111111111111111111111111111111111111111111111111\nsize 3\n",
        )
        .expect("write base pointer");
        git(repo, ["add", ".gitattributes", "a.bin"]);
        git(repo, ["commit", "-m", "base"]);

        fs::write(
            repo.join("b.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:2222222222222222222222222222222222222222222222222222222222222222\nsize 4\n",
        )
        .expect("write second pointer");
        git(repo, ["add", "b.bin"]);
        git(repo, ["commit", "-m", "second"]);
    }

    for args in [
        ["lfs", "ls-files", "HEAD~1"].as_slice(),
        ["lfs", "ls-files", "--name-only", "HEAD~1"].as_slice(),
        ["lfs", "ls-files", "--long", "HEAD"].as_slice(),
        ["lfs", "ls-files", "--size", "HEAD~1"].as_slice(),
    ] {
        assert_eq!(
            command_stdout_bytes("git", git_repo.path(), args),
            command_stdout_bytes(zmin_bin(), zmin_repo.path(), args),
            "{args:?}"
        );
    }
}

#[test]
fn lfs_ls_files_two_ref_diff_matches_stock_git_for_changed_lfs_entries() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        fs::write(
            repo.join("a.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:1111111111111111111111111111111111111111111111111111111111111111\nsize 3\n",
        )
        .expect("write base pointer");
        git(repo, ["add", ".gitattributes", "a.bin"]);
        git(repo, ["commit", "-m", "base"]);

        fs::write(
            repo.join("a.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:2222222222222222222222222222222222222222222222222222222222222222\nsize 4\n",
        )
        .expect("write modified pointer");
        fs::write(
            repo.join("b.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:3333333333333333333333333333333333333333333333333333333333333333\nsize 5\n",
        )
        .expect("write added pointer");
        git(repo, ["add", "a.bin", "b.bin"]);
        git(repo, ["commit", "-m", "second"]);

        fs::remove_file(repo.join("b.bin")).expect("remove pointer");
        git(repo, ["rm", "b.bin"]);
        git(repo, ["commit", "-m", "third"]);
    }

    for args in [
        ["lfs", "ls-files", "HEAD~1", "HEAD"].as_slice(),
        ["lfs", "ls-files", "HEAD~2", "HEAD~1"].as_slice(),
        ["lfs", "ls-files", "HEAD~2", "HEAD"].as_slice(),
        ["lfs", "ls-files", "--name-only", "HEAD~2", "HEAD"].as_slice(),
        ["lfs", "ls-files", "--long", "HEAD~2", "HEAD"].as_slice(),
        ["lfs", "ls-files", "--size", "HEAD~2", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_stdout_bytes("git", git_repo.path(), args),
            command_stdout_bytes(zmin_bin(), zmin_repo.path(), args),
            "{args:?}"
        );
    }
}

#[test]
fn lfs_ls_files_history_modes_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        fs::write(
            repo.join("a.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:1111111111111111111111111111111111111111111111111111111111111111\nsize 3\n",
        )
        .expect("write base pointer");
        git(repo, ["add", ".gitattributes", "a.bin"]);
        git(repo, ["commit", "-m", "base"]);

        fs::write(
            repo.join("a.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:2222222222222222222222222222222222222222222222222222222222222222\nsize 4\n",
        )
        .expect("write modified pointer");
        fs::write(
            repo.join("b.bin"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:3333333333333333333333333333333333333333333333333333333333333333\nsize 5\n",
        )
        .expect("write added pointer");
        git(repo, ["add", "a.bin", "b.bin"]);
        git(repo, ["commit", "-m", "second"]);

        fs::remove_file(repo.join("b.bin")).expect("remove pointer");
        git(repo, ["rm", "b.bin"]);
        git(repo, ["commit", "-m", "third"]);
    }

    for args in [
        ["lfs", "ls-files", "--all"].as_slice(),
        ["lfs", "ls-files", "--deleted"].as_slice(),
        ["lfs", "ls-files", "--deleted", "HEAD"].as_slice(),
        ["lfs", "ls-files", "--deleted", "HEAD~1"].as_slice(),
        ["lfs", "ls-files", "--debug"].as_slice(),
        ["lfs", "ls-files", "--json"].as_slice(),
        ["lfs", "ls-files", "--all", "--json"].as_slice(),
        ["lfs", "ls-files", "--deleted", "--json"].as_slice(),
        ["lfs", "ls-files", "--all", "--name-only"].as_slice(),
        ["lfs", "ls-files", "--deleted", "--name-only"].as_slice(),
    ] {
        assert_eq!(
            command_stdout_bytes("git", git_repo.path(), args),
            command_stdout_bytes(zmin_bin(), zmin_repo.path(), args),
            "{args:?}"
        );
    }
}

#[test]
fn lfs_ls_files_invalid_flag_and_history_conflicts_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for args in [
        ["lfs", "ls-files", "--cached"].as_slice(),
        ["lfs", "ls-files", "--modified"].as_slice(),
        ["lfs", "ls-files", "--all", "HEAD"].as_slice(),
        ["lfs", "ls-files", "--deleted", "HEAD~1", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(
                "git",
                git_repo.path(),
                args,
                "git lfs ls-files invalid/history"
            ),
            command_any_output(
                zmin_bin(),
                zmin_repo.path(),
                args,
                "zmin lfs ls-files invalid/history"
            ),
            "{args:?}"
        );
    }
}

#[test]
fn lfs_ls_files_invalid_ref_matches_stock_git_error_shape() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let stock_git = stock_git_bin().to_str().expect("stock git path");
    let args = ["lfs", "ls-files", "nosuch"];
    assert_eq!(
        command_any_output(
            stock_git,
            git_repo.path(),
            &args,
            "git lfs ls-files invalid ref"
        ),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs ls-files invalid ref"
        )
    );
}

#[test]
fn lfs_version_and_env_report_builtin_local_foundation_state() {
    let repo = git_init();

    let version = run_zmin(repo.path(), ["lfs", "version"]);
    assert!(version.starts_with("git-lfs/zmin (zmin "));
    assert!(version.contains("built-in local foundation"));

    run_zmin(repo.path(), ["lfs", "install", "--local", "--skip-smudge"]);
    let env = run_zmin(repo.path(), ["lfs", "env"]);
    let repo_path = fs::canonicalize(repo.path()).expect("canonicalize repo path");

    assert!(env.contains("git-lfs/zmin (zmin "));
    assert!(env.contains("git version "));
    assert!(env.contains(&format!("LocalWorkingDir={}", repo_path.display())));
    assert!(env.contains(&format!("LocalGitDir={}", repo_path.join(".git").display())));
    assert!(env.contains(&format!(
        "LocalMediaDir={}",
        repo_path.join(".git/lfs/objects").display()
    )));
    assert!(env.contains("git config lfs.repositoryformatversion = 0"));
    assert!(env.contains("git config filter.lfs.process = git-lfs filter-process --skip"));
    assert!(env.contains("git config filter.lfs.smudge = git-lfs smudge --skip -- %f"));
    assert!(env.contains("git config filter.lfs.clean = git-lfs clean -- %f"));
    assert!(env.contains("git config filter.lfs.required = true"));
}

#[test]
fn lfs_pre_push_validates_update_stream_shape() {
    let repo = git_init();
    git(repo.path(), ["remote", "add", "origin", "."]);

    let valid = "refs/heads/main 1111111111111111111111111111111111111111 refs/heads/main 0000000000000000000000000000000000000000\n";
    let (code, stdout, stderr) = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "pre-push", "origin", "."],
        valid,
        "zmin lfs pre-push valid",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "");

    let (code, stdout, stderr) = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "pre-push", "origin", "."],
        "oops\n",
        "zmin lfs pre-push invalid line",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "");

    let (code, stdout, stderr) = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "pre-push"],
        "",
        "zmin lfs pre-push usage",
    );
    assert_eq!(code, 1);
    assert_eq!(
        stdout,
        "This should be run through Git's pre-push hook.  Run `git lfs update` to install it."
    );
    assert_eq!(stderr, "");
}

#[test]
fn lfs_pre_push_missing_remote_matches_stock_git_status_shape() {
    let repo = git_init();

    let stock = command_any_output_with_stdin(
        stock_git_bin().to_str().expect("stock git bin path"),
        repo.path(),
        &["lfs", "pre-push", "origin", "."],
        "oops\n",
        "stock git lfs pre-push missing remote",
    );
    let zmin = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "pre-push", "origin", "."],
        "oops\n",
        "zmin git lfs pre-push missing remote",
    );
    assert_eq!(zmin.0, stock.0);
}

#[test]
fn lfs_standard_hook_callbacks_match_stock_git_local_foundation_shape() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for args in [
        ["lfs", "post-commit"].as_slice(),
        ["lfs", "post-commit", "1"].as_slice(),
        ["lfs", "post-merge"].as_slice(),
        ["lfs", "post-merge", "1"].as_slice(),
        [
            "lfs",
            "post-checkout",
            "1111111111111111111111111111111111111111",
            "2222222222222222222222222222222222222222",
            "1",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_any_output("git", git_repo.path(), args, "git lfs hook callback"),
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin lfs hook callback"),
            "{args:?}"
        );
    }

    for args in [
        ["lfs", "post-checkout"].as_slice(),
        ["lfs", "post-checkout", "a", "b"].as_slice(),
        ["lfs", "post-checkout", "a", "b", "c", "d"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(
                "git",
                git_repo.path(),
                args,
                "git lfs hook callback invalid"
            ),
            command_any_output(
                zmin_bin(),
                zmin_repo.path(),
                args,
                "zmin lfs hook callback invalid"
            ),
            "{args:?}"
        );
    }
}

fn write_lfs_pointer(path: &std::path::Path, oid: &str, size: u64) {
    fs::write(
        path,
        format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n"),
    )
    .expect("write lfs pointer");
}

#[test]
fn lfs_checkout_without_args_materializes_local_objects_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    let oid = "1111111111111111111111111111111111111111111111111111111111111111";
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        for name in ["a.bin", "b.bin"] {
            write_lfs_pointer(&repo.join(name), oid, 9);
        }
        git(repo, ["add", ".gitattributes", "a.bin", "b.bin"]);
        git(repo, ["commit", "-m", "base"]);
        fs::create_dir_all(repo.join(format!(".git/lfs/objects/{}/{}/", &oid[..2], &oid[2..4])))
            .expect("create lfs dir");
        fs::write(
            repo.join(format!(
                ".git/lfs/objects/{}/{}/{}",
                &oid[..2],
                &oid[2..4],
                oid
            )),
            b"REALDATA\n",
        )
        .expect("write local lfs object");
        for name in ["a.bin", "b.bin"] {
            write_lfs_pointer(&repo.join(name), oid, 9);
        }
    }

    let args = ["lfs", "checkout"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs checkout"),
        command_any_output(zmin_bin(), zmin_repo.path(), &args, "zmin lfs checkout")
    );

    assert_eq!(
        fs::read(git_repo.path().join("a.bin")).expect("read git a.bin"),
        fs::read(zmin_repo.path().join("a.bin")).expect("read zmin a.bin")
    );
    assert_eq!(
        fs::read(git_repo.path().join("b.bin")).expect("read git b.bin"),
        fs::read(zmin_repo.path().join("b.bin")).expect("read zmin b.bin")
    );
}

#[test]
fn lfs_checkout_path_argument_materializes_only_requested_file_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    let oid = "1111111111111111111111111111111111111111111111111111111111111111";
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        write_lfs_pointer(&repo.join("a.bin"), oid, 9);
        write_lfs_pointer(&repo.join("b.bin"), oid, 9);
        git(repo, ["add", ".gitattributes", "a.bin", "b.bin"]);
        git(repo, ["commit", "-m", "base"]);
        fs::create_dir_all(repo.join(format!(".git/lfs/objects/{}/{}/", &oid[..2], &oid[2..4])))
            .expect("create lfs dir");
        fs::write(
            repo.join(format!(
                ".git/lfs/objects/{}/{}/{}",
                &oid[..2],
                &oid[2..4],
                oid
            )),
            b"REALDATA\n",
        )
        .expect("write local lfs object");
        write_lfs_pointer(&repo.join("a.bin"), oid, 9);
        write_lfs_pointer(&repo.join("b.bin"), oid, 9);
    }

    let args = ["lfs", "checkout", "a.bin"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs checkout path"),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs checkout path"
        )
    );

    assert_eq!(
        fs::read(git_repo.path().join("a.bin")).expect("read git a.bin"),
        fs::read(zmin_repo.path().join("a.bin")).expect("read zmin a.bin")
    );
    assert_eq!(
        fs::read(git_repo.path().join("b.bin")).expect("read git b.bin"),
        fs::read(zmin_repo.path().join("b.bin")).expect("read zmin b.bin")
    );
}

#[test]
fn lfs_checkout_missing_local_object_matches_stock_git_shape() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    let oid = "1111111111111111111111111111111111111111111111111111111111111111";
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        write_lfs_pointer(&repo.join("a.bin"), oid, 9);
        git(repo, ["add", ".gitattributes", "a.bin"]);
        git(repo, ["commit", "-m", "base"]);
        write_lfs_pointer(&repo.join("a.bin"), oid, 9);
    }

    let args = ["lfs", "checkout"];
    assert_eq!(
        command_any_output(
            "git",
            git_repo.path(),
            &args,
            "git lfs checkout missing local"
        ),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &args,
            "zmin lfs checkout missing local"
        )
    );
}

#[test]
fn lfs_pull_materializes_local_objects_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    let oid = "2320bb89b3d79a57fe63ff8d2072dcc184c6d8df1869b975279b759e5845e009";
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write attributes");
        write_lfs_pointer(&repo.join("a.bin"), oid, 9);
        git(repo, ["add", ".gitattributes", "a.bin"]);
        git(repo, ["commit", "-m", "base"]);
        fs::create_dir_all(repo.join(format!(".git/lfs/objects/{}/{}/", &oid[..2], &oid[2..4])))
            .expect("create lfs dir");
        fs::write(
            repo.join(format!(
                ".git/lfs/objects/{}/{}/{}",
                &oid[..2],
                &oid[2..4],
                oid
            )),
            b"REALDATA\n",
        )
        .expect("write local lfs object");
        write_lfs_pointer(&repo.join("a.bin"), oid, 9);
    }

    let args = ["lfs", "pull"];
    assert_eq!(
        command_any_output("git", git_repo.path(), &args, "git lfs pull"),
        command_any_output(zmin_bin(), zmin_repo.path(), &args, "zmin lfs pull")
    );
    assert_eq!(
        fs::read(git_repo.path().join("a.bin")).expect("read git a.bin"),
        fs::read(zmin_repo.path().join("a.bin")).expect("read zmin a.bin")
    );
}

#[test]
fn lfs_pull_fetches_from_local_remote_like_stock_git() {
    let root = tempfile::TempDir::new().expect("temp root");
    let remote = root.path().join("remote");
    let git_clone = root.path().join("git-clone");
    let zmin_clone = root.path().join("zmin-clone");
    let oid = "2320bb89b3d79a57fe63ff8d2072dcc184c6d8df1869b975279b759e5845e009";

    git(root.path(), ["init", "remote"]);
    configure_identity(&remote);
    fs::write(
        remote.join(".gitattributes"),
        b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("write remote attributes");
    write_lfs_pointer(&remote.join("a.bin"), oid, 9);
    git(&remote, ["add", ".gitattributes", "a.bin"]);
    git(&remote, ["commit", "-m", "base"]);
    fs::create_dir_all(remote.join(format!(".git/lfs/objects/{}/{}/", &oid[..2], &oid[2..4])))
        .expect("create remote lfs dir");
    fs::write(
        remote.join(format!(
            ".git/lfs/objects/{}/{}/{}",
            &oid[..2],
            &oid[2..4],
            oid
        )),
        b"REALDATA\n",
    )
    .expect("write remote lfs object");

    git(
        root.path(),
        [
            "clone",
            remote.to_str().expect("remote path"),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    let clone_out = command_any_output(
        zmin_bin(),
        root.path(),
        &[
            "clone",
            remote.to_str().expect("remote path"),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
        "zmin clone",
    );
    assert_eq!(clone_out.0, 0, "zmin clone stderr: {}", clone_out.2);

    for repo in [&git_clone, &zmin_clone] {
        configure_identity(repo);
        write_lfs_pointer(&repo.join("a.bin"), oid, 9);
        let local_media = repo.join(format!(
            ".git/lfs/objects/{}/{}/{}",
            &oid[..2],
            &oid[2..4],
            oid
        ));
        if local_media.exists() {
            fs::remove_file(local_media).expect("remove local lfs media");
        }
    }

    let args = ["lfs", "pull", "origin"];
    assert_eq!(
        command_any_output("git", &git_clone, &args, "git lfs pull origin"),
        command_any_output(zmin_bin(), &zmin_clone, &args, "zmin lfs pull origin")
    );
    assert_eq!(
        fs::read(git_clone.join("a.bin")).expect("read git pulled file"),
        fs::read(zmin_clone.join("a.bin")).expect("read zmin pulled file")
    );
}

fn zmin_lfs_any_output_with_env(
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> (i32, String, String) {
    let output = Command::new(zmin_bin())
        .args(args)
        .current_dir(cwd)
        .envs(envs.iter().copied())
        .output()
        .expect("run zmin lfs command");
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

fn zmin_lfs_any_output_with_env_and_stdin(
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
    stdin: &str,
) -> (i32, String, String) {
    let mut child = Command::new(zmin_bin())
        .args(args)
        .current_dir(cwd)
        .envs(envs.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zmin lfs command");
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait for zmin lfs command");
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

#[test]
fn lfs_local_foundation_commands_do_not_depend_on_stock_git_runtime() {
    let repo = git_init();
    configure_identity(repo.path());
    let poisoned_envs = &[
        ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
        ("GIT_BIN", "/definitely/missing/git"),
    ];

    fs::write(
        repo.path().join(".gitattributes"),
        b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .expect("write attributes");
    fs::write(
        repo.path().join("a.bin"),
        b"version https://git-lfs.github.com/spec/v1\noid sha256:1111111111111111111111111111111111111111111111111111111111111111\nsize 3\n",
    )
    .expect("write pointer");
    git(repo.path(), ["add", ".gitattributes", "a.bin"]);
    git(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["remote", "add", "origin", "."]);

    let install = zmin_lfs_any_output_with_env(
        repo.path(),
        &["lfs", "install", "--local", "--skip-smudge"],
        poisoned_envs,
    );
    assert_eq!(install.0, 0);
    assert_eq!(install.2, "");

    let version = zmin_lfs_any_output_with_env(repo.path(), &["lfs", "version"], poisoned_envs);
    assert_eq!(version.0, 0);
    assert!(version.1.starts_with("git-lfs/zmin (zmin "));
    assert_eq!(version.2, "");

    let env = zmin_lfs_any_output_with_env(repo.path(), &["lfs", "env"], poisoned_envs);
    assert_eq!(env.0, 0);
    assert!(env.1.contains("git-lfs/zmin (zmin "));
    assert!(env.1.contains("git version 2.47.1.zmin"));
    assert!(env.1.contains("LocalWorkingDir="));
    assert_eq!(env.2, "");

    let ls_files = zmin_lfs_any_output_with_env(
        repo.path(),
        &["lfs", "ls-files", "--name-only"],
        poisoned_envs,
    );
    assert_eq!(ls_files.0, 0);
    assert_eq!(ls_files.1, "a.bin");
    assert_eq!(ls_files.2, "");

    fs::create_dir_all(repo.path().join(".git/lfs/objects/11/11")).expect("create lfs object dir");
    fs::write(
        repo.path()
            .join(".git/lfs/objects/11/11/1111111111111111111111111111111111111111111111111111111111111111"),
        b"abc",
    )
    .expect("write local lfs media");
    let checkout = zmin_lfs_any_output_with_env(repo.path(), &["lfs", "checkout"], poisoned_envs);
    assert_eq!(checkout.0, 0);
    assert!(checkout.1.contains("Checking out LFS objects: 100%"));
    assert_eq!(checkout.2, "");
    assert_eq!(
        fs::read(repo.path().join("a.bin")).expect("read checked out file"),
        b"abc"
    );

    write_lfs_pointer(
        &repo.path().join("a.bin"),
        "2320bb89b3d79a57fe63ff8d2072dcc184c6d8df1869b975279b759e5845e009",
        3,
    );
    fs::create_dir_all(repo.path().join(".git/lfs/objects/23/20"))
        .expect("create second lfs object dir");
    fs::write(
        repo.path()
            .join(".git/lfs/objects/23/20/2320bb89b3d79a57fe63ff8d2072dcc184c6d8df1869b975279b759e5845e009"),
        b"abc",
    )
    .expect("write second local lfs media");
    let pull = zmin_lfs_any_output_with_env(repo.path(), &["lfs", "pull"], poisoned_envs);
    assert_eq!(pull.0, 0);
    assert_eq!(pull.1, "");
    assert_eq!(pull.2, "");
    assert_eq!(
        fs::read(repo.path().join("a.bin")).expect("read pulled file"),
        b"abc"
    );

    for args in [
        ["lfs", "post-commit"].as_slice(),
        [
            "lfs",
            "post-checkout",
            "1111111111111111111111111111111111111111",
            "2222222222222222222222222222222222222222",
            "1",
        ]
        .as_slice(),
        ["lfs", "post-merge", "1"].as_slice(),
    ] {
        let output = zmin_lfs_any_output_with_env(repo.path(), args, poisoned_envs);
        assert_eq!(output.0, 0, "{args:?}");
        assert_eq!(output.2, "", "{args:?}");
    }

    let pre_push = zmin_lfs_any_output_with_env_and_stdin(
        repo.path(),
        &["lfs", "pre-push", "origin", "."],
        poisoned_envs,
        "refs/heads/main 1111111111111111111111111111111111111111 refs/heads/main 0000000000000000000000000000000000000000\n",
    );
    assert_eq!(pre_push.0, 0);
    assert_eq!(pre_push.1, "");
    assert_eq!(pre_push.2, "");
}

#[test]
fn lfs_install_reuses_existing_stock_style_lfs_hooks_without_stock_git_runtime() {
    let repo = git_init();
    configure_identity(repo.path());
    let poisoned_envs = &[
        ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
        ("GIT_BIN", "/definitely/missing/git"),
    ];

    let hooks_dir = repo.path().join(".git/hooks");
    fs::create_dir_all(&hooks_dir).expect("create hooks dir");
    for (hook_name, subcommand) in [
        ("pre-push", "pre-push"),
        ("post-checkout", "post-checkout"),
        ("post-commit", "post-commit"),
        ("post-merge", "post-merge"),
    ] {
        fs::write(
            hooks_dir.join(hook_name),
            format!(
                "#!/bin/sh\ncommand -v git-lfs >/dev/null 2>&1 || {{ printf >&2 \"\\n%s\\n\\n\" \"This repository is configured for Git LFS but 'git-lfs' was not found on your path. If you no longer wish to use Git LFS, remove this hook by deleting the '{hook_name}' file in the hooks directory (set by 'core.hookspath'; usually '.git/hooks').\"; exit 2; }}\ngit lfs {subcommand} \"$@\"\n"
            ),
        )
        .expect("write stock-style lfs hook");
    }

    let install = zmin_lfs_any_output_with_env(
        repo.path(),
        &["lfs", "install", "--local", "--skip-smudge"],
        poisoned_envs,
    );
    assert_eq!(install.0, 0);
    assert_eq!(install.1, "Updated Git hooks.\nGit LFS initialized.");
    assert_eq!(install.2, "");

    let pre_push_hook = fs::read_to_string(hooks_dir.join("pre-push")).expect("read pre-push hook");
    assert!(pre_push_hook.contains("# zmin-lfs-pre-push"));
    assert!(pre_push_hook.contains("lfs pre-push \"$@\""));
}

#[test]
fn lfs_install_reuses_existing_stock_style_lfs_hooks_in_custom_hookspath_without_stock_git_runtime()
{
    let repo = git_init();
    configure_identity(repo.path());
    run_zmin(repo.path(), ["config", "core.hooksPath", ".githooks"]);
    let poisoned_envs = &[
        ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
        ("GIT_BIN", "/definitely/missing/git"),
    ];

    let hooks_dir = repo.path().join(".githooks");
    fs::create_dir_all(&hooks_dir).expect("create custom hooks dir");
    for (hook_name, subcommand) in [
        ("pre-push", "pre-push"),
        ("post-checkout", "post-checkout"),
        ("post-commit", "post-commit"),
        ("post-merge", "post-merge"),
    ] {
        fs::write(
            hooks_dir.join(hook_name),
            format!(
                "#!/bin/sh\ncommand -v git-lfs >/dev/null 2>&1 || {{ printf >&2 \"\\n%s\\n\\n\" \"This repository is configured for Git LFS but 'git-lfs' was not found on your path. If you no longer wish to use Git LFS, remove this hook by deleting the '{hook_name}' file in the hooks directory (set by 'core.hookspath'; usually '.git/hooks').\"; exit 2; }}\ngit lfs {subcommand} \"$@\"\n"
            ),
        )
        .expect("write stock-style custom lfs hook");
    }

    let install = zmin_lfs_any_output_with_env(
        repo.path(),
        &["lfs", "install", "--local", "--skip-smudge"],
        poisoned_envs,
    );
    assert_eq!(install.0, 0);
    assert_eq!(install.1, "Updated Git hooks.\nGit LFS initialized.");
    assert_eq!(install.2, "");

    let pre_push_hook =
        fs::read_to_string(hooks_dir.join("pre-push")).expect("read custom pre-push hook");
    assert!(pre_push_hook.contains("# zmin-lfs-pre-push"));
    assert!(pre_push_hook.contains("lfs pre-push \"$@\""));
    assert!(!repo.path().join(".git/hooks/pre-push").exists());
}

#[test]
fn lfs_update_modes_do_not_depend_on_stock_git_runtime() {
    let repo = git_init();
    let poisoned_envs = &[
        ("ZMIN_STOCK_GIT", "/definitely/missing/git"),
        ("GIT_BIN", "/definitely/missing/git"),
    ];

    let update = zmin_lfs_any_output_with_env(repo.path(), &["lfs", "update"], poisoned_envs);
    assert_eq!(update.0, 0);
    assert_eq!(update.1, "Updated Git hooks.");
    assert_eq!(update.2, "");

    let manual =
        zmin_lfs_any_output_with_env(repo.path(), &["lfs", "update", "--manual"], poisoned_envs);
    assert_eq!(manual.0, 0);
    assert!(
        manual
            .1
            .contains("Add the following to '.git/hooks/pre-push':")
    );
    assert!(manual.1.contains("git lfs pre-push \"$@\""));
    assert_eq!(manual.2, "");
}
