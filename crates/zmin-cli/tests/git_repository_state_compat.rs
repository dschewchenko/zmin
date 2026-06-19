mod common;

use std::fs;
use std::path::Path;

use common::{assert_repository_state_matches, command_output_with_env, git, run_zmin, zmin_bin};
use tempfile::TempDir;

const FIXED_ENV: [(&str, &str); 6] = [
    ("GIT_AUTHOR_NAME", "Bench"),
    ("GIT_AUTHOR_EMAIL", "bench@example.test"),
    ("GIT_AUTHOR_DATE", "1700000000 +0000"),
    ("GIT_COMMITTER_NAME", "Bench"),
    ("GIT_COMMITTER_EMAIL", "bench@example.test"),
    ("GIT_COMMITTER_DATE", "1700000000 +0000"),
];

#[test]
fn git_seed_handoff_keeps_repository_state_identical() {
    let dir = TempDir::new().expect("temp dir");
    let seed = dir.path().join("git-seed");
    git(
        dir.path(),
        ["init", "-b", "main", seed.to_str().expect("seed path")],
    );
    configure_identity_with_git(&seed);
    seed_repository_with_git(&seed);

    git_with_fixed_env(
        dir.path(),
        ["clone", seed.to_str().expect("seed path"), "git-handoff"],
    );
    git_with_fixed_env(
        dir.path(),
        ["clone", seed.to_str().expect("seed path"), "zmin-handoff"],
    );

    let git_handoff = dir.path().join("git-handoff");
    let zmin_handoff = dir.path().join("zmin-handoff");
    configure_identity_with_git(&git_handoff);
    configure_identity_with_git(&zmin_handoff);
    apply_handoff_workflow_with_git(&git_handoff);
    apply_handoff_workflow_with_zmin(&zmin_handoff);

    assert_repository_state_matches(&zmin_handoff, &git_handoff);
}

#[test]
fn zmin_seed_handoff_keeps_repository_state_identical() {
    let dir = TempDir::new().expect("temp dir");
    let seed = dir.path().join("zmin-seed");
    run_zmin(
        dir.path(),
        [
            "init",
            "--initial-branch",
            "main",
            seed.to_str().expect("seed path"),
        ],
    );
    configure_identity_with_git(&seed);
    seed_repository_with_zmin(&seed);

    git_with_fixed_env(
        dir.path(),
        ["clone", seed.to_str().expect("seed path"), "git-handoff"],
    );
    git_with_fixed_env(
        dir.path(),
        ["clone", seed.to_str().expect("seed path"), "zmin-handoff"],
    );

    let git_handoff = dir.path().join("git-handoff");
    let zmin_handoff = dir.path().join("zmin-handoff");
    configure_identity_with_git(&git_handoff);
    configure_identity_with_git(&zmin_handoff);
    apply_handoff_workflow_with_git(&git_handoff);
    apply_handoff_workflow_with_zmin(&zmin_handoff);

    assert_repository_state_matches(&zmin_handoff, &git_handoff);
}

fn configure_identity_with_git(repo: &Path) {
    git(repo, ["config", "user.name", "Bench"]);
    git(repo, ["config", "user.email", "bench@example.test"]);
    git(repo, ["config", "commit.gpgsign", "false"]);
    git(repo, ["config", "tag.gpgsign", "false"]);
}

fn seed_repository_with_git(repo: &Path) {
    fs::write(repo.join("README.md"), b"seed\n").expect("write readme");
    fs::create_dir_all(repo.join("src")).expect("create src dir");
    fs::write(repo.join("src/lib.rs"), b"pub fn seed() {}\n").expect("write lib");
    git(repo, ["add", "-A"]);
    git_with_fixed_env(repo, ["commit", "-m", "seed"]);
    git(repo, ["tag", "seed-tag"]);
}

fn seed_repository_with_zmin(repo: &Path) {
    fs::write(repo.join("README.md"), b"seed\n").expect("write readme");
    fs::create_dir_all(repo.join("src")).expect("create src dir");
    fs::write(repo.join("src/lib.rs"), b"pub fn seed() {}\n").expect("write lib");
    run_zmin(repo, ["add", "-A"]);
    zmin_with_fixed_env(repo, ["commit", "-m", "seed"]);
    run_zmin(repo, ["tag", "seed-tag"]);
}

fn apply_handoff_workflow_with_git(repo: &Path) {
    git_with_fixed_env(repo, ["switch", "-c", "feature"]);
    fs::write(repo.join("README.md"), b"seed\nfeature\n").expect("update readme");
    fs::write(repo.join("src/lib.rs"), b"pub fn feature() {}\n").expect("update lib");
    fs::write(repo.join("notes.txt"), b"notes\n").expect("write notes");
    git(repo, ["add", "-A"]);
    git_with_fixed_env(repo, ["commit", "-m", "feature"]);
    git_with_fixed_env(repo, ["switch", "main"]);
    git_with_fixed_env(repo, ["merge", "--ff-only", "feature"]);
    git(repo, ["pack-refs", "--all"]);
    git(repo, ["repack", "-ad"]);
    git(repo, ["commit-graph", "write", "--reachable"]);
}

fn apply_handoff_workflow_with_zmin(repo: &Path) {
    zmin_with_fixed_env(repo, ["switch", "-c", "feature"]);
    fs::write(repo.join("README.md"), b"seed\nfeature\n").expect("update readme");
    fs::write(repo.join("src/lib.rs"), b"pub fn feature() {}\n").expect("update lib");
    fs::write(repo.join("notes.txt"), b"notes\n").expect("write notes");
    run_zmin(repo, ["add", "-A"]);
    zmin_with_fixed_env(repo, ["commit", "-m", "feature"]);
    zmin_with_fixed_env(repo, ["switch", "main"]);
    zmin_with_fixed_env(repo, ["merge", "--ff-only", "feature"]);
    run_zmin(repo, ["pack-refs", "--all"]);
    run_zmin(repo, ["repack", "-ad"]);
    run_zmin(repo, ["commit-graph", "write", "--reachable"]);
}

fn git_with_fixed_env<const N: usize>(repo: &Path, args: [&str; N]) {
    let _ = command_output_with_env("git", repo, &args, &FIXED_ENV, "git");
}

fn zmin_with_fixed_env<const N: usize>(repo: &Path, args: [&str; N]) {
    let _ = command_output_with_env(zmin_bin(), repo, &args, &FIXED_ENV, "zmin");
}
