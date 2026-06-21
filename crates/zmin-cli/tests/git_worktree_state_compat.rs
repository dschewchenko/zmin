mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    command_any_output, command_output, configure_identity, git, git_args, git_failure_output,
    git_init, git_with_env, git_with_stdin, run_zmin, run_zmin_failure_output, run_zmin_with_env,
    run_zmin_with_stdin, zmin_bin,
};

fn committed_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    run_zmin(repo.path(), ["add", "-A"]);
    run_zmin_with_env(repo.path(), ["commit", "-m", "initial"]);
    repo
}

fn two_commit_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"one\n").expect("write first");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    fs::write(repo.path().join("a.txt"), b"two\n").expect("write second");
    fs::write(repo.path().join("b.txt"), b"two\n").expect("write added");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    repo
}

fn checkout_index_fixture_repo() -> TempDir {
    let repo = git_init();
    fs::create_dir_all(repo.path().join("docs")).expect("create docs");
    fs::write(repo.path().join("README.md"), b"readme\n").expect("write readme");
    fs::write(repo.path().join("docs/guide.md"), b"guide\n").expect("write guide");
    git(repo.path(), ["add", "-A"]);
    fs::remove_file(repo.path().join("README.md")).expect("remove readme");
    fs::remove_file(repo.path().join("docs/guide.md")).expect("remove guide");
    repo
}

#[cfg(unix)]
fn write_delayed_smudge_filter_helper(path: &std::path::Path) {
    fs::write(
        path,
        r#"use strict;
use warnings;
binmode STDIN;
binmode STDOUT;
$| = 1;
sub readpkt {
    my $header = "";
    my $read = read(STDIN, $header, 4);
    exit 0 unless defined($read) && $read == 4;
    my $len = hex($header);
    return "" if $len == 0;
    my $payload = "";
    read(STDIN, $payload, $len - 4) == $len - 4 or die "short read";
    return $payload;
}
sub readtext {
    my $value = readpkt();
    $value =~ s/\n$// if defined $value;
    return $value;
}
sub writepkt {
    my ($payload) = @_;
    printf "%04x%s", length($payload) + 4, $payload;
}
sub flushpkt { print "0000"; }
readtext();
readtext();
readtext();
writepkt("git-filter-server");
writepkt("version=2");
flushpkt();
while ((my $capability = readtext()) ne "") {}
writepkt("capability=smudge");
flushpkt();
while (1) {
    my $command = readtext();
    last unless defined $command;
    readtext();
    while ((my $metadata = readtext()) ne "") {}
    while ((my $packet = readpkt()) ne "") {}
    writepkt("status=delayed");
    flushpkt();
}
"#,
    )
    .expect("write delayed smudge filter helper");
}

#[test]
fn checkout_dot_pathspec_restores_root_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("a.txt"), b"hello\n").expect("write a");
        fs::write(repo.join("b.txt"), b"world\n").expect("write b");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
        fs::write(repo.join("a.txt"), b"changed\n").expect("modify a");
        fs::remove_file(repo.join("b.txt")).expect("remove b");
    }

    git(
        git_repo.path(),
        ["checkout", "--quiet", "--no-progress", "."],
    );
    run_zmin(
        zmin_repo.path(),
        ["checkout", "--quiet", "--no-progress", "."],
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("b.txt")).expect("read zmin b"),
        fs::read(git_repo.path().join("b.txt")).expect("read git b")
    );
}

#[test]
fn checkout_dot_reports_updated_paths_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("a.txt"), b"hello\n").expect("write a");
        fs::write(repo.join("b.txt"), b"world\n").expect("write b");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
        fs::remove_file(repo.join("a.txt")).expect("remove a");
        fs::remove_file(repo.join("b.txt")).expect("remove b");
    }

    let git_checkout = command_output("git", git_repo.path(), &["checkout", "."], "git");
    let zmin_checkout = command_output(zmin_bin(), zmin_repo.path(), &["checkout", "."], "zmin");

    assert_eq!(zmin_checkout.0, git_checkout.0);
    assert_eq!(zmin_checkout.2, git_checkout.2);
}

