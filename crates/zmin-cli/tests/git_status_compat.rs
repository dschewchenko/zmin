mod common;

use std::fs;

#[cfg(unix)]
use std::process::Command;

#[cfg(not(windows))]
use common::write_file;
use common::{
    command_any_output, configure_identity, corrupt_first_index_entry_ctime,
    corrupt_first_index_entry_extended_stat, first_index_entry_device, git, git_args,
    git_failure_output, git_init, git_with_env, run_zmin, run_zmin_args, run_zmin_failure_output,
    run_zmin_with_env, set_first_index_entry_device, zmin_bin,
};
use tempfile::TempDir;

#[test]
fn status_porcelain_matches_stock_git_for_clean_dirty_and_ignored_worktrees() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "commit.gpgsign", "false"]);
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write tracked");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "-sb"]),
        git(repo.path(), ["status", "-sb"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--short"]),
        git(repo.path(), ["status", "--short"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain"]),
        git(repo.path(), ["status", "--porcelain"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "-z"]),
        git(repo.path(), ["status", "-z"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--null"]),
        git(repo.path(), ["status", "--null"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v2", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v2", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=2", "--branch"]),
        git(repo.path(), ["status", "--porcelain=2", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v2", "-z", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v2", "-z", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=2", "-z", "--branch"]),
        git(repo.path(), ["status", "--porcelain=2", "-z", "--branch"])
    );

    fs::write(repo.path().join("a.txt"), b"changed\n").expect("modify tracked");
    fs::write(repo.path().join("b.txt"), b"new\n").expect("write untracked");
    fs::create_dir_all(repo.path().join("dir")).expect("create untracked dir");
    fs::write(repo.path().join("dir/nested.txt"), b"nested\n").expect("write nested untracked");
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v1", "-z", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v1", "-z", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "-z"]),
        git(repo.path(), ["status", "-z"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--null"]),
        git(repo.path(), ["status", "--null"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v2", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v2", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=2", "--branch"]),
        git(repo.path(), ["status", "--porcelain=2", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v2", "-z", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v2", "-z", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=2", "-z", "--branch"]),
        git(repo.path(), ["status", "--porcelain=2", "-z", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v2", "--short"]),
        git(repo.path(), ["status", "--porcelain=v2", "--short"])
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            ["status", "--porcelain=v2", "--short", "--branch"]
        ),
        git(
            repo.path(),
            ["status", "--porcelain=v2", "--short", "--branch"]
        )
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            ["status", "--porcelain=v1", "--branch", "-uno"]
        ),
        git(
            repo.path(),
            ["status", "--porcelain=v1", "--branch", "-uno"]
        )
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            ["status", "--porcelain=v1", "--branch", "-uall"]
        ),
        git(
            repo.path(),
            ["status", "--porcelain=v1", "--branch", "-uall"]
        )
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--untracked-files=no"]),
        git(repo.path(), ["status", "--untracked-files=no"])
    );
    for args in [
        ["status", "--porcelain=v1", "-u"].as_slice(),
        ["status", "--porcelain=v1", "-unormal"].as_slice(),
        ["status", "--porcelain=v1", "--untracked-files"].as_slice(),
        ["status", "--porcelain=v1", "--untracked-files=normal"].as_slice(),
        ["status", "--porcelain=v1", "--untracked-files=all"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    fs::write(repo.path().join(".gitignore"), b"*.log\nignored-dir/\n").expect("write gitignore");
    fs::write(repo.path().join("debug.log"), b"ignored\n").expect("write ignored file");
    fs::create_dir_all(repo.path().join("ignored-dir")).expect("create ignored dir");
    fs::write(repo.path().join("ignored-dir/file.txt"), b"ignored\n")
        .expect("write ignored dir file");
    for args in [
        ["status", "--porcelain=v1", "--ignored"].as_slice(),
        ["status", "--porcelain=v1", "--ignored=traditional"].as_slice(),
        ["status", "--porcelain=v1", "--ignored=matching"].as_slice(),
        ["status", "--porcelain=v1", "--ignored=no"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args)
        );
    }
}

#[test]
fn status_porcelain_resolves_sha1_sha256_and_reftable_heads() {
    for (object_format, reftable) in status_format_cases() {
        let repo = status_format_repo(object_format, reftable);
        assert_eq!(
            run_zmin(repo.path(), ["status", "--porcelain"]),
            git(repo.path(), ["status", "--porcelain"]),
            "status failed for object format {object_format}, reftable={reftable}"
        );
    }
}

#[test]
fn status_human_and_branch_resolve_sha1_sha256_files_and_reftable() {
    for (object_format, reftable) in status_format_cases() {
        let repo = status_format_repo(object_format, reftable);
        for args in [
            ["status"].as_slice(),
            ["status", "--porcelain=v1", "--branch"].as_slice(),
            ["status", "--porcelain=v2", "--branch"].as_slice(),
        ] {
            assert_eq!(
                run_zmin_args(repo.path(), args),
                git_args(repo.path(), args),
                "status mismatch for object format {object_format}, reftable={reftable}, args={args:?}"
            );
        }
    }
}

#[test]
fn status_show_stash_counts_sha1_and_sha256_reftable_logs() {
    for object_format in ["sha1", "sha256"] {
        let repo = status_format_repo(object_format, true);
        fs::write(repo.path().join("tracked.txt"), b"first stash\n")
            .expect("write first stash change");
        git(repo.path(), ["stash", "push", "-m", "first"]);
        fs::write(repo.path().join("tracked.txt"), b"second stash\n")
            .expect("write second stash change");
        git(repo.path(), ["stash", "push", "-m", "second"]);

        let args = ["status", "--porcelain=v2", "--show-stash"];
        assert_eq!(
            run_zmin(repo.path(), args),
            git(repo.path(), args),
            "stash count mismatch for object format {object_format}"
        );
    }
}

#[test]
fn status_show_stash_tolerates_broken_sha1_and_sha256_reftable_logs() {
    for object_format in ["sha1", "sha256"] {
        for truncate in [false, true] {
            let repo = status_format_repo(object_format, true);
            fs::write(repo.path().join("tracked.txt"), b"first stash\n")
                .expect("write first stash change");
            git(repo.path(), ["stash", "push", "-m", "first"]);
            fs::write(repo.path().join("tracked.txt"), b"second stash\n")
                .expect("write second stash change");
            git(repo.path(), ["stash", "push", "-m", "second"]);
            corrupt_reftable_status_log(&repo, truncate);

            let args = ["status", "--porcelain=v2", "--show-stash"];
            let (zmin_status, zmin_stdout, zmin_stderr) =
                command_any_output(zmin_bin(), repo.path(), &args, "zmin");
            let (git_status, git_stdout, git_stderr) =
                command_any_output("git", repo.path(), &args, "git");
            assert_eq!(
                git_status, 0,
                "stock status failed for object format {object_format}, truncate={truncate}: stdout={git_stdout:?} stderr={git_stderr:?}"
            );
            assert_eq!(
                zmin_status, 0,
                "zmin status failed for object format {object_format}, truncate={truncate}: stdout={zmin_stdout:?} stderr={zmin_stderr:?}"
            );
            assert_eq!(
                zmin_stdout, git_stdout,
                "broken reftable stash log mismatch for object format {object_format}, truncate={truncate}"
            );
            if truncate {
                assert!(!zmin_stdout.lines().any(|line| line.starts_with("# stash ")));
            }
        }
    }
}

#[test]
fn status_reftable_head_corruption_matches_stock_branch_state() {
    for object_format in ["sha1", "sha256"] {
        for remove_tables_list in [false, true] {
            let repo = status_format_repo(object_format, true);
            fs::write(repo.path().join("tracked.txt"), b"worktree change\n")
                .expect("write worktree status change");
            fs::write(repo.path().join("staged.txt"), b"staged change\n")
                .expect("write staged status change");
            git(repo.path(), ["add", "staged.txt"]);
            let reftable_dir = repo.path().join(".git/reftable");
            if remove_tables_list {
                fs::remove_file(reftable_dir.join("tables.list"))
                    .expect("remove reftable tables.list");
            } else {
                let tables_content =
                    fs::read_to_string(reftable_dir.join("tables.list")).expect("read tables.list");
                let table_name = tables_content.lines().next().expect("active table");
                let table_path = reftable_dir.join(table_name);
                let table = fs::read(&table_path).expect("read active table");
                fs::write(&table_path, &table[..table.len() - 1]).expect("truncate active table");
            }

            for args in [
                ["status", "--porcelain=v2", "--branch"].as_slice(),
                ["status", "--porcelain=v1", "--branch"].as_slice(),
                ["status", "--porcelain=v1", "--branch", "-z"].as_slice(),
                ["status", "-sb"].as_slice(),
                ["status", "--porcelain=v1", "--branch", "--show-stash"].as_slice(),
                ["status"].as_slice(),
                ["status", "--verbose"].as_slice(),
                ["status", "--verbose", "--branch"].as_slice(),
            ] {
                assert_eq!(
                    run_zmin_args(repo.path(), args),
                    git_args(repo.path(), args),
                    "status mismatch for object format {object_format}, remove_tables_list={remove_tables_list}, args={args:?}"
                );
            }
        }
    }
}

#[test]
fn status_reftable_filesystem_head_syntax_matches_stock() {
    for object_format in ["sha1", "sha256"] {
        let repo = status_format_repo(object_format, true);
        let width = if object_format == "sha1" { 40 } else { 64 };
        for raw_head in [
            "not-a-valid-head".to_owned(),
            "0".repeat(width),
            "1".repeat(width),
        ] {
            fs::write(repo.path().join(".git/HEAD"), format!("{raw_head}\n"))
                .expect("write filesystem HEAD");
            for args in [
                ["status"].as_slice(),
                ["status", "--short"].as_slice(),
                ["status", "--porcelain=v1", "--branch"].as_slice(),
                ["status", "--porcelain=v2", "--branch"].as_slice(),
                ["status", "-sb"].as_slice(),
            ] {
                let (zmin_status, zmin_stdout, zmin_stderr) =
                    command_any_output(zmin_bin(), repo.path(), args, "zmin");
                let (git_status, git_stdout, _) =
                    command_any_output("git", repo.path(), args, "git");
                assert_eq!(
                    zmin_status, git_status,
                    "filesystem HEAD status mismatch for object format {object_format}, raw={raw_head:?}, args={args:?}"
                );
                assert_eq!(
                    zmin_stdout, git_stdout,
                    "filesystem HEAD stdout mismatch for object format {object_format}, raw={raw_head:?}, args={args:?}"
                );
                if raw_head == "not-a-valid-head" {
                    assert_eq!(zmin_status, 128);
                    assert!(zmin_stdout.is_empty());
                    assert_eq!(
                        zmin_stderr,
                        "fatal: not a git repository (or any of the parent directories): .git"
                    );
                }
            }
        }
    }
}

#[test]
fn status_bad_direct_head_fails_without_partial_branch_metadata() {
    for object_format in ["sha1", "sha256"] {
        let repo = status_format_repo(object_format, false);
        let width = if object_format == "sha1" { 40 } else { 64 };
        for all_zero in [false, true] {
            let head = if all_zero {
                "0".repeat(width)
            } else {
                "1".repeat(width)
            };
            fs::write(repo.path().join(".git/HEAD"), format!("{head}\n"))
                .expect("write direct HEAD");
            for args in [
                ["status", "--porcelain=v2", "--branch"].as_slice(),
                ["status", "--porcelain=v1", "--branch"].as_slice(),
                ["status", "-sb"].as_slice(),
            ] {
                let (zmin_status, zmin_stdout, _) =
                    command_any_output(zmin_bin(), repo.path(), args, "zmin");
                let (git_status, _, _) = command_any_output("git", repo.path(), args, "git");
                assert_ne!(
                    git_status, 0,
                    "stock accepted invalid direct HEAD: {args:?}"
                );
                assert_ne!(
                    zmin_status, 0,
                    "zmin accepted invalid direct HEAD: {args:?}"
                );
                assert!(
                    !zmin_stdout
                        .lines()
                        .any(|line| line.starts_with("# branch.")),
                    "zmin emitted branch metadata before rejecting direct HEAD: {zmin_stdout:?}"
                );
            }
        }
    }
}

fn corrupt_reftable_status_log(repo: &TempDir, truncate: bool) {
    let reftable_dir = repo.path().join(".git/reftable");
    let tables_list = reftable_dir.join("tables.list");
    if !truncate {
        fs::remove_file(tables_list).expect("remove reftable tables.list");
        return;
    }

    let tables_content = fs::read_to_string(&tables_list).expect("read reftable tables.list");
    let table_name = tables_content.lines().next().expect("reftable table entry");
    let table_path = reftable_dir.join(table_name);
    let table = fs::read(&table_path).expect("read reftable table");
    assert!(table.len() > 1, "reftable table should not be empty");
    fs::write(table_path, &table[..table.len() - 1]).expect("truncate reftable table");
}

fn status_format_cases() -> [(&'static str, bool); 3] {
    [("sha1", false), ("sha256", false), ("sha256", true)]
}

fn status_format_repo(object_format: &str, reftable: bool) -> TempDir {
    let repo = tempfile::TempDir::new().expect("temp status format repo");
    let mut init_args = vec!["init"];
    if object_format == "sha256" {
        init_args.push("--object-format=sha256");
    }
    if reftable {
        init_args.push("--ref-format=reftable");
    }
    git_args(repo.path(), &init_args);
    configure_identity(repo.path());
    fs::write(repo.path().join("tracked.txt"), b"status format\n").expect("write tracked file");
    git(repo.path(), ["add", "tracked.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    repo
}

#[test]
fn status_content_conversion_preserves_attribute_source_precedence() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "commit.gpgsign", "false"]);
    fs::create_dir_all(repo.path().join("nested")).expect("create nested directory");
    fs::create_dir_all(repo.path().join("other")).expect("create other directory");
    fs::write(repo.path().join(".gitattributes"), b"other/*.txt -text\n")
        .expect("write root attributes");
    fs::write(repo.path().join("nested/.gitattributes"), b"*.txt -text\n")
        .expect("write nested attributes");
    fs::write(repo.path().join("nested/a.txt"), b"one\r\n").expect("write nested file");
    fs::write(repo.path().join("other/b.txt"), b"two\r\n").expect("write other file");

    let global_attributes = repo.path().join(".git/global-attributes");
    fs::write(&global_attributes, b"*.txt -text\n").expect("write global attributes");
    fs::write(
        repo.path().join(".git/info/attributes"),
        b"nested/a.txt text\nother/b.txt text\n",
    )
    .expect("write info attributes");
    let attributes_config = format!("core.attributesFile={}", global_attributes.display());
    git_args(
        repo.path(),
        &["-c", attributes_config.as_str(), "add", "-A"],
    );
    git_with_env(
        repo.path(),
        ["-c", attributes_config.as_str(), "commit", "-m", "initial"],
    );

    let future = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    for path in ["nested/a.txt", "other/b.txt"] {
        fs::OpenOptions::new()
            .write(true)
            .open(repo.path().join(path))
            .expect("open tracked file")
            .set_modified(future)
            .expect("force tracked file stat mismatch");
    }
    let args = [
        "-c",
        attributes_config.as_str(),
        "status",
        "--porcelain=v1",
        "--untracked-files=no",
    ];
    let stock_clean = git_args(repo.path(), &args);
    assert!(stock_clean.is_empty(), "stock status: {stock_clean}");
    assert_eq!(run_zmin_args(repo.path(), &args), stock_clean);

    fs::write(repo.path().join(".git/info/attributes"), b"").expect("clear info attributes");
    let stock_modified = git_args(repo.path(), &args);
    assert!(
        stock_modified.contains("nested/a.txt") && stock_modified.contains("other/b.txt"),
        "stock status: {stock_modified}"
    );
    assert_eq!(run_zmin_args(repo.path(), &args), stock_modified);
}

#[test]
fn status_size_change_is_modified_before_content_conversion_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "commit.gpgsign", "false"]);
    git(repo.path(), ["config", "core.autocrlf", "input"]);
    fs::write(repo.path().join("tracked.txt"), b"one\ntwo\n").expect("write tracked file");
    git(repo.path(), ["add", "tracked.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    fs::write(repo.path().join("tracked.txt"), b"one\r\ntwo\r\n")
        .expect("change only worktree line endings");
    let args = ["status", "--porcelain=v1", "--untracked-files=no"];
    let stock = git(repo.path(), args);
    assert_eq!(stock, " M tracked.txt");
    assert_eq!(run_zmin(repo.path(), args), stock);
}

#[test]
fn status_porcelain_v2_matches_stock_git_for_staged_states() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("modified.txt"), b"old\n").expect("write modified");
    fs::write(repo.path().join("deleted.txt"), b"old\n").expect("write deleted");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    fs::write(repo.path().join("modified.txt"), b"new\n").expect("modify tracked");
    fs::write(repo.path().join("added.txt"), b"added\n").expect("write added");
    git(repo.path(), ["add", "modified.txt", "added.txt"]);
    git(repo.path(), ["rm", "-q", "deleted.txt"]);

    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v2", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v2", "--branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v2", "-z", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v2", "-z", "--branch"])
    );
}

#[test]
#[cfg(not(windows))]
fn status_detects_same_size_same_mtime_content_change_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "aaaa\n");
    write_file(zmin_repo.path(), "a.txt", "aaaa\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "initial"]);

    let git_path = git_repo.path().join("a.txt");
    let zmin_path = zmin_repo.path().join("a.txt");
    let git_mtime = fs::metadata(&git_path)
        .expect("git metadata")
        .modified()
        .expect("git modified time");
    let zmin_mtime = fs::metadata(&zmin_path)
        .expect("zmin metadata")
        .modified()
        .expect("zmin modified time");

    fs::write(&git_path, b"bbbb\n").expect("modify git file");
    fs::write(&zmin_path, b"bbbb\n").expect("modify zmin file");
    fs::OpenOptions::new()
        .write(true)
        .open(&git_path)
        .expect("open git file")
        .set_modified(git_mtime)
        .expect("restore git mtime");
    fs::OpenOptions::new()
        .write(true)
        .open(&zmin_path)
        .expect("open zmin file")
        .set_modified(zmin_mtime)
        .expect("restore zmin mtime");

    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
}

#[test]
#[cfg(unix)]
fn status_accepts_stock_git_index_with_different_device_without_rehashing() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("tracked.txt"), b"tracked\n").expect("write tracked file");
    git(repo.path(), ["add", "tracked.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    let recorded_device = first_index_entry_device(repo.path());
    set_first_index_entry_device(repo.path(), recorded_device.wrapping_add(1));
    let detail = run_status_with_trace(
        repo.path(),
        &["status", "--porcelain=v1", "--untracked-files=no"],
        "device",
    );

    assert!(detail.contains("\tstat_safe=1\t"), "trace: {detail}");
    assert!(detail.contains("\tcontent_hashes=0\t"), "trace: {detail}");
}

#[test]
#[cfg(unix)]
fn status_honors_core_checkstat_minimal() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("tracked.txt"), b"tracked\n").expect("write tracked file");
    git(repo.path(), ["add", "tracked.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    corrupt_first_index_entry_extended_stat(repo.path());

    let default_detail = run_status_with_trace(
        repo.path(),
        &["status", "--porcelain=v1", "--untracked-files=no"],
        "default-stat",
    );
    assert!(
        default_detail.contains("\tcontent_hashes=1\t"),
        "trace: {default_detail}"
    );

    let minimal_detail = run_status_with_trace(
        repo.path(),
        &[
            "-c",
            "core.checkStat=minimal",
            "status",
            "--porcelain=v1",
            "--untracked-files=no",
        ],
        "minimal-stat",
    );
    assert!(
        minimal_detail.contains("\tstat_safe=1\t"),
        "trace: {minimal_detail}"
    );
    assert!(
        minimal_detail.contains("\tcontent_hashes=0\t"),
        "trace: {minimal_detail}"
    );
}

#[test]
#[cfg(unix)]
fn status_honors_core_trustctime_false() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("tracked.txt"), b"tracked\n").expect("write tracked file");
    git(repo.path(), ["add", "tracked.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    corrupt_first_index_entry_ctime(repo.path());

    let default_detail = run_status_with_trace(
        repo.path(),
        &["status", "--porcelain=v1", "--untracked-files=no"],
        "default-ctime",
    );
    assert!(
        default_detail.contains("\tcontent_hashes=1\t"),
        "trace: {default_detail}"
    );

    let relaxed_detail = run_status_with_trace(
        repo.path(),
        &[
            "-c",
            "core.trustctime=false",
            "status",
            "--porcelain=v1",
            "--untracked-files=no",
        ],
        "relaxed-ctime",
    );
    assert!(
        relaxed_detail.contains("\tstat_safe=1\t"),
        "trace: {relaxed_detail}"
    );
    assert!(
        relaxed_detail.contains("\tcontent_hashes=0\t"),
        "trace: {relaxed_detail}"
    );
}

#[test]
fn status_index_stat_config_errors_match_stock_git() {
    let repo = git_init();
    for args in [
        ["-c", "core.checkStat=fast", "status", "--porcelain"].as_slice(),
        ["-c", "core.checkStat", "status", "--porcelain"].as_slice(),
        ["-c", "core.trustctime=garbage", "status", "--porcelain"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn status_index_stat_file_config_error_matches_stock_git() {
    let repo = git_init();
    git(repo.path(), ["config", "core.checkStat", "fast"]);
    let args = ["status", "--porcelain"];

    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );
}

#[cfg(unix)]
fn run_status_with_trace(repo: &std::path::Path, args: &[&str], trace_name: &str) -> String {
    let trace_path = repo.join(format!(".git/zmin-status-{trace_name}.trace"));
    let output = Command::new(common::test_command_program(zmin_bin()))
        .args(args)
        .current_dir(repo)
        .env("ZMIN_PHASE_TRACE", "1")
        .env("ZMIN_PHASE_TRACE_FILE", &trace_path)
        .output()
        .expect("run zmin status");

    assert!(
        output.status.success(),
        "zmin status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "clean status must be empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let trace = fs::read_to_string(trace_path).expect("read status phase trace");
    trace
        .lines()
        .find(|line| line.contains("status.worktree_status.detail"))
        .expect("status detail trace")
        .to_owned()
}

#[test]
fn status_human_matches_stock_git_for_common_states() {
    let unborn = git_init();
    assert_eq!(
        run_zmin(unborn.path(), ["status"]),
        git(unborn.path(), ["status"])
    );
    fs::write(unborn.path().join("new.txt"), b"new\n").expect("write unborn untracked");
    assert_eq!(
        run_zmin(unborn.path(), ["status"]),
        git(unborn.path(), ["status"])
    );
    git(unborn.path(), ["add", "new.txt"]);
    assert_eq!(
        run_zmin(unborn.path(), ["status"]),
        git(unborn.path(), ["status"])
    );

    let repo = committed_repo();
    assert_eq!(
        run_zmin(repo.path(), ["status"]),
        git(repo.path(), ["status"])
    );
    fs::write(repo.path().join("a.txt"), b"changed\n").expect("modify tracked");
    assert_eq!(
        run_zmin(repo.path(), ["status"]),
        git(repo.path(), ["status"])
    );
    fs::write(repo.path().join("staged.txt"), b"staged\n").expect("write staged");
    git(repo.path(), ["add", "staged.txt"]);
    fs::write(repo.path().join("untracked.txt"), b"untracked\n").expect("write untracked");
    assert_eq!(
        run_zmin(repo.path(), ["status"]),
        git(repo.path(), ["status"])
    );
    fs::remove_file(repo.path().join("a.txt")).expect("delete tracked");
    assert_eq!(
        run_zmin(repo.path(), ["status"]),
        git(repo.path(), ["status"])
    );
}

#[test]
fn status_branch_reports_upstream_ahead_count() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(
        dir.path(),
        ["clone", remote.to_str().expect("remote path"), "work"],
    );
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["push", "-u", "origin", "HEAD"]);

    fs::write(work.join("b.txt"), b"local\n").expect("write local");
    run_zmin(&work, ["add", "-A"]);
    run_zmin_with_env(&work, ["commit", "-m", "local"]);

    assert_eq!(
        run_zmin(&work, ["status", "--porcelain=v1", "--branch"]),
        git(&work, ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        run_zmin(
            &work,
            ["status", "--porcelain=v2", "--branch", "--ahead-behind"]
        ),
        git(
            &work,
            ["status", "--porcelain=v2", "--branch", "--ahead-behind"]
        )
    );
    assert_eq!(
        run_zmin(
            &work,
            ["status", "--porcelain=v2", "--branch", "--no-ahead-behind"]
        ),
        git(
            &work,
            ["status", "--porcelain=v2", "--branch", "--no-ahead-behind"]
        )
    );
    assert_eq!(
        run_zmin(
            &work,
            ["status", "--porcelain=v1", "--branch", "--no-ahead-behind"]
        ),
        git(
            &work,
            ["status", "--porcelain=v1", "--branch", "--no-ahead-behind"]
        )
    );
    assert_eq!(
        run_zmin(
            &work,
            ["status", "--short", "--branch", "--no-ahead-behind"]
        ),
        git(
            &work,
            ["status", "--short", "--branch", "--no-ahead-behind"]
        )
    );
}

#[test]
fn status_human_branch_modes_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(
        dir.path(),
        ["clone", remote.to_str().expect("remote path"), "work"],
    );
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["push", "-u", "origin", "HEAD"]);

    fs::write(work.join("a.txt"), b"changed\n").expect("modify tracked");
    fs::write(work.join("b.txt"), b"untracked\n").expect("write untracked");
    for args in [
        ["status", "-b"].as_slice(),
        ["status", "--branch"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&work, args),
            git_args(&work, args),
            "dirty human branch args: {args:?}"
        );
    }

    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "local"]);
    for args in [
        ["status", "-b"].as_slice(),
        ["status", "--branch"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&work, args),
            git_args(&work, args),
            "ahead human branch args: {args:?}"
        );
    }
}

#[test]
fn status_pathspec_modes_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::create_dir_all(repo.path().join("other")).expect("create other dir");
    for (path, content) in [
        ("a.txt", b"base\n".as_slice()),
        ("a*b.txt", b"base\n".as_slice()),
        ("dir/one.txt", b"base\n".as_slice()),
        ("dir/two.log", b"base\n".as_slice()),
        ("other/ABC.TXT", b"base\n".as_slice()),
    ] {
        fs::write(repo.path().join(path), content).expect("write tracked pathspec fixture");
    }
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "pathspec base"]);

    for path in [
        "a.txt",
        "a*b.txt",
        "dir/one.txt",
        "dir/two.log",
        "other/ABC.TXT",
    ] {
        fs::write(repo.path().join(path), b"changed\n").expect("modify pathspec fixture");
    }
    fs::write(repo.path().join("dir/new.txt"), b"new\n").expect("write nested untracked");
    fs::write(repo.path().join("root-new.txt"), b"new\n").expect("write root untracked");

    for args in [
        ["status", "--porcelain=v1", "--", "a.txt"].as_slice(),
        ["status", "--porcelain=v1", "--", "dir"].as_slice(),
        ["status", "--porcelain=v1", "--", "dir/"].as_slice(),
        ["status", "--porcelain=v1", "--", "*.txt"].as_slice(),
        ["status", "--porcelain=v1", "--", ":(glob)dir/*.txt"].as_slice(),
        ["status", "--porcelain=v1", "--", ":(literal)a*b.txt"].as_slice(),
        ["status", "--porcelain=v1", "--", ":(icase)other/abc.txt"].as_slice(),
        ["status", "--porcelain=v1", "--", "*.txt", ":(exclude)a.txt"].as_slice(),
        ["status", "--short", "--", "*.txt", ":(exclude)a.txt"].as_slice(),
        ["status", "--", "dir"].as_slice(),
        [
            "--literal-pathspecs",
            "status",
            "--porcelain=v1",
            "--",
            "a*b.txt",
        ]
        .as_slice(),
        [
            "--glob-pathspecs",
            "status",
            "--porcelain=v1",
            "--",
            "a*.txt",
        ]
        .as_slice(),
        [
            "--icase-pathspecs",
            "status",
            "--porcelain=v1",
            "--",
            "other/abc.txt",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "status pathspec args: {args:?}"
        );
    }
}

