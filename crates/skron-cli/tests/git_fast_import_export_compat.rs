mod common;

use std::fs;

use common::{
    configure_identity, git, git_init, git_with_env, git_with_stdin, run_skron,
    run_skron_with_stdin, write_file,
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

    let stream = run_skron(source.path(), ["fast-export", "--all"]);
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
    run_skron_with_stdin(imported.path(), ["fast-import"], &stream);

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
