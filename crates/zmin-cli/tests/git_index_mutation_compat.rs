mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    command_any_output, command_output, command_output_with_env, command_stdout_bytes,
    configure_identity, git, git_failure_output, git_init, git_status, git_with_env,
    git_with_stdin, run_zmin, run_zmin_failure_output, run_zmin_status, run_zmin_with_env,
    run_zmin_with_stdin, write_file, zmin_bin,
};

fn committed_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    run_zmin(repo.path(), ["add", "-A"]);
    run_zmin_with_env(repo.path(), ["commit", "-m", "initial"]);
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
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::create_dir_all(repo.join("dir")).expect("mkdir dir");
        write_file(repo, "dir/inside.txt", "inside\n");
        write_file(repo, "outside.txt", "outside\n");
    }
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "initial"]);

    fs::remove_file(git_repo.path().join("dir/inside.txt")).expect("remove git inside");
    fs::remove_file(zmin_repo.path().join("dir/inside.txt")).expect("remove zmin inside");
    fs::remove_file(git_repo.path().join("outside.txt")).expect("remove git outside");
    fs::remove_file(zmin_repo.path().join("outside.txt")).expect("remove zmin outside");

    git(git_repo.path(), ["add", "-A", "dir"]);
    run_zmin(zmin_repo.path(), ["add", "-A", "dir"]);

    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["diff", "--cached", "--name-status"]),
        git(git_repo.path(), ["diff", "--cached", "--name-status"])
    );
}

#[test]
fn add_force_stages_explicit_ignored_paths_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, ".gitignore", "ignored.txt\n");
        write_file(repo, "ignored.txt", "ignored\n");
    }

    git(git_repo.path(), ["add", "-f", "ignored.txt"]);
    run_zmin(zmin_repo.path(), ["add", "-f", "ignored.txt"]);

    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );
}

#[test]
fn add_rejects_explicit_ignored_paths_without_force_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, ".gitignore", "*.ig\n");
        write_file(repo, "a.if", "tracked\n");
        write_file(repo, "a.ig", "ignored\n");
    }

    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["add", "a.if", "a.ig"]),
        git_status(git_repo.path(), ["add", "a.if", "a.ig"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );
}

#[test]
fn add_honors_nested_gitignore_negation_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, ".gitignore", "*.ig\n");
        write_file(repo, "sub/.gitignore", "!dir/a.*\n");
        write_file(repo, "sub/a.ig", "ignored\n");
        write_file(repo, "sub/dir/a.ig", "tracked\n");
    }

    git(git_repo.path(), ["add", "sub/dir"]);
    run_zmin(zmin_repo.path(), ["add", "sub/dir"]);

    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );
}

#[test]
fn add_all_stages_mode_change_with_unchanged_content_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "script.sh", "#!/bin/sh\n");
    write_file(zmin_repo.path(), "script.sh", "#!/bin/sh\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "initial"]);

    make_executable(&git_repo.path().join("script.sh"));
    make_executable(&zmin_repo.path().join("script.sh"));
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);

    assert_eq!(
        git(zmin_repo.path(), ["diff", "--cached", "--summary"]),
        git(git_repo.path(), ["diff", "--cached", "--summary"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage", "script.sh"]),
        git(git_repo.path(), ["ls-files", "--stage", "script.sh"])
    );
}

#[cfg(unix)]
#[test]
fn add_respects_core_filemode_false_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "core.filemode", "false"]);
        write_file(repo, "script.sh", "#!/bin/sh\n");
        make_executable(&repo.join("script.sh"));
    }

    git(git_repo.path(), ["add", "script.sh"]);
    run_zmin(zmin_repo.path(), ["add", "script.sh"]);

    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage", "script.sh"]),
        git(git_repo.path(), ["ls-files", "--stage", "script.sh"])
    );
}

#[test]
fn add_preserves_index_symlink_mode_when_core_symlinks_false_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "core.symlinks", "false"]);
        let blob = git_with_stdin(repo, ["hash-object", "-w", "--stdin"], "foo");
        git(
            repo,
            [
                "update-index",
                "--add",
                "--cacheinfo",
                "120000",
                &blob,
                "xfoo1",
            ],
        );
        write_file(repo, "xfoo1", "foo");
    }

    git(git_repo.path(), ["add", "xfoo1"]);
    run_zmin(zmin_repo.path(), ["add", "xfoo1"]);

    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage", "xfoo1"]),
        git(git_repo.path(), ["ls-files", "--stage", "xfoo1"])
    );
}

#[cfg(unix)]
#[test]
fn add_nested_path_replaces_index_symlink_parent_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        std::os::unix::fs::symlink(".git", repo.join("a")).expect("create symlink");
    }
    git(git_repo.path(), ["add", "a"]);
    run_zmin(zmin_repo.path(), ["add", "a"]);
    git_with_env(git_repo.path(), ["commit", "-m", "symlink"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "symlink"]);

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::remove_file(repo.join("a")).expect("remove symlink");
        fs::create_dir_all(repo.join("a/hooks")).expect("create hooks");
        fs::write(repo.join("a/hooks/post-checkout"), b"hook\n").expect("write hook");
    }
    git(git_repo.path(), ["add", "a/hooks/post-checkout"]);
    run_zmin(zmin_repo.path(), ["add", "a/hooks/post-checkout"]);

    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["ls-files", "--stage", "a", "a/hooks/post-checkout"]
        ),
        git(
            git_repo.path(),
            ["ls-files", "--stage", "a", "a/hooks/post-checkout"]
        )
    );
}

