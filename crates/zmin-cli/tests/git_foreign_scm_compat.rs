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

#[cfg(unix)]
#[test]
fn cvsexportcommit_option_family_matches_stock_git() {
    for (label, extra_args) in [
        ("verbose", vec!["-v"]),
        ("add-author", vec!["-a"]),
        ("msgprefix", vec!["-m", "PREFIX:"]),
        ("cvsroot", vec!["-d", "/tmp/fake-root"]),
        ("update", vec!["-u"]),
        ("pedantic", vec!["-p"]),
        ("force", vec!["-f"]),
        ("commit", vec!["-c"]),
    ] {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join("source");
        let seed_cvs = dir.path().join("seed-cvs");
        let stock_cvs = dir.path().join("stock-cvs");
        let zmin_cvs = dir.path().join("zmin-cvs");
        let stock_bin = dir.path().join("stock-bin");
        let zmin_bin_dir = dir.path().join("zmin-bin");
        setup_cvsexportcommit_fixture(&source, &seed_cvs, false);
        copy_dir_recursive(&seed_cvs, &stock_cvs);
        copy_dir_recursive(&seed_cvs, &zmin_cvs);
        write_fake_cvs(&stock_bin, &dir.path().join(format!("{label}-stock.log")));
        write_fake_cvs(&zmin_bin_dir, &dir.path().join(format!("{label}-zmin.log")));

        let mut stock_args = vec!["cvsexportcommit"];
        stock_args.extend(extra_args.iter().copied());
        stock_args.push("-w");
        stock_args.push(stock_cvs.to_str().expect("stock cvs path"));
        stock_args.push("HEAD");
        let stock = run_command_with_path(
            stock_git_bin().to_str().expect("stock git path"),
            &source,
            &stock_bin,
            &stock_args,
        );

        let mut zmin_args = vec!["cvsexportcommit"];
        zmin_args.extend(extra_args.iter().copied());
        zmin_args.push("-w");
        zmin_args.push(zmin_cvs.to_str().expect("zmin cvs path"));
        zmin_args.push("HEAD");
        let zmin = run_command_with_path(zmin_bin(), &source, &zmin_bin_dir, &zmin_args);

        assert_eq!(zmin.0, stock.0, "{label} zmin stderr: {}", zmin.2);
        assert_eq!(zmin.1, stock.1, "{label}");
        assert_eq!(zmin.2, stock.2, "{label}");
        assert_eq!(
            visible_non_git_file_contents(&stock_cvs),
            visible_non_git_file_contents(&zmin_cvs),
            "{label}"
        );
        let stock_log = fs::read_to_string(dir.path().join(format!("{label}-stock.log")))
            .expect("read stock cvs log");
        let zmin_log =
            fs::read_to_string(dir.path().join(format!("{label}-zmin.log"))).expect("read zmin cvs log");
        assert_eq!(
            normalize_cvs_log(&stock_log),
            normalize_cvs_log(&zmin_log),
            "{label}"
        );
    }
}

#[cfg(unix)]
#[test]
fn cvsexportcommit_keyword_reverse_failure_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let seed_cvs = dir.path().join("seed-cvs");
    let stock_cvs = dir.path().join("stock-cvs");
    let zmin_cvs = dir.path().join("zmin-cvs");
    let stock_bin = dir.path().join("stock-bin");
    let zmin_bin_dir = dir.path().join("zmin-bin");
    setup_cvsexportcommit_fixture(&source, &seed_cvs, true);
    copy_dir_recursive(&seed_cvs, &stock_cvs);
    copy_dir_recursive(&seed_cvs, &zmin_cvs);
    write_fake_cvs(&stock_bin, &dir.path().join("reverse-stock.log"));
    write_fake_cvs(&zmin_bin_dir, &dir.path().join("reverse-zmin.log"));

    let stock = run_command_with_path(
        stock_git_bin().to_str().expect("stock git path"),
        &source,
        &stock_bin,
        &[
            "cvsexportcommit",
            "-k",
            "-w",
            stock_cvs.to_str().expect("stock cvs path"),
            "HEAD",
        ],
    );
    let zmin = run_command_with_path(
        zmin_bin(),
        &source,
        &zmin_bin_dir,
        &[
            "cvsexportcommit",
            "-k",
            "-w",
            zmin_cvs.to_str().expect("zmin cvs path"),
            "HEAD",
        ],
    );

    assert_ne!(stock.0, 0, "stock unexpectedly succeeded");
    assert_eq!(zmin.0, stock.0);
    assert_eq!(zmin.1, stock.1);
    assert_eq!(
        normalize_cvsexportcommit_stderr(&zmin.2),
        normalize_cvsexportcommit_stderr(&stock.2)
    );
    assert_eq!(
        visible_non_git_file_contents(&stock_cvs),
        visible_non_git_file_contents(&zmin_cvs)
    );
    let stock_log = fs::read_to_string(dir.path().join("reverse-stock.log"))
        .expect("read stock cvs log");
    let zmin_log =
        fs::read_to_string(dir.path().join("reverse-zmin.log")).expect("read zmin cvs log");
    assert_eq!(normalize_cvs_log(&stock_log), normalize_cvs_log(&zmin_log));
}

#[cfg(unix)]
#[test]
fn cvsexportcommit_force_parent_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let base = dir.path().join("base");
    let stock_cvs = dir.path().join("stock-cvs");
    let zmin_cvs = dir.path().join("zmin-cvs");
    let stock_bin = dir.path().join("stock-bin");
    let zmin_bin_dir = dir.path().join("zmin-bin");
    let source = dir.path().join("source");
    let base_commit = setup_cvsexportcommit_force_parent_fixture(&base, &source, &stock_cvs, &zmin_cvs);
    write_fake_cvs(&stock_bin, &dir.path().join("force-parent-stock.log"));
    write_fake_cvs(&zmin_bin_dir, &dir.path().join("force-parent-zmin.log"));

    let stock = run_command_with_path(
        stock_git_bin().to_str().expect("stock git path"),
        &source,
        &stock_bin,
        &[
            "cvsexportcommit",
            "-P",
            "-w",
            stock_cvs.to_str().expect("stock cvs path"),
            &base_commit,
            "HEAD",
        ],
    );
    let zmin = run_command_with_path(
        zmin_bin(),
        &source,
        &zmin_bin_dir,
        &[
            "cvsexportcommit",
            "-P",
            "-w",
            zmin_cvs.to_str().expect("zmin cvs path"),
            &base_commit,
            "HEAD",
        ],
    );

    assert_eq!(
        zmin.0, stock.0,
        "stock stdout:\n{}\nstock stderr:\n{}\nzmin stdout:\n{}\nzmin stderr:\n{}",
        stock.1, stock.2, zmin.1, zmin.2
    );
    assert_eq!(zmin.1, stock.1);
    assert_eq!(zmin.2, stock.2);
    assert_eq!(
        visible_non_git_file_contents(&stock_cvs),
        visible_non_git_file_contents(&zmin_cvs)
    );
    let stock_log =
        fs::read_to_string(dir.path().join("force-parent-stock.log")).expect("read stock cvs log");
    let zmin_log =
        fs::read_to_string(dir.path().join("force-parent-zmin.log")).expect("read zmin cvs log");
    assert_eq!(normalize_cvs_log(&stock_log), normalize_cvs_log(&zmin_log));
}