#[test]
fn status_webstorm_index_query_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("dir/file.txt"), b"base\n").expect("write tracked");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "status base"]);

    fs::write(repo.path().join("dir/file.txt"), b"changed\n").expect("modify tracked");

    let args = [
        "status",
        "--porcelain",
        "-z",
        "--untracked-files=no",
        "--ignored=no",
        "--",
        ".",
    ];
    assert_eq!(
        run_zmin_args(repo.path(), &args),
        git_args(repo.path(), &args),
        "webstorm-style status args: {args:?}"
    );
}

#[test]
fn status_observed_client_command_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("dir/file.txt"), b"base\n").expect("write tracked");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "status base"]);

    fs::write(repo.path().join("dir/file.txt"), b"changed\n").expect("modify tracked");
    fs::write(repo.path().join("dir/untracked.txt"), b"new\n").expect("write untracked");
    fs::write(repo.path().join(".gitignore"), b"ignored.log\n").expect("write ignore");
    fs::write(repo.path().join("ignored.log"), b"ignored\n").expect("write ignored");

    for args in [
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "status",
            "--porcelain",
            "-z",
            "--untracked-files=no",
            "--ignored=no",
            "--",
            ".",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "status",
            "--porcelain",
            "-z",
            "--no-renames",
            "--untracked-files=all",
            "--ignored=matching",
            "--",
        ]
        .as_slice(),
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.quotepath=false",
            "-c",
            "log.showSignature=false",
            "status",
            "--porcelain",
            "-z",
            "--untracked-files=no",
            "--ignored=no",
            "--",
            "dir/file.txt",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "observed client status args: {args:?}"
        );
    }
}