#[test]
fn checkout_separator_pathspec_omits_updated_paths_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("a.txt"), b"hello\n").expect("write a");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
        fs::remove_file(repo.join("a.txt")).expect("remove a");
    }

    let git_checkout = command_output("git", git_repo.path(), &["checkout", "--", "a.txt"], "git");
    let zmin_checkout = command_output(
        zmin_bin(),
        zmin_repo.path(),
        &["checkout", "--", "a.txt"],
        "zmin",
    );

    assert_eq!(zmin_checkout.0, git_checkout.0);
    assert_eq!(zmin_checkout.2, git_checkout.2);
}

#[test]
fn checkout_recurse_submodules_flag_keeps_dot_pathspec_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("a.txt"), b"hello\n").expect("write a");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
        fs::write(repo.join("a.txt"), b"changed\n").expect("modify a");
    }

    git(git_repo.path(), ["checkout", "--recurse-submodules", "."]);
    run_zmin(zmin_repo.path(), ["checkout", "--recurse-submodules", "."]);

    assert_eq!(
        fs::read(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read(git_repo.path().join("a.txt")).expect("read git a")
    );
}

#[test]
fn checkout_text_unset_ignores_eol_crlf_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join(".gitattributes"), b"*.txt -text eol=crlf\n")
            .expect("write attributes");
        fs::write(repo.join("lf.txt"), b"one\ntwo\n").expect("write lf fixture");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
        fs::remove_file(repo.join("lf.txt")).expect("remove lf fixture");
    }

    git(git_repo.path(), ["checkout", "--", "lf.txt"]);
    run_zmin(zmin_repo.path(), ["checkout", "--", "lf.txt"]);

    assert_eq!(
        fs::read(zmin_repo.path().join("lf.txt")).expect("read zmin lf"),
        fs::read(git_repo.path().join("lf.txt")).expect("read git lf")
    );
}

#[test]
fn checkout_core_autocrlf_true_smudges_lf_to_crlf_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("lf.txt"), b"one\ntwo\n").expect("write lf fixture");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
        fs::remove_file(repo.join("lf.txt")).expect("remove lf fixture");
    }

    git(
        git_repo.path(),
        ["-c", "core.autocrlf=true", "checkout", "--", "lf.txt"],
    );
    run_zmin(
        zmin_repo.path(),
        ["-c", "core.autocrlf=true", "checkout", "--", "lf.txt"],
    );

    assert_eq!(
        fs::read(zmin_repo.path().join("lf.txt")).expect("read zmin lf"),
        fs::read(git_repo.path().join("lf.txt")).expect("read git lf")
    );
}

#[test]
fn checkout_core_autocrlf_input_core_eol_lf_attribute_matrix_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let files: [(&str, &[u8]); 6] = [
        ("LF", b"one\ntwo\n"),
        ("CRLF", b"one\r\ntwo\r\n"),
        ("CRLF_mix_LF", b"one\r\ntwo\nthree\r\n"),
        ("LF_mix_CR", b"one\ntwo\rthree\n"),
        ("CRLF_nul", b"one\r\n\0two\r\n"),
        ("LF_nul", b"one\n\0two\n"),
    ];

    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join(".gitattributes"), b"").expect("write initial attributes");
        for (name, content) in files {
            fs::write(repo.join(format!("crlf_false_attr__{name}.txt")), content)
                .expect("write checkout matrix fixture");
        }
        git(repo, ["-c", "core.autocrlf=false", "add", "-A"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
    }

    let cases = [
        ("text unset eol crlf", "*.txt -text\n*.txt eol=crlf\n"),
        ("text eol lf", "*.txt text\n*.txt eol=lf\n"),
        ("text auto eol lf", "*.txt text=auto\n*.txt eol=lf\n"),
    ];
    for (label, attributes) in cases {
        for repo in [git_repo.path(), zmin_repo.path()] {
            fs::write(repo.join(".gitattributes"), attributes).expect("write attributes");
            git(repo, ["config", "core.autocrlf", "input"]);
            for (name, _) in files {
                let path = repo.join(format!("crlf_false_attr__{name}.txt"));
                if path.exists() {
                    fs::remove_file(path).expect("remove checkout fixture");
                }
            }
        }

        for (name, _) in files {
            let file = format!("crlf_false_attr__{name}.txt");
            git(
                git_repo.path(),
                ["-c", "core.eol=lf", "checkout", "--", file.as_str()],
            );
            command_output(
                zmin_bin(),
                zmin_repo.path(),
                &["-c", "core.eol=lf", "checkout", "--", file.as_str()],
                "zmin",
            );
        }

        let zmin_eol = command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["ls-files", "--eol", "crlf_false_attr__*"],
            "zmin",
        )
        .1;
        assert_eq!(
            zmin_eol,
            git_args(
                git_repo.path(),
                &["ls-files", "--eol", "crlf_false_attr__*"]
            ),
            "ls-files --eol mismatch for {label}"
        );

        for (name, _) in files {
            let file = format!("crlf_false_attr__{name}.txt");
            assert_eq!(
                fs::read(zmin_repo.path().join(&file)).expect("read zmin checkout fixture"),
                fs::read(git_repo.path().join(&file)).expect("read git checkout fixture"),
                "checkout bytes mismatch for {label} {file}"
            );
        }
    }
}

