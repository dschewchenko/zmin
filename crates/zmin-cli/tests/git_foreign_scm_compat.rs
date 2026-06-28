mod common;

use std::fs;
use std::process::Command;

use common::{
    configure_identity, git, git_failure_output, git_init, git_with_env, run_zmin,
    run_zmin_failure_output, run_zmin_status, run_zmin_with_stdin, stock_git_bin, zmin_bin,
};
use tempfile::TempDir;

#[test]
fn cvsexportcommit_exports_text_commit_to_cvs_checkout() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let cvs = dir.path().join("cvs");
    let bin = dir.path().join("bin");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"base\n").expect("write a");
    fs::write(source.join("remove.txt"), b"remove\n").expect("write remove");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    fs::create_dir_all(cvs.join("CVS")).expect("create CVS marker");
    fs::write(cvs.join("a.txt"), b"base\n").expect("write cvs a");
    fs::write(cvs.join("remove.txt"), b"remove\n").expect("write cvs remove");
    fs::write(source.join("a.txt"), b"base\nchanged\n").expect("modify a");
    fs::remove_file(source.join("remove.txt")).expect("delete remove");
    fs::create_dir_all(source.join("dir")).expect("create source dir");
    fs::write(source.join("dir/new.txt"), b"new\n").expect("write new");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "export me"]);
    write_fake_cvs(&bin, &dir.path().join("cvs.log"));

    let output = run_zmin_with_path(
        &source,
        &bin,
        [
            "cvsexportcommit",
            "-w",
            cvs.to_str().expect("cvs path"),
            "HEAD",
        ],
    );

    assert!(output.contains("Checking if patch will apply"));
    assert!(output.contains("Patch applied successfully"));
    assert!(output.contains("Ready for you to commit"));
    assert_eq!(
        fs::read_to_string(cvs.join("a.txt")).expect("read cvs a"),
        "base\nchanged\n"
    );
    assert_eq!(
        fs::read_to_string(cvs.join("dir/new.txt")).expect("read cvs new"),
        "new\n"
    );
    assert!(!cvs.join("remove.txt").exists());
    assert!(
        fs::read_to_string(cvs.join(".msg"))
            .expect("read message")
            .starts_with("export me\n")
    );
    let log = fs::read_to_string(dir.path().join("cvs.log")).expect("read cvs log");
    assert!(log.contains("status a.txt remove.txt"));
    assert!(log.contains("add dir"));
    assert!(log.contains("add dir/new.txt"));
    assert!(log.contains("rm -f remove.txt"));
}

#[test]
fn cvsimport_imports_cvsps_patchsets_into_git_commits() {
    let dir = TempDir::new().expect("temp dir");
    let bin = dir.path().join("bin");
    let cvs_data = dir.path().join("cvs-data");
    let target = dir.path().join("imported");
    fs::create_dir_all(cvs_data.join("module/a.txt")).expect("create cvs data a");
    fs::create_dir_all(cvs_data.join("module/b.txt")).expect("create cvs data b");
    fs::write(cvs_data.join("module/a.txt/1.1"), b"one\n").expect("write a 1.1");
    fs::write(cvs_data.join("module/a.txt/1.2"), b"two\n").expect("write a 1.2");
    fs::write(cvs_data.join("module/b.txt/1.1"), b"bee\n").expect("write b 1.1");
    write_fake_cvs_checkout(&bin, &cvs_data, &dir.path().join("cvsimport.log"));
    let cvsps = dir.path().join("changes.cvsps");
    fs::write(
        &cvsps,
        "---------------------\nPatchSet 1\nDate: 2001/01/01 00:00:00\nAuthor: dev <dev@example.test>\nBranch: HEAD\nTag: (none)\nLog:\nfirst import\nMembers:\n\ta.txt:INITIAL->1.1\n---------------------\nPatchSet 2\nDate: 2001/01/02 00:00:00\nAuthor: dev <dev@example.test>\nBranch: HEAD\nTag: v1\nLog:\nsecond import\nMembers:\n\ta.txt:1.1->1.2\n\tb.txt:INITIAL->1.1\n",
    )
    .expect("write cvsps");

    run_zmin_with_path(
        dir.path(),
        &bin,
        [
            "cvsimport",
            "-a",
            "-R",
            "-z",
            "0",
            "-P",
            cvsps.to_str().expect("cvsps path"),
            "-C",
            target.to_str().expect("target path"),
            "-d",
            cvs_data.to_str().expect("cvsroot path"),
            "module",
        ],
    );

    assert_eq!(
        fs::read_to_string(target.join("a.txt")).expect("read a"),
        "two\n"
    );
    assert_eq!(
        fs::read_to_string(target.join("b.txt")).expect("read b"),
        "bee\n"
    );
    assert_eq!(
        git(&target, ["log", "--format=%s", "refs/heads/origin"]),
        "second import\nfirst import"
    );
    assert_eq!(
        git(&target, ["rev-parse", "refs/tags/v1"]),
        git(&target, ["rev-parse", "refs/heads/origin"])
    );
    let revisions = fs::read_to_string(target.join(".git/cvs-revisions")).expect("revisions");
    assert!(revisions.contains("a.txt 1.1 "));
    assert!(revisions.contains("a.txt 1.2 "));
    assert!(revisions.contains("b.txt 1.1 "));
    let log = fs::read_to_string(dir.path().join("cvsimport.log")).expect("read fake cvs log");
    assert!(log.contains("-d "));
    assert!(log.contains("co -p -r 1.1 module/a.txt"));
    assert!(log.contains("co -p -r 1.2 module/a.txt"));
    assert!(log.contains("co -p -r 1.1 module/b.txt"));
}