#[test]
fn status_reports_an_untracked_nested_worktree_as_an_opaque_directory() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("tracked.txt"), b"tracked\n").expect("write tracked file");
    git(repo.path(), ["add", "tracked.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    let nested = repo.path().join("nested");
    fs::create_dir(&nested).expect("create nested worktree");
    git(&nested, ["init"]);
    fs::write(nested.join("untracked.txt"), b"nested\n").expect("write nested file");

    for args in [
        ["status", "--porcelain"].as_slice(),
        ["status", "--porcelain", "--untracked-files=all"].as_slice(),
        [
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignored=matching",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "nested worktree status args: {args:?}"
        );
    }
}

#[test]
fn status_branch_no_ahead_behind_reports_equal_upstream_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(
        dir.path(),
        ["clone", remote.to_str().expect("remote path"), "work"],
    );
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["push", "-u", "origin", "HEAD"]);

    assert_eq!(
        run_zmin(
            &work,
            ["status", "--porcelain=v2", "--branch", "--no-ahead-behind"]
        ),
        git(
            &work,
            ["status", "--porcelain=v2", "--branch", "--no-ahead-behind"]
        )
    );
    assert_eq!(
        run_zmin(
            &work,
            ["status", "--porcelain=v1", "--branch", "--no-ahead-behind"]
        ),
        git(
            &work,
            ["status", "--porcelain=v1", "--branch", "--no-ahead-behind"]
        )
    );
}

