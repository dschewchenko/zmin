mod common;

use std::{collections::BTreeSet, fs};

use tempfile::TempDir;
use zmin_git_core::{GitHashAlgorithm, GitObjectHash};

use common::{
    clone_repo_fixture, command_any_output_with_stdin_bytes, command_stdout_bytes,
    configure_identity, git, git_args, git_init, git_status, git_with_env, git_with_stdin,
    git_with_stdin_bytes, run_zmin, run_zmin_args, run_zmin_status, run_zmin_with_env,
    run_zmin_with_stdin, run_zmin_with_stdin_bytes, zmin_bin,
};

fn first_pack_index(repo: &std::path::Path) -> std::path::PathBuf {
    let mut paths = fs::read_dir(repo.join(".git/objects/pack"))
        .expect("read pack dir")
        .map(|entry| entry.expect("pack entry").path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("idx"))
        .collect::<Vec<_>>();
    paths.sort();
    paths.into_iter().next().expect("pack index")
}

fn rewrite_pack_index_version(path: &std::path::Path, version: u32) -> Vec<u8> {
    let mut bytes = fs::read(path).expect("read pack index");
    bytes[4..8].copy_from_slice(&version.to_be_bytes());
    let checksum_start = bytes.len() - GitHashAlgorithm::Sha1.digest_len();
    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update(&bytes[..checksum_start]);
    let checksum = hasher.finalize();
    bytes[checksum_start..].copy_from_slice(checksum.as_bytes());
    bytes
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

fn loose_object_path(repo: &std::path::Path, hex: &str) -> std::path::PathBuf {
    repo.join(".git/objects").join(&hex[..2]).join(&hex[2..])
}

fn init_promisor_work_repo(remote: &std::path::Path, branch: &str) -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(
        repo.path(),
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path utf8"),
        ],
    );
    git(repo.path(), ["fetch", "origin"]);
    git(
        repo.path(),
        ["checkout", "-B", branch, &format!("origin/{branch}")],
    );
    git(repo.path(), ["config", "remote.origin.promisor", "true"]);
    repo
}

#[test]
fn hash_object_and_cat_file_match_stock_git() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");

    let git_id = git(repo.path(), ["hash-object", "-w", "a.txt"]);
    let zmin_id = run_zmin(repo.path(), ["hash-object", "-w", "a.txt"]);
    assert_eq!(zmin_id, git_id);
    assert_eq!(
        run_zmin(repo.path(), ["hash-object", "/dev/null"]),
        git(repo.path(), ["hash-object", "/dev/null"])
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["hash-object", "--stdin", "a.txt"], "stdin\n"),
        git_with_stdin(repo.path(), ["hash-object", "--stdin", "a.txt"], "stdin\n")
    );
    let large_stdin = vec![b'x'; 300 * 1024];
    let git_large_id =
        git_with_stdin_bytes(repo.path(), ["hash-object", "-w", "--stdin"], &large_stdin);
    let zmin_large_id =
        run_zmin_with_stdin_bytes(repo.path(), ["hash-object", "-w", "--stdin"], &large_stdin);
    assert_eq!(zmin_large_id, git_large_id);
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-s", &zmin_large_id]),
        git(repo.path(), ["cat-file", "-s", &git_large_id])
    );

    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-t", &git_id]),
        git(repo.path(), ["cat-file", "-t", &git_id])
    );
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-s", &git_id]),
        git(repo.path(), ["cat-file", "-s", &git_id])
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            ["cat-file", "--batch-all-objects", "--batch-check"]
        ),
        git(
            repo.path(),
            ["cat-file", "--batch-all-objects", "--batch-check"]
        )
    );
    let zmin_unordered = run_zmin(
        repo.path(),
        [
            "cat-file",
            "--batch-all-objects",
            "--batch-check",
            "--unordered",
        ],
    );
    let git_unordered = git(
        repo.path(),
        [
            "cat-file",
            "--batch-all-objects",
            "--batch-check",
            "--unordered",
        ],
    );
    assert_eq!(
        zmin_unordered.lines().collect::<BTreeSet<_>>(),
        git_unordered.lines().collect::<BTreeSet<_>>()
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            [
                "cat-file",
                "--batch-all-objects",
                "--batch-check",
                "--no-unordered",
            ],
        ),
        git(
            repo.path(),
            [
                "cat-file",
                "--batch-all-objects",
                "--batch-check",
                "--no-unordered",
            ],
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check"],
            &format!("{git_id}\n")
        ),
        git_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check"],
            &format!("{git_id}\n")
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check=%(objectname)"],
            &format!("{git_id}\n")
        ),
        git_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check=%(objectname)"],
            &format!("{git_id}\n")
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            [
                "cat-file",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)"
            ],
            &format!("{git_id}\n")
        ),
        git_with_stdin(
            repo.path(),
            [
                "cat-file",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)"
            ],
            &format!("{git_id}\n")
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["cat-file", "--batch-command", "--buffer"],
            &format!("info {git_id}\ncontents {git_id}\nflush\n")
        ),
        git_with_stdin(
            repo.path(),
            ["cat-file", "--batch-command", "--buffer"],
            &format!("info {git_id}\ncontents {git_id}\nflush\n")
        )
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["cat-file", "-e", &git_id]),
        git_status(repo.path(), ["cat-file", "-e", &git_id])
    );

    git(repo.path(), ["repack", "-adq"]);
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-t", &git_id]),
        git(repo.path(), ["cat-file", "-t", &git_id])
    );
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-s", &git_id]),
        git(repo.path(), ["cat-file", "-s", &git_id])
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check"],
            &format!("{git_id}\n")
        ),
        git_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check"],
            &format!("{git_id}\n")
        )
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["cat-file", "-e", &git_id]),
        git_status(repo.path(), ["cat-file", "-e", &git_id])
    );
}