#[cfg(unix)]
#[test]
fn cvsexportcommit_same_worktree_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let seed = dir.path().join("seed");
    let stock_source = dir.path().join("stock-source");
    let zmin_source = dir.path().join("zmin-source");
    let stock_bin = dir.path().join("stock-bin");
    let zmin_bin_dir = dir.path().join("zmin-bin");
    setup_cvsexportcommit_same_worktree_fixture(&seed);
    copy_dir_recursive(&seed, &stock_source);
    copy_dir_recursive(&seed, &zmin_source);
    write_fake_cvs(&stock_bin, &dir.path().join("same-worktree-stock.log"));
    write_fake_cvs(&zmin_bin_dir, &dir.path().join("same-worktree-zmin.log"));

    let stock = run_command_with_path(
        stock_git_bin().to_str().expect("stock git path"),
        &stock_source,
        &stock_bin,
        &["cvsexportcommit", "-W", "refs/heads/export-target"],
    );
    let zmin = run_command_with_path(
        zmin_bin(),
        &zmin_source,
        &zmin_bin_dir,
        &["cvsexportcommit", "-W", "refs/heads/export-target"],
    );

    assert_eq!(
        zmin.0, stock.0,
        "stock stdout:\n{}\nstock stderr:\n{}\nzmin stdout:\n{}\nzmin stderr:\n{}",
        stock.1, stock.2, zmin.1, zmin.2
    );
    assert_eq!(zmin.1, stock.1);
    assert_eq!(zmin.2, stock.2);
    assert_eq!(
        visible_non_git_file_contents(&stock_source),
        visible_non_git_file_contents(&zmin_source)
    );
    let stock_log = fs::read_to_string(dir.path().join("same-worktree-stock.log"))
        .expect("read stock cvs log");
    let zmin_log = fs::read_to_string(dir.path().join("same-worktree-zmin.log"))
        .expect("read zmin cvs log");
    assert_eq!(normalize_cvs_log(&stock_log), normalize_cvs_log(&zmin_log));
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
fn p4_clone_transcript_control_option_family_matches_stock_git() {
    for extra_args in [
        ["--verbose"].as_slice(),
        ["-v"].as_slice(),
        ["--silent"].as_slice(),
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
            normalize_p4_clone_stdout(&stock.1),
            normalize_p4_clone_stdout(&zmin.1)
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
            git(
                &stock_target,
                ["log", "-1", "--format=%B", "refs/remotes/p4/master"]
            ),
            git(
                &zmin_target,
                ["log", "-1", "--format=%B", "refs/remotes/p4/master"]
            )
        );

        let zmin_log = fs::read_to_string(zmin_log_path).expect("read zmin p4 log");
        assert!(zmin_log.contains("files //depot/project/..."));
        assert!(zmin_log.contains("print -q //depot/project/a.txt#1"));
        assert!(zmin_log.contains("print -q //depot/project/dir/b.txt#2"));
    }
}

#[test]
fn p4_clone_helper_sensitive_failure_option_family_matches_stock_git() {
    for extra_args in [
        ["--detect-branches"].as_slice(),
        ["--detect-labels"].as_slice(),
        ["--import-labels"].as_slice(),
        ["--use-client-spec"].as_slice(),
    ] {
        let dir = TempDir::new().expect("temp dir");
        let bin = dir.path().join("bin");
        let data = dir.path().join("p4-data");
        let stock_target = dir.path().join("stock-project");
        let zmin_target = dir.path().join("zmin-project");
        fs::create_dir_all(&data).expect("create p4 data");
        fs::write(data.join("a.txt"), b"alpha\n").expect("write p4 a");
        fs::create_dir_all(data.join("dir")).expect("create p4 dir");
        fs::write(data.join("dir/b.txt"), b"bravo\n").expect("write p4 b");
        write_fake_p4(&bin, &data, &dir.path().join("unused-p4.log"));

        let mut stock_args = vec!["p4", "clone", "--branch", "master"];
        stock_args.extend_from_slice(extra_args);
        stock_args.push("//depot/project");
        stock_args.push(stock_target.to_str().expect("stock target path"));
        let stock = run_command_with_path(
            stock_git_bin().to_str().expect("stock git path"),
            dir.path(),
            &bin,
            &stock_args,
        );
        assert_ne!(stock.0, 0, "stock git unexpectedly succeeded");

        let mut zmin_args = vec!["p4", "clone", "--branch", "master"];
        zmin_args.extend_from_slice(extra_args);
        zmin_args.push("//depot/project");
        zmin_args.push(zmin_target.to_str().expect("zmin target path"));
        let zmin = run_command_with_path(
            zmin_bin(),
            dir.path(),
            &bin,
            &zmin_args,
        );
        assert_ne!(zmin.0, 0, "zmin unexpectedly succeeded");

        assert_eq!(zmin.0, stock.0);
        assert_eq!(
            normalize_p4_clone_stdout(&zmin.1),
            normalize_p4_clone_stdout(&stock.1)
        );
        assert_eq!(
            normalize_p4_clone_helper_sensitive_stderr(&zmin.2),
            normalize_p4_clone_helper_sensitive_stderr(&stock.2)
        );
        assert_eq!(
            git(&stock_target, ["status", "--short"]),
            git(&zmin_target, ["status", "--short"])
        );
    }
}