#[test]
fn cvsimport_runs_cvsps_when_patchset_file_is_not_provided() {
    let dir = TempDir::new().expect("temp dir");
    let bin = dir.path().join("bin");
    let cvs_data = dir.path().join("cvs-data");
    let target = dir.path().join("imported");
    let cvsps_log = dir.path().join("cvsps.log");
    fs::create_dir_all(cvs_data.join("module/a.txt")).expect("create cvs data");
    fs::write(cvs_data.join("module/a.txt/1.1"), b"one\n").expect("write a 1.1");
    write_fake_cvs_checkout(&bin, &cvs_data, &dir.path().join("cvsimport-no-p.log"));
    write_fake_cvsps(
        &bin,
        &cvsps_log,
        "---------------------\nPatchSet 1\nDate: 2001/01/01 00:00:00\nAuthor: dev <dev@example.test>\nBranch: HEAD\nTag: (none)\nLog:\nfirst import\nMembers:\n\ta.txt:INITIAL->1.1\n",
    );

    run_zmin_with_path(
        dir.path(),
        &bin,
        [
            "cvsimport",
            "-C",
            target.to_str().expect("target path"),
            "-d",
            cvs_data.to_str().expect("cvsroot path"),
            "module",
        ],
    );

    assert_eq!(
        fs::read_to_string(target.join("a.txt")).expect("read imported a"),
        "one\n"
    );
    assert_eq!(
        git(&target, ["log", "--format=%s", "refs/heads/origin"]),
        "first import"
    );
    let cvsps_invocation = fs::read_to_string(cvsps_log).expect("read cvsps log");
    assert!(cvsps_invocation.contains("-d "));
    assert!(cvsps_invocation.contains("module"));
}

#[cfg(unix)]
#[test]
fn p4_clone_imports_head_revision_into_git_refs_and_worktree() {
    let dir = TempDir::new().expect("temp dir");
    let bin = dir.path().join("bin");
    let data = dir.path().join("p4-data");
    let stock_target = dir.path().join("stock-project");
    let zmin_target = dir.path().join("zmin-project");
    fs::create_dir_all(&data).expect("create p4 data");
    fs::write(data.join("a.txt"), b"alpha\n").expect("write p4 a");
    fs::create_dir_all(data.join("dir")).expect("create p4 dir");
    fs::write(data.join("dir/b.txt"), b"bravo\n").expect("write p4 b");
    write_fake_p4(&bin, &data, &dir.path().join("p4.log"));

    let stock = run_command_with_path(
        stock_git_bin().to_str().expect("stock git path"),
        dir.path(),
        &bin,
        &[
            "p4",
            "clone",
            "--branch",
            "master",
            "//depot/project",
            stock_target.to_str().expect("stock target path"),
        ],
    );
    assert_eq!(stock.0, 0, "stock git stderr: {}", stock.2);

    let zmin = run_command_with_path(
        zmin_bin(),
        dir.path(),
        &bin,
        &[
            "p4",
            "clone",
            "--branch",
            "master",
            "//depot/project",
            zmin_target.to_str().expect("zmin target path"),
        ],
    );
    assert_eq!(zmin.0, 0, "zmin stderr: {}", zmin.2);

    assert_eq!(
        fs::read_to_string(zmin_target.join("a.txt")).expect("read zmin a"),
        "alpha\n"
    );
    assert_eq!(
        fs::read_to_string(zmin_target.join("dir/b.txt")).expect("read zmin b"),
        "bravo\n"
    );
    assert_eq!(
        normalize_p4_clone_stderr(&stock.2),
        normalize_p4_clone_stderr(&zmin.2)
    );
    assert_eq!(
        git(&stock_target, ["rev-parse", "--abbrev-ref", "HEAD"]),
        git(&zmin_target, ["rev-parse", "--abbrev-ref", "HEAD"])
    );
    assert_eq!(
        git(&zmin_target, ["rev-parse", "--abbrev-ref", "HEAD"]),
        "main"
    );
    assert_eq!(
        git(&zmin_target, ["rev-parse", "refs/remotes/p4/master"]),
        git(&zmin_target, ["rev-parse", "refs/heads/main"])
    );
    assert_eq!(
        git(
            &stock_target,
            ["log", "-1", "--format=%B", "refs/remotes/p4/master"]
        ),
        git(
            &zmin_target,
            ["log", "-1", "--format=%B", "refs/remotes/p4/master"]
        )
    );
    let log = fs::read_to_string(dir.path().join("p4.log")).expect("read p4 log");
    assert!(log.contains("-G files //depot/project/...#head"));
    assert!(log.contains("-G describe -s 2"));
    assert!(log.contains("-G -x - print"));
}

#[test]
fn p4_clone_noop_option_family_matches_stock_git() {
    for extra_args in [
        ["--changes-block-size=1"].as_slice(),
        ["--max-changes=1"].as_slice(),
    ] {
        let dir = TempDir::new().expect("temp dir");
        let bin = dir.path().join("bin");
        let data = dir.path().join("p4-data");
        let stock_target = dir.path().join("stock-project");
        let zmin_target = dir.path().join("zmin-project");
        let stock_log_path = dir.path().join("stock-p4.log");
        let zmin_log_path = dir.path().join("zmin-p4.log");
        fs::create_dir_all(&data).expect("create p4 data");
        fs::write(data.join("a.txt"), b"alpha\n").expect("write p4 a");
        fs::create_dir_all(data.join("dir")).expect("create p4 dir");
        fs::write(data.join("dir/b.txt"), b"bravo\n").expect("write p4 b");
        write_fake_p4(&bin, &data, &dir.path().join("unused-p4.log"));

        let mut stock_args = vec!["p4", "clone", "--branch", "master"];
        stock_args.extend_from_slice(extra_args);
        stock_args.push("//depot/project");
        stock_args.push(stock_target.to_str().expect("stock target path"));
        let stock = run_command_with_path_and_env(
            stock_git_bin().to_str().expect("stock git path"),
            dir.path(),
            &bin,
            &[(
                "P4_LOG_PATH",
                stock_log_path.to_str().expect("stock log path"),
            )],
            &stock_args,
        );
        assert_eq!(stock.0, 0, "stock git stderr: {}", stock.2);

        let mut zmin_args = vec!["p4", "clone", "--branch", "master"];
        zmin_args.extend_from_slice(extra_args);
        zmin_args.push("//depot/project");
        zmin_args.push(zmin_target.to_str().expect("zmin target path"));
        let zmin = run_command_with_path_and_env(
            zmin_bin(),
            dir.path(),
            &bin,
            &[(
                "P4_LOG_PATH",
                zmin_log_path.to_str().expect("zmin log path"),
            )],
            &zmin_args,
        );
        assert_eq!(zmin.0, 0, "zmin stderr: {}", zmin.2);

        assert_eq!(
            fs::read_to_string(zmin_target.join("a.txt")).expect("read zmin a"),
            "alpha\n"
        );
        assert_eq!(
            fs::read_to_string(zmin_target.join("dir/b.txt")).expect("read zmin b"),
            "bravo\n"
        );
        assert_eq!(
            git(
                &stock_target,
                ["log", "-1", "--format=%B", "refs/remotes/p4/master"]
            ),
            git(
                &zmin_target,
                ["log", "-1", "--format=%B", "refs/remotes/p4/master"]
            )
        );
        assert_eq!(
            git(&stock_target, ["rev-parse", "--abbrev-ref", "HEAD"]),
            git(&zmin_target, ["rev-parse", "--abbrev-ref", "HEAD"])
        );
        assert_eq!(
            git(&zmin_target, ["rev-parse", "refs/remotes/p4/master"]),
            git(&zmin_target, ["rev-parse", "refs/heads/main"])
        );

        let zmin_log = fs::read_to_string(zmin_log_path).expect("read zmin p4 log");
        assert!(zmin_log.contains("files //depot/project/..."));
        assert!(zmin_log.contains("print -q //depot/project/a.txt#1"));
        assert!(zmin_log.contains("print -q //depot/project/dir/b.txt#2"));
    }
}