#[test]
fn cat_file_promisor_remote_hydrates_missing_blob_on_demand() {
    let source = git_init();
    configure_identity(source.path());
    fs::write(source.path().join("a.txt"), b"one\n").expect("write tracked file");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "initial"]);
    let source_head = git(source.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    let remote = TempDir::new().expect("remote dir");
    git_args(
        source.path(),
        &[
            "clone",
            "--bare",
            ".",
            remote.path().to_str().expect("remote path utf8"),
        ],
    );

    let repo = init_promisor_work_repo(remote.path(), &source_head);
    let blob = git(repo.path(), ["rev-list", "--objects", "HEAD"])
        .lines()
        .find_map(|line| line.strip_suffix(" a.txt").map(str::to_owned))
        .expect("blob id");
    let object_path = loose_object_path(repo.path(), &blob);
    assert!(
        object_path.is_file(),
        "expected loose object at {}",
        object_path.display()
    );
    fs::remove_file(&object_path).expect("remove local blob");
    assert!(!object_path.exists());

    assert_eq!(run_zmin(repo.path(), ["cat-file", "-t", &blob]), "blob");
    assert!(
        object_path.is_file(),
        "expected demand hydration to restore {}",
        object_path.display()
    );
    fs::remove_file(&object_path).expect("remove local blob again");

    assert_eq!(run_zmin(repo.path(), ["cat-file", "blob", &blob]), "one");
    assert!(
        object_path.is_file(),
        "expected typed-object demand hydration to restore {}",
        object_path.display()
    );
}

#[test]
fn hash_object_write_prefers_bare_repo_at_current_directory() {
    let parent = git_init();
    let bare_path = parent.path().join("nested.git");
    run_zmin_args(
        parent.path(),
        &["init", "--bare", bare_path.to_str().expect("bare path")],
    );

    let object_id = run_zmin_with_stdin(&bare_path, ["hash-object", "-w", "--stdin"], "");
    assert_eq!(object_id, "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    assert!(
        bare_path
            .join("objects/e6/9de29bb2d1d6434b8b29ae775ad8c2e48c5391")
            .is_file()
    );
    assert!(
        !parent
            .path()
            .join(".git/objects/e6/9de29bb2d1d6434b8b29ae775ad8c2e48c5391")
            .exists()
    );
    assert_eq!(run_zmin(&bare_path, ["cat-file", "-s", &object_id]), "0");
}

#[test]
fn index_stage_object_paths_match_stock_git() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    fs::write(repo.path().join("b.txt"), b"hello\n").expect("write matching fixture");
    fs::write(repo.path().join("c.txt"), b"changed\n").expect("write changed fixture");
    git(repo.path(), ["add", "a.txt", "b.txt", "c.txt"]);

    for objectish in [":a.txt", ":0:a.txt"] {
        assert_eq!(
            run_zmin(repo.path(), ["rev-parse", objectish]),
            git(repo.path(), ["rev-parse", objectish])
        );
        assert_eq!(
            run_zmin(repo.path(), ["cat-file", "-p", objectish]),
            git(repo.path(), ["cat-file", "-p", objectish])
        );
    }
    assert_eq!(
        run_zmin_status(
            repo.path(),
            ["diff", "--raw", "--exit-code", ":a.txt", ":b.txt"]
        ),
        git_status(
            repo.path(),
            ["diff", "--raw", "--exit-code", ":a.txt", ":b.txt"]
        )
    );
    assert_eq!(
        run_zmin(repo.path(), ["diff", "--raw", ":a.txt", ":c.txt"]),
        git(repo.path(), ["diff", "--raw", ":a.txt", ":c.txt"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["diff", ":a.txt", ":c.txt"]),
        git(repo.path(), ["diff", ":a.txt", ":c.txt"])
    );
}