#[test]
fn checkout_core_autocrlf_false_core_eol_lf_text_attribute_matrix_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let files: [(&str, &[u8]); 5] = [
        ("LF", b"one\ntwo\n"),
        ("CRLF", b"one\r\ntwo\r\n"),
        ("CRLF_mix_LF", b"one\r\ntwo\nthree\r\n"),
        ("LF_mix_CR", b"one\ntwo\rthree\n"),
        ("LF_nul", b"one\n\0two\n"),
    ];

    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join(".gitattributes"), b"").expect("write initial attributes");
        for (name, content) in files {
            fs::write(repo.join(format!("crlf_false_attr__{name}.txt")), content)
                .expect("write checkout matrix fixture");
        }
        git(repo, ["-c", "core.autocrlf=false", "add", "-A"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
    }

    let cases = [
        ("text unset eol crlf", "*.txt -text\n*.txt eol=crlf\n"),
        ("text eol lf", "*.txt text\n*.txt eol=lf\n"),
        ("text eol crlf", "*.txt text\n*.txt eol=crlf\n"),
    ];
    for (label, attributes) in cases {
        for repo in [git_repo.path(), zmin_repo.path()] {
            fs::write(repo.join(".gitattributes"), attributes).expect("write attributes");
            git(repo, ["config", "core.autocrlf", "false"]);
            for (name, _) in files {
                let path = repo.join(format!("crlf_false_attr__{name}.txt"));
                if path.exists() {
                    fs::remove_file(path).expect("remove checkout fixture");
                }
            }
        }

        for (name, _) in files {
            let file = format!("crlf_false_attr__{name}.txt");
            git(
                git_repo.path(),
                ["-c", "core.eol=lf", "checkout", "--", file.as_str()],
            );
            run_zmin(
                zmin_repo.path(),
                ["-c", "core.eol=lf", "checkout", "--", file.as_str()],
            );
        }

        let zmin_eol = command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["ls-files", "--eol", "crlf_false_attr__*"],
            "zmin",
        )
        .1;
        assert_eq!(
            zmin_eol,
            git_args(
                git_repo.path(),
                &["ls-files", "--eol", "crlf_false_attr__*"]
            ),
            "ls-files --eol mismatch for {label}"
        );

        for (name, _) in files {
            let file = format!("crlf_false_attr__{name}.txt");
            assert_eq!(
                fs::read(zmin_repo.path().join(&file)).expect("read zmin checkout fixture"),
                fs::read(git_repo.path().join(&file)).expect("read git checkout fixture"),
                "checkout bytes mismatch for {label} {file}"
            );
        }
    }
}

