mod common;

use std::fs;
use std::process::Command;

use common::{
    configure_identity, git, git_init, git_with_env, run_skron, run_skron_status,
    run_skron_with_stdin, skron_bin,
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

    let output = run_skron_with_path(
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

    run_skron_with_path(
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

    run_skron_with_path(
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

#[test]
fn p4_clone_imports_head_revision_into_git_refs_and_worktree() {
    let dir = TempDir::new().expect("temp dir");
    let bin = dir.path().join("bin");
    let data = dir.path().join("p4-data");
    let target = dir.path().join("project");
    fs::create_dir_all(&data).expect("create p4 data");
    fs::write(data.join("a.txt"), b"alpha\n").expect("write p4 a");
    fs::create_dir_all(data.join("dir")).expect("create p4 dir");
    fs::write(data.join("dir/b.txt"), b"bravo\n").expect("write p4 b");
    write_fake_p4(&bin, &data, &dir.path().join("p4.log"));

    run_skron_with_path(
        dir.path(),
        &bin,
        [
            "p4",
            "clone",
            "--branch",
            "master",
            "//depot/project",
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
        git(&target, ["rev-parse", "refs/remotes/p4/master"]),
        git(&target, ["rev-parse", "refs/heads/master"])
    );
    assert_eq!(
        git(&target, ["config", "--get", "git-p4.depotpath"]),
        "//depot/project"
    );
    let log = fs::read_to_string(dir.path().join("p4.log")).expect("read p4 log");
    assert!(log.contains("files //depot/project/..."));
    assert!(log.contains("print -q //depot/project/a.txt#1"));
    assert!(log.contains("print -q //depot/project/dir/b.txt#2"));
}

#[test]
fn p4_submit_opens_changed_files_and_submits_head() {
    let dir = TempDir::new().expect("temp dir");
    let bin = dir.path().join("bin");
    let data = dir.path().join("p4-data");
    let target = dir.path().join("project");
    let log_path = dir.path().join("p4-submit.log");
    fs::create_dir_all(&data).expect("create p4 data");
    fs::write(data.join("a.txt"), b"alpha\n").expect("write p4 a");
    fs::create_dir_all(data.join("dir")).expect("create p4 dir");
    fs::write(data.join("dir/b.txt"), b"bravo\n").expect("write p4 b");
    write_fake_p4(&bin, &data, &log_path);

    run_skron_with_path(
        dir.path(),
        &bin,
        [
            "p4",
            "clone",
            "--branch",
            "master",
            "//depot/project",
            target.to_str().expect("target path"),
        ],
    );
    configure_identity(&target);
    fs::write(target.join("a.txt"), b"alpha\nchanged\n").expect("modify a");
    fs::write(target.join("new.txt"), b"new\n").expect("write new");
    fs::remove_file(target.join("dir/b.txt")).expect("remove b");
    git(&target, ["add", "-A"]);
    git_with_env(&target, ["commit", "-m", "submit change"]);

    run_skron_with_path(&target, &bin, ["p4", "submit"]);

    let log = fs::read_to_string(log_path).expect("read p4 log");
    assert!(log.contains("edit a.txt"));
    assert!(log.contains("add new.txt"));
    assert!(log.contains("delete dir/b.txt"));
    assert!(log.contains("submit -d submit change"));
    assert_eq!(
        git(&target, ["rev-parse", "refs/remotes/p4/master"]),
        git(&target, ["rev-parse", "HEAD"])
    );
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

    run_skron_with_path(
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

    run_skron_with_path(
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

    run_skron_with_path(&target, &bin, ["svn", "dcommit"]);

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

    run_skron_with_path(
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

    assert_eq!(run_skron_status(&target, ["archimport"]), 129);
    assert_eq!(
        run_skron_status(&target, ["archimport", "-o", "archive/project"]),
        129
    );
    assert_ne!(
        run_skron_with_path_status(
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
        run_skron_with_path_status(
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

    assert_eq!(run_skron_status(dir.path(), ["p4", "clone"]), 129);
    assert_eq!(run_skron_status(dir.path(), ["p4", "submit"]), 128);
    assert_eq!(run_skron_status(dir.path(), ["p4", "unknown"]), 129);
    assert_eq!(
        run_skron_with_path_status(
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

    assert_eq!(run_skron_status(dir.path(), ["svn", "clone"]), 129);
    assert_eq!(run_skron_status(dir.path(), ["svn", "dcommit"]), 128);
    assert_eq!(run_skron_status(dir.path(), ["svn", "unknown"]), 129);
    assert_eq!(
        run_skron_with_path_status(
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

    assert_eq!(run_skron(repo.path(), ["cvsserver"]), "");
    assert_eq!(run_skron(repo.path(), ["cvsserver", "-h"]), "");
    assert_eq!(
        run_skron_with_stdin(repo.path(), ["cvsserver", "server"], "valid-requests\n"),
        "Valid-requests Argument Argumentx Directory Entry Global_option Modified Questionable Root Sticky Unchanged Valid-responses add admin annotate ci co diff editors expand-modules history log noop remove rlog status tag update valid-requests watchers\nok"
    );
    assert_eq!(
        run_skron_with_stdin(repo.path(), ["cvsserver", "server"], "noop\n"),
        "ok"
    );
}

fn run_skron_with_path<const N: usize>(
    cwd: &std::path::Path,
    path_prefix: &std::path::Path,
    args: [&str; N],
) -> String {
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(path_prefix.to_path_buf()).chain(std::env::split_paths(&current_path)),
    )
    .expect("join PATH");
    let output = Command::new(skron_bin())
        .args(args)
        .env("PATH", path)
        .current_dir(cwd)
        .output()
        .expect("run skron");
    assert!(
        output.status.success(),
        "skron failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("skron stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn run_skron_with_path_status<const N: usize>(
    cwd: &std::path::Path,
    path_prefix: &std::path::Path,
    args: [&str; N],
) -> i32 {
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(path_prefix.to_path_buf()).chain(std::env::split_paths(&current_path)),
    )
    .expect("join PATH");
    Command::new(skron_bin())
        .args(args)
        .env("PATH", path)
        .current_dir(cwd)
        .output()
        .expect("run skron")
        .status
        .code()
        .expect("skron exited by signal")
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
            "@echo off\r\necho %*>>\"{}\"\r\nif \"%1\"==\"status\" (shift\r\n:loop\r\nif \"%1\"==\"\" exit /b 0\r\necho File: %1 Status: Up-to-date\r\nshift\r\ngoto loop)\r\nexit /b 0\r\n",
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
    fs::write(
        bin.join("cvs.bat"),
        format!(
            "@echo off\r\necho %*>>\"{}\"\r\nset rev=\r\nset last=\r\n:loop\r\nif \"%1\"==\"\" goto done\r\nif \"%1\"==\"-r\" (shift\r\nset rev=%1)\r\nset last=%1\r\nshift\r\ngoto loop\r\n:done\r\ntype \"{}\\%last%\\%rev%\"\r\n",
            log.display(),
            data.display()
        ),
    )
    .expect("write fake cvs checkout");
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
    let script = bin.join("cvsps.bat");
    let mut body = format!("@echo off\r\necho %*>>\"{}\"\r\n", log.display());
    for line in output.lines() {
        body.push_str("echo ");
        body.push_str(line);
        body.push_str("\r\n");
    }
    fs::write(script, body).expect("write fake cvsps");
}

#[cfg(unix)]
fn write_fake_p4(bin: &std::path::Path, data: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake p4 bin");
    let script = bin.join("p4");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = files ]; then echo '//depot/project/a.txt#1 - add change 1 (text)'; echo '//depot/project/dir/b.txt#2 - edit change 2 (text)'; exit 0; fi\nif [ \"$1\" = print ]; then case \"$3\" in '//depot/project/a.txt#1') cat '{}/a.txt' ;; '//depot/project/dir/b.txt#2') cat '{}/dir/b.txt' ;; *) exit 1 ;; esac; exit 0; fi\ncase \"$1\" in edit|add|delete|submit) exit 0 ;; esac\nexit 1\n",
            log.display(),
            data.display(),
            data.display()
        ),
    )
    .expect("write fake p4");
    make_executable(&script);
}

#[cfg(windows)]
fn write_fake_p4(bin: &std::path::Path, data: &std::path::Path, log: &std::path::Path) {
    fs::create_dir_all(bin).expect("create fake p4 bin");
    fs::write(
        bin.join("p4.bat"),
        format!(
            "@echo off\r\necho %*>>\"{}\"\r\nif \"%1\"==\"files\" (\r\necho //depot/project/a.txt#1 - add change 1 ^(text^)\r\necho //depot/project/dir/b.txt#2 - edit change 2 ^(text^)\r\nexit /b 0\r\n)\r\nif \"%1\"==\"print\" (\r\nif \"%3\"==\"//depot/project/a.txt#1\" type \"{}\\a.txt\"\r\nif \"%3\"==\"//depot/project/dir/b.txt#2\" type \"{}\\dir\\b.txt\"\r\nexit /b 0\r\n)\r\nif \"%1\"==\"edit\" exit /b 0\r\nif \"%1\"==\"add\" exit /b 0\r\nif \"%1\"==\"delete\" exit /b 0\r\nif \"%1\"==\"submit\" exit /b 0\r\nexit /b 1\r\n",
            log.display(),
            data.display(),
            data.display()
        ),
    )
    .expect("write fake p4");
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
    fs::write(
        bin.join("svn.bat"),
        format!(
            "@echo off\r\necho %*>>\"{}\"\r\nif \"%1\"==\"list\" (\r\necho a.txt\r\necho dir/\r\necho dir/b.txt\r\nexit /b 0\r\n)\r\nif \"%1\"==\"cat\" (\r\necho %2 | findstr /C:\"/a.txt\" >nul && type \"{}\\a.txt\"\r\necho %2 | findstr /C:\"/dir/b.txt\" >nul && type \"{}\\dir\\b.txt\"\r\nexit /b 0\r\n)\r\nif \"%1\"==\"add\" exit /b 0\r\nif \"%1\"==\"delete\" exit /b 0\r\nif \"%1\"==\"commit\" exit /b 0\r\nexit /b 1\r\n",
            log.display(),
            data.display(),
            data.display()
        ),
    )
    .expect("write fake svn");
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

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path).expect("script metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("chmod script");
}

#[cfg(windows)]
fn make_executable(_path: &std::path::Path) {}