#[test]
fn ident_attribute_add_and_checkout_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join(".gitattributes"), b"*.i ident\n").expect("write attributes");
        fs::write(repo.join("id.i"), b"before\n$Id$\nafter\n").expect("write ident file");
    }

    git(git_repo.path(), ["add", "id.i"]);
    run_zmin(zmin_repo.path(), ["add", "id.i"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-p", ":id.i"]),
        git(git_repo.path(), ["cat-file", "-p", ":id.i"])
    );

    fs::remove_file(git_repo.path().join("id.i")).expect("remove git worktree file");
    fs::remove_file(zmin_repo.path().join("id.i")).expect("remove zmin worktree file");
    git(git_repo.path(), ["checkout", "--", "id.i"]);
    run_zmin(zmin_repo.path(), ["checkout", "--", "id.i"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("id.i")).expect("read zmin ident file"),
        fs::read_to_string(git_repo.path().join("id.i")).expect("read git ident file")
    );
}

#[test]
fn filter_attribute_add_and_checkout_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let rot13 = "tr 'A-Za-z' 'N-ZA-Mn-za-m'";
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "filter.rot13.clean", rot13]);
        git(repo, ["config", "filter.rot13.smudge", rot13]);
        fs::write(repo.join(".gitattributes"), b"*.t filter=rot13\n").expect("write attributes");
        fs::write(repo.join("message.t"), b"hello abc xyz\n").expect("write filtered file");
    }
    assert_eq!(
        run_zmin(zmin_repo.path(), ["hash-object", "message.t"]),
        git(git_repo.path(), ["hash-object", "message.t"])
    );

    git(git_repo.path(), ["add", "message.t"]);
    run_zmin(zmin_repo.path(), ["add", "message.t"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-p", ":message.t"]),
        git(git_repo.path(), ["cat-file", "-p", ":message.t"])
    );

    fs::remove_file(git_repo.path().join("message.t")).expect("remove git worktree file");
    fs::remove_file(zmin_repo.path().join("message.t")).expect("remove zmin worktree file");
    git(git_repo.path(), ["checkout", "--", "message.t"]);
    run_zmin(zmin_repo.path(), ["checkout", "--", "message.t"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("message.t")).expect("read zmin filtered file"),
        fs::read_to_string(git_repo.path().join("message.t")).expect("read git filtered file")
    );
}

#[test]
fn cat_file_filters_match_stock_git_for_eol_and_smudge_attributes() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "filter.rot13.clean", "cat"]);
    git(
        repo.path(),
        [
            "config",
            "filter.rot13.smudge",
            "tr 'A-Za-z' 'N-ZA-Mn-za-m'",
        ],
    );
    fs::write(
        repo.path().join(".gitattributes"),
        b"*.txt text eol=crlf\n*.rot filter=rot13\n",
    )
    .expect("write attributes");
    fs::write(repo.path().join("line.txt"), b"one\ntwo\n").expect("write eol file");
    fs::write(repo.path().join("message.rot"), b"hello abc xyz\n").expect("write filtered file");
    git(repo.path(), ["add", "."]);
    git_with_env(repo.path(), ["commit", "-m", "filters"]);

    let text_blob = git(repo.path(), ["rev-parse", "HEAD:line.txt"]);
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--filters", "HEAD:line.txt"]
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--filters", "HEAD:line.txt"]
        )
    );
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--filters", "--path=line.txt", &text_blob],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--filters", "--path=line.txt", &text_blob],
        )
    );

    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--filters", "HEAD:message.rot"],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--filters", "HEAD:message.rot"],
        )
    );
}