#[test]
fn p4_clone_import_local_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let bin = dir.path().join("bin");
    let data = dir.path().join("p4-data");
    let stock_target = dir.path().join("stock-project");
    let zmin_target = dir.path().join("zmin-project");
    fs::create_dir_all(&data).expect("create p4 data");
    fs::write(data.join("a.txt"), b"alpha\n").expect("write p4 a");
    fs::create_dir_all(data.join("dir")).expect("create p4 dir");
    fs::write(data.join("dir/b.txt"), b"bravo\n").expect("write p4 b");
    write_fake_p4(&bin, &data, &dir.path().join("unused-p4.log"));

    let stock = run_command_with_path(
        stock_git_bin().to_str().expect("stock git path"),
        dir.path(),
        &bin,
        &[
            "p4",
            "clone",
            "--branch",
            "master",
            "--import-local",
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
            "--import-local",
            "//depot/project",
            zmin_target.to_str().expect("zmin target path"),
        ],
    );
    assert_eq!(zmin.0, 0, "zmin stderr: {}", zmin.2);

    assert_eq!(
        normalize_p4_clone_stdout(&zmin.1),
        normalize_p4_clone_stdout(&stock.1)
    );
    assert_eq!(
        normalize_p4_clone_stderr(&zmin.2),
        normalize_p4_clone_stderr(&stock.2)
    );
    assert_eq!(
        git(&stock_target, ["rev-parse", "--abbrev-ref", "HEAD"]),
        git(&zmin_target, ["rev-parse", "--abbrev-ref", "HEAD"])
    );
    assert_eq!(
        git(
            &stock_target,
            ["show-ref", "--verify", "--hash", "refs/heads/p4/master"]
        ),
        git(
            &zmin_target,
            ["show-ref", "--verify", "--hash", "refs/heads/p4/master"]
        )
    );
    assert_eq!(
        git(
            &stock_target,
            ["log", "-1", "--format=%B", "refs/heads/p4/master"]
        ),
        git(
            &zmin_target,
            ["log", "-1", "--format=%B", "refs/heads/p4/master"]
        )
    );
}

#[test]
fn p4_clone_failure_tail_option_family_matches_stock_git() {
    for extra_args in [
        ["--keep-path"].as_slice(),
        ["--destination", "destdir"].as_slice(),
    ] {
        let dir = TempDir::new().expect("temp dir");
        let bin = dir.path().join("bin");
        let data = dir.path().join("p4-data");
        let stock_target = dir.path().join("stock-project");
        let zmin_target = dir.path().join("zmin-project");
        fs::create_dir_all(&data).expect("create p4 data");
        fs::write(data.join("a.txt"), b"alpha\n").expect("write p4 a");
        fs::create_dir_all(data.join("dir")).expect("create p4 dir");
        fs::write(data.join("dir/b.txt"), b"bravo\n").expect("write p4 b");
        write_fake_p4(&bin, &data, &dir.path().join("unused-p4.log"));

        let mut stock_args = vec!["p4", "clone", "--branch", "master"];
        stock_args.extend_from_slice(extra_args);
        stock_args.push("//depot/project");
        stock_args.push(stock_target.to_str().expect("stock target path"));
        let stock = run_command_with_path(
            stock_git_bin().to_str().expect("stock git path"),
            dir.path(),
            &bin,
            &stock_args,
        );
        assert_ne!(stock.0, 0, "stock clone unexpectedly succeeded");

        let mut zmin_args = vec!["p4", "clone", "--branch", "master"];
        zmin_args.extend_from_slice(extra_args);
        zmin_args.push("//depot/project");
        zmin_args.push(zmin_target.to_str().expect("zmin target path"));
        let zmin = run_command_with_path(
            zmin_bin(),
            dir.path(),
            &bin,
            &zmin_args,
        );
        assert_ne!(zmin.0, 0, "zmin clone unexpectedly succeeded");

        assert_eq!(zmin.0, stock.0);
        assert_eq!(
            normalize_p4_clone_stdout(&zmin.1),
            normalize_p4_clone_stdout(&stock.1)
        );
        assert_eq!(
            normalize_p4_clone_stderr(&zmin.2),
            normalize_p4_clone_stderr(&stock.2)
        );
        assert_eq!(stock_target.exists(), zmin_target.exists());
    }
}