#[test]
fn add_resolves_unmerged_entries_preserving_disabled_mode_bits_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let mut git_index_info = String::new();
    let mut zmin_index_info = String::new();
    for stage in 1..=3 {
        write_file(
            git_repo.path(),
            &format!("file-stage-{stage}"),
            &format!("file {stage}\n"),
        );
        write_file(
            zmin_repo.path(),
            &format!("file-stage-{stage}"),
            &format!("file {stage}\n"),
        );
        let git_file_blob = git(
            git_repo.path(),
            ["hash-object", "-w", &format!("file-stage-{stage}")],
        );
        let zmin_file_blob = git(
            zmin_repo.path(),
            ["hash-object", "-w", &format!("file-stage-{stage}")],
        );
        assert_eq!(zmin_file_blob, git_file_blob);
        let git_link_blob =
            git_with_stdin(git_repo.path(), ["hash-object", "-w", "--stdin"], "target");
        let zmin_link_blob =
            git_with_stdin(zmin_repo.path(), ["hash-object", "-w", "--stdin"], "target");
        assert_eq!(zmin_link_blob, git_link_blob);
        let file_mode = if stage == 1 { "100644" } else { "100755" };
        let symlink_mode = if stage == 1 { "100644" } else { "120000" };
        git_index_info.push_str(&format!("{file_mode} {git_file_blob} {stage}\tfile\n"));
        git_index_info.push_str(&format!(
            "{symlink_mode} {git_link_blob} {stage}\tsymlink\n"
        ));
        zmin_index_info.push_str(&format!("{file_mode} {zmin_file_blob} {stage}\tfile\n"));
        zmin_index_info.push_str(&format!(
            "{symlink_mode} {zmin_link_blob} {stage}\tsymlink\n"
        ));
    }
    git_with_stdin(
        git_repo.path(),
        ["update-index", "--index-info"],
        &git_index_info,
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        ["update-index", "--index-info"],
        &zmin_index_info,
    );
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "core.filemode", "false"]);
        git(repo, ["config", "core.symlinks", "false"]);
        write_file(repo, "file", "new\n");
        write_file(repo, "symlink", "new\n");
    }

    git(git_repo.path(), ["add", "file", "symlink"]);
    run_zmin(zmin_repo.path(), ["add", "file", "symlink"]);

    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage", "file", "symlink"]),
        git(git_repo.path(), ["ls-files", "--stage", "file", "symlink"])
    );
}

#[test]
fn add_refresh_updates_stat_after_read_tree_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "foo", "");
    }
    git(git_repo.path(), ["add", "foo"]);
    run_zmin(zmin_repo.path(), ["add", "foo"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "initial"]);
    git(git_repo.path(), ["read-tree", "HEAD"]);
    run_zmin(zmin_repo.path(), ["read-tree", "HEAD"]);

    assert_eq!(
        run_zmin(zmin_repo.path(), ["diff-index", "HEAD", "--", "foo"]),
        git(git_repo.path(), ["diff-index", "HEAD", "--", "foo"])
    );

    git(git_repo.path(), ["add", "--refresh", "--", "foo"]);
    run_zmin(zmin_repo.path(), ["add", "--refresh", "--", "foo"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["diff-index", "HEAD", "--", "foo"]),
        git(git_repo.path(), ["diff-index", "HEAD", "--", "foo"])
    );
}

#[test]
fn add_refresh_pathspec_leaves_other_stat_dirty_paths_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "bar", "\n");
        write_file(repo, "baz", "\n");
    }
    git(git_repo.path(), ["add", "bar", "baz"]);
    run_zmin(zmin_repo.path(), ["add", "bar", "baz"]);
    git(git_repo.path(), ["update-index", "--refresh", "bar", "baz"]);
    run_zmin(
        zmin_repo.path(),
        ["update-index", "--refresh", "bar", "baz"],
    );
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "initial"]);
    git(git_repo.path(), ["read-tree", "HEAD"]);
    run_zmin(zmin_repo.path(), ["read-tree", "HEAD"]);

    git(git_repo.path(), ["add", "--refresh", "bar"]);
    run_zmin(zmin_repo.path(), ["add", "--refresh", "bar"]);

    assert_eq!(
        run_zmin(zmin_repo.path(), ["diff-files", "--name-only"]),
        git(git_repo.path(), ["diff-files", "--name-only"])
    );
}

