mod common;

use std::io::Write;
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