#[test]
fn cat_file_textconv_matches_stock_git_for_diff_driver_attributes() {
    let repo = git_init();
    configure_identity(repo.path());
    git(
        repo.path(),
        ["config", "diff.upper.textconv", "tr 'a-z' 'A-Z' <"],
    );
    fs::write(repo.path().join(".gitattributes"), b"*.bin diff=upper\n").expect("write attributes");
    fs::write(repo.path().join("payload.bin"), b"hello abc\n").expect("write payload");
    fs::write(repo.path().join("plain.txt"), b"plain\n").expect("write plain");
    git(repo.path(), ["add", "."]);
    git_with_env(repo.path(), ["commit", "-m", "textconv"]);

    let blob = git(repo.path(), ["rev-parse", "HEAD:payload.bin"]);
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--textconv", "HEAD:payload.bin"],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--textconv", "HEAD:payload.bin"],
        )
    );
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--textconv", "--path=payload.bin", &blob],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--textconv", "--path=payload.bin", &blob],
        )
    );
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--textconv", "HEAD:plain.txt"],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--textconv", "HEAD:plain.txt"],
        )
    );
}

#[test]
#[cfg(unix)]
fn process_filter_attribute_add_and_checkout_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let helper = git_repo.path().join("process-filter.pl");
    fs::write(
        &helper,
        r#"use strict;
use warnings;
binmode STDIN;
binmode STDOUT;
$| = 1;
open my $log, ">>", $ARGV[0] or die "log";
sub readpkt {
    my $header = "";
    my $read = read(STDIN, $header, 4);
    return undef unless defined($read) && $read == 4;
    my $len = hex($header);
    return "" if $len == 0;
    my $payload = "";
    read(STDIN, $payload, $len - 4) == $len - 4 or die "short read";
    return $payload;
}
sub readtext {
    my $value = readpkt();
    return undef unless defined $value;
    $value =~ s/\n$//;
    return $value;
}
sub writepkt {
    my ($payload) = @_;
    printf "%04x%s", length($payload) + 4, $payload;
}
sub flushpkt { print "0000"; }
sub rot13 {
    my ($value) = @_;
    $value =~ tr/A-Za-z/N-ZA-Mn-za-m/;
    return $value;
}
print $log "START\n";
die "client" unless readtext() eq "git-filter-client";
die "version" unless readtext() eq "version=2";
die "flush" unless readtext() eq "";
writepkt("git-filter-server");
writepkt("version=2");
flushpkt();
while ((my $cap = readtext()) ne "") {}
writepkt("capability=clean");
writepkt("capability=smudge");
flushpkt();
print $log "init handshake complete\n";
while (1) {
    my $command = readtext();
    last unless defined $command;
    $command =~ s/^command=// or die "command";
    my $path = readtext();
    $path =~ s/^pathname=// or die "path";
    while ((my $meta = readtext()) ne "") {}
    my $content = "";
    while ((my $packet = readpkt()) ne "") { $content .= $packet; }
    print $log "IN: $command $path\n";
    my $out = rot13($content);
    writepkt("status=success");
    flushpkt();
    while (length($out) > 0) {
        my $chunk = substr($out, 0, 65516, "");
        writepkt($chunk);
    }
    flushpkt();
    flushpkt();
}
print $log "STOP\n";
"#,
    )
    .expect("write process filter helper");
    let command = format!(
        "perl {} debug.log",
        shell_quote_for_test(&helper.to_string_lossy())
    );
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "filter.protocol.process", &command]);
        git(repo, ["config", "filter.protocol.required", "true"]);
        fs::write(repo.join(".gitattributes"), b"*.r filter=protocol\n").expect("write attributes");
        fs::write(repo.join("one.r"), b"hello abc\n").expect("write one");
        fs::write(repo.join("two.r"), b"xyz world\n").expect("write two");
    }
    assert_eq!(
        run_zmin(zmin_repo.path(), ["hash-object", "one.r"]),
        git(git_repo.path(), ["hash-object", "one.r"])
    );
    let _ = fs::remove_file(git_repo.path().join("debug.log"));
    let _ = fs::remove_file(zmin_repo.path().join("debug.log"));

    git(git_repo.path(), ["add", "."]);
    run_zmin(zmin_repo.path(), ["add", "."]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-p", ":one.r"]),
        git(git_repo.path(), ["cat-file", "-p", ":one.r"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-p", ":two.r"]),
        git(git_repo.path(), ["cat-file", "-p", ":two.r"])
    );
    let zmin_log = fs::read_to_string(zmin_repo.path().join("debug.log")).expect("read log");
    assert_eq!(zmin_log.matches("START\n").count(), 1);
    assert_eq!(zmin_log.matches("STOP\n").count(), 1);
    assert!(zmin_log.contains("IN: clean one.r\n"));
    assert!(zmin_log.contains("IN: clean two.r\n"));

    fs::remove_file(git_repo.path().join("one.r")).expect("remove git one");
    fs::remove_file(git_repo.path().join("two.r")).expect("remove git two");
    fs::remove_file(zmin_repo.path().join("one.r")).expect("remove zmin one");
    fs::remove_file(zmin_repo.path().join("two.r")).expect("remove zmin two");
    fs::remove_file(zmin_repo.path().join("debug.log")).expect("remove zmin log");
    git(git_repo.path(), ["checkout", "--", "one.r", "two.r"]);
    run_zmin(zmin_repo.path(), ["checkout", "--", "one.r", "two.r"]);
    assert_eq!(
        fs::read(zmin_repo.path().join("one.r")).expect("read zmin one"),
        fs::read(git_repo.path().join("one.r")).expect("read git one")
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("two.r")).expect("read zmin two"),
        fs::read(git_repo.path().join("two.r")).expect("read git two")
    );
    let zmin_log = fs::read_to_string(zmin_repo.path().join("debug.log")).expect("read log");
    assert_eq!(zmin_log.matches("START\n").count(), 1);
    assert_eq!(zmin_log.matches("STOP\n").count(), 1);
    assert!(zmin_log.contains("IN: smudge one.r\n"));
    assert!(zmin_log.contains("IN: smudge two.r\n"));
}