#[test]
fn add_all_stages_same_size_rewrite_after_reset_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "tracked.txt", "one\n");
        git(repo, ["add", "tracked.txt"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
        git(repo, ["reset", "--hard", "HEAD"]);
        write_file(repo, "tracked.txt", "two\n");
    }

    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);

    assert_eq!(
        git(zmin_repo.path(), ["diff", "--cached", "--", "tracked.txt"]),
        git(git_repo.path(), ["diff", "--cached", "--", "tracked.txt"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", ":tracked.txt"]),
        git(git_repo.path(), ["rev-parse", ":tracked.txt"])
    );
}

#[test]
fn add_refresh_reports_unmatched_pathspec_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "tracked.txt", "tracked\n");
        git(repo, ["add", "tracked.txt"]);
    }

    let zmin = run_zmin_failure_output(zmin_repo.path(), &["add", "--refresh", "nonexistent"]);
    let git = git_failure_output(git_repo.path(), &["add", "--refresh", "nonexistent"]);
    assert_eq!(zmin.0, git.0);
    assert_eq!(zmin.2, git.2);
}

#[test]
fn add_autocrlf_warning_and_blob_normalization_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join("LF"), b"LINEONE\nLINETWO\n").expect("write LF");
        fs::write(repo.join("CRLF"), b"LINEONE\r\nLINETWO\r\n").expect("write CRLF");
    }

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["-c", "core.autocrlf=true", "add", "LF"],
            "zmin",
        ),
        command_output(
            "git",
            git_repo.path(),
            &["-c", "core.autocrlf=true", "add", "LF"],
            "git",
        )
    );

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["-c", "core.autocrlf=input", "add", "CRLF"],
            "zmin",
        ),
        command_output(
            "git",
            git_repo.path(),
            &["-c", "core.autocrlf=input", "add", "CRLF"],
            "git",
        )
    );
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", ":CRLF"]),
        git(git_repo.path(), ["cat-file", "-p", ":CRLF"])
    );
}

fn zmin_cli_bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_zmin").unwrap_or(env!("CARGO_BIN_EXE_zmin"))
}

fn zmin_installed_git_shim(dir: &std::path::Path) -> std::path::PathBuf {
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    let shim = dir.join(name);
    fs::copy(zmin_cli_bin(), &shim).expect("copy zmin git shim");
    shim
}

#[test]
fn add_autocrlf_auto_attribute_warning_and_blob_matrix_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let zmin_shim_dir = tempfile::TempDir::new().expect("zmin shim dir");
    let zmin_shim = zmin_installed_git_shim(zmin_shim_dir.path());
    let zmin_shim = zmin_shim.to_string_lossy().into_owned();
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join("LF"), b"LINEONE\nLINETWO\n").expect("write LF");
        fs::write(repo.join("CRLF"), b"LINEONE\r\nLINETWO\r\n").expect("write CRLF");
        fs::write(repo.join("CRLF_mix_LF"), b"LINEONE\nLINETWO\r\nLINETHREE\n")
            .expect("write CRLF_mix_LF");
        fs::write(repo.join("LF_mix_CR"), b"LINEONE\nLINETWO\rLINETHREE\n")
            .expect("write LF_mix_CR");
        fs::write(repo.join("CRLF_nul"), b"LINEONE\0\r\nLINETWO\r\n").expect("write CRLF_nul");
    }

    let files = ["LF", "CRLF", "CRLF_mix_LF", "LF_mix_CR", "CRLF_nul"];

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join(".gitattributes"), b"*.txt text=auto\n").expect("write attributes");
    }
    for autocrlf in ["false", "true", "input"] {
        for file in files {
            let staged_name = format!("case_auto_{autocrlf}_{file}.txt");
            for repo in [git_repo.path(), zmin_repo.path()] {
                fs::copy(repo.join(file), repo.join(&staged_name)).expect("copy fixture");
            }
            let args = [
                "-c",
                &format!("core.autocrlf={autocrlf}"),
                "add",
                &staged_name,
            ];
            let zmin_add = command_output(&zmin_shim, zmin_repo.path(), &args, "zmin");
            let git_add = command_output("git", git_repo.path(), &args, "git");
            assert_eq!(
                zmin_add, git_add,
                "add output mismatch for attr=auto core.autocrlf={autocrlf} file={file}"
            );
            let cat_file_args = ["cat-file", "-p", &format!(":{staged_name}")];
            let zmin_blob = command_stdout_bytes("git", zmin_repo.path(), &cat_file_args);
            let git_blob = command_stdout_bytes("git", git_repo.path(), &cat_file_args);
            assert_eq!(
                zmin_blob, git_blob,
                "blob mismatch for attr=auto core.autocrlf={autocrlf} file={file}"
            );
        }
        let env = [
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ];
        command_output_with_env(
            &zmin_shim,
            zmin_repo.path(),
            &["commit", "-m", &format!("auto {autocrlf}")],
            &env,
            "zmin",
        );
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "-m", &format!("auto {autocrlf}")],
            &env,
            "git",
        );
    }
}