#[test]
fn checkout_text_auto_eol_crlf_preserves_mixed_eol_blob_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join(".gitattributes"), b"").expect("write initial attributes");
        fs::write(
            repo.join("mixed.txt"),
            b"$Id: 0000000000000000000000000000000000000000 $\nLINEONE\r\nLINETWO\nLINETHREE",
        )
        .expect("write mixed fixture");
        fs::write(
            repo.join("lone-cr.txt"),
            b"$Id: 0000000000000000000000000000000000000000 $\nLINEONE\nLINETWO\rLINETHREE",
        )
        .expect("write lone-cr fixture");
        fs::write(
            repo.join("with-nul.txt"),
            b"$Id: 0000000000000000000000000000000000000000 $\nLINEONE\0\nLINETWO\nLINETHREE",
        )
        .expect("write nul fixture");
        git(repo, ["-c", "core.autocrlf=false", "add", "-A"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
        fs::write(
            repo.join(".gitattributes"),
            b"*.txt text=auto\n*.txt eol=crlf\n",
        )
        .expect("write auto attributes");
        git(repo, ["add", ".gitattributes"]);
        git_with_env(repo, ["commit", "-m", "attributes"]);
        fs::remove_file(repo.join("mixed.txt")).expect("remove mixed fixture");
        fs::remove_file(repo.join("lone-cr.txt")).expect("remove lone-cr fixture");
        fs::remove_file(repo.join("with-nul.txt")).expect("remove nul fixture");
    }

    git(
        git_repo.path(),
        [
            "-c",
            "core.eol=lf",
            "checkout",
            "--",
            "mixed.txt",
            "lone-cr.txt",
            "with-nul.txt",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "-c",
            "core.eol=lf",
            "checkout",
            "--",
            "mixed.txt",
            "lone-cr.txt",
            "with-nul.txt",
        ],
    );

    for path in ["mixed.txt", "lone-cr.txt", "with-nul.txt"] {
        assert_eq!(
            fs::read(zmin_repo.path().join(path)).expect("read zmin file"),
            fs::read(git_repo.path().join(path)).expect("read git file"),
            "{path}"
        );
    }
}

#[test]
fn checkout_force_head_restores_missing_worktree_file_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("a.txt"), b"hello\n").expect("write a");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "initial"]);
        fs::remove_file(repo.join("a.txt")).expect("remove a");
    }

    git(git_repo.path(), ["checkout", "-f", "HEAD"]);
    run_zmin(zmin_repo.path(), ["checkout", "-f", "HEAD"]);

    assert_eq!(
        fs::read(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read(git_repo.path().join("a.txt")).expect("read git a")
    );
}

#[test]
fn checkout_reset_branch_allows_unborn_head_like_stock_git() {
    let repo = git_init();
    run_zmin(repo.path(), ["checkout", "-B", "main"]);
    assert_eq!(
        git(repo.path(), ["symbolic-ref", "HEAD"]),
        "refs/heads/main"
    );
    assert!(
        !repo.path().join(".git/refs/heads/main").exists(),
        "unborn checkout should not create a branch ref before the first commit"
    );
}

