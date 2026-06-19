mod common;

use std::fs;

use common::{
    git, git_args, git_init, git_with_stdin, git_with_stdin_args, run_zmin, run_zmin_args,
    run_zmin_with_stdin, run_zmin_with_stdin_args,
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
}