#[test]
fn add_autocrlf_mixed_eol_over_binary_index_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join("file.txt"), b"LINEONE\0\r\nLINETWO\r\n").expect("write binary");
    }
    git(
        git_repo.path(),
        ["-c", "core.autocrlf=false", "add", "file.txt"],
    );
    run_zmin(
        zmin_repo.path(),
        ["-c", "core.autocrlf=false", "add", "file.txt"],
    );
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join("file.txt"), b"LINEONE\nLINETWO\r\nLINETHREE\n").expect("write mixed");
    }

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["-c", "core.autocrlf=true", "add", "file.txt"],
            "zmin",
        ),
        command_output(
            "git",
            git_repo.path(),
            &["-c", "core.autocrlf=true", "add", "file.txt"],
            "git",
        )
    );
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", ":file.txt"]),
        git(git_repo.path(), ["cat-file", "-p", ":file.txt"])
    );
}

#[cfg(unix)]
#[test]
fn add_ignore_errors_stages_readable_siblings_like_stock_git() {
    use std::os::unix::fs::PermissionsExt;

    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "foo1", "readable\n");
        write_file(repo, "foo2", "unreadable\n");
        let mut permissions = fs::metadata(repo.join("foo2"))
            .expect("read foo2 metadata")
            .permissions();
        permissions.set_mode(0);
        fs::set_permissions(repo.join("foo2"), permissions).expect("chmod foo2");
    }

    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["add", "--ignore-errors", "."]),
        git_status(git_repo.path(), ["add", "--ignore-errors", "."])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "foo1"]),
        git(git_repo.path(), ["ls-files", "foo1"])
    );
}

#[cfg(unix)]
#[test]
fn add_ignore_errors_config_stages_readable_siblings_like_stock_git() {
    use std::os::unix::fs::PermissionsExt;

    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "add.ignore-errors", "true"]);
        write_file(repo, "foo1", "readable\n");
        write_file(repo, "foo2", "unreadable\n");
        let mut permissions = fs::metadata(repo.join("foo2"))
            .expect("read foo2 metadata")
            .permissions();
        permissions.set_mode(0);
        fs::set_permissions(repo.join("foo2"), permissions).expect("chmod foo2");
    }

    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["add", "."]),
        git_status(git_repo.path(), ["add", "."])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "foo1"]),
        git(git_repo.path(), ["ls-files", "foo1"])
    );
}

#[test]
fn add_escaped_bracket_pathspec_matches_literal_path_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "fo[ou]bar", "literal\n");
        write_file(repo, "foobar", "plain\n");
    }

    git(git_repo.path(), ["add", r"fo\[ou\]bar"]);
    run_zmin(zmin_repo.path(), ["add", r"fo\[ou\]bar"]);

    assert_eq!(
        git(zmin_repo.path(), ["ls-files"]),
        git(git_repo.path(), ["ls-files"])
    );
}

#[test]
fn add_resolves_conflict_on_ignored_path_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "base.txt", "base\n");
        git(repo, ["add", "base.txt"]);
        let blob = git(repo, ["rev-parse", ":base.txt"]);
        let index_info = format!("100644 {blob} 1\ttrack-this\n100644 {blob} 3\ttrack-this\n");
        git_with_stdin(repo, ["update-index", "--index-info"], &index_info);
        write_file(repo, ".gitignore", "track-this\n");
        write_file(repo, "track-this", "resolved\n");
    }

    git(git_repo.path(), ["add", "track-this"]);
    run_zmin(zmin_repo.path(), ["add", "track-this"]);

    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage", "track-this"]),
        git(git_repo.path(), ["ls-files", "--stage", "track-this"])
    );
}

#[test]
fn add_embedded_repository_warning_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        for name in ["inner1", "inner2"] {
            let inner = repo.join(name);
            git(repo, ["init", name]);
            configure_identity(&inner);
            git_with_env(&inner, ["commit", "--allow-empty", "-m", name]);
        }
    }

    let git_output = command_any_output("git", git_repo.path(), &["add", "."], "git");
    let zmin = command_any_output(zmin_bin(), zmin_repo.path(), &["add", "."], "zmin");

    assert_eq!(zmin.0, git_output.0);
    assert_eq!(zmin.1, git_output.1);
    assert_eq!(zmin.2, git_output.2);
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );
}

#[test]
fn add_empty_embedded_repository_error_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["init", "empty"]);
    }

    let git_output = command_any_output("git", git_repo.path(), &["add", "empty"], "git");
    let zmin = command_any_output(zmin_bin(), zmin_repo.path(), &["add", "empty"], "zmin");

    assert_ne!(git_output.0, 0);
    assert_ne!(zmin.0, 0);
    assert_eq!(zmin.1, "");
    assert_eq!(
        zmin.2,
        "error: 'empty/' does not have a commit checked out\nerror: unable to index file 'empty/'\nfatal: adding files failed"
    );
}

#[test]
fn add_dry_run_reports_without_mutating_index_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "track-this", "new\n");
    }

    assert_eq!(
        run_zmin(zmin_repo.path(), ["add", "--dry-run", "track-this"]),
        git(git_repo.path(), ["add", "--dry-run", "track-this"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );
}

#[test]
fn add_dry_run_allows_tracked_ignored_path_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "track-this", "tracked\n");
        git(repo, ["add", "track-this"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
        write_file(repo, ".gitignore", "track-this\n");
        write_file(repo, "track-this", "changed\n");
    }

    assert_eq!(
        run_zmin(zmin_repo.path(), ["add", "--dry-run", "track-this"]),
        git(git_repo.path(), ["add", "--dry-run", "track-this"])
    );
}