#[test]
fn checkout_switches_branches_and_updates_worktree() {
    let repo = committed_repo();
    let default_branch = git(repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    run_zmin(repo.path(), ["checkout", "-b", "feature"]);
    assert_eq!(
        git(repo.path(), ["symbolic-ref", "HEAD"]),
        "refs/heads/feature"
    );

    fs::write(repo.path().join("a.txt"), b"feature\n").expect("modify feature file");
    fs::write(repo.path().join("feature.txt"), b"only feature\n").expect("write feature file");
    run_zmin(repo.path(), ["add", "-A"]);
    run_zmin_with_env(repo.path(), ["commit", "-m", "feature"]);

    run_zmin(repo.path(), ["checkout", &default_branch]);
    assert_eq!(
        git(repo.path(), ["symbolic-ref", "HEAD"]),
        format!("refs/heads/{default_branch}")
    );
    assert_eq!(
        fs::read(repo.path().join("a.txt")).expect("read master file"),
        b"hello\n"
    );
    assert!(!repo.path().join("feature.txt").exists());
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v1", "--branch"])
    );

    run_zmin(repo.path(), ["checkout", "feature"]);
    assert_eq!(
        fs::read(repo.path().join("a.txt")).expect("read feature file"),
        b"feature\n"
    );
    assert_eq!(
        fs::read(repo.path().join("feature.txt")).expect("read feature-only file"),
        b"only feature\n"
    );

    run_zmin(
        repo.path(),
        ["checkout", "-B", "feature-reset", &default_branch],
    );
    assert_eq!(
        git(repo.path(), ["symbolic-ref", "HEAD"]),
        "refs/heads/feature-reset"
    );
    assert_eq!(
        fs::read(repo.path().join("a.txt")).expect("read reset branch file"),
        b"hello\n"
    );

    let feature_head = git(repo.path(), ["rev-parse", "feature"]);
    run_zmin(repo.path(), ["checkout", "--detach", "feature"]);
    assert_eq!(git(repo.path(), ["rev-parse", "HEAD"]), feature_head);
    assert_eq!(
        git(repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]),
        "HEAD"
    );
    assert_eq!(
        run_zmin(repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn checkout_head_aliases_do_not_detach_like_stock_git() {
    let repo = committed_repo();
    let before = fs::read_to_string(repo.path().join(".git/HEAD")).expect("read HEAD before");

    for alias in ["HEAD", "@"] {
        run_zmin(repo.path(), ["checkout", alias]);
        assert_eq!(
            fs::read_to_string(repo.path().join(".git/HEAD")).expect("read HEAD after"),
            before,
            "checkout {alias} should leave symbolic HEAD unchanged"
        );
        assert_eq!(
            git(repo.path(), ["symbolic-ref", "-q", "HEAD"]),
            before.trim().strip_prefix("ref: ").unwrap_or(before.trim())
        );
    }
}

#[test]
fn checkout_detach_defaults_to_head_like_stock_git() {
    let repo = committed_repo();
    let head = git(repo.path(), ["rev-parse", "HEAD"]);

    run_zmin(repo.path(), ["checkout", "--detach"]);

    assert_eq!(git(repo.path(), ["rev-parse", "HEAD"]), head);
    assert_eq!(
        git(repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]),
        "HEAD"
    );
}

#[test]
fn checkout_full_branch_ref_detaches_like_stock_git() {
    let repo = committed_repo();
    run_zmin(repo.path(), ["branch", "branch"]);
    let branch_head = git(repo.path(), ["rev-parse", "refs/heads/branch"]);

    run_zmin(repo.path(), ["checkout", "refs/heads/branch"]);

    assert_eq!(git(repo.path(), ["rev-parse", "HEAD"]), branch_head);
    assert_eq!(
        git(repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]),
        "HEAD"
    );
}

#[test]
fn checkout_paths_match_stock_git_state() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    fs::create_dir_all(git_repo.path().join("dir")).expect("mkdir git dir");
    fs::create_dir_all(zmin_repo.path().join("dir")).expect("mkdir zmin dir");
    fs::write(git_repo.path().join("a.txt"), b"hello\n").expect("write git a");
    fs::write(zmin_repo.path().join("a.txt"), b"hello\n").expect("write zmin a");
    fs::write(git_repo.path().join("remove.txt"), b"remove\n").expect("write git remove");
    fs::write(zmin_repo.path().join("remove.txt"), b"remove\n").expect("write zmin remove");
    fs::write(git_repo.path().join("dir/nested.txt"), b"nested\n").expect("write git nested");
    fs::write(zmin_repo.path().join("dir/nested.txt"), b"nested\n").expect("write zmin nested");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "initial"]);

    fs::write(git_repo.path().join("a.txt"), b"worktree\n").expect("dirty git a");
    fs::write(zmin_repo.path().join("a.txt"), b"worktree\n").expect("dirty zmin a");
    git(git_repo.path(), ["checkout", "--", "a.txt"]);
    run_zmin(zmin_repo.path(), ["checkout", "--", "a.txt"]);
    assert_eq!(
        fs::read(zmin_repo.path().join("a.txt")).expect("read zmin a"),
        fs::read(git_repo.path().join("a.txt")).expect("read git a")
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );

    fs::write(git_repo.path().join("a.txt"), b"staged\n").expect("stage git a");
    fs::write(zmin_repo.path().join("a.txt"), b"staged\n").expect("stage zmin a");
    fs::remove_file(git_repo.path().join("remove.txt")).expect("remove git file");
    fs::remove_file(zmin_repo.path().join("remove.txt")).expect("remove zmin file");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git(
        git_repo.path(),
        ["checkout", "HEAD", "--", "a.txt", "remove.txt"],
    );
    run_zmin(
        zmin_repo.path(),
        ["checkout", "HEAD", "--", "a.txt", "remove.txt"],
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("remove.txt")).expect("read zmin restored"),
        fs::read(git_repo.path().join("remove.txt")).expect("read git restored")
    );
}

