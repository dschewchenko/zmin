mod common;

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use common::{git, git_init, test_command_program, zmin_bin};

fn command_with_stdin_output(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
) -> (i32, String, String) {
    let mut child = Command::new(test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {command} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
    (
        output.status.code().expect("process exit code"),
        String::from_utf8(output.stdout).expect("stdout utf8"),
        String::from_utf8(output.stderr).expect("stderr utf8"),
    )
}

fn normalize_fast_import_crash_stderr(stderr: &str) -> String {
    let mut lines = stderr.lines();
    let fatal = lines.next().expect("fatal line");
    let crash = lines.next().expect("crash report line");
    assert_eq!(fatal, "fatal: Unsupported command: bogus");
    assert!(
        crash.starts_with("fast-import: dumping crash report to .git/fast_import_crash_"),
        "unexpected crash report line: {crash}"
    );
    assert_eq!(lines.next(), None);
    "fatal: Unsupported command: bogus\nfast-import: dumping crash report to .git/fast_import_crash_<pid>".to_owned()
}

fn fast_import_crash_reports(repo: &Path) -> Vec<String> {
    let mut reports = fs::read_dir(repo.join(".git"))
        .expect("read .git")
        .filter_map(|entry| {
            let entry = entry.expect("dir entry");
            let name = entry.file_name();
            name.to_str()
                .filter(|name| name.starts_with("fast_import_crash_"))
                .map(|_| fs::read_to_string(entry.path()).expect("crash report text"))
        })
        .collect::<Vec<_>>();
    reports.sort();
    reports
}

#[test]
fn fast_import_rfc2822_date_format_imports_stock_shape_but_stats_stderr_is_open() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stream = "\
commit refs/heads/main
author A U Thor <author@example.test> Thu, 01 Jan 1970 00:00:00 +0000
committer C O Mitter <committer@example.test> Thu, 01 Jan 1970 00:00:01 +0000
data <<EOF
rfc date
EOF
M 100644 inline a.txt
data <<EOF
contents
EOF
";

    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["fast-import", "--date-format=rfc2822"],
        stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--date-format=rfc2822"],
        stream,
    );

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert!(
        git_output.2.contains("fast-import statistics:"),
        "stock Git stderr should include import statistics: {}",
        git_output.2
    );
    assert!(
        zmin_output.2.is_empty(),
        "Zmin still lacks stock fast-import statistics stderr: {}",
        zmin_output.2
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["log", "--format=%an <%ae>|%cn <%ce>|%at %ct|%s", "main"]
        ),
        git(
            git_repo.path(),
            ["log", "--format=%an <%ae>|%cn <%ce>|%at %ct|%s", "main"]
        )
    );
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "main:a.txt"]),
        git(git_repo.path(), ["cat-file", "-p", "main:a.txt"])
    );
}

#[test]
fn fast_import_unknown_top_level_command_matches_stock_git_crash_shape() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let git_output = command_with_stdin_output("git", git_repo.path(), &["fast-import"], "bogus\n");
    let zmin_output =
        command_with_stdin_output(zmin_bin(), zmin_repo.path(), &["fast-import"], "bogus\n");

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_crash_stderr(&zmin_output.2),
        normalize_fast_import_crash_stderr(&git_output.2)
    );

    let git_reports = fast_import_crash_reports(git_repo.path());
    let zmin_reports = fast_import_crash_reports(zmin_repo.path());
    assert_eq!(git_reports.len(), 1);
    assert_eq!(zmin_reports.len(), 1);
    for report in [git_reports[0].as_str(), zmin_reports[0].as_str()] {
        assert!(report.contains("fast-import crash report:"));
        assert!(report.contains("fatal: Unsupported command: bogus"));
        assert!(report.contains("* bogus"));
        assert!(report.contains("END OF CRASH REPORT"));
    }
}
