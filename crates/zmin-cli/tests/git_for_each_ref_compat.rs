mod common;

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use common::{
    command_output_with_env, configure_identity, git, git_failure_output, git_with_env,
    pinned_git_args, pinned_git_init_sha256, pinned_git_with_env, required_pinned_stock_git,
    run_zmin, run_zmin_failure_output, write_file, zmin_bin,
};

fn exact_command(program: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run exact compatibility command")
}

fn exact_command_with_stdin(program: &Path, cwd: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn exact compatibility command");
    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(input)
        .expect("write exact compatibility input");
    child
        .wait_with_output()
        .expect("wait exact compatibility command")
}

fn assert_refs_list_exact(repo: &Path, stock: &Path, args: &[&str]) {
    let stock_output = exact_command(stock, repo, args);
    let zmin_output = exact_command(Path::new(zmin_bin()), repo, args);
    assert_eq!(
        zmin_output.status.code(),
        stock_output.status.code(),
        "exit mismatch for {args:?}: stock={:?} zmin={:?}",
        stock_output.status.code(),
        zmin_output.status.code()
    );
    assert_eq!(
        zmin_output.stdout, stock_output.stdout,
        "stdout mismatch for {args:?}"
    );
    assert_eq!(
        zmin_output.stderr, stock_output.stderr,
        "stderr mismatch for {args:?}"
    );
}

#[test]
fn refs_list_matches_stock_for_each_ref_format_families() {
    let stock = required_pinned_stock_git();
    let repo = common::git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "refs list\n");
    git(repo.path(), ["add", "a.txt"]);
    command_output_with_env(
        "git",
        repo.path(),
        &["commit", "-m", "refs list"],
        &[
            ("GIT_AUTHOR_DATE", "1700000100 +0230"),
            ("GIT_COMMITTER_DATE", "1700000200 +0230"),
        ],
        "git commit",
    );
    git(repo.path(), ["branch", "feature/topic"]);
    git(repo.path(), ["tag", "v1"]);

    for args in [
        vec!["refs", "list", "--format=%(refname)", "refs/heads"],
        vec![
            "refs",
            "list",
            "--sort=refname",
            "--format=%(refname:short)",
            "refs/heads",
        ],
        vec![
            "refs",
            "list",
            "--sort=refname",
            "--no-sort",
            "--format=%(refname)",
        ],
        vec![
            "refs",
            "list",
            "--format=%(authordate:iso-strict)|%(refname)",
            "refs/heads",
        ],
        vec![
            "refs",
            "list",
            "--shell",
            "--format=%(refname:short)=%(subject)",
            "refs/tags",
        ],
        vec![
            "refs",
            "list",
            "--perl",
            "--format=%(refname:short)",
            "refs/tags",
        ],
        vec![
            "refs",
            "list",
            "--python",
            "--format=%(refname:short)",
            "refs/tags",
        ],
        vec![
            "refs",
            "list",
            "--tcl",
            "--format=%(refname:short)",
            "refs/tags",
        ],
        vec!["refs", "list", "--count=1", "--format=%(refname:short)"],
        vec!["refs", "list", "--omit-empty", "--format="],
        vec!["refs", "list", "--color"],
        vec!["refs", "list", "--include-root-refs", "--format=%(refname)"],
        vec!["refs", "list", "-h"],
        vec!["refs", "list", "--bogus"],
        vec!["refs", "list", "--sort"],
        vec!["refs", "list", "--sort", "--no-sort"],
        vec!["refs", "list", "--sort", "--", "--no-sort"],
        vec!["refs", "list", "--color=bogus"],
        vec!["refs", "list", "--count=wat"],
        vec!["refs", "list", "--count=-1"],
        vec!["refs", "list", "--count=+1"],
        vec!["refs", "list", "--count=2147483647"],
        vec!["refs", "list", "--count=2147483648"],
        vec!["refs", "list", "--count=-2147483648"],
        vec!["refs", "list", "--count=-2147483649"],
        vec!["refs", "list", "--count=+2147483647"],
        vec!["refs", "list", "--count=+2147483648"],
        vec!["refs", "list", "--format", "--help"],
        vec!["refs", "list", "--sort", "--help"],
        vec!["refs", "list", "--count", "--help"],
        vec!["refs", "list", "--", "--help"],
        vec!["refs", "list", "--bogus", "--help"],
    ] {
        assert_refs_list_exact(repo.path(), &stock, &args);
    }

    let repo_path = repo
        .path()
        .to_str()
        .expect("temporary repository path is UTF-8");
    for args in [
        vec!["-C", repo_path, "refs", "list", "-h"],
        vec!["-C", repo_path, "refs", "list", "--format=%(refname)"],
        vec!["-c", "color.ui=false", "refs", "list", "-h"],
        vec![
            "-c",
            "color.ui=false",
            "refs",
            "list",
            "--format=%(refname)",
        ],
        vec!["--config=color.ui=false", "refs", "list", "-h"],
        vec![
            "--config=color.ui=false",
            "refs",
            "list",
            "--format=%(refname)",
        ],
    ] {
        assert_refs_list_exact(repo.path(), &stock, &args);
    }
}