#[test]
fn checkout_index_matches_stock_git_for_all_paths_stdin_and_prefix() {
    let git_repo = checkout_index_fixture_repo();
    let zmin_repo = checkout_index_fixture_repo();

    git(git_repo.path(), ["checkout-index", "-a"]);
    run_zmin(zmin_repo.path(), ["checkout-index", "-a"]);
    assert_eq!(
        fs::read(zmin_repo.path().join("README.md")).expect("read zmin readme"),
        fs::read(git_repo.path().join("README.md")).expect("read git readme")
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("docs/guide.md")).expect("read zmin guide"),
        fs::read(git_repo.path().join("docs/guide.md")).expect("read git guide")
    );

    fs::remove_file(git_repo.path().join("README.md")).expect("remove git readme");
    fs::remove_file(zmin_repo.path().join("README.md")).expect("remove zmin readme");
    git(git_repo.path(), ["checkout-index", "README.md"]);
    run_zmin(zmin_repo.path(), ["checkout-index", "README.md"]);
    assert_eq!(
        fs::read(zmin_repo.path().join("README.md")).expect("read zmin readme"),
        fs::read(git_repo.path().join("README.md")).expect("read git readme")
    );

    git(
        git_repo.path(),
        [
            "checkout-index",
            "--prefix=out/",
            "README.md",
            "docs/guide.md",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "checkout-index",
            "--prefix=out/",
            "README.md",
            "docs/guide.md",
        ],
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("out/README.md")).expect("read zmin out readme"),
        fs::read(git_repo.path().join("out/README.md")).expect("read git out readme")
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("out/docs/guide.md")).expect("read zmin out guide"),
        fs::read(git_repo.path().join("out/docs/guide.md")).expect("read git out guide")
    );

    fs::remove_file(git_repo.path().join("docs/guide.md")).expect("remove git guide");
    fs::remove_file(zmin_repo.path().join("docs/guide.md")).expect("remove zmin guide");
    assert_eq!(
        run_zmin_with_stdin(
            zmin_repo.path(),
            ["checkout-index", "--stdin"],
            "docs/guide.md\n",
        ),
        git_with_stdin(
            git_repo.path(),
            ["checkout-index", "--stdin"],
            "docs/guide.md\n",
        )
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("docs/guide.md")).expect("read zmin stdin guide"),
        fs::read(git_repo.path().join("docs/guide.md")).expect("read git stdin guide")
    );
}

#[cfg(unix)]
#[test]
fn checkout_index_delayed_smudge_process_filter_matches_stock_git_failure() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let helper_dir = TempDir::new().expect("helper tempdir");
    let helper = helper_dir.path().join("delayed-smudge-filter.pl");
    write_delayed_smudge_filter_helper(&helper);
    let command = format!("perl {}", helper.display());

    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("a.bad"), b"hello\n").expect("write indexed file");
        git(repo, ["add", "a.bad"]);
        fs::write(repo.join(".gitattributes"), b"*.bad filter=bad\n").expect("write attributes");
        git(repo, ["config", "filter.bad.process", &command]);
        git(repo, ["config", "filter.bad.required", "true"]);
        fs::remove_file(repo.join("a.bad")).expect("remove worktree file");
    }

    assert_eq!(
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &["checkout-index", "-f", "a.bad"],
            "zmin checkout-index delayed smudge process filter",
        ),
        command_any_output(
            "git",
            git_repo.path(),
            &["checkout-index", "-f", "a.bad"],
            "git checkout-index delayed smudge process filter",
        )
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("a.bad")).ok(),
        fs::read(git_repo.path().join("a.bad")).ok()
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
}

