mod common;

use std::fs;

use common::{
    command_any_output, command_any_output_with_stdin, command_stdout_bytes, configure_identity,
    git, git_init, run_zmin, zmin_bin,
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
fn lfs_install_local_skip_smudge_writes_builtin_pre_push_hook() {
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
    assert_eq!(code, 1);
    assert_eq!(stdout, "");
    assert!(stderr.contains("invalid lfs pre-push update line: oops"));

    let (code, stdout, stderr) = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["lfs", "pre-push"],
        "",
        "zmin lfs pre-push usage",
    );
    assert_eq!(code, 1);
    assert_eq!(stdout, "");
    assert!(stderr.contains("usage: git lfs pre-push <remote> [remoteurl]"));
}