#[cfg(unix)]
fn shell_quote_for_test(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

#[test]
fn cat_file_resolves_reflog_selector_for_ref_name_ending_with_at_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("hello"), b"hello\n").expect("write hello");
    git(repo.path(), ["add", "hello"]);
    git_with_env(repo.path(), ["commit", "-m", "hello"]);
    run_zmin(repo.path(), ["branch", "foo@"]);

    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-p", "foo@@{0}:hello"]),
        git(repo.path(), ["cat-file", "-p", "HEAD:hello"])
    );
}

#[test]
fn show_matches_stock_git_for_raw_commits_trees_blobs_and_tags() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "tag.gpgSign", "false"]);
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write a");
    fs::write(repo.path().join("dir/b.txt"), b"nested\n").expect("write b");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git_with_env(repo.path(), ["tag", "-a", "v1", "-m", "tag message"]);

    for args in [
        ["show", "HEAD:a.txt"].as_slice(),
        ["show", "HEAD^{tree}"].as_slice(),
        ["show", "HEAD"].as_slice(),
        ["show", "--oneline", "HEAD"].as_slice(),
        ["show", "--format=%H", "HEAD"].as_slice(),
        ["show", "--stat", "HEAD"].as_slice(),
        ["show", "--numstat", "--format=%H", "HEAD"].as_slice(),
        ["show", "--shortstat", "HEAD"].as_slice(),
        ["show", "--raw", "--format=%H", "HEAD"].as_slice(),
        ["show", "--summary", "--format=%H", "HEAD"].as_slice(),
        ["show", "--name-only", "--format=%H", "HEAD"].as_slice(),
        ["show", "--name-status", "--format=%H", "HEAD"].as_slice(),
        ["show", "--no-patch", "--format=raw", "HEAD"].as_slice(),
        ["show", "--format=raw", "HEAD"].as_slice(),
        ["show", "--no-patch", "--format=%H", "HEAD"].as_slice(),
        ["show", "--no-patch", "--pretty=format:%an <%ae>", "HEAD"].as_slice(),
        ["show", "--no-patch", "--oneline", "HEAD"].as_slice(),
        ["show", "--no-patch", "HEAD"].as_slice(),
        ["show", "--no-patch", "--format=raw", "v1"].as_slice(),
        ["show", "v1"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn treeish_path_resolution_and_ls_tree_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("src")).expect("create src");
    fs::write(repo.path().join("README.md"), b"hello\n").expect("write readme");
    fs::write(repo.path().join("src/main.rs"), b"fn main() {}\n").expect("write source");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    fs::write(repo.path().join("README.md"), b"hello again\n").expect("modify readme");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);

    for args in [
        ["rev-parse", "HEAD^{tree}"].as_slice(),
        ["rev-parse", "HEAD:src/main.rs"].as_slice(),
        ["cat-file", "-p", "HEAD:src/main.rs"].as_slice(),
        ["cat-file", "-p", "HEAD^{tree}"].as_slice(),
        ["rev-parse", "HEAD~1"].as_slice(),
        ["rev-parse", "HEAD~1^{tree}"].as_slice(),
        ["ls-tree", "HEAD"].as_slice(),
        ["ls-tree", "-r", "--name-only", "HEAD"].as_slice(),
        ["ls-tree", "-r", "-t", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn unpack_file_matches_stock_git_blob_behavior() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    let blob = git(repo.path(), ["hash-object", "-w", "a.txt"]);

    let zmin_path = run_zmin(repo.path(), ["unpack-file", &blob]);
    assert!(zmin_path.starts_with(".merge_file_"));
    assert_eq!(
        fs::read(repo.path().join(&zmin_path)).expect("read zmin unpacked file"),
        b"hello\n"
    );

    let git_path = git(repo.path(), ["unpack-file", &blob]);
    assert!(git_path.starts_with(".merge_file_"));
    assert_eq!(
        fs::read(repo.path().join(&git_path)).expect("read git unpacked file"),
        b"hello\n"
    );

    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let commit = git(repo.path(), ["rev-parse", "HEAD"]);
    assert_eq!(
        run_zmin_status(repo.path(), ["unpack-file", &commit]),
        git_status(repo.path(), ["unpack-file", &commit])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["unpack-file", "deadbeef"]),
        git_status(repo.path(), ["unpack-file", "deadbeef"])
    );
}