#[test]
fn switch_create_matches_stock_git_state() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    git(git_repo.path(), ["switch", "-c", "feature"]);
    run_zmin(zmin_repo.path(), ["switch", "-c", "feature"]);
    assert_eq!(
        git(zmin_repo.path(), ["symbolic-ref", "HEAD"]),
        git(git_repo.path(), ["symbolic-ref", "HEAD"])
    );

    fs::write(git_repo.path().join("a.txt"), b"feature\n").expect("modify git feature");
    fs::write(zmin_repo.path().join("a.txt"), b"feature\n").expect("modify zmin feature");
    fs::write(git_repo.path().join("feature.txt"), b"feature-only\n").expect("write git feature");
    fs::write(zmin_repo.path().join("feature.txt"), b"feature-only\n").expect("write zmin feature");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "feature"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "feature"]);

    git(git_repo.path(), ["switch", &default_branch]);
    run_zmin(zmin_repo.path(), ["switch", &default_branch]);
    assert_eq!(
        git(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("a.txt")).expect("read switched file"),
        b"hello\n"
    );
    assert!(!zmin_repo.path().join("feature.txt").exists());

    git(git_repo.path(), ["switch", "feature"]);
    run_zmin(zmin_repo.path(), ["switch", "feature"]);
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("feature.txt")).expect("read feature-only file"),
        b"feature-only\n"
    );
}

#[test]
fn switch_orphan_and_discard_changes_match_stock_git_state() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();

    git(git_repo.path(), ["switch", "--orphan", "orphan"]);
    run_zmin(zmin_repo.path(), ["switch", "--orphan", "orphan"]);
    assert_eq!(
        git(zmin_repo.path(), ["symbolic-ref", "HEAD"]),
        git(git_repo.path(), ["symbolic-ref", "HEAD"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(run_zmin(zmin_repo.path(), ["ls-files", "--stage"]), "");
    assert!(!zmin_repo.path().join("a.txt").exists());

    let git_dirty = two_commit_repo();
    let zmin_dirty = two_commit_repo();
    fs::write(git_dirty.path().join("a.txt"), b"dirty\n").expect("dirty git a");
    fs::write(zmin_dirty.path().join("a.txt"), b"dirty\n").expect("dirty zmin a");
    assert_eq!(
        run_zmin_failure_output(zmin_dirty.path(), &["switch", "--orphan", "blocked"]),
        git_failure_output(git_dirty.path(), &["switch", "--orphan", "blocked"])
    );

    let git_force = two_commit_repo();
    let zmin_force = two_commit_repo();
    git(git_force.path(), ["branch", "feature", "HEAD~1"]);
    run_zmin(zmin_force.path(), ["branch", "feature", "HEAD~1"]);
    fs::write(git_force.path().join("a.txt"), b"dirty\n").expect("force dirty git a");
    fs::write(zmin_force.path().join("a.txt"), b"dirty\n").expect("force dirty zmin a");
    git(git_force.path(), ["switch", "--discard-changes", "feature"]);
    run_zmin(
        zmin_force.path(),
        ["switch", "--discard-changes", "feature"],
    );
    assert_eq!(
        git(zmin_force.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_force.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read(zmin_force.path().join("a.txt")).expect("read discarded a"),
        fs::read(git_force.path().join("a.txt")).expect("read git discarded a")
    );
}

#[test]
fn switch_detach_matches_stock_git_for_branch_targets() {
    let git_repo = two_commit_repo();
    let zmin_repo = two_commit_repo();

    git(git_repo.path(), ["branch", "feature"]);
    run_zmin(zmin_repo.path(), ["branch", "feature"]);
    git(git_repo.path(), ["switch", "--detach", "feature"]);
    run_zmin(zmin_repo.path(), ["switch", "--detach", "feature"]);

    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]),
        "HEAD"
    );
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
}