#[test]
fn add_dry_run_ignore_missing_reports_tracked_and_ignored_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "track-this", "tracked\n");
        git(repo, ["add", "track-this"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
        write_file(repo, ".gitignore", "ignored-file\ntrack-this\n");
        write_file(repo, "track-this", "changed\n");
    }

    let args = [
        "add",
        "--dry-run",
        "--ignore-missing",
        "track-this",
        "ignored-file",
    ];
    assert_eq!(
        command_any_output(zmin_bin(), zmin_repo.path(), &args, "zmin"),
        command_any_output("git", git_repo.path(), &args, "git")
    );
}

#[test]
fn add_chmod_stages_mode_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "foo", "foo\n");
    }

    run_zmin(zmin_repo.path(), ["add", "--chmod=+x", "foo"]);
    git(git_repo.path(), ["add", "--chmod=+x", "foo"]);
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage", "foo"]),
        git(git_repo.path(), ["ls-files", "--stage", "foo"])
    );

    run_zmin(zmin_repo.path(), ["add", "--chmod=-x", "foo"]);
    git(git_repo.path(), ["add", "--chmod=-x", "foo"]);
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage", "foo"]),
        git(git_repo.path(), ["ls-files", "--stage", "foo"])
    );
}

#[test]
fn add_chmod_dry_run_and_symlink_errors_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "foo", "foo\n");
        git(repo, ["add", "foo"]);
    }

    assert_eq!(
        run_zmin(zmin_repo.path(), ["add", "--chmod=+x", "--dry-run", "foo"]),
        git(git_repo.path(), ["add", "--chmod=+x", "--dry-run", "foo"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage", "foo"]),
        git(git_repo.path(), ["ls-files", "--stage", "foo"])
    );

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("foo", git_repo.path().join("link")).expect("git symlink");
        std::os::unix::fs::symlink("foo", zmin_repo.path().join("link")).expect("zmin symlink");
        git(git_repo.path(), ["add", "link"]);
        run_zmin(zmin_repo.path(), ["add", "link"]);
        assert_eq!(
            run_zmin_failure_output(
                zmin_repo.path(),
                &["add", "--chmod=+x", "--dry-run", "link"]
            ),
            git_failure_output(git_repo.path(), &["add", "--chmod=+x", "--dry-run", "link"])
        );
    }
}

#[cfg(unix)]
#[test]
fn add_chmod_stages_regular_paths_when_non_regular_path_fails_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "foo", "foo\n");
        write_file(repo, "regular", "regular\n");
        std::os::unix::fs::symlink("foo", repo.join("link")).expect("symlink");
        git(repo, ["add", "link"]);
    }

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["add", "--chmod=+x", "link", "regular"]),
        git_failure_output(git_repo.path(), &["add", "--chmod=+x", "link", "regular"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage", "link", "regular"]),
        git(git_repo.path(), ["ls-files", "--stage", "link", "regular"])
    );
}

#[test]
fn add_chmod_rejects_index_symlink_even_when_worktree_path_is_regular_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "core.symlinks", "false"]);
        write_file(repo, "foo", "foo");
        write_file(repo, "link", "foo");
        write_file(repo, "regular", "regular\n");
        let link_oid = git(repo, ["hash-object", "-w", "link"]);
        git(
            repo,
            [
                "update-index",
                "--add",
                "--cacheinfo",
                "120000",
                link_oid.trim(),
                "link",
            ],
        );
        git(repo, ["update-index", "link"]);
    }

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["add", "--chmod=+x", "link", "regular"]),
        git_failure_output(git_repo.path(), &["add", "--chmod=+x", "link", "regular"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage", "link", "regular"]),
        git(git_repo.path(), ["ls-files", "--stage", "link", "regular"])
    );
}

#[test]
fn add_rejects_submodule_object_format_mismatch_like_upstream_git() {
    for (parent_format, nested_format) in [("sha256", "sha1"), ("sha1", "sha256")] {
        let repo = TempDir::new().expect("temp repo");
        git(
            repo.path(),
            ["init", &format!("--object-format={parent_format}")],
        );
        configure_identity(repo.path());
        write_file(repo.path(), "parent.txt", "parent\n");
        git(repo.path(), ["add", "parent.txt"]);
        git_with_env(repo.path(), ["commit", "-m", "parent"]);

        git(
            repo.path(),
            [
                "init",
                &format!("--object-format={nested_format}"),
                "submodule",
            ],
        );
        configure_identity(&repo.path().join("submodule"));
        write_file(&repo.path().join("submodule"), "nested.txt", "nested\n");
        git(&repo.path().join("submodule"), ["add", "nested.txt"]);
        git_with_env(&repo.path().join("submodule"), ["commit", "-m", "nested"]);

        let output = run_zmin_failure_output(repo.path(), &["add", "submodule"]);
        assert_eq!(output.0, 128);
        assert_eq!(output.1, "");
        assert!(
            output
                .2
                .contains("cannot add a submodule of a different hash algorithm"),
            "unexpected stderr: {}",
            output.2
        );
        assert!(!git(repo.path(), ["ls-files", "--stage"]).contains("160000"));
    }
}