#[test]
fn show_index_matches_stock_git_for_pack_index_stdin() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["repack", "-adq"]);
    let idx = fs::read(first_pack_index(repo.path())).expect("read pack index");

    assert_eq!(
        run_zmin_with_stdin_bytes(repo.path(), ["show-index"], &idx),
        git_with_stdin_bytes(repo.path(), ["show-index"], &idx)
    );
}

#[test]
fn show_index_rejects_unsupported_pack_index_version_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["repack", "-adq"]);
    let idx = rewrite_pack_index_version(&first_pack_index(repo.path()), 3);

    assert_eq!(
        command_any_output_with_stdin_bytes(zmin_bin(), repo.path(), &["show-index"], &idx, "zmin"),
        command_any_output_with_stdin_bytes("git", repo.path(), &["show-index"], &idx, "git")
    );
}

#[test]
fn update_server_info_matches_stock_git_for_bare_repo() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(dir.path(), ["init", "-b", "main", "source"]);
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"hello\n").expect("write fixture");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["branch", "feature"]);
    git(&source, ["tag", "lightweight"]);
    git_with_env(&source, ["tag", "-a", "annotated", "-m", "tag message"]);
    git(
        dir.path(),
        [
            "clone",
            "--bare",
            source.to_str().expect("source path"),
            "git.git",
        ],
    );
    git(
        dir.path(),
        [
            "clone",
            "--bare",
            source.to_str().expect("source path"),
            "zmin.git",
        ],
    );
    let git_repo = dir.path().join("git.git");
    let zmin_repo = dir.path().join("zmin.git");

    git(&git_repo, ["update-server-info"]);
    run_zmin(&zmin_repo, ["update-server-info"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.join("info/refs")).expect("read zmin info refs"),
        fs::read_to_string(git_repo.join("info/refs")).expect("read git info refs")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.join("objects/info/packs")).expect("read zmin packs info"),
        fs::read_to_string(git_repo.join("objects/info/packs")).expect("read git packs info")
    );

    git(&git_repo, ["repack", "-adq"]);
    git(&zmin_repo, ["repack", "-adq"]);
    git(&git_repo, ["update-server-info", "-f"]);
    run_zmin(&zmin_repo, ["update-server-info", "-f"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.join("info/refs")).expect("read packed zmin info refs"),
        fs::read_to_string(git_repo.join("info/refs")).expect("read packed git info refs")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.join("objects/info/packs"))
            .expect("read packed zmin packs info"),
        fs::read_to_string(git_repo.join("objects/info/packs"))
            .expect("read packed git packs info")
    );
}