#[test]
fn restore_staged_and_worktree_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();

    fs::write(git_repo.path().join("a.txt"), b"modified\n").expect("modify git a");
    fs::write(zmin_repo.path().join("a.txt"), b"modified\n").expect("modify zmin a");
    fs::write(git_repo.path().join("b.txt"), b"new\n").expect("write git b");
    fs::write(zmin_repo.path().join("b.txt"), b"new\n").expect("write zmin b");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);

    git(git_repo.path(), ["restore", "--staged", "a.txt", "b.txt"]);
    run_zmin(zmin_repo.path(), ["restore", "--staged", "a.txt", "b.txt"]);
    assert_eq!(
        git(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );

    git(git_repo.path(), ["restore", "a.txt"]);
    run_zmin(zmin_repo.path(), ["restore", "a.txt"]);
    assert_eq!(
        fs::read(zmin_repo.path().join("a.txt")).expect("read restored a"),
        b"hello\n"
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn restore_source_can_remove_paths_from_index_and_worktree() {
    let git_repo = two_commit_repo();
    let zmin_repo = two_commit_repo();

    git(
        git_repo.path(),
        [
            "restore",
            "--source",
            "HEAD~1",
            "--staged",
            "--worktree",
            "b.txt",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "restore",
            "--source",
            "HEAD~1",
            "--staged",
            "--worktree",
            "b.txt",
        ],
    );

    assert!(!zmin_repo.path().join("b.txt").exists());
    assert_eq!(
        git(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
}

#[test]
fn reset_modes_match_stock_git_state() {
    for mode in ["--soft", "--mixed", "--hard"] {
        let git_repo = two_commit_repo();
        let zmin_repo = two_commit_repo();
        let target = git(git_repo.path(), ["rev-parse", "HEAD~1"]);

        git(git_repo.path(), ["reset", mode, &target]);
        run_zmin(zmin_repo.path(), ["reset", mode, &target]);

        assert_eq!(
            git(zmin_repo.path(), ["rev-parse", "HEAD"]),
            git(git_repo.path(), ["rev-parse", "HEAD"])
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
            git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
        );
        assert_eq!(
            git(zmin_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
            git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
        );
        if mode == "--hard" {
            assert_eq!(
                fs::read(zmin_repo.path().join("a.txt")).expect("read hard-reset file"),
                b"one\n"
            );
            assert!(!zmin_repo.path().join("b.txt").exists());
        }
    }
}

#[test]
fn reset_mode_after_revision_matches_stock_git_state() {
    let git_repo = two_commit_repo();
    let zmin_repo = two_commit_repo();

    git(git_repo.path(), ["reset", "HEAD~1", "--hard"]);
    run_zmin(zmin_repo.path(), ["reset", "HEAD~1", "--hard"]);

    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("a.txt")).expect("read hard-reset file"),
        b"one\n"
    );
    assert!(!zmin_repo.path().join("b.txt").exists());
}

#[test]
fn checkout_new_branch_from_head_preserves_dirty_state_like_stock_git() {
    let git_repo = two_commit_repo();
    let zmin_repo = two_commit_repo();

    fs::write(git_repo.path().join("topic.txt"), b"topic\n").expect("write git topic");
    fs::write(zmin_repo.path().join("topic.txt"), b"topic\n").expect("write zmin topic");
    git(git_repo.path(), ["add", "topic.txt"]);
    run_zmin(zmin_repo.path(), ["add", "topic.txt"]);

    git(git_repo.path(), ["checkout", "-b", "some/topic"]);
    run_zmin(zmin_repo.path(), ["checkout", "-b", "some/topic"]);

    assert_eq!(
        git(zmin_repo.path(), ["branch", "--show-current"]),
        git(git_repo.path(), ["branch", "--show-current"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn reset_paths_match_stock_git_state() {
    let git_repo = two_commit_repo();
    let zmin_repo = two_commit_repo();

    fs::write(git_repo.path().join("a.txt"), b"staged\n").expect("stage git a");
    fs::write(zmin_repo.path().join("a.txt"), b"staged\n").expect("stage zmin a");
    fs::write(git_repo.path().join("new.txt"), b"new\n").expect("write git new");
    fs::write(zmin_repo.path().join("new.txt"), b"new\n").expect("write zmin new");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);

    git(git_repo.path(), ["reset", "HEAD", "--", "a.txt", "new.txt"]);
    run_zmin(
        zmin_repo.path(),
        ["reset", "HEAD", "--", "a.txt", "new.txt"],
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["diff", "--cached", "--name-status"]),
        git(git_repo.path(), ["diff", "--cached", "--name-status"])
    );

    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git(git_repo.path(), ["reset", "--", "a.txt"]);
    run_zmin(zmin_repo.path(), ["reset", "--", "a.txt"]);
    assert_eq!(
        git(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );
}