#[test]
fn version_refname_sort_matches_stock_git() {
    let repo = common::git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "tracked.txt", "version sort\n");
    git(repo.path(), ["add", "tracked.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "version sort"]);

    for name in [
        "foo1", "foo1.0", "foo1.001", "foo1.01", "foo1.3", "foo1.6", "foo1.10", "foo02a", "foo2a",
    ] {
        git(repo.path(), ["tag", name]);
    }
    for name in ["topic-1", "topic-2", "topic-10"] {
        git(repo.path(), ["branch", name]);
    }

    for sort in ["version:refname", "v:refname", "-version:refname"] {
        assert_eq!(
            run_zmin(
                repo.path(),
                [
                    "for-each-ref",
                    &format!("--sort={sort}"),
                    "--format=%(refname:short)",
                    "refs/tags",
                ],
            ),
            git(
                repo.path(),
                [
                    "for-each-ref",
                    &format!("--sort={sort}"),
                    "--format=%(refname:short)",
                    "refs/tags",
                ],
            )
        );
        assert_eq!(
            run_zmin(repo.path(), ["branch", "--list", &format!("--sort={sort}")]),
            git(repo.path(), ["branch", "--list", &format!("--sort={sort}")])
        );
    }
}

#[test]
fn for_each_ref_matches_stock_git_for_common_formats() {
    let repo = common::git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "tag.gpgSign", "false"]);
    write_file(repo.path(), "a.txt", "hello\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial subject"]);
    git(repo.path(), ["branch", "feature-flat"]);
    git(repo.path(), ["branch", "feature/topic"]);
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
                "--format=%(refname)%00%(objectname)",
                "refs/heads",
            ],
        ),
        git(
            repo.path(),
            [
                "for-each-ref",
                "--format=%(refname)%00%(objectname)",
                "refs/heads",
            ],
        )
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
                "--sort=refname:lstrip=2",
                "--format=%(refname:lstrip=0)|%(refname:lstrip=1)|%(refname:lstrip=2)|%(refname:lstrip=-1)|%(refname:rstrip=0)|%(refname:rstrip=1)|%(refname:rstrip=2)|%(refname:rstrip=-1)",
                "refs/heads",
                "refs/tags",
            ],
        ),
        git(
            repo.path(),
            [
                "for-each-ref",
                "--sort=refname:lstrip=2",
                "--format=%(refname:lstrip=0)|%(refname:lstrip=1)|%(refname:lstrip=2)|%(refname:lstrip=-1)|%(refname:rstrip=0)|%(refname:rstrip=1)|%(refname:rstrip=2)|%(refname:rstrip=-1)",
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
                "--sort=refname:rstrip=1",
                "--format=%(refname)",
                "refs/heads",
                "refs/tags",
            ],
        ),
        git(
            repo.path(),
            [
                "for-each-ref",
                "--sort=refname:rstrip=1",
                "--format=%(refname)",
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
fn for_each_ref_describe_batches_object_abbreviation_lookup() {
    let stock = required_pinned_stock_git();
    let repo = common::git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "first\n");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    git(repo.path(), ["tag", "v1"]);
    write_file(repo.path(), "a.txt", "second\n");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    git(repo.path(), ["branch", "old", "HEAD~1"]);

    let args = [
        "for-each-ref",
        "--format=%(refname) %(describe:abbrev=7)",
        "refs/heads",
        "refs/tags",
    ];
    let stock_output = exact_command(&stock, repo.path(), &args);
    let zmin_output = exact_command(Path::new(zmin_bin()), repo.path(), &args);
    assert_eq!(zmin_output.status.code(), stock_output.status.code());
    assert_eq!(zmin_output.stdout, stock_output.stdout);
    assert_eq!(zmin_output.stderr, stock_output.stderr);
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
fn for_each_ref_peeled_tag_objectname_matches_stock_git() {
    let repo = common::git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "tag.gpgSign", "false"]);
    write_file(repo.path(), "a.txt", "hello\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial subject"]);
    git_with_env(repo.path(), ["tag", "-a", "annotated", "-m", "tag subject"]);
    git(repo.path(), ["tag", "lightweight"]);

    let args = [
        "for-each-ref",
        "refs/tags/**",
        "--no-color",
        "--format=%(refname)\t%(*objectname)\t%(objectname)",
    ];
    assert_eq!(
        common::run_zmin_args(repo.path(), &args),
        common::git_args(repo.path(), &args)
    );
}

