mod common;

use std::fs;

use common::{configure_identity, git, git_with_env, run_zmin, write_file};

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
                "refs/heads",
                "refs/tags",
            ],
        ),
        git(
            repo.path(),
            [
                "for-each-ref",
                "--format=%(refname) %(objectname) %(objecttype) %(subject)",
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
}