#[test]
fn count_objects_matches_stock_git_for_loose_and_packed_objects() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    assert_eq!(
        run_zmin(repo.path(), ["count-objects"]),
        git(repo.path(), ["count-objects"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-H"]),
        git(repo.path(), ["count-objects", "-H"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-vH"]),
        git(repo.path(), ["count-objects", "-vH"])
    );

    git(repo.path(), ["repack", "-adq"]);
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-vH"]),
        git(repo.path(), ["count-objects", "-vH"])
    );
}

#[test]
fn count_objects_in_pack_counts_pack_index_entries_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"one\n").expect("write first");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    git(repo.path(), ["repack", "-adq"]);

    let saved_pack_dir = repo.path().join("saved-pack");
    fs::create_dir_all(&saved_pack_dir).expect("create saved pack dir");
    for entry in fs::read_dir(repo.path().join(".git/objects/pack")).expect("read pack dir") {
        let path = entry.expect("pack entry").path();
        if path.is_file() {
            fs::copy(
                &path,
                saved_pack_dir.join(path.file_name().expect("pack file name")),
            )
            .expect("save pack file");
        }
    }

    fs::write(repo.path().join("b.txt"), b"two\n").expect("write second");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    git(repo.path(), ["repack", "-adq"]);
    for entry in fs::read_dir(&saved_pack_dir).expect("read saved pack dir") {
        let path = entry.expect("saved pack entry").path();
        fs::copy(
            &path,
            repo.path()
                .join(".git/objects/pack")
                .join(path.file_name().expect("saved pack file name")),
        )
        .expect("restore saved pack file");
    }

    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
}

#[test]
fn count_objects_prune_packable_counts_loose_objects_once_with_duplicate_packs() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"one\n").expect("write first");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    let blob = git(repo.path(), ["rev-parse", "HEAD:a.txt"]);
    let loose_path = repo
        .path()
        .join(".git/objects")
        .join(&blob[..2])
        .join(&blob[2..]);
    let loose_copy = fs::read(&loose_path).expect("read loose blob");
    git(repo.path(), ["repack", "-adq"]);

    let saved_pack_dir = repo.path().join("saved-pack");
    fs::create_dir_all(&saved_pack_dir).expect("create saved pack dir");
    for entry in fs::read_dir(repo.path().join(".git/objects/pack")).expect("read pack dir") {
        let path = entry.expect("pack entry").path();
        if path.is_file() {
            fs::copy(
                &path,
                saved_pack_dir.join(path.file_name().expect("pack file name")),
            )
            .expect("save pack file");
        }
    }

    fs::write(repo.path().join("b.txt"), b"two\n").expect("write second");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    git(repo.path(), ["repack", "-adq"]);
    for entry in fs::read_dir(&saved_pack_dir).expect("read saved pack dir") {
        let path = entry.expect("saved pack entry").path();
        fs::copy(
            &path,
            repo.path()
                .join(".git/objects/pack")
                .join(path.file_name().expect("saved pack file name")),
        )
        .expect("restore saved pack file");
    }
    fs::create_dir_all(loose_path.parent().expect("loose parent")).expect("recreate loose parent");
    fs::write(&loose_path, loose_copy).expect("restore loose blob");

    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
}

#[test]
fn write_tree_and_commit_tree_match_stock_git_objects() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    fs::create_dir_all(repo.path().join("src")).expect("create src");
    fs::write(repo.path().join("src/lib.rs"), b"pub fn fixture() {}\n").expect("write source");
    git(repo.path(), ["add", "-A"]);

    let git_tree = git(repo.path(), ["write-tree"]);
    let zmin_tree = run_zmin(repo.path(), ["write-tree"]);
    assert_eq!(zmin_tree, git_tree);
    assert_eq!(
        run_zmin(repo.path(), ["write-tree", "--prefix=src"]),
        git(repo.path(), ["write-tree", "--prefix=src"])
    );

    let git_root = git_with_env(repo.path(), ["commit-tree", &git_tree, "-m", "root"]);
    let zmin_root = run_zmin_with_env(repo.path(), ["commit-tree", &git_tree, "-m", "root"]);
    assert_eq!(zmin_root, git_root);
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-p", &zmin_root]),
        git(repo.path(), ["cat-file", "-p", &git_root])
    );

    fs::write(repo.path().join("a.txt"), b"second\n").expect("modify fixture");
    git(repo.path(), ["add", "-A"]);
    let tree = git(repo.path(), ["write-tree"]);
    let git_child = git_with_env(
        repo.path(),
        ["commit-tree", &tree, "-p", &git_root, "-m", "child"],
    );
    let zmin_child = run_zmin_with_env(
        repo.path(),
        ["commit-tree", &tree, "-p", &zmin_root, "-m", "child"],
    );
    assert_eq!(zmin_child, git_child);

    let git_dedup = git_with_env(
        repo.path(),
        [
            "commit-tree",
            &tree,
            "-p",
            &git_root,
            "-p",
            &git_root,
            "-m",
            "dedup",
        ],
    );
    let zmin_dedup = run_zmin_with_env(
        repo.path(),
        [
            "commit-tree",
            &tree,
            "-p",
            &zmin_root,
            "-p",
            &zmin_root,
            "-m",
            "dedup",
        ],
    );
    assert_eq!(zmin_dedup, git_dedup);
}

