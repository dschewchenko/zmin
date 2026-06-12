mod common;

use std::fs;
use std::path::Path;

use common::{assert_repository_state_matches, command_output_with_env, git, run_skron, skron_bin};
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
        ["clone", seed.to_str().expect("seed path"), "skron-handoff"],
    );

    let git_handoff = dir.path().join("git-handoff");
    let skron_handoff = dir.path().join("skron-handoff");
    configure_identity_with_git(&git_handoff);
    configure_identity_with_git(&skron_handoff);
    apply_handoff_workflow_with_git(&git_handoff);
    apply_handoff_workflow_with_skron(&skron_handoff);

    assert_repository_state_matches(&skron_handoff, &git_handoff);
}

#[test]
fn skron_seed_handoff_keeps_repository_state_identical() {
    let dir = TempDir::new().expect("temp dir");
    let seed = dir.path().join("skron-seed");
    run_skron(
        dir.path(),
        [
            "init",
            "--initial-branch",
            "main",
            seed.to_str().expect("seed path"),
        ],
    );
    configure_identity_with_git(&seed);
    seed_repository_with_skron(&seed);

    git_with_fixed_env(
        dir.path(),
        ["clone", seed.to_str().expect("seed path"), "git-handoff"],
    );
    git_with_fixed_env(
        dir.path(),
        ["clone", seed.to_str().expect("seed path"), "skron-handoff"],
    );

    let git_handoff = dir.path().join("git-handoff");
    let skron_handoff = dir.path().join("skron-handoff");
    configure_identity_with_git(&git_handoff);
    configure_identity_with_git(&skron_handoff);
    apply_handoff_workflow_with_git(&git_handoff);
    apply_handoff_workflow_with_skron(&skron_handoff);

    assert_repository_state_matches(&skron_handoff, &git_handoff);
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

fn seed_repository_with_skron(repo: &Path) {
    fs::write(repo.join("README.md"), b"seed\n").expect("write readme");
    fs::create_dir_all(repo.join("src")).expect("create src dir");
    fs::write(repo.join("src/lib.rs"), b"pub fn seed() {}\n").expect("write lib");
    run_skron(repo, ["add", "-A"]);
    skron_with_fixed_env(repo, ["commit", "-m", "seed"]);
    run_skron(repo, ["tag", "seed-tag"]);
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

fn apply_handoff_workflow_with_skron(repo: &Path) {
    skron_with_fixed_env(repo, ["switch", "-c", "feature"]);
    fs::write(repo.join("README.md"), b"seed\nfeature\n").expect("update readme");
    fs::write(repo.join("src/lib.rs"), b"pub fn feature() {}\n").expect("update lib");
    fs::write(repo.join("notes.txt"), b"notes\n").expect("write notes");
    run_skron(repo, ["add", "-A"]);
    skron_with_fixed_env(repo, ["commit", "-m", "feature"]);
    skron_with_fixed_env(repo, ["switch", "main"]);
    skron_with_fixed_env(repo, ["merge", "--ff-only", "feature"]);
    run_skron(repo, ["pack-refs", "--all"]);
    run_skron(repo, ["repack", "-ad"]);
    run_skron(repo, ["commit-graph", "write", "--reachable"]);
}

fn git_with_fixed_env<const N: usize>(repo: &Path, args: [&str; N]) {
    let _ = command_output_with_env("git", repo, &args, &FIXED_ENV, "git");
}

fn skron_with_fixed_env<const N: usize>(repo: &Path, args: [&str; N]) {
    let _ = command_output_with_env(skron_bin(), repo, &args, &FIXED_ENV, "skron");
}
