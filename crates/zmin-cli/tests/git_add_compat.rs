mod common;

use std::fs;

use tempfile::TempDir;

use common::{command_any_output, configure_identity, git, git_init, run_zmin};

#[cfg(unix)]
fn write_bad_capability_filter_helper(path: &std::path::Path) {
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
writepkt("capability=bogus");
flushpkt();
"#,
    )
    .expect("write bad capability filter helper");
}

#[cfg(unix)]
fn write_bad_status_filter_helper(path: &std::path::Path) {
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
writepkt("capability=clean");
flushpkt();
while (1) {
    my $command = readtext();
    last unless defined $command;
    readtext();
    while ((my $metadata = readtext()) ne "") {}
    while ((my $packet = readpkt()) ne "") {}
    writepkt("status=bogus");
    flushpkt();
}
"#,
    )
    .expect("write bad status filter helper");
}

#[cfg(unix)]
#[test]
fn add_process_filter_unknown_capability_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let helper_dir = TempDir::new().expect("helper tempdir");
    let helper = helper_dir.path().join("bad-capability-filter.pl");
    write_bad_capability_filter_helper(&helper);
    let command = format!("perl {}", helper.display());

    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        git(repo, ["config", "filter.bad.process", &command]);
        git(repo, ["config", "filter.bad.required", "true"]);
        fs::write(repo.join(".gitattributes"), b"*.bad filter=bad\n").expect("write attributes");
        fs::write(repo.join("a.bad"), b"hello\n").expect("write filtered file");
    }

    assert_eq!(
        command_any_output(
            common::zmin_bin(),
            zmin_repo.path(),
            &["add", "a.bad"],
            "zmin add bad capability process filter",
        ),
        command_any_output(
            "git",
            git_repo.path(),
            &["add", "a.bad"],
            "git add bad capability process filter",
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
}

#[cfg(unix)]
#[test]
fn add_process_filter_unknown_status_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let helper_dir = TempDir::new().expect("helper tempdir");
    let helper = helper_dir.path().join("bad-status-filter.pl");
    write_bad_status_filter_helper(&helper);
    let command = format!("perl {}", helper.display());

    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        git(repo, ["config", "filter.bad.process", &command]);
        git(repo, ["config", "filter.bad.required", "true"]);
        fs::write(repo.join(".gitattributes"), b"*.bad filter=bad\n").expect("write attributes");
        fs::write(repo.join("a.bad"), b"hello\n").expect("write filtered file");
    }

    assert_eq!(
        command_any_output(
            common::zmin_bin(),
            zmin_repo.path(),
            &["add", "a.bad"],
            "zmin add bad status process filter",
        ),
        command_any_output(
            "git",
            git_repo.path(),
            &["add", "a.bad"],
            "git add bad status process filter",
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1"]),
        git(git_repo.path(), ["status", "--porcelain=v1"])
    );
}