#[test]
fn add_case_insensitive_absolute_path_matches_stock_git_when_supported() {
    let probe = TempDir::new().expect("temp probe");
    write_file(probe.path(), "CamelCase", "good\n");
    let lower_probe = probe.path().join("camelcase");
    if !lower_probe.exists() {
        return;
    }

    let zmin_repo = git_init();
    write_file(zmin_repo.path(), "BLUB", "content\n");
    let zmin_path = zmin_repo
        .path()
        .join("BLUB")
        .display()
        .to_string()
        .to_lowercase();

    run_zmin(zmin_repo.path(), ["add", &zmin_path]);
    assert!(git(zmin_repo.path(), ["ls-files", "--stage"]).contains("\tBLUB"));
}

#[test]
fn add_update_matches_stock_git_state() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();

    fs::create_dir_all(git_repo.path().join("dir")).expect("mkdir git dir");
    fs::create_dir_all(zmin_repo.path().join("dir")).expect("mkdir zmin dir");
    fs::write(git_repo.path().join("dir/nested.txt"), b"nested\n").expect("write git nested");
    fs::write(zmin_repo.path().join("dir/nested.txt"), b"nested\n").expect("write zmin nested");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "nested"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "nested"]);

    fs::write(git_repo.path().join("a.txt"), b"tracked change\n").expect("modify git a");
    fs::write(zmin_repo.path().join("a.txt"), b"tracked change\n").expect("modify zmin a");
    fs::write(git_repo.path().join("dir/nested.txt"), b"nested change\n")
        .expect("modify git nested");
    fs::write(zmin_repo.path().join("dir/nested.txt"), b"nested change\n")
        .expect("modify zmin nested");
    fs::write(git_repo.path().join("new.txt"), b"new\n").expect("write git new");
    fs::write(zmin_repo.path().join("new.txt"), b"new\n").expect("write zmin new");

    git(git_repo.path(), ["add", "-u", "dir"]);
    run_zmin(zmin_repo.path(), ["add", "-u", "dir"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );

    fs::remove_file(git_repo.path().join("dir/nested.txt")).expect("remove git nested");
    fs::remove_file(zmin_repo.path().join("dir/nested.txt")).expect("remove zmin nested");
    git(git_repo.path(), ["stage", "-u"]);
    run_zmin(zmin_repo.path(), ["stage", "-u"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );

    git_with_env(git_repo.path(), ["commit", "-m", "update tracked"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "update tracked"]);
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn update_index_matches_stock_git_for_core_index_mutations() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "a.txt", "one\n");
        write_file(repo, "c.txt", "cee\n");
        write_file(repo, "d.txt", "dee\n");
    }

    git(git_repo.path(), ["update-index", "--add", "a.txt"]);
    run_zmin(zmin_repo.path(), ["update-index", "--add", "a.txt"]);
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );

    write_file(git_repo.path(), "a.txt", "two\n");
    write_file(zmin_repo.path(), "a.txt", "two\n");
    git(git_repo.path(), ["update-index", "a.txt"]);
    run_zmin(zmin_repo.path(), ["update-index", "a.txt"]);
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );

    fs::remove_file(git_repo.path().join("a.txt")).expect("remove git a");
    fs::remove_file(zmin_repo.path().join("a.txt")).expect("remove zmin a");
    git(git_repo.path(), ["update-index", "--remove", "a.txt"]);
    run_zmin(zmin_repo.path(), ["update-index", "--remove", "a.txt"]);
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );

    let git_blob = git_with_stdin(git_repo.path(), ["hash-object", "-w", "--stdin"], "blob\n");
    let zmin_blob = git_with_stdin(zmin_repo.path(), ["hash-object", "-w", "--stdin"], "blob\n");
    assert_eq!(zmin_blob, git_blob);
    let cacheinfo = format!("100644,{git_blob},b.txt");
    git(
        git_repo.path(),
        ["update-index", "--add", "--cacheinfo", &cacheinfo],
    );
    run_zmin(
        zmin_repo.path(),
        ["update-index", "--add", "--cacheinfo", &cacheinfo],
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );

    write_file(git_repo.path(), "link.txt", "b.txt");
    write_file(zmin_repo.path(), "link.txt", "b.txt");
    let git_link_blob = git(git_repo.path(), ["hash-object", "-w", "link.txt"]);
    let zmin_link_blob = git(zmin_repo.path(), ["hash-object", "-w", "link.txt"]);
    assert_eq!(zmin_link_blob, git_link_blob);
    git(
        git_repo.path(),
        [
            "update-index",
            "--add",
            "--cacheinfo",
            "120000",
            &git_link_blob,
            "link.txt",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "update-index",
            "--add",
            "--cacheinfo",
            "120000",
            &zmin_link_blob,
            "link.txt",
        ],
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );

    write_file(git_repo.path(), "b.txt", "blob\n");
    write_file(zmin_repo.path(), "b.txt", "blob\n");
    git(git_repo.path(), ["update-index", "--chmod=+x", "b.txt"]);
    run_zmin(zmin_repo.path(), ["update-index", "--chmod=+x", "b.txt"]);
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );

    write_file(git_repo.path(), "b.txt", "worktree only\n");
    write_file(zmin_repo.path(), "b.txt", "worktree only\n");
    git(
        git_repo.path(),
        ["update-index", "--skip-worktree", "b.txt"],
    );
    run_zmin(
        zmin_repo.path(),
        ["update-index", "--skip-worktree", "b.txt"],
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-s", "-v", "b.txt"]),
        git(git_repo.path(), ["ls-files", "-s", "-v", "b.txt"])
    );

    fs::remove_file(git_repo.path().join("b.txt")).expect("remove git b");
    fs::remove_file(zmin_repo.path().join("b.txt")).expect("remove zmin b");
    git(
        git_repo.path(),
        ["update-index", "--no-skip-worktree", "b.txt"],
    );
    run_zmin(
        zmin_repo.path(),
        ["update-index", "--no-skip-worktree", "b.txt"],
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-s", "-v", "b.txt"]),
        git(git_repo.path(), ["ls-files", "-s", "-v", "b.txt"])
    );

    git_with_stdin(
        git_repo.path(),
        ["update-index", "--add", "-z", "--stdin"],
        "c.txt\0d.txt\0",
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        ["update-index", "--add", "-z", "--stdin"],
        "c.txt\0d.txt\0",
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );

    let mut git_index_info = String::new();
    let mut zmin_index_info = String::new();
    for stage in 1..=3 {
        write_file(
            git_repo.path(),
            &format!("stage-{stage}.txt"),
            &format!("stage {stage}\n"),
        );
        write_file(
            zmin_repo.path(),
            &format!("stage-{stage}.txt"),
            &format!("stage {stage}\n"),
        );
        let git_stage_blob = git(
            git_repo.path(),
            ["hash-object", "-w", &format!("stage-{stage}.txt")],
        );
        let zmin_stage_blob = git(
            zmin_repo.path(),
            ["hash-object", "-w", &format!("stage-{stage}.txt")],
        );
        assert_eq!(zmin_stage_blob, git_stage_blob);
        git_index_info.push_str(&format!("100644 {git_stage_blob} {stage}\tconflict.txt\n"));
        zmin_index_info.push_str(&format!("100644 {zmin_stage_blob} {stage}\tconflict.txt\n"));
    }
    git_with_stdin(
        git_repo.path(),
        ["update-index", "--index-info"],
        &git_index_info,
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        ["update-index", "--index-info"],
        &zmin_index_info,
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage", "conflict.txt"]),
        git(git_repo.path(), ["ls-files", "--stage", "conflict.txt"])
    );
}

