mod common;

use std::fs;

use common::{
    command_output_with_env, configure_identity, git, git_failure_output, git_with_env, run_zmin,
    run_zmin_failure_output, write_file,
};

#[test]
fn for_each_ref_matches_stock_git_for_common_formats() {
    let repo = common::git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "tag.gpgSign", "false"]);
    write_file(repo.path(), "a.txt", "hello\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial subject"]);
    git(repo.path(), ["branch", "feature"]);
    git_with_env(
        repo.path(),
        ["tag", "-a", "v1", "-m", "tag subject\nsecond line\n\nbody"],
    );
    let head = git(repo.path(), ["rev-parse", "HEAD"]);
    fs::write(
        repo.path().join("signed-tag.txt"),
        format!(
            concat!(
                "object {}\n",
                "type commit\n",
                "tag signed\n",
                "tagger Tester <tester@example.test> 1700000000 +0000\n",
                "\n",
                "signed subject\n",
                "-----BEGIN PGP SIGNATURE-----\n",
                "test-signature\n",
                "-----END PGP SIGNATURE-----\n"
            ),
            head
        ),
    )
    .expect("write signed tag object");
    let signed_id = git(
        repo.path(),
        ["hash-object", "-t", "tag", "-w", "signed-tag.txt"],
    );
    git(repo.path(), ["update-ref", "refs/tags/signed", &signed_id]);
    let blob_id = git(repo.path(), ["hash-object", "a.txt"]);
    let tree_id = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);
    git(repo.path(), ["update-ref", "refs/blobs/a", &blob_id]);
    git(repo.path(), ["update-ref", "refs/trees/root", &tree_id]);

    assert_eq!(
        run_zmin(repo.path(), ["for-each-ref"]),
        git(repo.path(), ["for-each-ref"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["for-each-ref", "refs/heads"]),
        git(repo.path(), ["for-each-ref", "refs/heads"])
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            [
                "for-each-ref",
                "--format=%(refname) %(objectname) %(objecttype) %(subject)",
                "refs/blobs",
                "refs/heads",
                "refs/tags",
                "refs/trees",
            ],
        ),
        git(
            repo.path(),
            [
                "for-each-ref",
                "--format=%(refname) %(objectname) %(objecttype) %(subject)",
                "refs/blobs",
                "refs/heads",
                "refs/tags",
                "refs/trees",
            ],
        )
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            [
                "for-each-ref",
                "--sort=objectsize",
                "--format=%(objectsize)|%(refname:short)",
                "refs/blobs",
                "refs/heads",
                "refs/tags",
                "refs/trees",
            ],
        ),
        git(
            repo.path(),
            [
                "for-each-ref",
                "--sort=objectsize",
                "--format=%(objectsize)|%(refname:short)",
                "refs/blobs",
                "refs/heads",
                "refs/tags",
                "refs/trees",
            ],
        )
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            [
                "for-each-ref",
                "--sort=refname",
                "--format=%(refname:short)|%(objecttype)|%(objectsize)",
                "refs/blobs",
                "refs/heads",
                "refs/tags",
                "refs/trees",
            ],
        ),
        git(
            repo.path(),
            [
                "for-each-ref",
                "--sort=refname",
                "--format=%(refname:short)|%(objecttype)|%(objectsize)",
                "refs/blobs",
                "refs/heads",
                "refs/tags",
                "refs/trees",
            ],
        )
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            [
                "for-each-ref",
                "--sort=-refname",
                "--format=%(refname:short)",
                "refs/heads",
                "refs/tags",
            ],
        ),
        git(
            repo.path(),
            [
                "for-each-ref",
                "--sort=-refname",
                "--format=%(refname:short)",
                "refs/heads",
                "refs/tags",
            ],
        )
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            [
                "for-each-ref",
                "--sort=objecttype",
                "--sort=-subject",
                "--format=%(objecttype) %(subject) %(refname:short)",
                "refs/heads",
                "refs/tags",
            ],
        ),
        git(
            repo.path(),
            [
                "for-each-ref",
                "--sort=objecttype",
                "--sort=-subject",
                "--format=%(objecttype) %(subject) %(refname:short)",
                "refs/heads",
                "refs/tags",
            ],
        )
    );
    for format in [
        "%(objectname:short=4)",
        "%(refname:short) %(objectname:short=12)",
        "%(objectname:short=40)",
    ] {
        assert_eq!(
            common::run_zmin_args(repo.path(), &["for-each-ref", "--format", format]),
            common::git_args(repo.path(), &["for-each-ref", "--format", format]),
            "for-each-ref objectname length should match for {format}"
        );
    }
}

