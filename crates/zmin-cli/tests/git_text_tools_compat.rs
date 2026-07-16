mod common;

use std::fs;

use common::{
    command_stdout_bytes, command_stdout_bytes_with_stdin, git, git_args, git_failure_output,
    git_init, git_with_env, git_with_stdin, git_with_stdin_args, run_zmin, run_zmin_args,
    run_zmin_failure_output, run_zmin_with_stdin, run_zmin_with_stdin_args, zmin_bin,
};

#[test]
fn check_mailmap_matches_stock_git_for_common_entries() {
    let repo = git_init();
    fs::write(
        repo.path().join(".mailmap"),
        b"Proper Name <proper@example.com> Alias Name <alias@example.com>\n<canonical@example.com> <old@example.com>\nDisplay Only <display@example.com>\n<emailonly@example.com>\n",
    )
    .expect("write mailmap");

    for identity in [
        "Alias Name <alias@example.com>",
        "Other <old@example.com>",
        "Someone <display@example.com>",
        "Name <emailonly@example.com>",
        "None <none@example.com>",
    ] {
        assert_eq!(
            run_zmin(repo.path(), ["check-mailmap", identity]),
            git(repo.path(), ["check-mailmap", identity])
        );
    }

    let input =
        "Alias Name <alias@example.com>\nOther <old@example.com>\nNone <none@example.com>\n";
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["check-mailmap", "--stdin"], input),
        git_with_stdin(repo.path(), ["check-mailmap", "--stdin"], input)
    );

    let alt_mailmap = repo.path().join("alt.mailmap");
    fs::write(
        &alt_mailmap,
        b"Alt Name <alt@example.com> Alt Alias <alt-alias@example.com>\n",
    )
    .expect("write alt mailmap");
    assert_eq!(
        run_zmin(
            repo.path(),
            [
                "check-mailmap",
                "--mailmap-file=alt.mailmap",
                "Alt Alias <alt-alias@example.com>",
            ],
        ),
        git(
            repo.path(),
            [
                "check-mailmap",
                "--mailmap-file=alt.mailmap",
                "Alt Alias <alt-alias@example.com>",
            ],
        )
    );

    let blob_mailmap = repo.path().join("blob.mailmap");
    fs::write(
        &blob_mailmap,
        b"Blob Name <blob@example.com> Blob Alias <blob-alias@example.com>\n",
    )
    .expect("write blob mailmap");
    let blob_oid = git(repo.path(), ["hash-object", "-w", "blob.mailmap"]);
    let blob_oid = blob_oid.trim();
    assert_eq!(
        run_zmin_args(
            repo.path(),
            &[
                "check-mailmap",
                &format!("--mailmap-blob={blob_oid}"),
                "Blob Alias <blob-alias@example.com>",
            ],
        ),
        git_args(
            repo.path(),
            &[
                "check-mailmap",
                &format!("--mailmap-blob={blob_oid}"),
                "Blob Alias <blob-alias@example.com>",
            ],
        )
    );

    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &["check-mailmap", "--stdin", "--stdin", "--stdin"],
            input,
        ),
        git_with_stdin_args(
            repo.path(),
            &["check-mailmap", "--stdin", "--stdin", "--stdin"],
            input,
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--stdin",
                "--no-stdin",
                "Alias Name <alias@example.com>",
            ],
            "Alias Name <alias@example.com>\n",
        ),
        git_with_stdin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--stdin",
                "--no-stdin",
                "Alias Name <alias@example.com>",
            ],
            "Alias Name <alias@example.com>\n",
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &["check-mailmap", "--no-stdin", "--stdin"],
            "Alias Name <alias@example.com>\n",
        ),
        git_with_stdin_args(
            repo.path(),
            &["check-mailmap", "--no-stdin", "--stdin"],
            "Alias Name <alias@example.com>\n",
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &["check-mailmap", "--stdin", "--mailmap-file", "alt.mailmap"],
            "Alt Alias <alt-alias@example.com>\n",
        ),
        git_with_stdin_args(
            repo.path(),
            &["check-mailmap", "--stdin", "--mailmap-file", "alt.mailmap"],
            "Alt Alias <alt-alias@example.com>\n",
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &["check-mailmap", "--stdin", "--mailmap-blob", blob_oid],
            "Blob Alias <blob-alias@example.com>\n",
        ),
        git_with_stdin_args(
            repo.path(),
            &["check-mailmap", "--stdin", "--mailmap-blob", blob_oid],
            "Blob Alias <blob-alias@example.com>\n",
        )
    );
    assert_eq!(
        run_zmin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--mailmap-file",
                "alt.mailmap",
                "--mailmap-blob",
                blob_oid,
                "Blob Alias <blob-alias@example.com>",
            ],
        ),
        git_args(
            repo.path(),
            &[
                "check-mailmap",
                "--mailmap-file",
                "alt.mailmap",
                "--mailmap-blob",
                blob_oid,
                "Blob Alias <blob-alias@example.com>",
            ],
        )
    );
    assert_eq!(
        run_zmin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--mailmap-blob",
                blob_oid,
                "--mailmap-file",
                "alt.mailmap",
                "Alt Alias <alt-alias@example.com>",
            ],
        ),
        git_args(
            repo.path(),
            &[
                "check-mailmap",
                "--mailmap-blob",
                blob_oid,
                "--mailmap-file",
                "alt.mailmap",
                "Alt Alias <alt-alias@example.com>",
            ],
        )
    );
    assert_eq!(
        run_zmin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--mailmap-file=alt.mailmap",
                &format!("--mailmap-blob={blob_oid}"),
                "Blob Alias <blob-alias@example.com>",
            ],
        ),
        git_args(
            repo.path(),
            &[
                "check-mailmap",
                "--mailmap-file=alt.mailmap",
                &format!("--mailmap-blob={blob_oid}"),
                "Blob Alias <blob-alias@example.com>",
            ],
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--stdin",
                "--stdin",
                "--no-stdin",
                "--stdin"
            ],
            "Alias Name <alias@example.com>\n",
        ),
        git_with_stdin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--stdin",
                "--stdin",
                "--no-stdin",
                "--stdin"
            ],
            "Alias Name <alias@example.com>\n",
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--stdin",
                "--no-stdin",
                "--mailmap-file",
                "alt.mailmap",
                "Alt Alias <alt-alias@example.com>",
            ],
            "Alias Name <alias@example.com>\n",
        ),
        git_with_stdin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--stdin",
                "--no-stdin",
                "--mailmap-file",
                "alt.mailmap",
                "Alt Alias <alt-alias@example.com>",
            ],
            "Alias Name <alias@example.com>\n",
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--stdin",
                "--no-stdin",
                "--mailmap-blob",
                blob_oid,
                "Blob Alias <blob-alias@example.com>",
            ],
            "Alias Name <alias@example.com>\n",
        ),
        git_with_stdin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--stdin",
                "--no-stdin",
                "--mailmap-blob",
                blob_oid,
                "Blob Alias <blob-alias@example.com>",
            ],
            "Alias Name <alias@example.com>\n",
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--stdin",
                "--mailmap-file",
                "alt.mailmap",
                "--mailmap-blob",
                blob_oid,
            ],
            "Blob Alias <blob-alias@example.com>\n",
        ),
        git_with_stdin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--stdin",
                "--mailmap-file",
                "alt.mailmap",
                "--mailmap-blob",
                blob_oid,
            ],
            "Blob Alias <blob-alias@example.com>\n",
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--stdin",
                "--mailmap-blob",
                blob_oid,
                "--mailmap-file",
                "alt.mailmap",
            ],
            "Alt Alias <alt-alias@example.com>\n",
        ),
        git_with_stdin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--stdin",
                "--mailmap-blob",
                blob_oid,
                "--mailmap-file",
                "alt.mailmap",
            ],
            "Alt Alias <alt-alias@example.com>\n",
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--stdin",
                "--no-stdin",
                "--stdin",
                "--mailmap-file",
                "alt.mailmap",
            ],
            "Alt Alias <alt-alias@example.com>\n",
        ),
        git_with_stdin_args(
            repo.path(),
            &[
                "check-mailmap",
                "--stdin",
                "--no-stdin",
                "--stdin",
                "--mailmap-file",
                "alt.mailmap",
            ],
            "Alt Alias <alt-alias@example.com>\n",
        )
    );
}

