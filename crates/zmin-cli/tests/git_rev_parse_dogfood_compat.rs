mod common;

use std::fs;
use std::path::Path;

use common::{
    command_any_output, command_any_output as command_output, configure_identity, git, zmin_bin,
};
use tempfile::TempDir;

#[test]
fn rev_parse_abbrev_ref_head_modes_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_repo = dir.path().join("git");
    let zmin_repo = dir.path().join("zmin");
    git(
        dir.path(),
        ["init", "-b", "main", git_repo.to_str().expect("git path")],
    );
    git(
        dir.path(),
        ["init", "-b", "main", zmin_repo.to_str().expect("zmin path")],
    );
    configure_identity(&git_repo);
    configure_identity(&zmin_repo);
    fs::write(git_repo.join("tracked.txt"), "one\n").expect("write git file");
    fs::write(zmin_repo.join("tracked.txt"), "one\n").expect("write zmin file");
    git(&git_repo, ["add", "tracked.txt"]);
    git(&zmin_repo, ["add", "tracked.txt"]);
    git(&git_repo, ["commit", "-m", "initial"]);
    git(&zmin_repo, ["commit", "-m", "initial"]);

    for args in [
        ["rev-parse", "--abbrev-ref", "HEAD"].as_slice(),
        ["rev-parse", "--abbrev-ref=loose", "HEAD"].as_slice(),
        ["rev-parse", "--abbrev-ref=strict", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), &zmin_repo, args, "zmin branch"),
            command_output("git", &git_repo, args, "git branch"),
            "{args:?}"
        );
    }

    git(&git_repo, ["checkout", "--detach", "HEAD"]);
    git(&zmin_repo, ["checkout", "--detach", "HEAD"]);
    for args in [
        ["rev-parse", "--abbrev-ref", "HEAD"].as_slice(),
        ["rev-parse", "--abbrev-ref=loose", "HEAD"].as_slice(),
        ["rev-parse", "--abbrev-ref=strict", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), &zmin_repo, args, "zmin detached"),
            command_output("git", &git_repo, args, "git detached"),
            "{args:?}"
        );
    }
}

#[test]
fn rev_parse_short_head_lengths_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_repo = dir.path().join("git");
    let zmin_repo = dir.path().join("zmin");
    git(
        dir.path(),
        ["init", "-b", "main", git_repo.to_str().expect("git path")],
    );
    git(
        dir.path(),
        ["init", "-b", "main", zmin_repo.to_str().expect("zmin path")],
    );
    configure_identity(&git_repo);
    configure_identity(&zmin_repo);
    fs::write(git_repo.join("tracked.txt"), "one\n").expect("write git file");
    fs::write(zmin_repo.join("tracked.txt"), "one\n").expect("write zmin file");
    git(&git_repo, ["add", "tracked.txt"]);
    git(&zmin_repo, ["add", "tracked.txt"]);
    git(&git_repo, ["commit", "-m", "initial"]);
    git(&zmin_repo, ["commit", "-m", "initial"]);

    for args in [
        ["rev-parse", "--short", "HEAD"].as_slice(),
        ["rev-parse", "--short=12", "HEAD"].as_slice(),
        ["rev-parse", "--short=100", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_output(zmin_bin(), &zmin_repo, args, "zmin short"),
            command_output("git", &git_repo, args, "git short"),
            "{args:?}"
        );
    }
}

#[test]
fn rev_parse_verify_quiet_modes_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_repo = dir.path().join("git");
    let zmin_repo = dir.path().join("zmin");
    git(
        dir.path(),
        ["init", "-b", "main", git_repo.to_str().expect("git path")],
    );
    git(
        dir.path(),
        ["init", "-b", "main", zmin_repo.to_str().expect("zmin path")],
    );
    configure_identity(&git_repo);
    configure_identity(&zmin_repo);
    fs::write(git_repo.join("tracked.txt"), "one\n").expect("write git file");
    fs::write(zmin_repo.join("tracked.txt"), "one\n").expect("write zmin file");
    git(&git_repo, ["add", "tracked.txt"]);
    git(&zmin_repo, ["add", "tracked.txt"]);
    git(&git_repo, ["commit", "-m", "initial"]);
    git(&zmin_repo, ["commit", "-m", "initial"]);

    for args in [
        ["rev-parse", "--verify", "HEAD"].as_slice(),
        ["rev-parse", "--verify", "missing"].as_slice(),
        ["rev-parse", "--quiet", "--verify", "missing"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), &zmin_repo, args, "zmin verify"),
            command_any_output("git", &git_repo, args, "git verify"),
            "{args:?}"
        );
    }
}

#[test]
fn rev_parse_head_follows_nested_symbolic_refs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    fs::write(repo.join("tracked.txt"), "one\n").expect("write tracked file");
    git(&repo, ["add", "tracked.txt"]);
    git(&repo, ["commit", "-m", "initial"]);
    write_nested_symbolic_head(&repo);

    let args = ["rev-parse", "HEAD"];
    assert_eq!(
        command_any_output(zmin_bin(), &repo, &args, "zmin nested symbolic HEAD"),
        command_any_output("git", &repo, &args, "git nested symbolic HEAD")
    );
}

fn write_nested_symbolic_head(repo: &Path) {
    let branch = git(repo, ["symbolic-ref", "--short", "HEAD"]);
    fs::write(
        repo.join(".git").join("refs").join("heads").join("alias"),
        format!("ref: refs/heads/{branch}\n"),
    )
    .expect("write alias ref");
    fs::write(repo.join(".git").join("HEAD"), "ref: refs/heads/alias\n")
        .expect("write nested HEAD");
}