#[test]
fn p4_clone_repo_shape_option_family_matches_stock_git() {
    for extra_args in [
        ["--changesfile", "__CHANGESFILE__"].as_slice(),
        ["--bare"].as_slice(),
    ] {
        let dir = TempDir::new().expect("temp dir");
        let bin = dir.path().join("bin");
        let data = dir.path().join("p4-data");
        let stock_target = dir.path().join("stock-project");
        let zmin_target = dir.path().join("zmin-project");
        fs::create_dir_all(&data).expect("create p4 data");
        fs::write(data.join("a.txt"), b"alpha\n").expect("write p4 a");
        fs::create_dir_all(data.join("dir")).expect("create p4 dir");
        fs::write(data.join("dir/b.txt"), b"bravo\n").expect("write p4 b");
        let changesfile = dir.path().join("changes.txt");
        fs::write(&changesfile, b"2\n").expect("write changes file");
        write_fake_p4(&bin, &data, &dir.path().join("unused-p4.log"));

        let mut stock_args = vec!["p4", "clone", "--branch", "master"];
        if extra_args[0] == "--changesfile" {
            stock_args.extend_from_slice(&[
                "--changesfile",
                changesfile.to_str().expect("changesfile path"),
            ]);
        } else {
            stock_args.extend_from_slice(extra_args);
        }
        stock_args.push("//depot/project");
        stock_args.push(stock_target.to_str().expect("stock target path"));
        let stock = run_command_with_path(
            stock_git_bin().to_str().expect("stock git path"),
            dir.path(),
            &bin,
            &stock_args,
        );
        assert_eq!(stock.0, 0, "stock clone stderr: {}", stock.2);

        let mut zmin_args = vec!["p4", "clone", "--branch", "master"];
        if extra_args[0] == "--changesfile" {
            zmin_args.extend_from_slice(&[
                "--changesfile",
                changesfile.to_str().expect("changesfile path"),
            ]);
        } else {
            zmin_args.extend_from_slice(extra_args);
        }
        zmin_args.push("//depot/project");
        zmin_args.push(zmin_target.to_str().expect("zmin target path"));
        let zmin = run_command_with_path(
            zmin_bin(),
            dir.path(),
            &bin,
            &zmin_args,
        );
        assert_eq!(zmin.0, 0, "zmin clone stderr: {}", zmin.2);

        assert_eq!(
            normalize_p4_clone_stdout(&zmin.1),
            normalize_p4_clone_stdout(&stock.1)
        );
        assert_eq!(
            normalize_p4_clone_stderr(&zmin.2),
            normalize_p4_clone_stderr(&stock.2)
        );

        if extra_args[0] == "--changesfile" {
            assert!(stock_target.join(".git").is_dir());
            assert!(zmin_target.join(".git").is_dir());
            assert_eq!(git_maybe(&stock_target, ["show-ref"]), git_maybe(&zmin_target, ["show-ref"]));
            assert_eq!(git(&stock_target, ["status", "--short"]), git(&zmin_target, ["status", "--short"]));
            assert_eq!(list_dir_names(&stock_target), list_dir_names(&zmin_target));
        } else {
            assert!(!stock_target.join(".git").exists());
            assert!(!zmin_target.join(".git").exists());
            assert_eq!(show_ref_names(&stock_target), show_ref_names(&zmin_target));
            assert!(show_ref_hashes_are_uniform(&stock_target));
            assert!(show_ref_hashes_are_uniform(&zmin_target));
            assert_eq!(
                git(&stock_target, ["log", "-1", "--format=%B", "HEAD"]),
                git(&zmin_target, ["log", "-1", "--format=%B", "HEAD"])
            );
            assert_eq!(
                git(&stock_target, ["ls-tree", "-r", "--name-only", "HEAD"]),
                git(&zmin_target, ["ls-tree", "-r", "--name-only", "HEAD"])
            );
            assert_eq!(git_maybe(&stock_target, ["status", "--short"]), git_maybe(&zmin_target, ["status", "--short"]));
            assert_eq!(list_dir_names(&stock_target), list_dir_names(&zmin_target));
        }
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
fn p4_submit_noop_option_family_matches_stock_git() {
    for extra_args in [
        ["--origin", "refs/remotes/p4/master"].as_slice(),
        ["-M"].as_slice(),
        ["--conflict=skip"].as_slice(),
        ["--commit", "HEAD"].as_slice(),
        ["--git-dir", ".git"].as_slice(),
    ] {
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
        let stock_log = fs::read_to_string(stock_log_path).expect("read stock p4 log");
        let zmin_log = fs::read_to_string(zmin_log_path).expect("read zmin p4 log");
        assert_eq!(stock_log, zmin_log);
    }
}

#[test]
fn p4_submit_disable_followup_option_family_matches_stock_git() {
    for extra_args in [
        ["--disable-rebase"].as_slice(),
        ["--disable-p4sync"].as_slice(),
    ] {
        let dir = TempDir::new().expect("temp dir");
        let bin = dir.path().join("bin");
        let data = dir.path().join("p4-data");
        let seed_target = dir.path().join("seed-project");
        let stock_target = dir.path().join("stock-project");
        let zmin_target = dir.path().join("zmin-project");
        let seed_log_path = dir.path().join("seed-p4.log");
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
        let stock = run_command_with_path(
            stock_git_bin().to_str().expect("stock git path"),
            &stock_target,
            &bin,
            &stock_args,
        );
        assert_eq!(stock.0, 0, "stock submit stderr: {}", stock.2);

        let mut zmin_args = vec!["p4", "submit"];
        zmin_args.extend_from_slice(extra_args);
        let zmin = run_command_with_path(
            zmin_bin(),
            &zmin_target,
            &bin,
            &zmin_args,
        );
        assert_eq!(zmin.0, 0, "zmin submit stderr: {}", zmin.2);

        assert_eq!(
            normalize_p4_submit_stdout(&zmin.1),
            normalize_p4_submit_stdout(&stock.1)
        );
        assert_eq!(zmin.2, stock.2);
    }
}

#[test]
fn p4_submit_preserve_user_failure_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let bin = dir.path().join("bin");
    let data = dir.path().join("p4-data");
    let seed_target = dir.path().join("seed-project");
    let stock_target = dir.path().join("stock-project");
    let zmin_target = dir.path().join("zmin-project");
    let seed_log_path = dir.path().join("seed-p4.log");
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

    let stock = run_command_with_path(
        stock_git_bin().to_str().expect("stock git path"),
        &stock_target,
        &bin,
        &["p4", "submit", "--preserve-user"],
    );
    assert_ne!(stock.0, 0, "stock submit unexpectedly succeeded");
    let zmin = run_command_with_path(
        zmin_bin(),
        &zmin_target,
        &bin,
        &["p4", "submit", "--preserve-user"],
    );
    assert_ne!(zmin.0, 0, "zmin submit unexpectedly succeeded");

    assert_eq!(zmin.0, stock.0);
    assert_eq!(zmin.1, stock.1);
    assert_eq!(zmin.2, stock.2);
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
fn p4_submit_prepare_p4_only_matches_stock_git() {
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

    let stock = run_command_with_path_and_env(
        stock_git_bin().to_str().expect("stock git path"),
        &stock_target,
        &bin,
        &[(
            "P4_LOG_PATH",
            stock_log_path.to_str().expect("stock log path"),
        )],
        &["p4", "submit", "--prepare-p4-only"],
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
        &["p4", "submit", "--prepare-p4-only"],
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
}

#[test]
fn p4_submit_shelve_option_family_matches_stock_git() {
    for extra_args in [
        ["--shelve"].as_slice(),
        ["--update-shelve", "1234"].as_slice(),
    ] {
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
        assert_ne!(stock.0, 0, "stock submit unexpectedly succeeded");

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
        assert_ne!(zmin.0, 0, "zmin submit unexpectedly succeeded");

        assert_eq!(zmin.0, stock.0);
        assert_eq!(
            normalize_p4_submit_stdout(&zmin.1),
            normalize_p4_submit_stdout(&stock.1)
        );
        assert_eq!(zmin.2, stock.2);
    }
}

#[test]
fn p4_submit_export_labels_failure_matches_stock_git() {
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

    let stock = run_command_with_path_and_env(
        stock_git_bin().to_str().expect("stock git path"),
        &stock_target,
        &bin,
        &[(
            "P4_LOG_PATH",
            stock_log_path.to_str().expect("stock log path"),
        )],
        &["p4", "submit", "--export-labels"],
    );
    assert_ne!(stock.0, 0, "stock submit unexpectedly succeeded");
    let zmin = run_command_with_path_and_env(
        zmin_bin(),
        &zmin_target,
        &bin,
        &[(
            "P4_LOG_PATH",
            zmin_log_path.to_str().expect("zmin log path"),
        )],
        &["p4", "submit", "--export-labels"],
    );
    assert_ne!(zmin.0, 0, "zmin submit unexpectedly succeeded");

    assert_eq!(zmin.0, stock.0);
    assert_eq!(
        normalize_p4_submit_stdout(&zmin.1),
        normalize_p4_submit_stdout(&stock.1)
    );
    assert_eq!(zmin.2, stock.2);
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
        git(&target, ["rev-parse", "HEAD"])
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

#[cfg(unix)]
#[test]
fn svn_clone_revision_option_family_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let (_repo_url, trunk_url) = create_local_svn_trunk_history(dir.path());
    let stock_target = dir.path().join("stock-project");
    let zmin_target = dir.path().join("zmin-project");
    let envs = [("GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME", "main")];

    let stock = run_command_with_path_and_env(
        stock_git_bin().to_str().expect("stock git path"),
        dir.path(),
        dir.path(),
        &envs,
        &["svn", "clone", "-r", "2", &trunk_url, stock_target.to_str().expect("stock target")],
    );
    let zmin = run_command_with_path_and_env(
        zmin_bin(),
        dir.path(),
        dir.path(),
        &envs,
        &["svn", "clone", "-r", "2", &trunk_url, zmin_target.to_str().expect("zmin target")],
    );
    assert_eq!(stock.0, 0, "stock stderr: {}", stock.2);
    assert_eq!(zmin.0, 0, "zmin stderr: {}", zmin.2);
    assert_eq!(
        visible_non_git_file_contents(&stock_target),
        visible_non_git_file_contents(&zmin_target)
    );
    assert_eq!(
        git(&stock_target, ["rev-parse", "--abbrev-ref", "HEAD"]),
        git(&zmin_target, ["rev-parse", "--abbrev-ref", "HEAD"])
    );
    assert_eq!(
        git(&stock_target, ["config", "--get", "svn-remote.svn.url"]),
        git(&zmin_target, ["config", "--get", "svn-remote.svn.url"])
    );
    assert_eq!(
        git(&stock_target, ["cat-file", "-p", "refs/remotes/git-svn:a.txt"]),
        git(&zmin_target, ["cat-file", "-p", "refs/remotes/git-svn:a.txt"])
    );
    assert_eq!(
        fs::read_to_string(stock_target.join("a.txt")).expect("read stock a"),
        "alpha\n"
    );
    assert_eq!(
        fs::read_to_string(zmin_target.join("a.txt")).expect("read zmin a"),
        "alpha\n"
    );

    let stock_long = run_command_with_path_and_env(
        stock_git_bin().to_str().expect("stock git path"),
        dir.path(),
        dir.path(),
        &envs,
        &[
            "svn",
            "clone",
            "--revision",
            "2",
            &trunk_url,
            dir.path()
                .join("stock-project-long")
                .to_str()
                .expect("stock target long"),
        ],
    );
    let zmin_long = run_command_with_path_and_env(
        zmin_bin(),
        dir.path(),
        dir.path(),
        &envs,
        &[
            "svn",
            "clone",
            "--revision",
            "2",
            &trunk_url,
            dir.path()
                .join("zmin-project-long")
                .to_str()
                .expect("zmin target long"),
        ],
    );
    assert_eq!(stock_long.0, zmin_long.0);
    assert_eq!(
        visible_non_git_file_contents(&dir.path().join("stock-project-long")),
        visible_non_git_file_contents(&dir.path().join("zmin-project-long"))
    );
}

#[cfg(unix)]
#[test]
fn svn_dcommit_dry_run_option_family_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let (_repo_url, trunk_url) = create_local_svn_trunk_history(dir.path());
    let stock_target = dir.path().join("stock-project");
    let zmin_target = dir.path().join("zmin-project");
    let envs = [("GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME", "main")];

    let stock_clone = run_command_with_path_and_env(
        stock_git_bin().to_str().expect("stock git path"),
        dir.path(),
        dir.path(),
        &envs,
        &["svn", "clone", &trunk_url, stock_target.to_str().expect("stock target")],
    );
    assert_eq!(stock_clone.0, 0, "stock clone stderr: {}", stock_clone.2);
    let zmin_clone = run_command_with_path_and_env(
        zmin_bin(),
        dir.path(),
        dir.path(),
        &envs,
        &["svn", "clone", &trunk_url, zmin_target.to_str().expect("zmin target")],
    );
    assert_eq!(zmin_clone.0, 0, "zmin clone stderr: {}", zmin_clone.2);

    configure_identity(&stock_target);
    configure_identity(&zmin_target);
    for target in [&stock_target, &zmin_target] {
        fs::write(target.join("a.txt"), b"alpha\nsecond\nchanged\n").expect("modify a");
        fs::write(target.join("new.txt"), b"new\n").expect("write new");
        fs::remove_file(target.join("dir/b.txt")).expect("remove b");
        git(target, ["add", "-A"]);
        git_with_env(target, ["commit", "-m", "svn submit change"]);
    }

    let stock_short = run_command_with_path_and_env(
        stock_git_bin().to_str().expect("stock git path"),
        &stock_target,
        dir.path(),
        &envs,
        &["svn", "dcommit", "-n"],
    );
    let zmin_short = run_command_with_path_and_env(
        zmin_bin(),
        &zmin_target,
        dir.path(),
        &envs,
        &["svn", "dcommit", "-n"],
    );
    assert_eq!(stock_short.0, zmin_short.0);
    assert_eq!(stock_short.2, zmin_short.2);
    assert_eq!(
        normalize_svn_dcommit_dry_run_stdout(&stock_short.1),
        normalize_svn_dcommit_dry_run_stdout(&zmin_short.1)
    );

    let stock_long = run_command_with_path_and_env(
        stock_git_bin().to_str().expect("stock git path"),
        &stock_target,
        dir.path(),
        &envs,
        &["svn", "dcommit", "--dry-run"],
    );
    let zmin_long = run_command_with_path_and_env(
        zmin_bin(),
        &zmin_target,
        dir.path(),
        &envs,
        &["svn", "dcommit", "--dry-run"],
    );
    assert_eq!(stock_long.0, zmin_long.0);
    assert_eq!(stock_long.2, zmin_long.2);
    assert_eq!(
        normalize_svn_dcommit_dry_run_stdout(&stock_long.1),
        normalize_svn_dcommit_dry_run_stdout(&zmin_long.1)
    );

    let stock_quiet_short = run_command_with_path_and_env(
        stock_git_bin().to_str().expect("stock git path"),
        &stock_target,
        dir.path(),
        &envs,
        &["svn", "dcommit", "-q", "-n"],
    );
    let zmin_quiet_short = run_command_with_path_and_env(
        zmin_bin(),
        &zmin_target,
        dir.path(),
        &envs,
        &["svn", "dcommit", "-q", "-n"],
    );
    assert_eq!(stock_quiet_short.0, zmin_quiet_short.0);
    assert_eq!(stock_quiet_short.2, zmin_quiet_short.2);
    assert_eq!(
        normalize_svn_dcommit_dry_run_stdout(&stock_quiet_short.1),
        normalize_svn_dcommit_dry_run_stdout(&zmin_quiet_short.1)
    );

    let stock_quiet_long = run_command_with_path_and_env(
        stock_git_bin().to_str().expect("stock git path"),
        &stock_target,
        dir.path(),
        &envs,
        &["svn", "dcommit", "--quiet", "--dry-run"],
    );
    let zmin_quiet_long = run_command_with_path_and_env(
        zmin_bin(),
        &zmin_target,
        dir.path(),
        &envs,
        &["svn", "dcommit", "--quiet", "--dry-run"],
    );
    assert_eq!(stock_quiet_long.0, zmin_quiet_long.0);
    assert_eq!(stock_quiet_long.2, zmin_quiet_long.2);
    assert_eq!(
        normalize_svn_dcommit_dry_run_stdout(&stock_quiet_long.1),
        normalize_svn_dcommit_dry_run_stdout(&zmin_quiet_long.1)
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

#[cfg(unix)]
#[test]
fn archimport_noop_option_family_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let stock_bin = dir.path().join("stock-bin");
    let zmin_bin_dir = dir.path().join("zmin-bin");
    let data = dir.path().join("arch-data");
    let stock_target = dir.path().join("stock-project");
    let zmin_target = dir.path().join("zmin-project");
    let stock_temp = dir.path().join("stock-temp");
    let zmin_temp = dir.path().join("zmin-temp");
    let stock_home = dir.path().join("stock-home");
    let zmin_home = dir.path().join("zmin-home");
    fs::create_dir_all(data.join("dir")).expect("create arch dir");
    fs::create_dir_all(data.join("{arch}")).expect("create arch metadata");
    fs::write(data.join("a.txt"), b"alpha\n").expect("write arch a");
    fs::write(data.join("dir/b.txt"), b"bravo\n").expect("write arch b");
    fs::write(data.join("{arch}/internal"), b"ignored\n").expect("write arch metadata");
    fs::create_dir_all(&stock_target).expect("create stock target");
    fs::create_dir_all(&zmin_target).expect("create zmin target");
    fs::create_dir_all(&stock_home).expect("create stock home");
    fs::create_dir_all(&zmin_home).expect("create zmin home");
    write_fake_tla(&stock_bin, &data, &dir.path().join("stock-tla.log"));
    write_fake_tla(&zmin_bin_dir, &data, &dir.path().join("zmin-tla.log"));

    let stock = run_command_with_path_and_env(
        stock_git_bin().to_str().expect("stock git path"),
        &stock_target,
        &stock_bin,
        &[("HOME", stock_home.to_str().expect("stock home path"))],
        &[
            "archimport",
            "-f",
            "-T",
            "-a",
            "-D",
            "1",
            "-t",
            stock_temp.to_str().expect("stock temp path"),
            "archive@example.test/project--main--1--base-0:master",
        ],
    );
    assert_eq!(stock.0, 0, "stock git stderr: {}", stock.2);

    let zmin = run_command_with_path_and_env(
        zmin_bin(),
        &zmin_target,
        &zmin_bin_dir,
        &[("HOME", zmin_home.to_str().expect("zmin home path"))],
        &[
            "archimport",
            "-f",
            "-T",
            "-a",
            "-D",
            "1",
            "-t",
            zmin_temp.to_str().expect("zmin temp path"),
            "archive@example.test/project--main--1--base-0:master",
        ],
    );
    assert_eq!(zmin.0, 0, "zmin stderr: {}", zmin.2);

    assert_eq!(stock.1, zmin.1);
    assert_eq!(
        normalize_archimport_stderr(&stock.2),
        normalize_archimport_stderr(&zmin.2)
    );
    assert_eq!(
        visible_non_git_file_contents(&stock_target),
        visible_non_git_file_contents(&zmin_target)
    );
    assert_eq!(
        git(&stock_target, ["rev-parse", "--abbrev-ref", "HEAD"]),
        git(&zmin_target, ["rev-parse", "--abbrev-ref", "HEAD"])
    );
    assert_eq!(
        git(&stock_target, ["log", "-1", "--format=%B"]),
        git(&zmin_target, ["log", "-1", "--format=%B"])
    );
    assert!(stock_target.join("dir/b.txt").exists());
    assert!(zmin_target.join("dir/b.txt").exists());
    assert!(!stock_target.join("{arch}/internal").exists());
    assert!(!zmin_target.join("{arch}/internal").exists());
}

#[cfg(unix)]
#[test]
fn archimport_help_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let stock = run_command_with_path(
        stock_git_bin().to_str().expect("stock git path"),
        dir.path(),
        dir.path(),
        &["archimport", "-h"],
    );
    let zmin = run_command_with_path(zmin_bin(), dir.path(), dir.path(), &["archimport", "-h"]);
    assert_eq!(stock.0, zmin.0);
    assert_eq!(stock.1, zmin.1);
    assert_eq!(stock.2, zmin.2);
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
    let home = envs
        .iter()
        .find(|(key, _)| *key == "HOME")
        .map(|(_, value)| std::path::PathBuf::from(value))
        .unwrap_or_else(|| cwd.join("home"));
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

fn visible_non_git_file_contents(root: &std::path::Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    collect_visible_non_git_file_contents(root, root, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn collect_visible_non_git_file_contents(
    root: &std::path::Path,
    current: &std::path::Path,
    files: &mut Vec<(String, String)>,
) {
    for entry in fs::read_dir(current).expect("read tree dir") {
        let entry = entry.expect("read tree entry");
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        if path.is_dir() {
            collect_visible_non_git_file_contents(root, &path, files);
        } else {
            let rel = path
                .strip_prefix(root)
                .expect("strip tree prefix")
                .to_string_lossy()
                .replace('\\', "/");
            files.push((rel, fs::read_to_string(&path).expect("read tree file")));
        }
    }
}

fn normalize_cvs_log(log: &str) -> Vec<String> {
    let mut lines = log
        .lines()
        .map(normalize_cvs_log_line)
        .collect::<Vec<_>>();
    lines.sort();
    lines
}

fn normalize_cvsexportcommit_stderr(stderr: &str) -> String {
    stderr
        .replace(
            "/usr/local/opt/git/libexec/git-core/git-cvsexportcommit",
            "git-cvsexportcommit",
        )
        .replace(
            "/Applications/Xcode.app/Contents/Developer/usr/libexec/git-core/git-cvsexportcommit",
            "git-cvsexportcommit",
        )
}

fn normalize_cvs_log_line(line: &str) -> String {
    let mut parts = line.split_whitespace().collect::<Vec<_>>();
    let mut prefix = Vec::new();
    if parts.first() == Some(&"-d") && parts.len() >= 2 {
        prefix.push(parts.remove(0));
        prefix.push(parts.remove(0));
    }
    if parts.is_empty() {
        return line.to_owned();
    }
    let command = parts.remove(0);
    let mut normalized = prefix
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    normalized.push(command.to_owned());
    match command {
        "status" | "update" => {
            parts.sort();
            normalized.extend(parts.into_iter().map(str::to_owned));
        }
        "commit" => {
            if parts.first() == Some(&"-F") && parts.len() >= 2 {
                normalized.push(parts.remove(0).to_owned());
                normalized.push(parts.remove(0).to_owned());
            }
            parts.sort();
            normalized.extend(parts.into_iter().map(str::to_owned));
        }
        _ => normalized.extend(parts.into_iter().map(str::to_owned)),
    }
    normalized.join(" ")
}

fn setup_cvsexportcommit_fixture(
    source: &std::path::Path,
    cvs: &std::path::Path,
    keyworded_base: bool,
) {
    git(
        source.parent().expect("source parent"),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(source);
    let base_text = if keyworded_base {
        "base\n$Id: demo 1.1 $\n"
    } else {
        "base\n"
    };
    let changed_text = if keyworded_base {
        "base\n$Id: demo 1.1 $\nchanged\n"
    } else {
        "base\nchanged\n"
    };
    fs::write(source.join("a.txt"), base_text).expect("write a");
    fs::write(source.join("remove.txt"), b"remove\n").expect("write remove");
    git(source, ["add", "-A"]);
    git_with_env(source, ["commit", "-m", "base"]);
    fs::create_dir_all(cvs.join("CVS")).expect("create CVS marker");
    fs::write(cvs.join("a.txt"), base_text).expect("write cvs a");
    fs::write(cvs.join("remove.txt"), b"remove\n").expect("write cvs remove");
    fs::write(source.join("a.txt"), changed_text).expect("modify a");
    fs::remove_file(source.join("remove.txt")).expect("delete remove");
    fs::create_dir_all(source.join("dir")).expect("create source dir");
    fs::write(source.join("dir/new.txt"), b"new\n").expect("write new");
    git(source, ["add", "-A"]);
    git_with_env(source, ["commit", "-m", "export me"]);
}

fn setup_cvsexportcommit_force_parent_fixture(
    seed: &std::path::Path,
    source: &std::path::Path,
    stock_cvs: &std::path::Path,
    zmin_cvs: &std::path::Path,
) -> String {
    git(
        seed.parent().expect("seed parent"),
        ["init", "-b", "main", seed.to_str().expect("seed path")],
    );
    configure_identity(seed);
    fs::write(seed.join("a.txt"), b"base\n").expect("write a");
    fs::write(seed.join("remove.txt"), b"remove\n").expect("write remove");
    git(seed, ["add", "-A"]);
    git_with_env(seed, ["commit", "-m", "base"]);
    let base_commit = git(seed, ["rev-parse", "HEAD"]);
    fs::write(seed.join("a.txt"), b"base\nmiddle\n").expect("write middle a");
    git(seed, ["add", "a.txt"]);
    git_with_env(seed, ["commit", "-m", "middle"]);
    fs::write(seed.join("a.txt"), b"base\nmiddle\nchanged\n").expect("write head a");
    fs::remove_file(seed.join("remove.txt")).expect("remove file");
    fs::create_dir_all(seed.join("dir")).expect("create dir");
    fs::write(seed.join("dir/new.txt"), b"new\n").expect("write new");
    git(seed, ["add", "-A"]);
    git_with_env(seed, ["commit", "-m", "export me"]);

    fs::create_dir_all(stock_cvs.join("CVS")).expect("create stock CVS marker");
    fs::create_dir_all(zmin_cvs.join("CVS")).expect("create zmin CVS marker");
    fs::write(stock_cvs.join("a.txt"), b"base\n").expect("write stock cvs a");
    fs::write(stock_cvs.join("remove.txt"), b"remove\n").expect("write stock cvs remove");
    fs::write(zmin_cvs.join("a.txt"), b"base\n").expect("write zmin cvs a");
    fs::write(zmin_cvs.join("remove.txt"), b"remove\n").expect("write zmin cvs remove");
    copy_dir_recursive(seed, source);

    base_commit
}

fn setup_cvsexportcommit_same_worktree_fixture(source: &std::path::Path) {
    git(
        source.parent().expect("source parent"),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(source);
    fs::write(source.join("a.txt"), b"base\n").expect("write a");
    fs::write(source.join("remove.txt"), b"remove\n").expect("write remove");
    git(source, ["add", "-A"]);
    git_with_env(source, ["commit", "-m", "base"]);
    git(source, ["branch", "cvs-parent"]);
    git(source, ["checkout", "-b", "export-target"]);
    fs::write(source.join("a.txt"), b"base\nchanged\n").expect("modify a");
    fs::remove_file(source.join("remove.txt")).expect("delete remove");
    fs::create_dir_all(source.join("dir")).expect("create source dir");
    fs::write(source.join("dir/new.txt"), b"new\n").expect("write new");
    git(source, ["add", "-A"]);
    git_with_env(source, ["commit", "-m", "export me"]);
    git(source, ["checkout", "cvs-parent"]);
    fs::create_dir_all(source.join("CVS")).expect("create CVS marker");
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

fn normalize_archimport_stderr(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|line| {
            !line.starts_with(
                "Use of each() on hash after insertion without resetting hash iterator",
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_p4_clone_stderr(stderr: &str) -> String {
    stderr
        .replace("stock-project", "<target>")
        .replace("zmin-project", "<target>")
}

fn normalize_p4_clone_stdout(stdout: &str) -> String {
    stdout
        .replace("/private/var/", "/var/")
        .replace("stock-project", "<target>")
        .replace("zmin-project", "<target>")
}

fn normalize_p4_clone_helper_sensitive_stderr(stderr: &str) -> String {
    stderr
        .lines()
        .find(|line| line.starts_with("KeyError: "))
        .unwrap_or(stderr)
        .to_owned()
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
            if line.contains("/stock-project/") || line.contains("/zmin-project/") {
                return "  <target>".to_owned();
            }
            if line.starts_with("or \"p4 submit -i <") && line.contains("git p4") {
                return "or \"<template>\" to use the one prepared by \"git p4\".".to_owned();
            }
            if line.contains("tmp") && line.contains("git p4") {
                return line
                    .replace(
                        line.split('"').nth(1).unwrap_or_default(),
                        "<template>",
                    )
                    .replace(
                        line.split('<').nth(1).and_then(|rest| rest.split('>').next()).unwrap_or_default(),
                        "<template>",
                    );
            }
            if line.starts_with("You can delete the file ") || line.starts_with("the submit template file ") {
                return line
                    .replace(
                        line.split('"').nth(1).unwrap_or_default(),
                        "<template>",
                    );
            }
            if line.starts_with("TryPatch: git diff-tree --full-index -p \"")
                && line.ends_with("\" | git apply --check -")
            {
                return "TryPatch: git diff-tree --full-index -p \"<commit>\" | git apply --check -"
                    .to_owned();
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

fn git_maybe<const N: usize>(repo: &std::path::Path, args: [&str; N]) -> (i32, String, String) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("run git");
    (
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    )
}

fn list_dir_names(path: &std::path::Path) -> Vec<String> {
    let mut names = fs::read_dir(path)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn show_ref_names(repo: &std::path::Path) -> Vec<String> {
    let (_, stdout, stderr) = git_maybe(repo, ["show-ref"]);
    assert!(stderr.is_empty(), "unexpected show-ref stderr: {stderr}");
    let mut names = stdout
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn show_ref_hashes_are_uniform(repo: &std::path::Path) -> bool {
    let (_, stdout, stderr) = git_maybe(repo, ["show-ref"]);
    assert!(stderr.is_empty(), "unexpected show-ref stderr: {stderr}");
    let hashes = stdout
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect::<Vec<_>>();
    !hashes.is_empty() && hashes.windows(2).all(|pair| pair[0] == pair[1])
}

#[cfg(unix)]
fn write_fake_cvs(bin: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake cvs bin");
    let script = bin.join("cvs");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = '-d' ]; then shift; shift; fi\nif [ \"$1\" = status ]; then shift; for f in \"$@\"; do printf 'File: %s Status: Up-to-date\\n' \"$f\"; done; exit 0; fi\nif [ \"$1\" = update ]; then shift; for f in \"$@\"; do printf 'U %s\\n' \"$f\"; done; exit 0; fi\nexit 0\n",
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

#[cfg(unix)]
fn create_local_svn_trunk_history(root: &std::path::Path) -> (String, String) {
    let repo = root.join("local-svn-repo");
    let wc = root.join("local-svn-wc");
    let wc2 = root.join("local-svn-wc-2");
    let status = Command::new("svnadmin")
        .args(["create", repo.to_str().expect("repo path")])
        .status()
        .expect("run svnadmin create");
    assert!(status.success(), "svnadmin create failed");
    let repo_url = format!("file://{}", repo.display());
    let status = Command::new("svn")
        .args(["mkdir", &format!("{repo_url}/trunk"), "-m", "create trunk"])
        .status()
        .expect("run svn mkdir");
    assert!(status.success(), "svn mkdir failed");
    let trunk_url = format!("{repo_url}/trunk");
    let status = Command::new("svn")
        .args(["checkout", &trunk_url, wc.to_str().expect("wc path")])
        .status()
        .expect("run svn checkout");
    assert!(status.success(), "svn checkout failed");
    fs::write(wc.join("a.txt"), b"alpha\n").expect("write a");
    fs::create_dir_all(wc.join("dir")).expect("create dir");
    fs::write(wc.join("dir/b.txt"), b"bravo\n").expect("write b");
    let status = Command::new("svn")
        .args([
            "add",
            wc.join("a.txt").to_str().expect("a path"),
            wc.join("dir").to_str().expect("dir path"),
        ])
        .status()
        .expect("run svn add");
    assert!(status.success(), "svn add failed");
    let status = Command::new("svn")
        .args(["commit", wc.to_str().expect("wc path"), "-m", "initial import"])
        .status()
        .expect("run svn commit");
    assert!(status.success(), "svn commit failed");

    let status = Command::new("svn")
        .args(["checkout", &trunk_url, wc2.to_str().expect("wc2 path")])
        .status()
        .expect("run svn checkout 2");
    assert!(status.success(), "svn checkout 2 failed");
    fs::write(wc2.join("a.txt"), b"alpha\nsecond\n").expect("write second");
    let status = Command::new("svn")
        .args(["commit", wc2.to_str().expect("wc2 path"), "-m", "second import"])
        .status()
        .expect("run svn commit 2");
    assert!(status.success(), "svn commit 2 failed");

    (repo_url, trunk_url)
}

#[cfg(unix)]
fn normalize_svn_dcommit_dry_run_stdout(stdout: &str) -> String {
    let hash = regex::Regex::new(r"[0-9a-f]{40}").expect("hash regex");
    hash.replace_all(stdout, "<oid>").into_owned()
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
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = abrowse ]; then printf '        %s (initial import)\\n' '{}'; printf '          2001-01-01 00:00:00\\n'; exit 0; fi\nif [ \"$1\" = my-default-archive ]; then printf '%s\\n' '{}'; exit 0; fi\nif [ \"$1\" = cat-log ] || [ \"$1\" = cat-archive-log ]; then printf 'Summary: import arch history\\n'; printf 'Creator: Test User <test@example.test>\\n'; printf '\\n'; printf 'import arch history\\n'; exit 0; fi\nif [ \"$1\" = get ]; then last=''; for arg in \"$@\"; do last=\"$arg\"; done; mkdir -p \"$last\"; cp -R '{}/.' \"$last/\"; exit 0; fi\nexit 1\n",
            log.display(),
            "archive@example.test/project--main--1--base-0",
            "archive@example.test",
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