#[test]
fn p4_submit_opens_changed_files_and_submits_head() {
    let dir = TempDir::new().expect("temp dir");
    let bin = dir.path().join("bin");
    let data = dir.path().join("p4-data");
    let seed_target = dir.path().join("seed-project");
    let stock_target = dir.path().join("stock-project");
    let zmin_target = dir.path().join("zmin-project");
    let stock_log_path = dir.path().join("stock-p4-submit.log");
    let zmin_log_path = dir.path().join("zmin-p4-submit.log");
    fs::create_dir_all(&data).expect("create p4 data");
    fs::write(data.join("a.txt"), b"alpha\n").expect("write p4 a");
    fs::create_dir_all(data.join("dir")).expect("create p4 dir");
    fs::write(data.join("dir/b.txt"), b"bravo\n").expect("write p4 b");
    write_fake_p4(&bin, &data, &dir.path().join("unused-p4.log"));

    let seed_clone = run_command_with_path_and_env(
        zmin_bin(),
        dir.path(),
        &bin,
        &[(
            "P4_LOG_PATH",
            zmin_log_path.to_str().expect("zmin log path"),
        )],
        &[
            "p4",
            "clone",
            "--branch",
            "master",
            "//depot/project",
            seed_target.to_str().expect("seed target path"),
        ],
    );
    assert_eq!(seed_clone.0, 0, "zmin clone stderr: {}", seed_clone.2);
    copy_dir_recursive(&seed_target, &stock_target);
    copy_dir_recursive(&seed_target, &zmin_target);

    for target in [&stock_target, &zmin_target] {
        configure_identity(target);
        git(target, ["config", "git-p4.skipSubmitEdit", "true"]);
        fs::write(target.join("a.txt"), b"alpha\nchanged\n").expect("modify a");
        fs::write(target.join("new.txt"), b"new\n").expect("write new");
        fs::remove_file(target.join("dir/b.txt")).expect("remove b");
        git(target, ["add", "-A"]);
        git_with_env(target, ["commit", "-m", "submit change"]);
    }

    let stock = run_command_with_path_and_env(
        stock_git_bin().to_str().expect("stock git path"),
        &stock_target,
        &bin,
        &[(
            "P4_LOG_PATH",
            stock_log_path.to_str().expect("stock log path"),
        )],
        &["p4", "submit"],
    );
    assert_eq!(stock.0, 0, "stock submit stderr: {}", stock.2);
    let zmin = run_command_with_path_and_env(
        zmin_bin(),
        &zmin_target,
        &bin,
        &[(
            "P4_LOG_PATH",
            zmin_log_path.to_str().expect("zmin log path"),
        )],
        &["p4", "submit"],
    );
    assert_eq!(zmin.0, 0, "zmin submit stderr: {}", zmin.2);

    assert_eq!(zmin.2, stock.2);
    assert_eq!(
        normalize_p4_submit_stdout(&zmin.1),
        normalize_p4_submit_stdout(&stock.1)
    );

    let log = fs::read_to_string(zmin_log_path).expect("read zmin p4 log");
    assert!(log.contains("sync"));
    assert!(log.contains("edit a.txt"));
    assert!(log.contains("add new.txt"));
    assert!(log.contains("delete dir/b.txt"));
    assert!(log.contains("submit -d submit change"));
    assert_eq!(
        git(&zmin_target, ["rev-parse", "refs/remotes/p4/master"]),
        git(&zmin_target, ["rev-parse", "HEAD"])
    );
}