#[test]
fn check_attr_matches_stock_git_for_common_attributes() {
    let repo = git_init();
    fs::write(
        repo.path().join(".gitattributes"),
        b"*.rs text diff=rust custom\n*.bin -text binary\n/docs/** linguist-documentation\n*.md !diff\n",
    )
    .expect("write attributes");
    fs::create_dir_all(repo.path().join("docs")).expect("create docs");
    fs::write(repo.path().join("main.rs"), b"fn main() {}\n").expect("write rust");
    fs::write(repo.path().join("file.bin"), b"\0bin\n").expect("write bin");
    fs::write(repo.path().join("docs/a.md"), b"doc\n").expect("write doc");
    fs::write(repo.path().join("readme.md"), b"readme\n").expect("write readme");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "attrs"]);
    fs::write(
        repo.path().join(".gitattributes"),
        b"*.rs -text diff=changed custom\n*.bin text -binary\n/docs/** linguist-documentation\n*.md diff=markdown\n",
    )
    .expect("rewrite attributes");

    for args in [
        ["check-attr", "text", "diff", "custom", "--", "main.rs"].as_slice(),
        ["check-attr", "text", "binary", "--", "file.bin"].as_slice(),
        ["check-attr", "linguist-documentation", "--", "docs/a.md"].as_slice(),
        ["check-attr", "diff", "--", "readme.md"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args)
        );
    }

    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["check-attr", "--stdin", "text", "diff"],
            "main.rs\nfile.bin\n"
        ),
        git_with_stdin(
            repo.path(),
            ["check-attr", "--stdin", "text", "diff"],
            "main.rs\nfile.bin\n"
        )
    );

    for args in [
        ["check-attr", "--all", "--", "main.rs"].as_slice(),
        ["check-attr", "-a", "--", "file.bin"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args)
        );
    }

    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["check-attr", "--stdin", "--all"],
            "main.rs\nfile.bin\n"
        ),
        git_with_stdin(
            repo.path(),
            ["check-attr", "--stdin", "--all"],
            "main.rs\nfile.bin\n"
        )
    );

    for args in [
        ["check-attr", "--cached", "text", "diff", "--", "main.rs"].as_slice(),
        [
            "check-attr",
            "--source=HEAD",
            "text",
            "diff",
            "--",
            "main.rs",
        ]
        .as_slice(),
        [
            "check-attr",
            "--source",
            "HEAD",
            "text",
            "diff",
            "--",
            "main.rs",
        ]
        .as_slice(),
        ["check-attr", "-z", "text", "diff", "--", "main.rs"].as_slice(),
        ["check-attr", "--all", "-z", "--", "main.rs"].as_slice(),
        [
            "check-attr",
            "--source=HEAD",
            "--all",
            "-z",
            "--",
            "main.rs",
        ]
        .as_slice(),
        [
            "check-attr",
            "--cached",
            "-z",
            "text",
            "diff",
            "--",
            "main.rs",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_stdout_bytes(zmin_bin(), repo.path(), args),
            command_stdout_bytes("git", repo.path(), args)
        );
    }

    assert_eq!(
        command_stdout_bytes_with_stdin(
            zmin_bin(),
            repo.path(),
            &["check-attr", "--stdin", "-z", "text", "diff"],
            b"main.rs\0file.bin\0",
        ),
        command_stdout_bytes_with_stdin(
            "git",
            repo.path(),
            &["check-attr", "--stdin", "-z", "text", "diff"],
            b"main.rs\0file.bin\0",
        )
    );

    fs::create_dir_all(repo.path().join("case/a/b/d")).expect("create case dirs");
    fs::create_dir_all(repo.path().join("case/a/c")).expect("create case dirs");
    fs::write(
        repo.path().join("case/.gitattributes"),
        b"[attr]notest !test\nf test=f\na/i test=a/i\nonoff test -test\noffon -test test\nno notest\nA/e/F test=A/e/F\n",
    )
    .expect("write case root attributes");
    fs::write(
        repo.path().join("case/a/.gitattributes"),
        b"g test=a/g\nb/g test=a/b/g\n",
    )
    .expect("write case a attributes");
    fs::write(
        repo.path().join("case/a/b/.gitattributes"),
        b"h test=a/b/h\nd/* test=a/b/d/*\nd/yes notest\n",
    )
    .expect("write case a/b attributes");

    for args in [
        [
            "-c",
            "core.ignorecase=0",
            "check-attr",
            "test",
            "--",
            "case/a/B/g",
        ]
        .as_slice(),
        [
            "-c",
            "core.ignorecase=0",
            "check-attr",
            "test",
            "--",
            "case/A/B/D/NO",
        ]
        .as_slice(),
        [
            "-c",
            "core.ignorecase=1",
            "check-attr",
            "test",
            "--",
            "case/a/B/g",
        ]
        .as_slice(),
        [
            "-c",
            "core.ignorecase=1",
            "check-attr",
            "test",
            "--",
            "case/A/B/D/NO",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args)
        );
    }

    assert_eq!(
        run_zmin_failure_output(
            repo.path(),
            &["check-attr", "builtin_objectmode", "--", "missing.file"],
        ),
        git_failure_output(
            repo.path(),
            &["check-attr", "builtin_objectmode", "--", "missing.file"],
        )
    );
}