#[test]
fn status_exclude_standard_matches_stock_git_with_nested_gitignores_inside_ignored_dirs() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join(".gitignore"), b"build/\n.idea/\n").expect("write root ignore");
    fs::create_dir_all(repo.path().join(".idea")).expect("create tracked ignored dir");
    fs::write(repo.path().join(".idea/tracked.xml"), b"<tracked />\n").expect("write tracked");
    git(repo.path(), ["add", ".gitignore"]);
    git(repo.path(), ["add", "-f", ".idea/tracked.xml"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    fs::create_dir_all(repo.path().join("build/deep/nested")).expect("create ignored tree");
    fs::write(repo.path().join("build/.gitignore"), b"!keep.txt\n").expect("write nested ignore");
    fs::write(repo.path().join("build/deep/nested/file.txt"), b"ignored\n").expect("write ignored");
    fs::write(repo.path().join("build/keep.txt"), b"still ignored\n").expect("write keep");
    fs::write(repo.path().join(".idea/.gitignore"), b"workspace.xml\n").expect("write idea ignore");
    fs::write(repo.path().join(".idea/workspace.xml"), b"<workspace />\n").expect("write idea");

    for args in [
        ["status", "--porcelain=v1", "-z"].as_slice(),
        ["status", "--porcelain=v1", "-z", "--ignored=matching"].as_slice(),
        ["status", "--short", "--ignored=matching"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "status args: {args:?}"
        );
    }
}

#[test]
fn status_branch_no_ahead_behind_reports_different_commits_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(
        dir.path(),
        ["clone", remote.to_str().expect("remote path"), "work"],
    );
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["push", "-u", "origin", "HEAD"]);

    fs::write(work.join("b.txt"), b"local\n").expect("write local");
    run_zmin(&work, ["add", "-A"]);
    run_zmin_with_env(&work, ["commit", "-m", "local"]);

    assert_eq!(
        run_zmin(&work, ["status", "--branch", "--no-ahead-behind"]),
        git(&work, ["status", "--branch", "--no-ahead-behind"])
    );
}