#[test]
fn p4_submit_dry_run_option_family_matches_stock_git() {
    for extra_args in [["--dry-run"].as_slice(), ["-n"].as_slice()] {
        let dir = TempDir::new().expect("temp dir");
        let bin = dir.path().join("bin");
        let data = dir.path().join("p4-data");
        let seed_target = dir.path().join("seed-project");
        let stock_target = dir.path().join("stock-project");
        let zmin_target = dir.path().join("zmin-project");
        let seed_log_path = dir.path().join("seed-p4.log");
        let stock_log_path = dir.path().join("stock-p4-submit.log");
        let zmin_log_path = dir.path().join("zmin-p4-submit.log");
        fs::create_dir_all(&data).expect("create p4 data");
        fs::write(data.join("a.txt"), b"alpha\n").expect("write p4 a");
        fs::create_dir_all(data.join("dir")).expect("create p4 dir");
        fs::write(data.join("dir/b.txt"), b"bravo\n").expect("write p4 b");
        write_fake_p4(&bin, &data, &dir.path().join("unused-p4.log"));

        let seed_clone = run_command_with_path_and_env(
            zmin_bin(),
            dir.path(),
            &bin,
            &[("P4_LOG_PATH", seed_log_path.to_str().expect("seed log path"))],
            &[
                "p4",
                "clone",
                "--branch",
                "master",
                "//depot/project",
                seed_target.to_str().expect("seed target path"),
            ],
        );
        assert_eq!(seed_clone.0, 0, "zmin clone stderr: {}", seed_clone.2);
        copy_dir_recursive(&seed_target, &stock_target);
        copy_dir_recursive(&seed_target, &zmin_target);

        for target in [&stock_target, &zmin_target] {
            configure_identity(target);
            git(target, ["config", "git-p4.skipSubmitEdit", "true"]);
            fs::write(target.join("a.txt"), b"alpha\nchanged\n").expect("modify a");
            fs::write(target.join("new.txt"), b"new\n").expect("write new");
            fs::remove_file(target.join("dir/b.txt")).expect("remove b");
            git(target, ["add", "-A"]);
            git_with_env(target, ["commit", "-m", "submit change"]);
        }

        let mut stock_args = vec!["p4", "submit"];
        stock_args.extend_from_slice(extra_args);
        let stock = run_command_with_path_and_env(
            stock_git_bin().to_str().expect("stock git path"),
            &stock_target,
            &bin,
            &[(
                "P4_LOG_PATH",
                stock_log_path.to_str().expect("stock log path"),
            )],
            &stock_args,
        );
        assert_eq!(stock.0, 0, "stock submit stderr: {}", stock.2);

        let mut zmin_args = vec!["p4", "submit"];
        zmin_args.extend_from_slice(extra_args);
        let zmin = run_command_with_path_and_env(
            zmin_bin(),
            &zmin_target,
            &bin,
            &[(
                "P4_LOG_PATH",
                zmin_log_path.to_str().expect("zmin log path"),
            )],
            &zmin_args,
        );
        assert_eq!(zmin.0, 0, "zmin submit stderr: {}", zmin.2);

        assert_eq!(
            normalize_p4_submit_stdout(&zmin.1),
            normalize_p4_submit_stdout(&stock.1)
        );
        assert_eq!(zmin.2, stock.2);
        assert_eq!(
            git(&stock_target, ["rev-parse", "refs/remotes/p4/master"]),
            git(&zmin_target, ["rev-parse", "refs/remotes/p4/master"])
        );
        assert_eq!(
            git(&stock_target, ["rev-parse", "HEAD"]),
            git(&zmin_target, ["rev-parse", "HEAD"])
        );
        assert_eq!(
            git(&stock_target, ["rev-parse", "refs/remotes/p4/master"]),
            git(&stock_target, ["rev-parse", "HEAD~1"])
        );
        assert_eq!(
            git(&zmin_target, ["rev-parse", "refs/remotes/p4/master"]),
            git(&zmin_target, ["rev-parse", "HEAD~1"])
        );
        let stock_log = fs::read_to_string(&stock_log_path).unwrap_or_default();
        let zmin_log = fs::read_to_string(&zmin_log_path).unwrap_or_default();
        for forbidden in ["edit ", "add ", "delete ", "submit "] {
            assert!(
                !stock_log.contains(forbidden),
                "stock dry-run should not mutate p4: {stock_log}"
            );
            assert!(
                !zmin_log.contains(forbidden),
                "zmin dry-run should not mutate p4: {zmin_log}"
            );
        }
    }
}

#[test]
fn p4_unknown_subcommand_matches_stock_git_usage() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stock = git_failure_output(git_repo.path(), &["p4", "unknown"]);
    if !stock
        .1
        .contains("valid commands: submit, commit, sync, rebase, clone, branches, unshelve")
    {
        return;
    }
    let zmin = run_zmin_failure_output(zmin_repo.path(), &["p4", "unknown"]);

    assert_eq!(zmin.0, stock.0);
    assert_eq!(
        normalize_git_p4_usage_stdout(&zmin.1),
        normalize_git_p4_usage_stdout(&stock.1)
    );
    assert_eq!(zmin.2, stock.2);
}

#[test]
fn svn_clone_imports_head_tree_into_git_svn_ref_and_worktree() {
    let dir = TempDir::new().expect("temp dir");
    let bin = dir.path().join("bin");
    let data = dir.path().join("svn-data");
    let target = dir.path().join("project");
    fs::create_dir_all(data.join("dir")).expect("create svn dir");
    fs::write(data.join("a.txt"), b"alpha\n").expect("write svn a");
    fs::write(data.join("dir/b.txt"), b"bravo\n").expect("write svn b");
    write_fake_svn(&bin, &data, &dir.path().join("svn.log"));

    run_zmin_with_path(
        dir.path(),
        &bin,
        [
            "svn",
            "clone",
            "https://svn.example.test/project",
            target.to_str().expect("target path"),
        ],
    );

    assert_eq!(
        fs::read_to_string(target.join("a.txt")).expect("read a"),
        "alpha\n"
    );
    assert_eq!(
        fs::read_to_string(target.join("dir/b.txt")).expect("read b"),
        "bravo\n"
    );
    assert_eq!(
        git(&target, ["rev-parse", "refs/remotes/git-svn"]),
        git(&target, ["rev-parse", "refs/heads/master"])
    );
    assert_eq!(
        git(&target, ["config", "--get", "svn-remote.svn.url"]),
        "https://svn.example.test/project"
    );
    let log = fs::read_to_string(dir.path().join("svn.log")).expect("read svn log");
    assert!(log.contains("list -R https://svn.example.test/project"));
    assert!(log.contains("cat https://svn.example.test/project/a.txt"));
    assert!(log.contains("cat https://svn.example.test/project/dir/b.txt"));
}

#[test]
fn svn_dcommit_adds_deletes_commits_and_updates_git_svn_ref() {
    let dir = TempDir::new().expect("temp dir");
    let bin = dir.path().join("bin");
    let data = dir.path().join("svn-data");
    let target = dir.path().join("project");
    let log_path = dir.path().join("svn-dcommit.log");
    fs::create_dir_all(data.join("dir")).expect("create svn dir");
    fs::write(data.join("a.txt"), b"alpha\n").expect("write svn a");
    fs::write(data.join("dir/b.txt"), b"bravo\n").expect("write svn b");
    write_fake_svn(&bin, &data, &log_path);

    run_zmin_with_path(
        dir.path(),
        &bin,
        [
            "svn",
            "clone",
            "https://svn.example.test/project",
            target.to_str().expect("target path"),
        ],
    );
    configure_identity(&target);
    fs::write(target.join("a.txt"), b"alpha\nchanged\n").expect("modify a");
    fs::write(target.join("new.txt"), b"new\n").expect("write new");
    fs::remove_file(target.join("dir/b.txt")).expect("remove b");
    git(&target, ["add", "-A"]);
    git_with_env(&target, ["commit", "-m", "svn submit change"]);

    run_zmin_with_path(&target, &bin, ["svn", "dcommit"]);

    let log = fs::read_to_string(log_path).expect("read svn log");
    assert!(log.contains("add new.txt"));
    assert!(log.contains("delete dir/b.txt"));
    assert!(log.contains("commit -m svn submit change"));
    assert_eq!(
        git(&target, ["rev-parse", "refs/remotes/git-svn"]),
        git(&target, ["rev-parse", "HEAD"])
    );
}