#[test]
fn update_index_add_replace_resolves_directory_file_conflicts_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "path0", "root\n");
        write_file(repo, "path2/file2", "nested\n");
        git(repo, ["update-index", "--add", "path0", "path2/file2"]);
        fs::rename(repo.join("path0"), repo.join("tmp")).expect("move path0");
        fs::rename(repo.join("path2"), repo.join("path0")).expect("move path2");
        fs::rename(repo.join("tmp"), repo.join("path2")).expect("move tmp");
    }

    git(
        git_repo.path(),
        ["update-index", "--add", "--replace", "path2", "path0/file2"],
    );
    run_zmin(
        zmin_repo.path(),
        ["update-index", "--add", "--replace", "path2", "path0/file2"],
    );

    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files", "path0"]),
        "path0/file2"
    );
}

#[test]
fn update_index_cacheinfo_rejects_directory_file_conflicts_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_blob = git_with_stdin(git_repo.path(), ["hash-object", "-w", "--stdin"], "");
    let zmin_blob = git_with_stdin(zmin_repo.path(), ["hash-object", "-w", "--stdin"], "");
    assert_eq!(zmin_blob, git_blob);
    let parent = format!("100644,{git_blob},path5/a");
    git(
        git_repo.path(),
        ["update-index", "--add", "--cacheinfo", &parent],
    );
    run_zmin(
        zmin_repo.path(),
        ["update-index", "--add", "--cacheinfo", &parent],
    );

    let child = format!("100644,{git_blob},path5/a/file");
    let git_failure = git_failure_output(
        git_repo.path(),
        &["update-index", "--add", "--cacheinfo", &child],
    );
    let zmin_failure = run_zmin_failure_output(
        zmin_repo.path(),
        &["update-index", "--add", "--cacheinfo", &child],
    );
    assert_eq!(zmin_failure.0, git_failure.0);
    assert_eq!(
        git(zmin_repo.path(), ["ls-files"]),
        git(git_repo.path(), ["ls-files"])
    );
}