#[test]
fn status_show_stash_matches_stock_git() {
    let repo = committed_repo();
    fs::write(repo.path().join("a.txt"), b"stash one\n").expect("modify first stash");
    run_zmin(repo.path(), ["stash", "push", "-m", "one"]);
    fs::write(repo.path().join("a.txt"), b"stash two\n").expect("modify second stash");
    run_zmin(repo.path(), ["stash", "push", "-m", "two"]);

    for args in [
        ["status", "--show-stash"].as_slice(),
        ["status", "--no-show-stash"].as_slice(),
        ["status", "--show-stash", "--no-show-stash"].as_slice(),
        ["status", "--no-show-stash", "--show-stash"].as_slice(),
        ["status", "--porcelain=v2", "--show-stash"].as_slice(),
        ["status", "--porcelain=v2", "--branch", "--show-stash"].as_slice(),
        ["status", "--porcelain=v1", "--branch", "--show-stash"].as_slice(),
        ["status", "--short", "--show-stash"].as_slice(),
        ["status", "-z", "--show-stash"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn status_long_mode_toggles_match_stock_git() {
    let repo = committed_repo();
    fs::write(repo.path().join("a.txt"), b"changed\n").expect("modify tracked");
    fs::write(repo.path().join("staged.txt"), b"staged\n").expect("write staged");
    git(repo.path(), ["add", "staged.txt"]);

    for args in [
        ["status", "--long"].as_slice(),
        ["status", "--no-long"].as_slice(),
        ["status", "--short", "--long"].as_slice(),
        ["status", "--long", "--short"].as_slice(),
        ["status", "--short", "--no-long"].as_slice(),
        ["status", "--no-long", "--short"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn status_verbose_modes_match_stock_git() {
    let repo = committed_repo();
    fs::write(repo.path().join("a.txt"), b"hello\nchanged\n").expect("modify tracked");
    fs::write(repo.path().join("staged.txt"), b"staged\n").expect("write staged");
    git(repo.path(), ["add", "staged.txt"]);

    for args in [
        ["status", "--verbose"].as_slice(),
        ["status", "-v"].as_slice(),
        ["status", "-vv"].as_slice(),
        ["status", "--verbose", "--no-verbose"].as_slice(),
        ["status", "--no-verbose", "--verbose"].as_slice(),
        ["status", "-vv", "--no-verbose"].as_slice(),
        ["status", "--no-verbose", "-vv"].as_slice(),
        ["status", "--short", "--verbose"].as_slice(),
        ["status", "--porcelain=v1", "--verbose"].as_slice(),
        ["status", "--porcelain=v2", "--verbose"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn status_verbose_branch_matches_stock_for_empty_and_staged_diffs() {
    for (object_format, reftable) in status_format_cases() {
        let repo = status_format_repo(object_format, reftable);
        let args_list = [
            ["status"].as_slice(),
            ["status", "--verbose"].as_slice(),
            ["status", "--verbose", "--branch"].as_slice(),
        ];
        for args in args_list {
            assert_eq!(
                run_zmin_args(repo.path(), args),
                git_args(repo.path(), args),
                "clean status mismatch for object format {object_format}, reftable={reftable}, args={args:?}"
            );
        }

        fs::write(repo.path().join("tracked.txt"), b"worktree change\n")
            .expect("write worktree change");
        for args in args_list {
            assert_eq!(
                run_zmin_args(repo.path(), args),
                git_args(repo.path(), args),
                "unstaged status mismatch for object format {object_format}, reftable={reftable}, args={args:?}"
            );
        }

        git(repo.path(), ["add", "tracked.txt"]);
        for args in args_list {
            assert_eq!(
                run_zmin_args(repo.path(), args),
                git_args(repo.path(), args),
                "staged status mismatch for object format {object_format}, reftable={reftable}, args={args:?}"
            );
        }

        fs::write(repo.path().join("tracked.txt"), b"staged and worktree\n")
            .expect("write staged and worktree change");
        for args in args_list {
            assert_eq!(
                run_zmin_args(repo.path(), args),
                git_args(repo.path(), args),
                "staged and worktree status mismatch for object format {object_format}, reftable={reftable}, args={args:?}"
            );
        }
    }
}

#[test]
fn status_column_modes_match_stock_git() {
    let repo = committed_repo();
    for index in 1..=12 {
        fs::write(
            repo.path().join(format!("untracked-{index:02}.txt")),
            b"untracked\n",
        )
        .expect("write untracked fixture");
    }

    for args in [
        ["status", "--column"].as_slice(),
        ["status", "--no-column"].as_slice(),
        ["status", "--column", "--no-column"].as_slice(),
        ["status", "--no-column", "--column"].as_slice(),
        ["status", "--column=never"].as_slice(),
        ["status", "--column=always"].as_slice(),
        ["status", "--short", "--column"].as_slice(),
        ["status", "--porcelain=v1", "--column"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    git(repo.path(), ["config", "column.status", "always"]);
    run_zmin(repo.path(), ["config", "column.status", "always"]);
    for args in [
        ["status"].as_slice(),
        ["status", "--no-column"].as_slice(),
        ["status", "--column=never"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args with column.status=always: {args:?}"
        );
    }
}

#[test]
fn status_cache_index_toggles_are_not_stock_git_options() {
    let repo = committed_repo();
    for args in [
        ["status", "--untracked-cache"].as_slice(),
        ["status", "--no-untracked-cache"].as_slice(),
        ["status", "--split-index"].as_slice(),
        ["status", "--no-split-index"].as_slice(),
    ] {
        let git_output = git_failure_output(repo.path(), args);
        let zmin_output = run_zmin_failure_output(repo.path(), args);
        assert_eq!(git_output.0, 129, "stock Git args: {args:?}");
        assert_eq!(zmin_output, git_output, "Zmin args: {args:?}");
    }
}

#[test]
fn status_unknown_long_option_matches_stock_git() {
    let repo = committed_repo();
    for args in [
        ["status", "--frobnicate"].as_slice(),
        ["status", "--frobnicate=value"].as_slice(),
        ["status", "-Q"].as_slice(),
        ["status", "-sQ"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn status_negated_long_options_match_stock_git() {
    let repo = committed_repo();
    fs::write(repo.path().join("untracked.txt"), b"untracked\n").expect("write untracked");
    for args in [
        ["status", "--porcelain", "--no-porcelain"].as_slice(),
        ["status", "--short", "--no-short"].as_slice(),
        ["status", "--porcelain", "--null", "--no-null"].as_slice(),
        ["status", "--ignored", "--no-ignored", "--porcelain"].as_slice(),
        [
            "status",
            "--ignore-submodules=all",
            "--no-ignore-submodules",
            "--porcelain",
        ]
        .as_slice(),
        ["status", "--no-untracked-files", "--porcelain"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn status_invalid_porcelain_version_matches_stock_git() {
    let repo = committed_repo();
    let args = ["status", "--porcelain=v3"];

    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );
}

#[test]
fn status_invalid_untracked_files_mode_matches_stock_git() {
    let repo = committed_repo();
    let args = ["status", "--untracked-files=bogus"];

    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );
}

#[test]
fn status_invalid_ignored_mode_matches_stock_git() {
    let repo = committed_repo();
    let args = ["status", "--ignored=bogus"];

    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );
}

#[test]
fn status_invalid_column_mode_matches_stock_git() {
    let repo = committed_repo();
    let args = ["status", "--column=bogus"];

    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );
}

#[test]
fn status_invalid_ignore_submodules_mode_matches_stock_git() {
    let repo = committed_repo();
    let args = ["status", "--ignore-submodules=bogus"];

    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );
}

#[test]
fn status_invalid_object_format_config_matches_stock_git() {
    let repo = committed_repo();
    fs::write(
        repo.path().join(".git/config"),
        "[core]\n\trepositoryformatversion = 1\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n[extensions]\n\tobjectFormat = bogus\n",
    )
    .expect("write invalid object format config");
    let args = ["status", "--short"];

    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );
}

#[test]
fn status_unsupported_repository_format_version_matches_stock_git() {
    let repo = committed_repo();
    git(repo.path(), ["config", "core.repositoryformatversion", "2"]);
    let args = ["status", "--porcelain"];

    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );
}

#[test]
fn status_rename_modes_match_stock_git() {
    let repo = committed_repo();
    run_zmin(repo.path(), ["mv", "a.txt", "renamed.txt"]);

    for args in [
        ["status", "--porcelain=v1"].as_slice(),
        ["status", "--porcelain=v1", "--renames"].as_slice(),
        ["status", "--porcelain=v1", "--no-renames"].as_slice(),
        ["status", "--porcelain=v1", "--find-renames"].as_slice(),
        ["status", "--porcelain=v1", "--find-renames=50%"].as_slice(),
        ["status", "--porcelain=v2", "--renames"].as_slice(),
        ["status", "--short", "--renames"].as_slice(),
        ["status", "--renames"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn status_ignore_submodules_modes_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let sub_src = dir.path().join("sub-src");
    let super_repo = dir.path().join("super");
    git(dir.path(), ["init", "sub-src"]);
    configure_identity(&sub_src);
    fs::write(sub_src.join("file.txt"), b"base\n").expect("write submodule source");
    git(&sub_src, ["add", "-A"]);
    git_with_env(&sub_src, ["commit", "-m", "sub init"]);

    git(dir.path(), ["init", "super"]);
    configure_identity(&super_repo);
    git(
        &super_repo,
        [
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "../sub-src",
            "sub",
        ],
    );
    configure_identity(&super_repo.join("sub"));
    git_with_env(&super_repo, ["commit", "-m", "add submodule"]);

    fs::write(super_repo.join("sub/file.txt"), b"base\ndirty\n").expect("dirty submodule");
    for args in [
        ["status", "--porcelain=v1"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules=all"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules=dirty"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules=untracked"].as_slice(),
        ["status", "--porcelain=v2"].as_slice(),
        ["status", "--short", "--ignore-submodules=untracked"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&super_repo, args),
            git_args(&super_repo, args),
            "dirty submodule args: {args:?}"
        );
    }

    fs::write(super_repo.join("sub/new.txt"), b"untracked\n").expect("untracked submodule");
    for args in [
        ["status", "--porcelain=v1"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules=dirty"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules=untracked"].as_slice(),
        ["status", "--short", "--ignore-submodules=untracked"].as_slice(),
        ["status", "--ignore-submodules=untracked"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&super_repo, args),
            git_args(&super_repo, args),
            "dirty and untracked submodule args: {args:?}"
        );
    }

    git(&super_repo.join("sub"), ["add", "-A"]);
    git_with_env(&super_repo.join("sub"), ["commit", "-m", "sub change"]);
    for args in [
        ["status", "--porcelain=v1"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules=dirty"].as_slice(),
        ["status", "--porcelain=v1", "--ignore-submodules=all"].as_slice(),
        ["status", "--porcelain=v2", "--ignore-submodules=dirty"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&super_repo, args),
            git_args(&super_repo, args),
            "new submodule commit args: {args:?}"
        );
    }
}

fn committed_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    run_zmin(repo.path(), ["add", "-A"]);
    run_zmin_with_env(repo.path(), ["commit", "-m", "initial"]);
    repo
}