#[test]
fn archimport_imports_tree_snapshot_into_git_repo() {
    let dir = TempDir::new().expect("temp dir");
    let bin = dir.path().join("bin");
    let data = dir.path().join("arch-data");
    let target = dir.path().join("project");
    fs::create_dir_all(data.join("dir")).expect("create arch dir");
    fs::create_dir_all(data.join("{arch}")).expect("create arch metadata");
    fs::write(data.join("a.txt"), b"alpha\n").expect("write arch a");
    fs::write(data.join("dir/b.txt"), b"bravo\n").expect("write arch b");
    fs::write(data.join("{arch}/internal"), b"ignored\n").expect("write arch metadata");
    fs::create_dir_all(&target).expect("create import dir");
    write_fake_tla(&bin, &data, &dir.path().join("tla.log"));

    run_zmin_with_path(
        &target,
        &bin,
        [
            "archimport",
            "-v",
            "archive@example.test/project--main--1--base-0:master",
        ],
    );

    assert_eq!(
        fs::read_to_string(target.join("a.txt")).expect("read a"),
        "alpha\n"
    );
    assert_eq!(
        fs::read_to_string(target.join("dir/b.txt")).expect("read b"),
        "bravo\n"
    );
    assert!(!target.join("{arch}/internal").exists());
    assert_eq!(
        git(&target, ["rev-parse", "--abbrev-ref", "HEAD"]),
        "master"
    );
    assert!(
        git(&target, ["log", "-1", "--format=%B"])
            .contains("git-archimport-id: archive@example.test/project--main--1--base-0")
    );
    let log = fs::read_to_string(dir.path().join("tla.log")).expect("read tla log");
    assert!(log.contains("get --no-pristine archive@example.test/project--main--1--base-0"));
}

#[test]
fn archimport_rejects_invalid_or_unsupported_invocations() {
    let dir = TempDir::new().expect("temp dir");
    let bin = dir.path().join("bin");
    let target = dir.path().join("project");
    fs::create_dir_all(&target).expect("create import dir");
    write_failing_tla(&bin, &dir.path().join("tla-fail.log"));

    assert_eq!(run_zmin_status(&target, ["archimport"]), 129);
    assert_eq!(
        run_zmin_status(&target, ["archimport", "-o", "archive/project"]),
        129
    );
    assert_ne!(
        run_zmin_with_path_status(
            &target,
            &bin,
            [
                "archimport",
                "archive@example.test/project--main--1--base-0:bad..branch",
            ],
        ),
        0
    );
    assert_eq!(
        run_zmin_with_path_status(
            &target,
            &bin,
            [
                "archimport",
                "archive@example.test/project--main--1--base-0"
            ],
        ),
        7
    );
}

#[test]
fn foreign_scm_adapters_cover_unsupported_and_client_failures() {
    let dir = TempDir::new().expect("temp dir");
    let bin = dir.path().join("bin");
    let p4_target = dir.path().join("p4-project");
    let svn_target = dir.path().join("svn-project");
    fs::create_dir_all(&p4_target).expect("create p4 target");
    fs::create_dir_all(&svn_target).expect("create svn target");
    write_failing_command(&bin, "p4", &dir.path().join("p4-fail.log"));
    write_failing_command(&bin, "svn", &dir.path().join("svn-fail.log"));

    assert_eq!(run_zmin_status(dir.path(), ["p4", "clone"]), 129);
    assert_eq!(run_zmin_status(dir.path(), ["p4", "submit"]), 128);
    assert_eq!(
        run_zmin_status(dir.path(), ["p4", "unknown"]),
        Command::new(stock_git_bin())
            .args(["p4", "unknown"])
            .current_dir(dir.path())
            .output()
            .expect("run stock git p4 unknown")
            .status
            .code()
            .expect("stock git p4 unknown exit code")
    );
    assert_eq!(
        run_zmin_with_path_status(
            dir.path(),
            &bin,
            [
                "p4",
                "clone",
                "//depot/project",
                p4_target.to_str().expect("p4 target"),
            ],
        ),
        7
    );

    assert_eq!(run_zmin_status(dir.path(), ["svn", "clone"]), 129);
    assert_eq!(run_zmin_status(dir.path(), ["svn", "dcommit"]), 128);
    assert_eq!(
        run_zmin_status(dir.path(), ["svn", "unknown"]),
        Command::new(stock_git_bin())
            .args(["svn", "unknown"])
            .current_dir(dir.path())
            .output()
            .expect("run stock git svn unknown")
            .status
            .code()
            .expect("stock git svn unknown exit code")
    );
    assert_eq!(
        run_zmin_with_path_status(
            dir.path(),
            &bin,
            [
                "svn",
                "clone",
                "https://svn.example.test/project",
                svn_target.to_str().expect("svn target"),
            ],
        ),
        7
    );
}

#[test]
fn cvsserver_valid_requests_match_git_232_protocol_start() {
    let repo = git_init();

    assert_eq!(run_zmin(repo.path(), ["cvsserver"]), "");
    assert_eq!(run_zmin(repo.path(), ["cvsserver", "-h"]), "");
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["cvsserver", "server"], "valid-requests\n"),
        "Valid-requests Argument Argumentx Directory Entry Global_option Modified Questionable Root Sticky Unchanged Valid-responses add admin annotate ci co diff editors expand-modules history log noop remove rlog status tag update valid-requests watchers\nok"
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["cvsserver", "server"], "noop\n"),
        "ok"
    );
}

fn run_zmin_with_path<const N: usize>(
    cwd: &std::path::Path,
    path_prefix: &std::path::Path,
    args: [&str; N],
) -> String {
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(path_prefix.to_path_buf()).chain(std::env::split_paths(&current_path)),
    )
    .expect("join PATH");
    let output = Command::new(zmin_bin())
        .args(args)
        .env("PATH", path)
        .current_dir(cwd)
        .output()
        .expect("run zmin");
    assert!(
        output.status.success(),
        "zmin failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("zmin stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn run_zmin_with_path_status<const N: usize>(
    cwd: &std::path::Path,
    path_prefix: &std::path::Path,
    args: [&str; N],
) -> i32 {
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(path_prefix.to_path_buf()).chain(std::env::split_paths(&current_path)),
    )
    .expect("join PATH");
    Command::new(zmin_bin())
        .args(args)
        .env("PATH", path)
        .current_dir(cwd)
        .output()
        .expect("run zmin")
        .status
        .code()
        .expect("zmin exited by signal")
}