#[test]
fn update_index_index_info_accepts_missing_object_ids_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let input = "\
100644 blob 0000000000000000000000000000000000000001\tdir/file1\n\
100644 blob 0000000000000000000000000000000000000002\tdir/file2\n";

    git_with_stdin(git_repo.path(), ["update-index", "--index-info"], input);
    run_zmin_with_stdin(zmin_repo.path(), ["update-index", "--index-info"], input);

    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );
    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["write-tree"]),
        git_status(git_repo.path(), ["write-tree"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["write-tree", "--missing-ok"]),
        git(git_repo.path(), ["write-tree", "--missing-ok"])
    );
}

#[test]
fn rm_file_dir_and_cached_match_stock_git_state() {
    let git_repo = rm_fixture_repo();
    let zmin_repo = rm_fixture_repo();

    git(git_repo.path(), ["rm", "a.txt"]);
    run_zmin(zmin_repo.path(), ["rm", "a.txt"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert!(!zmin_repo.path().join("a.txt").exists());

    git(git_repo.path(), ["rm", "-r", "dir"]);
    run_zmin(zmin_repo.path(), ["rm", "-r", "dir"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert!(!zmin_repo.path().join("dir/tracked.txt").exists());
    assert!(zmin_repo.path().join("dir/untracked.txt").exists());

    git(git_repo.path(), ["rm", "--cached", "cached.txt"]);
    run_zmin(zmin_repo.path(), ["rm", "--cached", "cached.txt"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert!(zmin_repo.path().join("cached.txt").exists());
}

#[test]
fn rm_cached_recursive_root_pathspec_matches_stock_git() {
    let git_repo = rm_fixture_repo();
    let zmin_repo = rm_fixture_repo();

    git(git_repo.path(), ["rm", "--cached", "-r", "."]);
    run_zmin(zmin_repo.path(), ["rm", "--cached", "-r", "."]);

    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files"]),
        git(git_repo.path(), ["ls-files"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn rm_common_options_match_stock_git() {
    let git_repo = rm_fixture_repo();
    let zmin_repo = rm_fixture_repo();
    assert_eq!(
        command_output(zmin_bin(), zmin_repo.path(), &["rm", "-n", "a.txt"], "zmin",),
        command_output("git", git_repo.path(), &["rm", "-n", "a.txt"], "git")
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
    assert!(zmin_repo.path().join("a.txt").exists());

    let git_repo = rm_fixture_repo();
    let zmin_repo = rm_fixture_repo();
    assert_eq!(
        command_output(zmin_bin(), zmin_repo.path(), &["rm", "-q", "a.txt"], "zmin",),
        command_output("git", git_repo.path(), &["rm", "-q", "a.txt"], "git")
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
    assert!(!zmin_repo.path().join("a.txt").exists());

    let git_repo = rm_fixture_repo();
    let zmin_repo = rm_fixture_repo();
    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["rm", "--ignore-unmatch", "missing.txt"],
            "zmin",
        ),
        command_output(
            "git",
            git_repo.path(),
            &["rm", "--ignore-unmatch", "missing.txt"],
            "git",
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );

    let git_repo = rm_fixture_repo();
    let zmin_repo = rm_fixture_repo();
    fs::write(git_repo.path().join("paths.nul"), b"a.txt\0cached.txt\0").expect("git paths");
    fs::write(zmin_repo.path().join("paths.nul"), b"a.txt\0cached.txt\0").expect("zmin paths");
    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "rm",
                "--cached",
                "--pathspec-from-file",
                "paths.nul",
                "--pathspec-file-nul",
            ],
            "zmin",
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
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );

    let repo = rm_fixture_repo();
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["rm", "--pathspec-file-nul"]),
        git_failure_output(repo.path(), &["rm", "--pathspec-file-nul"])
    );
}

#[test]
fn mv_file_and_directory_match_stock_git_tree_after_commit() {
    let git_repo = mv_fixture_repo();
    let zmin_repo = mv_fixture_repo();

    git(git_repo.path(), ["mv", "a.txt", "renamed.txt"]);
    run_zmin(zmin_repo.path(), ["mv", "a.txt", "renamed.txt"]);
    git(git_repo.path(), ["mv", "dir", "renamed-dir"]);
    run_zmin(zmin_repo.path(), ["mv", "dir", "renamed-dir"]);

    assert!(!zmin_repo.path().join("a.txt").exists());
    assert!(zmin_repo.path().join("renamed.txt").exists());
    assert!(!zmin_repo.path().join("dir/tracked.txt").exists());
    assert!(zmin_repo.path().join("renamed-dir/tracked.txt").exists());
    assert!(zmin_repo.path().join("renamed-dir/untracked.txt").exists());

    assert_eq!(
        git(
            zmin_repo.path(),
            ["diff", "--cached", "--name-status", "--no-renames"],
        ),
        git(
            git_repo.path(),
            ["diff", "--cached", "--name-status", "--no-renames"],
        )
    );

    git_with_env(git_repo.path(), ["commit", "-m", "move"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "move"]);
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
}