#[test]
fn column_matches_stock_git_for_common_modes() {
    let repo = git_init();
    let input = "alpha\nbeta\ngamma\ndelta\n";
    for args in [
        ["column", "--mode=plain"].as_slice(),
        ["column", "--mode=column", "--padding=2", "--width=20"].as_slice(),
        ["column", "--mode=row", "--padding=2", "--width=20"].as_slice(),
        ["column", "--padding=2", "--width=20"].as_slice(),
        ["column", "--no-mode", "--width=20"].as_slice(),
        ["column", "--mode", "--width=20"].as_slice(),
        ["column", "--mode=", "--width=20"].as_slice(),
        ["column", "--width=20", "--no-width"].as_slice(),
        ["column", "--no-command", "--width=20"].as_slice(),
        ["column", "--raw-mode=1", "--width=20"].as_slice(),
        ["column", "--raw-mode=17", "--width=20"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_with_stdin_args(repo.path(), args, input),
            git_with_stdin_args(repo.path(), args, input)
        );
    }

    git(
        repo.path(),
        ["config", "--replace-all", "column.status", "column,dense"],
    );
    assert_eq!(
        run_zmin_with_stdin_args(repo.path(), &["column", "--command=status"], input),
        git_with_stdin_args(repo.path(), &["column", "--command=status"], input)
    );
    git(
        repo.path(),
        ["config", "--replace-all", "column.status", "column,nodense"],
    );
    assert_eq!(
        run_zmin_with_stdin_args(repo.path(), &["column", "--command=status"], input),
        git_with_stdin_args(repo.path(), &["column", "--command=status"], input)
    );
    git(
        repo.path(),
        ["config", "--replace-all", "column.status", "row,dense"],
    );
    assert_eq!(
        run_zmin_with_stdin_args(repo.path(), &["column", "--command=status"], input),
        git_with_stdin_args(repo.path(), &["column", "--command=status"], input)
    );
    git(
        repo.path(),
        ["config", "--replace-all", "column.status", "dense"],
    );
    assert_eq!(
        run_zmin_with_stdin_args(repo.path(), &["column", "--command=status"], input),
        git_with_stdin_args(repo.path(), &["column", "--command=status"], input)
    );
    git(
        repo.path(),
        ["config", "--replace-all", "column.status", "nodense"],
    );
    assert_eq!(
        run_zmin_with_stdin_args(repo.path(), &["column", "--command=status"], input),
        git_with_stdin_args(repo.path(), &["column", "--command=status"], input)
    );
    git(
        repo.path(),
        ["config", "--replace-all", "column.status", "row,nodense"],
    );
    assert_eq!(
        run_zmin_with_stdin_args(repo.path(), &["column", "--command=status"], input),
        git_with_stdin_args(repo.path(), &["column", "--command=status"], input)
    );
    git(repo.path(), ["config", "--unset-all", "column.status"]);
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &["column", "--command=", "--mode=column", "--width=20"],
            input,
        ),
        git_with_stdin_args(
            repo.path(),
            &["column", "--command=", "--mode=column", "--width=20"],
            input,
        )
    );
    for args in [
        ["column", "--command=status", "--mode=column"].as_slice(),
        ["column", "--command=status", "--mode=row"].as_slice(),
        ["column", "--command=status", "--mode=plain"].as_slice(),
        ["column", "--command=status", "--mode=nodense"].as_slice(),
        ["column", "--command=status", "--mode=row,nodense"].as_slice(),
        ["column", "--command=status", "--mode=column,nodense"].as_slice(),
        ["column", "--command=status", "--mode="].as_slice(),
        ["column", "--command=status", "--no-mode"].as_slice(),
        ["column", "--command=status", "--raw-mode=0"].as_slice(),
        ["column", "--command=status", "--raw-mode=16"].as_slice(),
        ["column", "--command=status", "--raw-mode=17"].as_slice(),
        ["column", "--command=", "--width=20"].as_slice(),
        ["column", "--command=", "--raw-mode=16", "--width=20"].as_slice(),
        ["column", "--no-command", "--mode=column", "--width=20"].as_slice(),
        ["column", "--no-command", "--raw-mode=16", "--width=20"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_with_stdin_args(repo.path(), args, input),
            git_with_stdin_args(repo.path(), args, input)
        );
    }

    let dense_input = "one\ntwo\nthree\nfour\nfive\n";
    for args in [
        ["column", "--mode=dense", "--padding=2", "--width=20"].as_slice(),
        ["column", "--mode=nodense", "--padding=2", "--width=20"].as_slice(),
        ["column", "--mode=column,dense", "--padding=2", "--width=18"].as_slice(),
        ["column", "--mode=row,dense", "--padding=2", "--width=20"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_with_stdin_args(repo.path(), args, dense_input),
            git_with_stdin_args(repo.path(), args, dense_input)
        );
    }

    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &["column", "--command=status", "--width=20"],
            input,
        ),
        git_with_stdin_args(
            repo.path(),
            &["column", "--command=status", "--width=20"],
            input,
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &["column", "--mode=column", "--indent=>>", "--width=20"],
            input,
        ),
        git_with_stdin_args(
            repo.path(),
            &["column", "--mode=column", "--indent=>>", "--width=20"],
            input,
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &["column", "--mode=column", "--nl=ZZ", "--width=20"],
            input,
        ),
        git_with_stdin_args(
            repo.path(),
            &["column", "--mode=column", "--nl=ZZ", "--width=20"],
            input,
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &["column", "--raw-mode=0", "--width=20"],
            input,
        ),
        git_with_stdin_args(
            repo.path(),
            &["column", "--raw-mode=0", "--width=20"],
            input,
        )
    );
    for args in [
        ["column", "--raw-mode=0", "--mode=column", "--width=20"].as_slice(),
        ["column", "--mode=column", "--raw-mode=0", "--width=20"].as_slice(),
        ["column", "--raw-mode=1", "--mode=column", "--width=20"].as_slice(),
        ["column", "--mode=column", "--raw-mode=1", "--width=20"].as_slice(),
        ["column", "--raw-mode=1", "--padding=2", "--width=20"].as_slice(),
        ["column", "--raw-mode=1", "--indent=>>", "--width=20"].as_slice(),
        ["column", "--raw-mode=1", "--nl=ZZ", "--width=20"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_with_stdin_args(repo.path(), args, input),
            git_with_stdin_args(repo.path(), args, input)
        );
    }
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &[
                "column",
                "--indent=>>",
                "--no-indent",
                "--mode=column",
                "--width=20"
            ],
            input,
        ),
        git_with_stdin_args(
            repo.path(),
            &[
                "column",
                "--indent=>>",
                "--no-indent",
                "--mode=column",
                "--width=20"
            ],
            input,
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &[
                "column",
                "--nl=ZZ",
                "--no-nl",
                "--mode=column",
                "--width=20"
            ],
            input,
        ),
        git_with_stdin_args(
            repo.path(),
            &[
                "column",
                "--nl=ZZ",
                "--no-nl",
                "--mode=column",
                "--width=20"
            ],
            input,
        )
    );
    assert_eq!(
        run_zmin_with_stdin_args(
            repo.path(),
            &[
                "column",
                "--padding=2",
                "--mode=column",
                "--width=20",
                "--no-padding",
            ],
            input,
        ),
        git_with_stdin_args(
            repo.path(),
            &[
                "column",
                "--padding=2",
                "--mode=column",
                "--width=20",
                "--no-padding",
            ],
            input,
        )
    );

    assert_eq!(
        run_zmin_failure_output(repo.path(), &["column", "--mode=bad"]),
        git_failure_output(repo.path(), &["column", "--mode=bad"])
    );
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["column", "--width="]),
        git_failure_output(repo.path(), &["column", "--width="])
    );
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["column", "--padding="]),
        git_failure_output(repo.path(), &["column", "--padding="])
    );
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["column", "--raw-mode=bogus", "--width=20"]),
        git_failure_output(repo.path(), &["column", "--raw-mode=bogus", "--width=20"])
    );
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["column", "--command", "status"]),
        git_failure_output(repo.path(), &["column", "--command", "status"])
    );
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["column", "--width=20", "--command=status"]),
        git_failure_output(repo.path(), &["column", "--width=20", "--command=status"])
    );
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["column", "--command=status", "--no-command"]),
        git_failure_output(repo.path(), &["column", "--command=status", "--no-command"])
    );
    assert_eq!(
        run_zmin_failure_output(
            repo.path(),
            &["column", "--command=", "--no-command", "--width=20"]
        ),
        git_failure_output(
            repo.path(),
            &["column", "--command=", "--no-command", "--width=20"]
        )
    );
}