fn run_command_with_path(
    program: &str,
    cwd: &std::path::Path,
    path_prefix: &std::path::Path,
    args: &[&str],
) -> (i32, String, String) {
    run_command_with_path_and_env(program, cwd, path_prefix, &[], args)
}

fn run_command_with_path_and_env(
    program: &str,
    cwd: &std::path::Path,
    path_prefix: &std::path::Path,
    envs: &[(&str, &str)],
    args: &[&str],
) -> (i32, String, String) {
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(path_prefix.to_path_buf()).chain(std::env::split_paths(&current_path)),
    )
    .expect("join PATH");
    let home = cwd.join("home");
    fs::create_dir_all(&home).expect("create command home");
    let mut command = Command::new(program);
    command
        .args(args)
        .env("PATH", path)
        .env("HOME", &home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME", "main")
        .current_dir(cwd)
        .envs(envs.iter().copied());
    if program == stock_git_bin().to_str().expect("stock git path") {
        command.env(
            "GIT_EXEC_PATH",
            git(
                stock_git_bin().parent().expect("stock git dir"),
                ["--exec-path"],
            ),
        );
    }
    let output = command.output().expect("run command");
    (
        output.status.code().expect("command exited by signal"),
        String::from_utf8(output.stdout)
            .expect("command stdout utf8")
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .expect("command stderr utf8")
            .trim_end_matches('\n')
            .to_owned(),
    )
}

fn normalize_git_p4_usage_stdout(stdout: &str) -> String {
    stdout
        .lines()
        .map(|line| {
            if let Some(rest) = line.strip_prefix("usage: ") {
                if rest.ends_with("git-p4 <command> [options]") {
                    return "usage: git-p4 <command> [options]".to_owned();
                }
            }
            if let Some(rest) = line.strip_prefix("Try ") {
                if rest.ends_with("git-p4 <command> --help for command specific help.") {
                    return "Try git-p4 <command> --help for command specific help.".to_owned();
                }
            }
            line.to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_p4_clone_stderr(stderr: &str) -> String {
    stderr
        .replace("stock-project", "<target>")
        .replace("zmin-project", "<target>")
}

fn normalize_p4_submit_stdout(stdout: &str) -> String {
    stdout
        .lines()
        .map(|line| {
            let line = line.trim_start_matches('\r');
            if let Some((prefix, _)) = line.split_once(" located at ") {
                if prefix.starts_with("Perforce checkout for depot path ") {
                    return format!("{prefix} located at <target>");
                }
            }
            if line.starts_with("Would synchronize p4 checkout in ") {
                return "Would synchronize p4 checkout in <target>".to_owned();
            }
            if let Some(rest) = line.strip_prefix("Applying ") {
                if let Some((_, message)) = rest.split_once(' ') {
                    return format!("Applying <commit> {message}");
                }
            }
            if line.starts_with("Importing revision ")
                && line.contains("Current branch ")
                && line.ends_with(" is up to date.")
            {
                let prefix = line
                    .split("Current branch ")
                    .next()
                    .expect("submit import prefix");
                return format!("{prefix}Current branch <branch> is up to date.");
            }
            line.to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn copy_dir_recursive(source: &std::path::Path, destination: &std::path::Path) {
    fs::create_dir_all(destination).expect("create destination dir");
    for entry in fs::read_dir(source).expect("read source dir") {
        let entry = entry.expect("read source entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().expect("source file type");
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &destination_path);
        } else {
            fs::copy(&source_path, &destination_path).expect("copy file");
        }
    }
}

#[cfg(unix)]
fn write_fake_cvs(bin: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake cvs bin");
    let script = bin.join("cvs");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = status ]; then shift; for f in \"$@\"; do printf 'File: %s Status: Up-to-date\\n' \"$f\"; done; fi\nexit 0\n",
            log.display()
        ),
    )
    .expect("write fake cvs");
    make_executable(&script);
}

#[cfg(windows)]
fn write_fake_cvs(bin: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake cvs bin");
    fs::write(
        bin.join("cvs.bat"),
        format!(
            "@echo off\r\necho %*>>\"{}\"\r\nif not \"%1\"==\"status\" exit /b 0\r\nshift\r\n:loop\r\nif \"%1\"==\"\" exit /b 0\r\necho File: %1 Status: Up-to-date\r\nshift\r\ngoto loop\r\n",
            log.display()
        ),
    )
    .expect("write fake cvs");
}

#[cfg(unix)]
fn write_fake_cvs_checkout(bin: &std::path::Path, data: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake cvs bin");
    let script = bin.join("cvs");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nrev=''\nlast=''\nwhile [ \"$#\" -gt 0 ]; do if [ \"$1\" = '-r' ]; then shift; rev=\"$1\"; fi; last=\"$1\"; shift; done\ncat '{}/'\"$last\"'/'\"$rev\"\n",
            log.display(),
            data.display()
        ),
    )
    .expect("write fake cvs checkout");
    make_executable(&script);
}

#[cfg(windows)]
fn write_fake_cvs_checkout(bin: &std::path::Path, data: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake cvs bin");
    write_windows_powershell_command(
        bin,
        "cvs",
        format!(
            "$ErrorActionPreference = 'Stop'\r\nfunction Write-RawFile([string]$Path) {{\r\n  $bytes = [IO.File]::ReadAllBytes($Path)\r\n  [Console]::OpenStandardOutput().Write($bytes, 0, $bytes.Length)\r\n}}\r\nAdd-Content -LiteralPath {} -Value ($args -join ' ')\r\n$rev = ''\r\n$last = ''\r\nfor ($i = 0; $i -lt $args.Count; $i++) {{\r\n  if ($args[$i] -eq '-r') {{\r\n    $i++\r\n    $rev = $args[$i]\r\n  }}\r\n  $last = $args[$i]\r\n}}\r\nWrite-RawFile (Join-Path (Join-Path {} $last) $rev)\r\n",
            ps_literal(log),
            ps_literal(data)
        ),
    );
}

#[cfg(unix)]
fn write_fake_cvsps(bin: &std::path::Path, log: &std::path::Path, output: &str) {
    fs::create_dir_all(bin).expect("create fake cvsps bin");
    let script = bin.join("cvsps");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncat <<'EOF'\n{}EOF\n",
            log.display(),
            output
        ),
    )
    .expect("write fake cvsps");
    make_executable(&script);
}