#[test]
fn for_each_ref_objectname_short_invalid_lengths_match_stock_git() {
    let repo = common::git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "hello\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial subject"]);

    for format in [
        "%(objectname:short=0)",
        "%(objectname:short=abc)",
        "%(objectname:short=-1)",
        "%(objectname:short=)",
    ] {
        let args = ["for-each-ref", "--format", format, "refs/heads"];
        assert_eq!(
            run_zmin_failure_output(repo.path(), &args),
            git_failure_output(repo.path(), &args),
            "for-each-ref invalid objectname length should match for {format}"
        );
    }
}

#[test]
fn for_each_ref_date_atoms_match_stock_git() {
    let repo = common::git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "tag.gpgSign", "false"]);
    write_file(repo.path(), "dated.txt", "dated\n");
    git(repo.path(), ["add", "-A"]);
    command_output_with_env(
        "git",
        repo.path(),
        &["commit", "-m", "dated"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000100 +0230"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000200 +0230"),
        ],
        "git",
    );
    command_output_with_env(
        "git",
        repo.path(),
        &["tag", "-a", "dated-tag", "-m", "dated tag"],
        &[
            ("GIT_COMMITTER_NAME", "Tagger"),
            ("GIT_COMMITTER_EMAIL", "tagger@example.test"),
            ("GIT_COMMITTER_DATE", "1700000300 -0500"),
        ],
        "git",
    );

    for (pattern, format) in [
        (
            "refs/heads",
            "%(refname:short)|%(authorname)|%(authoremail)|%(authordate)|%(authordate:unix)|%(authordate:raw)|%(authordate:iso)|%(authordate:iso-strict)|%(authordate:rfc)|%(authordate:rfc2822)|%(authordate:short)",
        ),
        (
            "refs/heads",
            "%(refname:short)|%(committerdate)|%(committerdate:unix)|%(committerdate:raw)|%(committerdate:iso)|%(committerdate:iso-strict)|%(committerdate:rfc)|%(committerdate:rfc2822)|%(committerdate:short)",
        ),
        (
            "refs/heads refs/tags",
            "%(refname:short)|%(committername)|%(committeremail)",
        ),
        (
            "refs/tags",
            "%(refname:short)|%(taggerdate)|%(taggerdate:unix)|%(taggerdate:raw)|%(taggerdate:iso)|%(taggerdate:iso-strict)|%(taggerdate:rfc)|%(taggerdate:rfc2822)|%(taggerdate:short)",
        ),
        (
            "refs/heads refs/tags",
            "%(refname:short)|%(taggername)|%(taggeremail)",
        ),
        (
            "refs/tags",
            "%(refname:short)|%(authorname)|%(authoremail)|%(authordate)|%(authordate:unix)|%(authordate:raw)|%(authordate:iso)|%(authordate:iso-strict)|%(authordate:rfc)|%(authordate:rfc2822)|%(authordate:short)",
        ),
        (
            "refs/heads refs/tags",
            "%(refname:short)|%(creator)|%(creatordate)|%(creatordate:unix)|%(creatordate:raw)|%(creatordate:iso)|%(creatordate:iso-strict)|%(creatordate:rfc)|%(creatordate:rfc2822)|%(creatordate:short)",
        ),
    ] {
        let patterns = pattern.split_whitespace().collect::<Vec<_>>();
        let mut zmin_args = vec!["for-each-ref", "--format", format];
        zmin_args.extend(patterns.iter().copied());
        let mut git_args = vec!["for-each-ref", "--format", format];
        git_args.extend(patterns.iter().copied());
        assert_eq!(
            common::run_zmin_args(repo.path(), &zmin_args),
            common::git_args(repo.path(), &git_args),
            "for-each-ref date atoms should match for {pattern}"
        );
    }
}