#[test]
fn show_pretty_raw_for_commit_tree_records_tree_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("a.txt"), b"hello\n").expect("write fixture");
        git(repo, ["add", "-A"]);
    }
    let git_tree = git(git_repo.path(), ["write-tree"]);
    let zmin_tree = git(zmin_repo.path(), ["write-tree"]);
    assert_eq!(zmin_tree, git_tree);

    let git_commit = git_with_stdin(git_repo.path(), ["commit-tree", &git_tree], "NO\n");
    let zmin_commit =
        run_zmin_with_stdin(zmin_repo.path(), ["commit-tree", &zmin_tree], "NO\n");
    let git_raw = git(
        git_repo.path(),
        ["show", "--pretty=raw", "--no-patch", &git_commit],
    );
    let zmin_raw = run_zmin(
        zmin_repo.path(),
        ["show", "--pretty=raw", "--no-patch", &zmin_commit],
    );

    assert_eq!(
        zmin_raw
            .lines()
            .find(|line| line.starts_with("tree "))
            .map(str::to_owned),
        git_raw
            .lines()
            .find(|line| line.starts_with("tree "))
            .map(str::to_owned)
    );
    assert!(
        zmin_raw
            .lines()
            .take_while(|line| !line.starts_with("author "))
            .any(|line| line == format!("tree {zmin_tree}")),
        "show --pretty=raw should print tree before author: {zmin_raw}"
    );
}

#[test]
fn read_tree_matches_stock_git_for_tree_empty_and_prefix() {
    let git_repo = two_commit_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    let tree = git(git_repo.path(), ["rev-parse", "HEAD^{tree}"]);

    git(git_repo.path(), ["read-tree", &tree]);
    run_zmin(zmin_repo.path(), ["read-tree", &tree]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["write-tree"]),
        git(git_repo.path(), ["write-tree"])
    );

    git(git_repo.path(), ["read-tree", "--empty"]);
    run_zmin(zmin_repo.path(), ["read-tree", "--empty"]);
    git(git_repo.path(), ["read-tree", "-m", &tree]);
    run_zmin(zmin_repo.path(), ["read-tree", "-m", &tree]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );

    git(git_repo.path(), ["read-tree", "--empty"]);
    run_zmin(zmin_repo.path(), ["read-tree", "--empty"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );

    git(git_repo.path(), ["read-tree", "--prefix=import/", &tree]);
    run_zmin(zmin_repo.path(), ["read-tree", "--prefix=import/", &tree]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["write-tree"]),
        git(git_repo.path(), ["write-tree"])
    );
}

#[test]
fn mktree_matches_stock_git_for_text_nul_and_batch_input() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"a\n").expect("write a");
    fs::write(repo.path().join("b.txt"), b"b\n").expect("write b");
    let a = git(repo.path(), ["hash-object", "-w", "a.txt"]);
    let b = git(repo.path(), ["hash-object", "-w", "b.txt"]);

    let input = format!("100644 blob {b}\tb.txt\n100644 blob {a}\ta.txt\n");
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktree"], &input),
        git_with_stdin(repo.path(), ["mktree"], &input)
    );

    let nul_input = format!("100644 blob {a}\ta.txt\0");
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktree", "-z"], &nul_input),
        git_with_stdin(repo.path(), ["mktree", "-z"], &nul_input)
    );

    let batch_input = format!(
        "100644 blob {a}\ta.txt\n\n100644 blob {b}\tb.txt\n160000 commit 1111111111111111111111111111111111111111\tsub\n"
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktree", "--batch"], &batch_input),
        git_with_stdin(repo.path(), ["mktree", "--batch"], &batch_input)
    );
}

#[test]
fn mktag_matches_stock_git_for_valid_tag_object() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"a\n").expect("write a");
    let blob = git(repo.path(), ["hash-object", "-w", "a.txt"]);
    let input = format!(
        "object {blob}\ntype blob\ntag v1\ntagger Bench <bench@example.test> 1700000000 +0000\n\ntag message\n"
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktag"], &input),
        git_with_stdin(repo.path(), ["mktag"], &input)
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktag", "--strict"], &input),
        git_with_stdin(repo.path(), ["mktag", "--strict"], &input)
    );
}