#[cfg(windows)]
fn write_fake_cvsps(bin: &std::path::Path, log: &std::path::Path, output: &str) {
    fs::create_dir_all(bin).expect("create fake cvsps bin");
    write_windows_powershell_command(
        bin,
        "cvsps",
        format!(
            "$ErrorActionPreference = 'Stop'\r\nAdd-Content -LiteralPath {} -Value ($args -join ' ')\r\nWrite-Output @'\r\n{}'@\r\n",
            ps_literal(log),
            output
        ),
    );
}

#[cfg(unix)]
fn write_fake_p4(bin: &std::path::Path, data: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake p4 bin");
    let script = bin.join("p4");
    fs::write(
        &script,
        format!(
            "#!/usr/bin/env python3\nimport marshal\nimport os\nimport pathlib\nimport shutil\nimport sys\n\nLOG = pathlib.Path(os.environ.get('P4_LOG_PATH', {log:?}))\nDATA = pathlib.Path({data:?})\nargs = sys.argv[1:]\nplain_args = list(args)\nwhile plain_args[:1] and plain_args[0] == '-r' and len(plain_args) >= 2:\n    plain_args = plain_args[2:]\nCWD = pathlib.Path.cwd()\nLOG.parent.mkdir(parents=True, exist_ok=True)\nwith LOG.open('a', encoding='utf-8') as handle:\n    handle.write(' '.join(args) + '\\n')\n\nFILES = {{\n    '//depot/project/a.txt#1': (b'alpha\\n', b'add', b'1'),\n    '//depot/project/dir/b.txt#2': (b'bravo\\n', b'edit', b'2'),\n}}\n\nOPENED_TYPES = {{\n    'a.txt': 'text',\n    'dir/b.txt': 'text',\n    'new.txt': 'text',\n}}\n\ndef reset_workspace():\n    for rel in ['a.txt', 'dir/b.txt', 'new.txt', 'a.txt#1', 'dir/b.txt#2']:\n        path = CWD / rel\n        if path.exists() or path.is_symlink():\n            path.unlink()\n    shutil.rmtree(CWD / 'dir', ignore_errors=True)\n    (CWD / 'dir').mkdir(parents=True, exist_ok=True)\n    (CWD / 'a.txt').write_bytes((DATA / 'a.txt').read_bytes())\n    (CWD / 'dir' / 'b.txt').write_bytes((DATA / 'dir' / 'b.txt').read_bytes())\n\nif '-G' in args:\n    payload = []\n    if 'login' in args and '-s' in args:\n        payload.append({{b'code': b'stat', b'User': b'p4-user'}})\n    elif 'user' in args and '-o' in args:\n        payload.append({{b'code': b'stat', b'User': b'p4-user'}})\n    elif 'users' in args:\n        payload.append({{b'code': b'stat', b'User': b'p4-user', b'Email': b'a@b', b'FullName': b'git perforce import user'}})\n    elif 'describe' in args and '-s' in args:\n        payload.append({{b'code': b'stat', b'desc': b'Import snapshot', b'user': b'p4-user', b'time': b'1700000000', b'change': b'2'}})\n    elif 'changes' in args:\n        payload.append({{b'code': b'stat', b'change': b'2'}})\n    elif 'files' in args:\n        payload.extend([\n            {{b'code': b'stat', b'depotFile': b'//depot/project/a.txt', b'rev': b'1', b'action': b'add', b'change': b'1', b'type': b'text'}},\n            {{b'code': b'stat', b'depotFile': b'//depot/project/dir/b.txt', b'rev': b'2', b'action': b'edit', b'change': b'2', b'type': b'text'}},\n        ])\n    elif 'where' in args:\n        payload.append({{b'code': b'stat', b'depotFile': b'//depot/project/...', b'path': str(CWD / '...').encode()}})\n    elif 'opened' in args:\n        payload = []\n    elif 'change' in args and '-o' in args:\n        payload.append({{b'code': b'stat', b'Change': b'new', b'Client': b'fake-client', b'User': b'p4-user', b'Status': b'new', b'Description': b'<enter description here>', b'File0': b'//depot/project/a.txt', b'File1': b'//depot/project/new.txt'}})\n    elif '-x' in args and 'print' in args:\n        stdin_items = [line.strip() for line in sys.stdin.read().splitlines() if line.strip()]\n        for depot in stdin_items:\n            data, action, rev = FILES[depot]\n            payload.append({{b'code': b'stat', b'depotFile': depot.encode(), b'data': data, b'action': action, b'rev': rev, b'type': b'text'}})\n    else:\n        sys.exit(1)\n    for row in payload:\n        marshal.dump(row, sys.stdout.buffer)\n    sys.exit(0)\n\nif plain_args and plain_args[0] == 'files':\n    print('//depot/project/a.txt#1 - add change 1 (text)')\n    print('//depot/project/dir/b.txt#2 - edit change 2 (text)')\n    sys.exit(0)\nif plain_args and plain_args[0] == 'print':\n    depot = plain_args[2]\n    rel = depot.removeprefix('//depot/project/').split('#', 1)[0]\n    sys.stdout.buffer.write((DATA / rel).read_bytes())\n    sys.exit(0)\nif plain_args and plain_args[0] == 'sync':\n    reset_workspace()\n    sys.exit(0)\nif plain_args and plain_args[0] == 'diff':\n    sys.exit(0)\nif plain_args and plain_args[0] == 'opened':\n    if len(plain_args) > 1 and plain_args[1] in OPENED_TYPES:\n        print(plain_args[1] + '#1 - opened for edit change 1 (' + OPENED_TYPES[plain_args[1]] + ')')\n    sys.exit(0)\nif plain_args and plain_args[0] in {{'edit', 'add', 'delete', 'submit', 'revert', 'reopen'}}:\n    sys.exit(0)\nsys.exit(1)\n",
            log = log.display().to_string(),
            data = data.display().to_string(),
        ),
    )
    .expect("write fake p4");
    make_executable(&script);
}