#[test]
fn for_each_ref_refname_strip_invalid_values_match_stock_git() {
    let repo = common::git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "hello\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial subject"]);

    for args in [
        vec![
            "for-each-ref",
            "--format=%(refname:lstrip=abc)",
            "refs/heads",
        ],
        vec!["for-each-ref", "--format=%(refname:lstrip=)", "refs/heads"],
        vec![
            "for-each-ref",
            "--format=%(refname:rstrip=abc)",
            "refs/heads",
        ],
        vec!["for-each-ref", "--format=%(refname:rstrip=)", "refs/heads"],
        vec![
            "for-each-ref",
            "--sort=refname:lstrip=abc",
            "--format=%(refname)",
            "refs/heads",
        ],
        vec![
            "for-each-ref",
            "--sort=refname:rstrip=abc",
            "--format=%(refname)",
            "refs/heads",
        ],
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), &args),
            git_failure_output(repo.path(), &args),
            "for-each-ref invalid refname strip value should match for {args:?}"
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

#[test]
fn for_each_ref_remaining_documented_flags_match_stock_git() {
    let stock = required_pinned_stock_git();
    let repo = common::git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "tag.gpgSign", "false"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial subject"]);
    git(repo.path(), ["branch", "feature"]);
    git_with_env(
        repo.path(),
        ["tag", "-a", "v1", "-m", "tag subject\nsecond line\n\nbody"],
    );
    let blob_id = git(repo.path(), ["hash-object", "a.txt"]);
    git(repo.path(), ["update-ref", "refs/blobs/a", &blob_id]);
    write_file(repo.path(), "a.txt", "two\n");
    command_output_with_env(
        "git",
        repo.path(),
        &["add", "-A"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000100 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000100 +0000"),
        ],
        "git add",
    );
    command_output_with_env(
        "git",
        repo.path(),
        &["commit", "-m", "second subject"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000100 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000100 +0000"),
        ],
        "git commit",
    );
    git(repo.path(), ["tag", "tip"]);
    let head = git(repo.path(), ["rev-parse", "HEAD"]);
    let base = git(repo.path(), ["rev-parse", "HEAD^"]);

    for args in [
        vec![
            "for-each-ref",
            "--color=always",
            "--format=%(refname:short)",
            "refs/heads",
        ],
        vec![
            "for-each-ref",
            "--no-color",
            "--format=%(refname:short)",
            "refs/heads",
        ],
        vec![
            "for-each-ref",
            "--contains",
            base.trim(),
            "--format=%(refname:short)",
        ],
        vec!["for-each-ref", "--count=1", "--format=%(refname:short)"],
        vec![
            "for-each-ref",
            "--exclude=refs/tags/*",
            "--format=%(refname)",
        ],
        vec![
            "for-each-ref",
            "--ignore-case",
            "refs/HEADS/*",
            "--format=%(refname)",
        ],
        vec!["for-each-ref", "--include-root-refs", "--format=%(refname)"],
        vec![
            "for-each-ref",
            "--merged",
            head.trim(),
            "--format=%(refname:short)",
        ],
        vec![
            "for-each-ref",
            "--no-contains",
            head.trim(),
            "--format=%(refname:short)",
        ],
        vec![
            "for-each-ref",
            "--no-merged",
            base.trim(),
            "--format=%(refname:short)",
        ],
        vec!["for-each-ref", "--omit-empty", "--format="],
        vec![
            "for-each-ref",
            "--shell",
            "--format=%(refname:short)=%(subject)",
            "refs/tags",
        ],
        vec![
            "for-each-ref",
            "--python",
            "--format=%(refname:short)=%(subject)",
            "refs/tags",
        ],
        vec![
            "for-each-ref",
            "--perl",
            "--format=%(refname:short)=%(subject)",
            "refs/tags",
        ],
        vec![
            "for-each-ref",
            "--tcl",
            "--format=%(refname:short)=%(subject)",
            "refs/tags",
        ],
        vec![
            "for-each-ref",
            "--points-at",
            head.trim(),
            "--format=%(refname:short)",
        ],
    ] {
        assert_eq!(
            exact_command(Path::new(zmin_bin()), repo.path(), &args),
            exact_command(&stock, repo.path(), &args),
            "for-each-ref args should match for {args:?}"
        );
    }

    assert_eq!(
        exact_command_with_stdin(
            Path::new(zmin_bin()),
            repo.path(),
            &["for-each-ref", "--stdin", "--format=%(refname)"],
            b"refs/heads\nrefs/tags\n",
        ),
        exact_command_with_stdin(
            &stock,
            repo.path(),
            &["for-each-ref", "--stdin", "--format=%(refname)"],
            b"refs/heads\nrefs/tags\n",
        ),
        "for-each-ref --stdin should match stock Git"
    );
    assert_eq!(
        exact_command_with_stdin(
            Path::new(zmin_bin()),
            repo.path(),
            &[
                "for-each-ref",
                "--count=1",
                "--stdin",
                "--format=%(refname)"
            ],
            b"refs/heads\nrefs/tags\n",
        ),
        exact_command_with_stdin(
            &stock,
            repo.path(),
            &[
                "for-each-ref",
                "--count=1",
                "--stdin",
                "--format=%(refname)"
            ],
            b"refs/heads\nrefs/tags\n",
        ),
        "for-each-ref --count with --stdin should match stock Git"
    );
    assert_eq!(
        exact_command(
            Path::new(zmin_bin()),
            repo.path(),
            &[
                "for-each-ref",
                "--stdin",
                "--format=%(refname)",
                "refs/heads",
            ],
        ),
        exact_command(
            &stock,
            repo.path(),
            &[
                "for-each-ref",
                "--stdin",
                "--format=%(refname)",
                "refs/heads",
            ],
        ),
        "for-each-ref should reject extra arguments with --stdin like stock Git"
    );
}

