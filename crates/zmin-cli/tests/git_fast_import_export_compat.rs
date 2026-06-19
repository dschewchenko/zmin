mod common;

use std::fs;

use common::{
    configure_identity, git, git_init, git_with_env, git_with_stdin, run_zmin,
    run_zmin_with_stdin, write_file,
};

#[test]
fn fast_export_stream_imports_into_stock_git() {
    let source = git_init();
    configure_identity(source.path());
    git(source.path(), ["checkout", "-b", "main"]);
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    fs::create_dir_all(source.path().join("dir")).expect("create dir");
    write_file(source.path(), "dir/b.txt", "two\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "two"]);

    let stream = run_zmin(source.path(), ["fast-export", "--all"]);
    let imported = git_init();
    git_with_stdin(imported.path(), ["fast-import"], &stream);

    assert_eq!(
        git(imported.path(), ["log", "--all", "--format=%s"]),
        git(source.path(), ["log", "--all", "--format=%s"])
    );
    assert_eq!(
        git(imported.path(), ["cat-file", "-p", "refs/heads/main:a.txt"]),
        git(source.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );
    assert_eq!(
        git(
            imported.path(),
            ["cat-file", "-p", "refs/heads/main:dir/b.txt"]
        ),
        git(
            source.path(),
            ["cat-file", "-p", "refs/heads/main:dir/b.txt"]
        )
    );
}

#[test]
fn fast_import_reads_stock_fast_export_stream() {
    let source = git_init();
    configure_identity(source.path());
    git(source.path(), ["checkout", "-b", "main"]);
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    fs::create_dir_all(source.path().join("dir")).expect("create dir");
    write_file(source.path(), "dir/b.txt", "two\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "two"]);

    let stream = git(source.path(), ["fast-export", "--all"]);
    let imported = git_init();
    run_zmin_with_stdin(imported.path(), ["fast-import"], &stream);

    assert_eq!(
        git(imported.path(), ["log", "--all", "--format=%s"]),
        git(source.path(), ["log", "--all", "--format=%s"])
    );
    assert_eq!(
        git(imported.path(), ["cat-file", "-p", "refs/heads/main:a.txt"]),
        git(source.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );
    assert_eq!(
        git(
            imported.path(),
            ["cat-file", "-p", "refs/heads/main:dir/b.txt"]
        ),
        git(
            source.path(),
            ["cat-file", "-p", "refs/heads/main:dir/b.txt"]
        )
    );
}

#[test]
fn fast_import_reads_bulk_commit_helper_stream_shape() {
    let imported = git_init();
    configure_identity(imported.path());
    git(imported.path(), ["checkout", "-b", "main"]);
    write_file(imported.path(), "base.txt", "base\n");
    git(imported.path(), ["add", "-A"]);
    git_with_env(imported.path(), ["commit", "-m", "base"]);

    let stream = "\
commit HEAD
author A U Thor <author@example.com> 1112912593 -0700
committer C O Mitter <committer@example.com> 1112912593 -0700
data <<EOF
commit 1
EOF
from HEAD^0
M 644 inline 1.t
data <<EOF
content 1
EOF

commit HEAD
author A U Thor <author@example.com> 1112912653 -0700
committer C O Mitter <committer@example.com> 1112912653 -0700
data <<EOF
commit 2
EOF
M 644 inline 2.t
data <<EOF
content 2
EOF

";
    run_zmin_with_stdin(imported.path(), ["fast-import"], stream);

    assert_eq!(
        git(imported.path(), ["log", "--format=%s"]),
        "commit 2\ncommit 1\nbase"
    );
    assert_eq!(
        git(imported.path(), ["rev-parse", "HEAD~2^{commit}"]).len(),
        40
    );
    assert_eq!(
        git(imported.path(), ["cat-file", "-p", "HEAD:2.t"]),
        "content 2"
    );
}

#[test]
fn fast_import_accepts_now_date_format_and_missing_author() {
    let imported = git_init();
    let stream = "\
commit refs/heads/main
mark :1
committer Author <a@uth.or> now
data <<EOF
start
EOF
M 100644 inline file
data <<EOF
contents
EOF
";
    run_zmin_with_stdin(
        imported.path(),
        ["fast-import", "--date-format=now"],
        stream,
    );

    assert_eq!(
        git(
            imported.path(),
            ["log", "--format=%an <%ae>|%cn <%ce>", "main"]
        ),
        "Author <a@uth.or>|Author <a@uth.or>"
    );
    assert_eq!(
        git(imported.path(), ["cat-file", "-p", "main:file"]),
        "contents"
    );
}

#[test]
fn fast_import_accepts_adjacent_commit_records_without_blank_separator() {
    let imported = git_init();
    let stream = "\
commit refs/heads/main
mark :1
committer Author <a@uth.or> now
data <<EOF
start
EOF
M 100644 inline file
data <<EOF
contents
EOF
commit refs/heads/main
committer Author <a@uth.or> now
data <<EOF
tip
EOF
from :1
";
    run_zmin_with_stdin(
        imported.path(),
        ["fast-import", "--date-format=now"],
        stream,
    );

    assert_eq!(
        git(imported.path(), ["log", "--format=%s", "main"]),
        "tip\nstart"
    );
    assert_eq!(
        git(imported.path(), ["cat-file", "-p", "main:file"]),
        "contents"
    );
}