#[test]
fn stripspace_matches_stock_git_for_common_modes() {
    let repo = git_init();
    let fixture = "\n  a  \n\n\n  b  \n\n";
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["stripspace"], fixture),
        git_with_stdin(repo.path(), ["stripspace"], fixture)
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["stripspace", "-s"], fixture),
        git_with_stdin(repo.path(), ["stripspace", "-s"], fixture)
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["stripspace", "-c"], fixture),
        git_with_stdin(repo.path(), ["stripspace", "-c"], fixture)
    );

    let comment_fixture = "# c\n\n x # not comment\n#d\n\n";
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["stripspace", "--strip-comments"],
            comment_fixture
        ),
        git_with_stdin(
            repo.path(),
            ["stripspace", "--strip-comments"],
            comment_fixture
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["stripspace", "--comment-lines"],
            comment_fixture
        ),
        git_with_stdin(
            repo.path(),
            ["stripspace", "--comment-lines"],
            comment_fixture
        )
    );

    for args in [
        ["stripspace", "-s", "--comment-lines"].as_slice(),
        ["stripspace", "--comment-lines", "-s"].as_slice(),
        ["stripspace", "-c", "--strip-comments"].as_slice(),
        ["stripspace", "--strip-comments", "-c"].as_slice(),
        ["stripspace", "-s", "-c"].as_slice(),
        ["stripspace", "-c", "-s"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args),
            "args: {args:?}"
        );
    }

    for args in [
        ["stripspace", "-s", "--strip-comments"].as_slice(),
        ["stripspace", "--strip-comments", "-s"].as_slice(),
        ["stripspace", "-c", "--comment-lines"].as_slice(),
        ["stripspace", "--comment-lines", "-c"].as_slice(),
        ["stripspace", "-s", "--strip-comments", "-s"].as_slice(),
        ["stripspace", "-c", "--comment-lines", "-c"].as_slice(),
        ["stripspace", "--strip-comments", "-s", "--strip-comments"].as_slice(),
        ["stripspace", "--comment-lines", "-c", "--comment-lines"].as_slice(),
        ["stripspace", "-s", "--strip-comments", "--strip-comments"].as_slice(),
        ["stripspace", "-c", "--comment-lines", "--comment-lines"].as_slice(),
        ["stripspace", "-s", "-s", "--strip-comments"].as_slice(),
        ["stripspace", "--strip-comments", "--strip-comments", "-s"].as_slice(),
        ["stripspace", "-c", "-c", "--comment-lines"].as_slice(),
        ["stripspace", "--comment-lines", "--comment-lines", "-c"].as_slice(),
        [
            "stripspace",
            "-s",
            "--strip-comments",
            "--strip-comments",
            "-s",
        ]
        .as_slice(),
        [
            "stripspace",
            "-c",
            "--comment-lines",
            "--comment-lines",
            "-c",
        ]
        .as_slice(),
        ["stripspace", "--strip-comments", "-s", "-s"].as_slice(),
        ["stripspace", "--comment-lines", "-c", "-c"].as_slice(),
        [
            "stripspace",
            "--strip-comments",
            "--strip-comments",
            "--strip-comments",
        ]
        .as_slice(),
        [
            "stripspace",
            "--comment-lines",
            "--comment-lines",
            "--comment-lines",
        ]
        .as_slice(),
        ["stripspace", "-s", "-s", "-s"].as_slice(),
        ["stripspace", "-c", "-c", "-c"].as_slice(),
        ["stripspace", "-s", "-s", "--strip-comments", "-s"].as_slice(),
        ["stripspace", "-c", "-c", "--comment-lines", "-c"].as_slice(),
        [
            "stripspace",
            "--strip-comments",
            "-s",
            "--strip-comments",
            "-s",
        ]
        .as_slice(),
        [
            "stripspace",
            "--comment-lines",
            "-c",
            "--comment-lines",
            "-c",
        ]
        .as_slice(),
        [
            "stripspace",
            "-s",
            "--strip-comments",
            "-s",
            "--strip-comments",
        ]
        .as_slice(),
        [
            "stripspace",
            "-c",
            "--comment-lines",
            "-c",
            "--comment-lines",
        ]
        .as_slice(),
        [
            "stripspace",
            "--strip-comments",
            "--strip-comments",
            "-s",
            "-s",
        ]
        .as_slice(),
        [
            "stripspace",
            "--comment-lines",
            "--comment-lines",
            "-c",
            "-c",
        ]
        .as_slice(),
        ["stripspace", "-s", "-s", "-s", "-s"].as_slice(),
        ["stripspace", "-c", "-c", "-c", "-c"].as_slice(),
        [
            "stripspace",
            "--strip-comments",
            "--strip-comments",
            "--strip-comments",
            "--strip-comments",
        ]
        .as_slice(),
        [
            "stripspace",
            "--comment-lines",
            "--comment-lines",
            "--comment-lines",
            "--comment-lines",
        ]
        .as_slice(),
        [
            "stripspace",
            "-s",
            "--strip-comments",
            "--strip-comments",
            "--strip-comments",
        ]
        .as_slice(),
        [
            "stripspace",
            "-c",
            "--comment-lines",
            "--comment-lines",
            "--comment-lines",
        ]
        .as_slice(),
        ["stripspace", "--strip-comments", "-s", "-s", "-s"].as_slice(),
        ["stripspace", "--comment-lines", "-c", "-c", "-c"].as_slice(),
        [
            "stripspace",
            "-s",
            "-s",
            "--strip-comments",
            "--strip-comments",
        ]
        .as_slice(),
        [
            "stripspace",
            "-c",
            "-c",
            "--comment-lines",
            "--comment-lines",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_with_stdin_args(repo.path(), args, comment_fixture),
            git_with_stdin_args(repo.path(), args, comment_fixture),
            "args: {args:?}"
        );
    }
}