#[cfg(windows)]
fn write_fake_p4(bin: &std::path::Path, data: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake p4 bin");
    write_windows_powershell_command(
        bin,
        "p4",
        format!(
            "$ErrorActionPreference = 'Stop'\r\nfunction Write-RawFile([string]$Path) {{\r\n  $bytes = [IO.File]::ReadAllBytes($Path)\r\n  [Console]::OpenStandardOutput().Write($bytes, 0, $bytes.Length)\r\n}}\r\nAdd-Content -LiteralPath {} -Value ($args -join ' ')\r\nswitch ($args[0]) {{\r\n  'files' {{\r\n    Write-Output '//depot/project/a.txt#1 - add change 1 (text)'\r\n    Write-Output '//depot/project/dir/b.txt#2 - edit change 2 (text)'\r\n    exit 0\r\n  }}\r\n  'print' {{\r\n    switch ($args[2]) {{\r\n      '//depot/project/a.txt#1' {{ Write-RawFile {}; exit 0 }}\r\n      '//depot/project/dir/b.txt#2' {{ Write-RawFile {}; exit 0 }}\r\n    }}\r\n    exit 1\r\n  }}\r\n  {{ @('edit', 'add', 'delete', 'submit') -contains $_ }} {{ exit 0 }}\r\n}}\r\nexit 1\r\n",
            ps_literal(log),
            ps_literal(data.join("a.txt")),
            ps_literal(data.join("dir/b.txt"))
        ),
    );
}

#[cfg(unix)]
fn write_fake_svn(bin: &std::path::Path, data: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake svn bin");
    let script = bin.join("svn");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = list ]; then echo 'a.txt'; echo 'dir/'; echo 'dir/b.txt'; exit 0; fi\nif [ \"$1\" = cat ]; then case \"$2\" in */a.txt) cat '{}/a.txt' ;; */dir/b.txt) cat '{}/dir/b.txt' ;; *) exit 1 ;; esac; exit 0; fi\ncase \"$1\" in add|delete|commit) exit 0 ;; esac\nexit 1\n",
            log.display(),
            data.display(),
            data.display()
        ),
    )
    .expect("write fake svn");
    make_executable(&script);
}

#[cfg(windows)]
fn write_fake_svn(bin: &std::path::Path, data: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake svn bin");
    write_windows_powershell_command(
        bin,
        "svn",
        format!(
            "$ErrorActionPreference = 'Stop'\r\nfunction Write-RawFile([string]$Path) {{\r\n  $bytes = [IO.File]::ReadAllBytes($Path)\r\n  [Console]::OpenStandardOutput().Write($bytes, 0, $bytes.Length)\r\n}}\r\nAdd-Content -LiteralPath {} -Value ($args -join ' ')\r\nswitch ($args[0]) {{\r\n  'list' {{\r\n    Write-Output 'a.txt'\r\n    Write-Output 'dir/'\r\n    Write-Output 'dir/b.txt'\r\n    exit 0\r\n  }}\r\n  'cat' {{\r\n    if ($args[1] -like '*/a.txt') {{ Write-RawFile {}; exit 0 }}\r\n    if ($args[1] -like '*/dir/b.txt') {{ Write-RawFile {}; exit 0 }}\r\n    exit 1\r\n  }}\r\n  {{ @('add', 'delete', 'commit') -contains $_ }} {{ exit 0 }}\r\n}}\r\nexit 1\r\n",
            ps_literal(log),
            ps_literal(data.join("a.txt")),
            ps_literal(data.join("dir/b.txt"))
        ),
    );
}

#[cfg(unix)]
fn write_fake_tla(bin: &std::path::Path, data: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake tla bin");
    let script = bin.join("tla");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = get ]; then mkdir -p \"$4\"; cp -R '{}/.' \"$4/\"; exit 0; fi\nexit 1\n",
            log.display(),
            data.display()
        ),
    )
    .expect("write fake tla");
    make_executable(&script);
}

#[cfg(windows)]
fn write_fake_tla(bin: &std::path::Path, data: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake tla bin");
    fs::write(
        bin.join("tla.bat"),
        format!(
            "@echo off\r\necho %*>>\"{}\"\r\nif \"%1\"==\"get\" (\r\nmkdir \"%4\" 2>nul\r\nxcopy /E /I /Y \"{}\" \"%4\" >nul\r\nexit /b 0\r\n)\r\nexit /b 1\r\n",
            log.display(),
            data.display()
        ),
    )
    .expect("write fake tla");
}

#[cfg(unix)]
fn write_failing_tla(bin: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake tla bin");
    let script = bin.join("tla");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nprintf 'arch failure\\n' >&2\nexit 7\n",
            log.display()
        ),
    )
    .expect("write failing tla");
    make_executable(&script);
}

#[cfg(windows)]
fn write_failing_tla(bin: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake tla bin");
    fs::write(
        bin.join("tla.bat"),
        format!(
            "@echo off\r\necho %*>>\"{}\"\r\necho arch failure 1>&2\r\nexit /b 7\r\n",
            log.display()
        ),
    )
    .expect("write failing tla");
}

#[cfg(unix)]
fn write_failing_command(bin: &std::path::Path, name: &str, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake command bin");
    let script = bin.join(name);
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nprintf '{} failure\\n' >&2\nexit 7\n",
            log.display(),
            name
        ),
    )
    .expect("write failing command");
    make_executable(&script);
}

#[cfg(windows)]
fn write_failing_command(bin: &std::path::Path, name: &str, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake command bin");
    fs::write(
        bin.join(format!("{name}.bat")),
        format!(
            "@echo off\r\necho %*>>\"{}\"\r\necho {} failure 1>&2\r\nexit /b 7\r\n",
            log.display(),
            name
        ),
    )
    .expect("write failing command");
}

#[cfg(windows)]
fn write_windows_powershell_command(bin: &std::path::Path, name: &str, script_body: String) {
    fs::write(bin.join(format!("{name}.ps1")), script_body).expect("write fake command ps1");
    fs::write(
        bin.join(format!("{name}.bat")),
        format!(
            "@echo off\r\npowershell -NoProfile -ExecutionPolicy Bypass -File \"%~dp0{name}.ps1\" %*\r\nexit /b %ERRORLEVEL%\r\n"
        ),
    )
    .expect("write fake command bat");
}

#[cfg(windows)]
fn ps_literal(path: impl AsRef<std::path::Path>) -> String {
    format!(
        "'{}'",
        path.as_ref().display().to_string().replace('\'', "''")
    )
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path).expect("script metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("chmod script");
}

#[cfg(windows)]
fn make_executable(_path: &std::path::Path) {}