#[test]
fn for_each_ref_stdin_preserves_raw_record_boundaries() {
    let stock = required_pinned_stock_git();
    let repo = common::git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "stdin patterns\n");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "stdin patterns"]);

    let args = ["for-each-ref", "--stdin", "--format=%(refname)"];
    for input in [
        b" refs/heads\nrefs/tags \r\n\nrefs/heads".as_slice(),
        b"refs/heads/\x80\n".as_slice(),
    ] {
        assert_eq!(
            exact_command_with_stdin(Path::new(zmin_bin()), repo.path(), &args, input),
            exact_command_with_stdin(&stock, repo.path(), &args, input),
            "for-each-ref stdin bytes should match for {input:?}"
        );
    }
}

#[test]
fn for_each_ref_sha256_uses_repository_algorithm_for_metadata() {
    let stock = required_pinned_stock_git();
    let repo = pinned_git_init_sha256();
    pinned_git_args(repo.path(), &["config", "user.name", "Bench"]);
    pinned_git_args(repo.path(), &["config", "user.email", "bench@example.test"]);
    write_file(repo.path(), "a.txt", "sha256 refs\n");
    pinned_git_args(repo.path(), &["add", "a.txt"]);
    pinned_git_with_env(
        repo.path(),
        &["commit", "-m", "sha256 refs"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ],
    );
    pinned_git_args(repo.path(), &["tag", "-a", "-m", "annotated", "v1"]);
    pinned_git_args(repo.path(), &["tag", "light"]);
    write_file(repo.path(), "after.txt", "after tag\n");
    pinned_git_args(repo.path(), &["add", "after.txt"]);
    pinned_git_with_env(
        repo.path(),
        &["commit", "-m", "after tag"],
        &[
            ("GIT_AUTHOR_NAME", "Bench"),
            ("GIT_AUTHOR_EMAIL", "bench@example.test"),
            ("GIT_AUTHOR_DATE", "1700000001 +0000"),
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", "1700000001 +0000"),
        ],
    );

    for args in [
        [
            "for-each-ref",
            "--format=%(refname)|%(objectname)|%(objecttype)|%(objectsize)|%(objectsize:disk)|%(*objectname)|%(*objecttype)|%(*objectsize:disk)|%(describe:tags)|%(*describe:tags)",
            "refs/heads",
            "refs/tags",
        ]
        .as_slice(),
        [
            "for-each-ref",
            "--format=%(refname) %(describe:tags)",
            "refs/heads",
            "refs/tags",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            exact_command(Path::new(zmin_bin()), repo.path(), &args),
            exact_command(&stock, repo.path(), &args),
            "SHA-256 for-each-ref tuple: {args:?}"
        );
    }

    let peeled_args = [
        "for-each-ref",
        "--format=%(refname)|%(objectname)|%(*objectname)|%(*objecttype)",
        "refs/tags",
    ];
    let peeled = exact_command(Path::new(zmin_bin()), repo.path(), &peeled_args);
    let stock_peeled = exact_command(&stock, repo.path(), &peeled_args);
    assert_eq!(peeled, stock_peeled, "SHA-256 peeled tag tuple");
    assert!(
        String::from_utf8_lossy(&peeled.stdout).lines().any(|line| {
            let fields = line.split('|').collect::<Vec<_>>();
            fields.len() == 4 && fields[2].len() == 64 && fields[3] == "commit"
        }),
        "SHA-256 peeled tag output must include the full 64-digit target"
    );

    for (config, expected_width) in [("core.abbrev=12", 12), ("core.abbrev=no", 64)] {
        let args = [
            "-c",
            config,
            "for-each-ref",
            "--format=%(refname) %(describe:tags)",
            "refs/heads",
        ];
        let stock_output = exact_command(&stock, repo.path(), &args);
        let zmin_output = exact_command(Path::new(zmin_bin()), repo.path(), &args);
        assert_eq!(
            zmin_output.status, stock_output.status,
            "describe rc: {config}"
        );
        assert_eq!(
            zmin_output.stdout, stock_output.stdout,
            "describe stdout: {config}"
        );
        assert_eq!(
            zmin_output.stderr, stock_output.stderr,
            "describe stderr: {config}"
        );
        let describe_output =
            String::from_utf8(stock_output.stdout).expect("describe output is UTF-8");
        let describe = describe_output.trim_end();
        assert_eq!(
            describe
                .rsplit_once("-g")
                .expect("describe hash suffix")
                .1
                .len(),
            expected_width,
            "pinned describe width: {config}"
        );
    }
}
