mod common;

use tempfile::TempDir;

use common::{
    clone_repo_fixture, command_any_output, command_output_with_env, configure_identity, git,
    git_init, git_with_env, run_zmin, run_zmin_with_env, write_file, zmin_bin,
};

const SEQUENCER_ENV: [(&str, &str); 6] = [
    ("GIT_AUTHOR_NAME", "Bench"),
    ("GIT_AUTHOR_EMAIL", "bench@example.test"),
    ("GIT_AUTHOR_DATE", "1700000000 +0000"),
    ("GIT_COMMITTER_NAME", "Bench"),
    ("GIT_COMMITTER_EMAIL", "bench@example.test"),
    ("GIT_COMMITTER_DATE", "1700000000 +0000"),
];

fn normalize_rebase_progress(output: (i32, String, String)) -> (i32, String, String) {
    let mut segments = Vec::new();
    let mut saw_progress = false;
    for segment in output.2.split('\r') {
        if segment.starts_with("Rebasing (") {
            if !saw_progress {
                segments.push("Rebasing (<normalized>)");
                saw_progress = true;
            }
            continue;
        }
        segments.push(segment);
    }
    let stderr = segments.join("\r");
    (output.0, output.1, stderr)
}

fn sequencer_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["checkout", "-b", "feature"]);
    write_file(repo.path(), "feature.txt", "feature\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "feature"]);
    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "main.txt", "main\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);
    repo
}

fn bisect_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    for idx in 0..5 {
        write_file(repo.path(), "a.txt", &format!("{idx}\n"));
        git(repo.path(), ["add", "-A"]);
        git_with_env(repo.path(), ["commit", "-m", &format!("c{idx}")]);
    }
    repo
}

fn bisect_first_parent_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "0\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "c0"]);
    write_file(repo.path(), "a.txt", "1\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "c1"]);
    git(repo.path(), ["checkout", "-b", "side"]);
    for idx in 0..5 {
        write_file(repo.path(), &format!("side-{idx}.txt"), &format!("{idx}\n"));
        git(repo.path(), ["add", "-A"]);
        git_with_env(repo.path(), ["commit", "-m", &format!("side {idx}")]);
    }
    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "a.txt", "2\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "c2"]);
    git_with_env(
        repo.path(),
        ["merge", "--no-ff", "side", "-m", "merge side"],
    );
    repo
}

fn rebase_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["checkout", "-b", "topic"]);
    write_file(repo.path(), "topic.txt", "topic\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "topic"]);
    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "main.txt", "main\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);
    git(repo.path(), ["checkout", "topic"]);
    repo
}

fn rebase_onto_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["checkout", "-b", "oldbase"]);
    write_file(repo.path(), "oldbase.txt", "oldbase\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "oldbase"]);
    git(repo.path(), ["checkout", "-b", "topic"]);
    write_file(repo.path(), "topic.txt", "topic\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "topic"]);
    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "newbase.txt", "newbase\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "newbase"]);
    repo
}

fn rebase_merges_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["checkout", "-b", "side"]);
    write_file(repo.path(), "side.txt", "side\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "side"]);
    git(repo.path(), ["checkout", "-b", "topic", "main"]);
    write_file(repo.path(), "topic.txt", "topic\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "topic"]);
    git(repo.path(), ["merge", "side", "-m", "Merge side"]);
    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "main.txt", "main\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);
    repo
}

fn rebase_merges_onto_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["checkout", "-b", "oldbase"]);
    write_file(repo.path(), "oldbase.txt", "oldbase\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "oldbase"]);
    git(repo.path(), ["checkout", "-b", "side", "oldbase"]);
    write_file(repo.path(), "side.txt", "side\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "side"]);
    git(repo.path(), ["checkout", "-b", "topic", "oldbase"]);
    write_file(repo.path(), "topic.txt", "topic\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "topic"]);
    git(repo.path(), ["merge", "side", "-m", "Merge side"]);
    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "newbase.txt", "newbase\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "newbase"]);
    repo
}

fn cherry_pick_initially_empty_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["checkout", "-b", "feature"]);
    git_with_env(
        repo.path(),
        ["commit", "--allow-empty", "-m", "empty-feature"],
    );
    git(repo.path(), ["checkout", "main"]);
    repo
}

fn cherry_pick_becomes_empty_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["checkout", "-b", "feature"]);
    write_file(repo.path(), "base.txt", "feature\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "feature"]);
    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "base.txt", "feature\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main-applied-same-change"]);
    repo
}

#[test]
fn cherry_pick_and_revert_match_stock_git_for_clean_single_commit() {
    let git_repo = sequencer_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    let feature_commit = git(git_repo.path(), ["rev-parse", "feature"]);

    git(git_repo.path(), ["checkout", "main"]);
    git(zmin_repo.path(), ["checkout", "main"]);
    git_with_env(git_repo.path(), ["cherry-pick", &feature_commit]);
    run_zmin_with_env(zmin_repo.path(), ["cherry-pick", &feature_commit]);
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["log", "-1", "--format=%s"]),
        git(git_repo.path(), ["log", "-1", "--format=%s"])
    );
    assert_eq!(git(zmin_repo.path(), ["status", "--short"]), "");

    git_with_env(git_repo.path(), ["revert", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["revert", "HEAD"]);
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["log", "-1", "--format=%s"]),
        git(git_repo.path(), ["log", "-1", "--format=%s"])
    );
    assert_eq!(git(zmin_repo.path(), ["status", "--short"]), "");
}

#[test]
fn cherry_pick_and_revert_mainline_merge_match_stock_git() {
    let source = git_init();
    configure_identity(source.path());
    git(source.path(), ["checkout", "-b", "main"]);
    write_file(source.path(), "base.txt", "base\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "base"]);
    git(source.path(), ["checkout", "-b", "side"]);
    write_file(source.path(), "side.txt", "side\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "side"]);
    git(source.path(), ["checkout", "main"]);
    write_file(source.path(), "main.txt", "main\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "main"]);
    let main_parent = git(source.path(), ["rev-parse", "HEAD"]);
    git(
        source.path(),
        ["merge", "--no-ff", "side", "-m", "merge side"],
    );
    let merge_commit = git(source.path(), ["rev-parse", "HEAD"]);

    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    git(
        git_repo.path(),
        ["checkout", "-B", "pick-mainline", &main_parent],
    );
    git(
        zmin_repo.path(),
        ["checkout", "-B", "pick-mainline", &main_parent],
    );
    git_with_env(git_repo.path(), ["cherry-pick", "-m", "1", &merge_commit]);
    run_zmin_with_env(zmin_repo.path(), ["cherry-pick", "-m", "1", &merge_commit]);
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(git(zmin_repo.path(), ["status", "--short"]), "");

    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    git_with_env(git_repo.path(), ["revert", "-m", "1", &merge_commit]);
    run_zmin_with_env(zmin_repo.path(), ["revert", "-m", "1", &merge_commit]);
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(git(zmin_repo.path(), ["status", "--short"]), "");
}

#[test]
fn cherry_pick_and_revert_documented_surface_batch_matches_stock_git() {
    let source = sequencer_fixture_repo();
    let feature_commit = git(source.path(), ["rev-parse", "feature"]);
    let base_commit = git(source.path(), ["rev-parse", "main~1"]);

    for args in [
        ["cherry-pick", "-x", &feature_commit].as_slice(),
        ["cherry-pick", "-r", &feature_commit].as_slice(),
        ["cherry-pick", "--signoff", &feature_commit].as_slice(),
        ["cherry-pick", "-s", &feature_commit].as_slice(),
        ["cherry-pick", "--cleanup=strip", &feature_commit].as_slice(),
        ["cherry-pick", "--cleanup=scissors", &feature_commit].as_slice(),
        ["cherry-pick", "--rerere-autoupdate", &feature_commit].as_slice(),
        ["cherry-pick", "--no-rerere-autoupdate", &feature_commit].as_slice(),
        ["cherry-pick", "--strategy=ort", &feature_commit].as_slice(),
        ["cherry-pick", "-Xpatience", &feature_commit].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["checkout", "main"]);
        git(zmin_repo.path(), ["checkout", "main"]);
        assert_eq!(
            command_output_with_env(zmin_bin(), zmin_repo.path(), args, &SEQUENCER_ENV, "zmin"),
            command_output_with_env("git", git_repo.path(), args, &SEQUENCER_ENV, "git"),
            "args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["cat-file", "-p", "HEAD"]),
            git(git_repo.path(), ["cat-file", "-p", "HEAD"]),
            "args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
    }

    for args in [
        ["cherry-pick", "--edit", &feature_commit].as_slice(),
        ["cherry-pick", "-e", &feature_commit].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["checkout", "main"]);
        git(zmin_repo.path(), ["checkout", "main"]);
        let mut env = SEQUENCER_ENV.to_vec();
        env.push(("GIT_EDITOR", "true"));
        assert_eq!(
            command_output_with_env(zmin_bin(), zmin_repo.path(), args, &env, "zmin"),
            command_output_with_env("git", git_repo.path(), args, &env, "git"),
            "args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["cat-file", "-p", "HEAD"]),
            git(git_repo.path(), ["cat-file", "-p", "HEAD"]),
            "args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
    }

    {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(
            git_repo.path(),
            ["checkout", "-B", "ff-target", &base_commit],
        );
        git(
            zmin_repo.path(),
            ["checkout", "-B", "ff-target", &base_commit],
        );
        let args = ["cherry-pick", "--ff", &feature_commit];
        assert_eq!(
            command_output_with_env(zmin_bin(), zmin_repo.path(), &args, &SEQUENCER_ENV, "zmin"),
            command_output_with_env("git", git_repo.path(), &args, &SEQUENCER_ENV, "git")
        );
        assert_eq!(
            git(zmin_repo.path(), ["rev-parse", "HEAD"]),
            git(git_repo.path(), ["rev-parse", "HEAD"])
        );
        assert_eq!(
            git(zmin_repo.path(), ["cat-file", "-p", "HEAD"]),
            git(git_repo.path(), ["cat-file", "-p", "HEAD"])
        );
        assert_eq!(git(zmin_repo.path(), ["status", "--short"]), "");
    }

    for args in [
        ["revert", "-r", "HEAD"].as_slice(),
        ["revert", "--signoff", "HEAD"].as_slice(),
        ["revert", "-s", "HEAD"].as_slice(),
        ["revert", "--no-edit", "HEAD"].as_slice(),
        ["revert", "--cleanup=strip", "HEAD"].as_slice(),
        ["revert", "--cleanup=scissors", "HEAD"].as_slice(),
        ["revert", "--rerere-autoupdate", "HEAD"].as_slice(),
        ["revert", "--no-rerere-autoupdate", "HEAD"].as_slice(),
        ["revert", "--strategy=ort", "HEAD"].as_slice(),
        ["revert", "-Xpatience", "HEAD"].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["checkout", "feature"]);
        git(zmin_repo.path(), ["checkout", "feature"]);
        assert_eq!(
            command_output_with_env(zmin_bin(), zmin_repo.path(), args, &SEQUENCER_ENV, "zmin"),
            command_output_with_env("git", git_repo.path(), args, &SEQUENCER_ENV, "git"),
            "args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["cat-file", "-p", "HEAD"]),
            git(git_repo.path(), ["cat-file", "-p", "HEAD"]),
            "args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
    }

    for args in [
        ["revert", "--edit", "HEAD"].as_slice(),
        ["revert", "-e", "HEAD"].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["checkout", "feature"]);
        git(zmin_repo.path(), ["checkout", "feature"]);
        let mut env = SEQUENCER_ENV.to_vec();
        env.push(("GIT_EDITOR", "true"));
        assert_eq!(
            command_output_with_env(zmin_bin(), zmin_repo.path(), args, &env, "zmin"),
            command_output_with_env("git", git_repo.path(), args, &env, "git"),
            "args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["cat-file", "-p", "HEAD"]),
            git(git_repo.path(), ["cat-file", "-p", "HEAD"]),
            "args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
    }
}

#[test]
fn cherry_pick_and_revert_followup_documented_surface_batch_matches_stock_git() {
    let source = sequencer_fixture_repo();
    let feature_commit = git(source.path(), ["rev-parse", "feature"]);

    for args in [
        ["cherry-pick", "--allow-empty-message", &feature_commit].as_slice(),
        ["cherry-pick", "--strategy-option=patience", &feature_commit].as_slice(),
        ["revert", "--reference", "HEAD"].as_slice(),
        ["revert", "--strategy-option=patience", "HEAD"].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["checkout", "main"]);
        git(zmin_repo.path(), ["checkout", "main"]);
        assert_eq!(
            command_output_with_env("git", git_repo.path(), args, &SEQUENCER_ENV, "git"),
            command_output_with_env(zmin_bin(), zmin_repo.path(), args, &SEQUENCER_ENV, "zmin"),
            "args: {args:?}"
        );
        assert_eq!(
            git(git_repo.path(), ["cat-file", "-p", "HEAD"]),
            git(zmin_repo.path(), ["cat-file", "-p", "HEAD"]),
            "args: {args:?}"
        );
        assert_eq!(
            git(git_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
    }

    let source = cherry_pick_initially_empty_fixture_repo();
    let empty_commit = git(source.path(), ["rev-parse", "feature"]);
    for args in [
        ["cherry-pick", "--allow-empty", &empty_commit].as_slice(),
        ["cherry-pick", "--keep-redundant-commits", &empty_commit].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["checkout", "main"]);
        git(zmin_repo.path(), ["checkout", "main"]);
        assert_eq!(
            command_output_with_env("git", git_repo.path(), args, &SEQUENCER_ENV, "git"),
            command_output_with_env(zmin_bin(), zmin_repo.path(), args, &SEQUENCER_ENV, "zmin"),
            "args: {args:?}"
        );
        assert_eq!(
            git(git_repo.path(), ["cat-file", "-p", "HEAD"]),
            git(zmin_repo.path(), ["cat-file", "-p", "HEAD"]),
            "args: {args:?}"
        );
        assert_eq!(
            git(git_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
    }

    let source = cherry_pick_becomes_empty_fixture_repo();
    let feature_commit = git(source.path(), ["rev-parse", "feature"]);
    let args = ["cherry-pick", "--empty=keep", &feature_commit];
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    git(git_repo.path(), ["checkout", "main"]);
    git(zmin_repo.path(), ["checkout", "main"]);
    assert_eq!(
        command_output_with_env("git", git_repo.path(), &args, &SEQUENCER_ENV, "git"),
        command_output_with_env(zmin_bin(), zmin_repo.path(), &args, &SEQUENCER_ENV, "zmin"),
    );
    assert_eq!(
        git(git_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(zmin_repo.path(), ["cat-file", "-p", "HEAD"])
    );
    assert_eq!(git(git_repo.path(), ["status", "--short"]), "");
    assert_eq!(git(zmin_repo.path(), ["status", "--short"]), "");
}

#[test]
fn cherry_pick_and_revert_expansion_batch_matches_stock_git() {
    let source = sequencer_fixture_repo();
    let feature_commit = git(source.path(), ["rev-parse", "feature"]);

    for args in [
        ["cherry-pick", "--signoff", "-s", &feature_commit].as_slice(),
        [
            "cherry-pick",
            "--rerere-autoupdate",
            "--no-rerere-autoupdate",
            &feature_commit,
        ]
        .as_slice(),
        ["cherry-pick", "-x", "-r", &feature_commit].as_slice(),
        [
            "cherry-pick",
            "--strategy",
            "ort",
            "--strategy-option",
            "patience",
            &feature_commit,
        ]
        .as_slice(),
        ["cherry-pick", "-X", "patience", &feature_commit].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["checkout", "main"]);
        git(zmin_repo.path(), ["checkout", "main"]);
        assert_eq!(
            command_output_with_env("git", git_repo.path(), args, &SEQUENCER_ENV, "git"),
            command_output_with_env(zmin_bin(), zmin_repo.path(), args, &SEQUENCER_ENV, "zmin"),
            "args: {args:?}"
        );
        assert_eq!(
            git(git_repo.path(), ["cat-file", "-p", "HEAD"]),
            git(zmin_repo.path(), ["cat-file", "-p", "HEAD"]),
            "args: {args:?}"
        );
        assert_eq!(
            git(git_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
    }

    for args in [["cherry-pick", "--edit", "-e", &feature_commit].as_slice()] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["checkout", "main"]);
        git(zmin_repo.path(), ["checkout", "main"]);
        let mut env = SEQUENCER_ENV.to_vec();
        env.push(("GIT_EDITOR", "true"));
        assert_eq!(
            command_output_with_env("git", git_repo.path(), args, &env, "git"),
            command_output_with_env(zmin_bin(), zmin_repo.path(), args, &env, "zmin"),
            "args: {args:?}"
        );
        assert_eq!(
            git(git_repo.path(), ["cat-file", "-p", "HEAD"]),
            git(zmin_repo.path(), ["cat-file", "-p", "HEAD"]),
            "args: {args:?}"
        );
        assert_eq!(
            git(git_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
    }

    let source = cherry_pick_initially_empty_fixture_repo();
    let empty_commit = git(source.path(), ["rev-parse", "feature"]);
    let args = [
        "cherry-pick",
        "--allow-empty",
        "--keep-redundant-commits",
        &empty_commit,
    ];
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    git(git_repo.path(), ["checkout", "main"]);
    git(zmin_repo.path(), ["checkout", "main"]);
    assert_eq!(
        command_output_with_env("git", git_repo.path(), &args, &SEQUENCER_ENV, "git"),
        command_output_with_env(zmin_bin(), zmin_repo.path(), &args, &SEQUENCER_ENV, "zmin"),
    );
    assert_eq!(
        git(git_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(zmin_repo.path(), ["cat-file", "-p", "HEAD"])
    );
    assert_eq!(git(git_repo.path(), ["status", "--short"]), "");
    assert_eq!(git(zmin_repo.path(), ["status", "--short"]), "");

    let source = sequencer_fixture_repo();
    for args in [
        ["revert", "--signoff", "-s", "HEAD"].as_slice(),
        [
            "revert",
            "--rerere-autoupdate",
            "--no-rerere-autoupdate",
            "HEAD",
        ]
        .as_slice(),
        [
            "revert",
            "--strategy",
            "ort",
            "--strategy-option",
            "patience",
            "HEAD",
        ]
        .as_slice(),
        ["revert", "-X", "patience", "HEAD"].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["checkout", "feature"]);
        git(zmin_repo.path(), ["checkout", "feature"]);
        assert_eq!(
            command_output_with_env("git", git_repo.path(), args, &SEQUENCER_ENV, "git"),
            command_output_with_env(zmin_bin(), zmin_repo.path(), args, &SEQUENCER_ENV, "zmin"),
            "args: {args:?}"
        );
        assert_eq!(
            git(git_repo.path(), ["cat-file", "-p", "HEAD"]),
            git(zmin_repo.path(), ["cat-file", "-p", "HEAD"]),
            "args: {args:?}"
        );
        assert_eq!(
            git(git_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
    }

    for args in [
        ["revert", "--edit", "-e", "HEAD"].as_slice(),
        ["revert", "--no-edit", "--edit", "HEAD"].as_slice(),
        ["revert", "--reference", "--no-edit", "HEAD"].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["checkout", "feature"]);
        git(zmin_repo.path(), ["checkout", "feature"]);
        let mut env = SEQUENCER_ENV.to_vec();
        env.push(("GIT_EDITOR", "true"));
        assert_eq!(
            command_output_with_env("git", git_repo.path(), args, &env, "git"),
            command_output_with_env(zmin_bin(), zmin_repo.path(), args, &env, "zmin"),
            "args: {args:?}"
        );
        assert_eq!(
            git(git_repo.path(), ["cat-file", "-p", "HEAD"]),
            git(zmin_repo.path(), ["cat-file", "-p", "HEAD"]),
            "args: {args:?}"
        );
        assert_eq!(
            git(git_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            "",
            "args: {args:?}"
        );
    }
}

#[test]
fn bisect_matches_stock_git_for_linear_history() {
    let git_repo = bisect_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_zmin(zmin_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
    git(git_repo.path(), ["bisect", "good"]);
    run_zmin(zmin_repo.path(), ["bisect", "good"]);
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );

    git(git_repo.path(), ["bisect", "bad"]);
    run_zmin(zmin_repo.path(), ["bisect", "bad"]);
    let zmin_log = run_zmin(zmin_repo.path(), ["bisect", "log"]);
    assert!(zmin_log.contains("git bisect good"));
    assert!(zmin_log.contains("git bisect bad"));

    git(git_repo.path(), ["bisect", "reset"]);
    run_zmin(zmin_repo.path(), ["bisect", "reset"]);
    assert_eq!(
        git(zmin_repo.path(), ["symbolic-ref", "--short", "HEAD"]),
        git(git_repo.path(), ["symbolic-ref", "--short", "HEAD"])
    );
}

#[test]
fn bisect_terms_next_skip_and_replay_match_stock_git_state() {
    let git_repo = bisect_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    assert_eq!(
        command_any_output("git", git_repo.path(), &["bisect", "terms"], "git"),
        command_any_output(zmin_bin(), zmin_repo.path(), &["bisect", "terms"], "zmin")
    );

    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_zmin(zmin_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    assert_eq!(
        git(git_repo.path(), ["bisect", "terms"]),
        run_zmin(zmin_repo.path(), ["bisect", "terms"])
    );
    git(git_repo.path(), ["bisect", "skip"]);
    run_zmin(zmin_repo.path(), ["bisect", "skip"]);
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
    git(git_repo.path(), ["bisect", "next"]);
    run_zmin(zmin_repo.path(), ["bisect", "next"]);
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
    let zmin_log = run_zmin(zmin_repo.path(), ["bisect", "log"]);
    assert!(zmin_log.contains("git bisect skip"));

    let replay_source = bisect_fixture_repo();
    let replay_log_source = clone_repo_fixture(replay_source.path());
    let replay_git = clone_repo_fixture(replay_source.path());
    let replay_zmin = clone_repo_fixture(replay_source.path());
    git(
        replay_log_source.path(),
        ["bisect", "start", "HEAD", "HEAD~4"],
    );
    let log = git(replay_log_source.path(), ["bisect", "log"]);
    std::fs::write(replay_git.path().join("bisect.log"), &log).expect("write git replay log");
    std::fs::write(replay_zmin.path().join("bisect.log"), &log).expect("write zmin replay log");
    git(replay_git.path(), ["bisect", "replay", "bisect.log"]);
    run_zmin(replay_zmin.path(), ["bisect", "replay", "bisect.log"]);
    assert!(run_zmin(replay_zmin.path(), ["bisect", "log"]).contains("git bisect start"));
}

#[test]
fn bisect_custom_terms_and_skip_range_match_stock_git_state() {
    let git_repo = bisect_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    git(
        git_repo.path(),
        [
            "bisect",
            "start",
            "--term-old=fixed",
            "--term-new=broken",
            "HEAD",
            "HEAD~4",
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "bisect",
            "start",
            "--term-old=fixed",
            "--term-new=broken",
            "HEAD",
            "HEAD~4",
        ],
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["bisect", "terms"]),
        git(git_repo.path(), ["bisect", "terms"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["bisect", "terms", "--term-good"]),
        git(git_repo.path(), ["bisect", "terms", "--term-good"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["bisect", "terms", "--term-bad"]),
        git(git_repo.path(), ["bisect", "terms", "--term-bad"])
    );
    git(git_repo.path(), ["bisect", "fixed"]);
    run_zmin(zmin_repo.path(), ["bisect", "fixed"]);
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );

    let git_repo = bisect_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_zmin(zmin_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    git(git_repo.path(), ["bisect", "skip", "HEAD~2..HEAD"]);
    run_zmin(zmin_repo.path(), ["bisect", "skip", "HEAD~2..HEAD"]);
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
}

#[test]
#[cfg(not(windows))]
fn bisect_run_marks_commits_from_command_exit_code() {
    let repo = bisect_fixture_repo();
    run_zmin(repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_zmin(
        repo.path(),
        ["bisect", "run", "sh", "-c", "test \"$(cat a.txt)\" -lt 3"],
    );
    let log = run_zmin(repo.path(), ["bisect", "log"]);
    assert!(log.contains("git bisect good"));
    assert!(log.contains("git bisect bad"));
}

#[test]
#[cfg(not(windows))]
fn bisect_run_aborts_on_exit_code_128_like_stock_git() {
    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_zmin(zmin_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);

    let git_output = command_any_output(
        "git",
        git_repo.path(),
        &["bisect", "run", "sh", "-c", "exit 128"],
        "git",
    );
    let zmin_output = command_any_output(
        zmin_bin(),
        zmin_repo.path(),
        &["bisect", "run", "sh", "-c", "exit 128"],
        "zmin",
    );

    assert_eq!(zmin_output, git_output);
    let zmin_log = run_zmin(zmin_repo.path(), ["bisect", "log"]);
    assert!(zmin_log.contains("git bisect start"));
    assert!(!zmin_log.contains("git bisect good"));
    assert!(!zmin_log.contains("git bisect bad"));
    assert!(!zmin_log.contains("git bisect skip"));
}

#[test]
#[cfg(not(windows))]
fn bisect_run_detects_bogus_126_on_known_good_like_stock_git() {
    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_zmin(zmin_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);

    let git_output = command_any_output(
        "git",
        git_repo.path(),
        &["bisect", "run", "sh", "-c", "exit 126"],
        "git",
    );
    let zmin_output = command_any_output(
        zmin_bin(),
        zmin_repo.path(),
        &["bisect", "run", "sh", "-c", "exit 126"],
        "zmin",
    );

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.2, git_output.2);
    assert!(zmin_output.1.contains("running 'sh' '-c' 'exit 126'"));
    assert!(
        run_zmin(zmin_repo.path(), ["bisect", "log"]).contains("git bisect bad"),
        "first run should still mark the tested commit bad before known-good validation"
    );
}

#[test]
fn bisect_skip_reports_skipped_only_candidates_like_stock_git() {
    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~2"]);
    run_zmin(zmin_repo.path(), ["bisect", "start", "HEAD", "HEAD~2"]);

    let git_output = command_any_output("git", git_repo.path(), &["bisect", "skip"], "git");
    let zmin_output = command_any_output(zmin_bin(), zmin_repo.path(), &["bisect", "skip"], "zmin");

    assert_eq!(zmin_output, git_output);
    let zmin_log = run_zmin(zmin_repo.path(), ["bisect", "log"]);
    assert!(zmin_log.contains("# only skipped commits left to test"));
    assert!(zmin_log.contains("# possible first bad commit:"));
}

#[test]
fn bisect_help_view_no_checkout_and_pathspec_cover_stable_modes() {
    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());

    assert_eq!(
        command_any_output("git", git_repo.path(), &["bisect", "-h"], "git"),
        command_any_output(zmin_bin(), zmin_repo.path(), &["bisect", "-h"], "zmin")
    );
    assert_eq!(
        command_any_output("git", git_repo.path(), &["bisect", "help"], "git"),
        command_any_output(zmin_bin(), zmin_repo.path(), &["bisect", "help"], "zmin")
    );

    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_zmin(zmin_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    assert_eq!(
        command_any_output("git", git_repo.path(), &["bisect", "help"], "git"),
        command_any_output(zmin_bin(), zmin_repo.path(), &["bisect", "help"], "zmin")
    );
    assert_eq!(
        command_any_output(
            "git",
            git_repo.path(),
            &["bisect", "view", "--oneline"],
            "git"
        ),
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &["bisect", "view", "--oneline"],
            "zmin"
        )
    );

    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    let git_head = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let zmin_head = git(zmin_repo.path(), ["rev-parse", "HEAD"]);
    git(
        git_repo.path(),
        ["bisect", "start", "--no-checkout", "HEAD", "HEAD~4"],
    );
    run_zmin(
        zmin_repo.path(),
        ["bisect", "start", "--no-checkout", "HEAD", "HEAD~4"],
    );
    assert_eq!(git(git_repo.path(), ["rev-parse", "HEAD"]), git_head);
    assert_eq!(git(zmin_repo.path(), ["rev-parse", "HEAD"]), zmin_head);
    assert_eq!(
        std::fs::read_to_string(git_repo.path().join(".git/BISECT_HEAD")).expect("git BISECT_HEAD"),
        std::fs::read_to_string(zmin_repo.path().join(".git/BISECT_HEAD"))
            .expect("zmin BISECT_HEAD")
    );
    git(git_repo.path(), ["bisect", "skip"]);
    run_zmin(zmin_repo.path(), ["bisect", "skip"]);
    assert_eq!(
        std::fs::read_to_string(git_repo.path().join(".git/BISECT_HEAD"))
            .expect("git BISECT_HEAD after skip"),
        std::fs::read_to_string(zmin_repo.path().join(".git/BISECT_HEAD"))
            .expect("zmin BISECT_HEAD after skip")
    );

    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    git(
        git_repo.path(),
        ["bisect", "start", "HEAD", "HEAD~4", "--", "a.txt"],
    );
    run_zmin(
        zmin_repo.path(),
        ["bisect", "start", "HEAD", "HEAD~4", "--", "a.txt"],
    );
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );

    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    let git_output = command_any_output(
        "git",
        git_repo.path(),
        &["bisect", "start", "HEAD", "HEAD~4", "--", "missing.txt"],
        "git",
    );
    let zmin_output = command_any_output(
        zmin_bin(),
        zmin_repo.path(),
        &["bisect", "start", "HEAD", "HEAD~4", "--", "missing.txt"],
        "zmin",
    );
    assert_eq!(zmin_output, git_output);
    assert!(!zmin_repo.path().join(".git/BISECT_START").exists());
}

#[test]
fn bisect_first_parent_limits_candidates_to_first_parent_chain() {
    let source = bisect_first_parent_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());

    git(
        git_repo.path(),
        ["bisect", "start", "--first-parent", "HEAD", "HEAD~2"],
    );
    run_zmin(
        zmin_repo.path(),
        ["bisect", "start", "--first-parent", "HEAD", "HEAD~2"],
    );
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
    assert_eq!(
        std::fs::read_to_string(zmin_repo.path().join(".git/BISECT_FIRST_PARENT"))
            .expect("zmin first-parent state"),
        "1\n"
    );
}

#[test]
fn rebase_replays_linear_topic_like_stock_git() {
    let source = rebase_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    git_with_env(git_repo.path(), ["rebase", "origin/main"]);
    run_zmin_with_env(zmin_repo.path(), ["rebase", "origin/main"]);
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_repo.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["log", "--format=%s", "--max-count=3"]),
        git(git_repo.path(), ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(git(zmin_repo.path(), ["status", "--short"]), "");
}

#[test]
fn rebase_uses_configured_upstream_like_stock_git() {
    let source = rebase_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    git(git_repo.path(), ["branch", "-u", "origin/main"]);
    git(zmin_repo.path(), ["branch", "-u", "origin/main"]);

    git_with_env(git_repo.path(), ["rebase"]);
    run_zmin_with_env(zmin_repo.path(), ["rebase"]);
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_repo.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["log", "--format=%s", "--max-count=3"]),
        git(git_repo.path(), ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(git(zmin_repo.path(), ["status", "--short"]), "");
}

#[test]
fn rebase_with_branch_argument_checks_out_and_replays_like_stock_git() {
    let source = rebase_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    git(git_repo.path(), ["checkout", "main"]);
    git(zmin_repo.path(), ["checkout", "main"]);

    git_with_env(git_repo.path(), ["rebase", "origin/main", "topic"]);
    run_zmin_with_env(zmin_repo.path(), ["rebase", "origin/main", "topic"]);
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_repo.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["log", "--format=%s", "--max-count=3"]),
        git(git_repo.path(), ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["symbolic-ref", "--short", "HEAD"]),
        git(git_repo.path(), ["symbolic-ref", "--short", "HEAD"])
    );
    assert_eq!(git(zmin_repo.path(), ["status", "--short"]), "");
}

#[test]
fn rebase_onto_replays_topic_like_stock_git() {
    let source = rebase_onto_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());
    git(git_repo.path(), ["checkout", "-B", "topic", "origin/topic"]);
    git(
        zmin_repo.path(),
        ["checkout", "-B", "topic", "origin/topic"],
    );

    git_with_env(
        git_repo.path(),
        ["rebase", "--onto", "origin/main", "origin/oldbase", "topic"],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["rebase", "--onto", "origin/main", "origin/oldbase", "topic"],
    );
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_repo.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["log", "--format=%s", "--max-count=3"]),
        git(git_repo.path(), ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["symbolic-ref", "--short", "HEAD"]),
        git(git_repo.path(), ["symbolic-ref", "--short", "HEAD"])
    );
    assert_eq!(git(zmin_repo.path(), ["status", "--short"]), "");
}

#[test]
fn rebase_merge_backend_option_family_matches_stock_git() {
    let cases: [(&str, &[&str], bool, bool); 12] = [
        (
            "merge_short_upstream",
            &["rebase", "-m", "origin/main"],
            false,
            false,
        ),
        (
            "merge_long_upstream",
            &["rebase", "--merge", "origin/main"],
            false,
            false,
        ),
        (
            "no_stat_short_upstream",
            &["rebase", "-n", "origin/main"],
            false,
            false,
        ),
        (
            "no_stat_long_upstream",
            &["rebase", "--no-stat", "origin/main"],
            false,
            false,
        ),
        (
            "merge_short_branch",
            &["rebase", "-m", "origin/main", "topic"],
            true,
            false,
        ),
        (
            "merge_long_branch",
            &["rebase", "--merge", "origin/main", "topic"],
            true,
            false,
        ),
        (
            "no_stat_short_branch",
            &["rebase", "-n", "origin/main", "topic"],
            true,
            false,
        ),
        (
            "no_stat_long_branch",
            &["rebase", "--no-stat", "origin/main", "topic"],
            true,
            false,
        ),
        (
            "merge_short_onto",
            &[
                "rebase",
                "-m",
                "--onto",
                "origin/main",
                "origin/oldbase",
                "topic",
            ],
            false,
            true,
        ),
        (
            "merge_long_onto",
            &[
                "rebase",
                "--merge",
                "--onto",
                "origin/main",
                "origin/oldbase",
                "topic",
            ],
            false,
            true,
        ),
        (
            "no_stat_short_onto",
            &[
                "rebase",
                "-n",
                "--onto",
                "origin/main",
                "origin/oldbase",
                "topic",
            ],
            false,
            true,
        ),
        (
            "no_stat_long_onto",
            &[
                "rebase",
                "--no-stat",
                "--onto",
                "origin/main",
                "origin/oldbase",
                "topic",
            ],
            false,
            true,
        ),
    ];

    for (name, args, checkout_main, onto_fixture) in cases {
        let source = if onto_fixture {
            rebase_onto_fixture_repo()
        } else {
            rebase_fixture_repo()
        };
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        if onto_fixture {
            git(git_repo.path(), ["checkout", "-B", "topic", "origin/topic"]);
            git(
                zmin_repo.path(),
                ["checkout", "-B", "topic", "origin/topic"],
            );
        } else if checkout_main {
            git(git_repo.path(), ["checkout", "main"]);
            git(zmin_repo.path(), ["checkout", "main"]);
        }

        let git_output = command_output_with_env("git", git_repo.path(), args, &SEQUENCER_ENV, "git");
        let zmin_output =
            command_output_with_env(zmin_bin(), zmin_repo.path(), args, &SEQUENCER_ENV, "zmin");

        assert_eq!(zmin_output, git_output, "{name} output");
        assert_eq!(
            git(zmin_repo.path(), ["rev-parse", "HEAD^{tree}"]),
            git(git_repo.path(), ["rev-parse", "HEAD^{tree}"]),
            "{name} tree"
        );
        assert_eq!(
            git(zmin_repo.path(), ["log", "--format=%s", "--max-count=3"]),
            git(git_repo.path(), ["log", "--format=%s", "--max-count=3"]),
            "{name} log"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            git(git_repo.path(), ["status", "--short"]),
            "{name} status"
        );
    }
}

#[test]
fn rebase_quiet_option_family_matches_stock_git() {
    let cases: [(&str, &[&str], bool, bool); 6] = [
        (
            "quiet_short_upstream",
            &["rebase", "-q", "origin/main"],
            false,
            false,
        ),
        (
            "quiet_long_upstream",
            &["rebase", "--quiet", "origin/main"],
            false,
            false,
        ),
        (
            "quiet_short_branch",
            &["rebase", "-q", "origin/main", "topic"],
            true,
            false,
        ),
        (
            "quiet_long_branch",
            &["rebase", "--quiet", "origin/main", "topic"],
            true,
            false,
        ),
        (
            "quiet_short_onto",
            &[
                "rebase",
                "-q",
                "--onto",
                "origin/main",
                "origin/oldbase",
                "topic",
            ],
            false,
            true,
        ),
        (
            "quiet_long_onto",
            &[
                "rebase",
                "--quiet",
                "--onto",
                "origin/main",
                "origin/oldbase",
                "topic",
            ],
            false,
            true,
        ),
    ];

    for (name, args, checkout_main, onto_fixture) in cases {
        let source = if onto_fixture {
            rebase_onto_fixture_repo()
        } else {
            rebase_fixture_repo()
        };
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        if onto_fixture {
            git(git_repo.path(), ["checkout", "-B", "topic", "origin/topic"]);
            git(
                zmin_repo.path(),
                ["checkout", "-B", "topic", "origin/topic"],
            );
        } else if checkout_main {
            git(git_repo.path(), ["checkout", "main"]);
            git(zmin_repo.path(), ["checkout", "main"]);
        }

        let git_output = command_output_with_env("git", git_repo.path(), args, &SEQUENCER_ENV, "git");
        let zmin_output =
            command_output_with_env(zmin_bin(), zmin_repo.path(), args, &SEQUENCER_ENV, "zmin");

        assert_eq!(zmin_output, git_output, "{name} output");
        assert_eq!(
            git(zmin_repo.path(), ["rev-parse", "HEAD^{tree}"]),
            git(git_repo.path(), ["rev-parse", "HEAD^{tree}"]),
            "{name} tree"
        );
        assert_eq!(
            git(zmin_repo.path(), ["log", "--format=%s", "--max-count=3"]),
            git(git_repo.path(), ["log", "--format=%s", "--max-count=3"]),
            "{name} log"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            git(git_repo.path(), ["status", "--short"]),
            "{name} status"
        );
    }
}

#[test]
fn rebase_merge_topology_option_family_matches_stock_git() {
    let cases: [(&str, &[&str], bool, bool); 9] = [
        (
            "rebase_merges_short_upstream",
            &["rebase", "-r", "origin/main"],
            false,
            false,
        ),
        (
            "rebase_merges_long_upstream",
            &["rebase", "--rebase-merges", "origin/main"],
            false,
            false,
        ),
        (
            "no_rebase_merges_upstream",
            &["rebase", "--no-rebase-merges", "origin/main"],
            false,
            false,
        ),
        (
            "rebase_merges_short_branch",
            &["rebase", "-r", "origin/main", "topic"],
            true,
            false,
        ),
        (
            "rebase_merges_long_branch",
            &["rebase", "--rebase-merges", "origin/main", "topic"],
            true,
            false,
        ),
        (
            "no_rebase_merges_branch",
            &["rebase", "--no-rebase-merges", "origin/main", "topic"],
            true,
            false,
        ),
        (
            "rebase_merges_short_onto",
            &[
                "rebase",
                "-r",
                "--onto",
                "origin/main",
                "origin/oldbase",
                "topic",
            ],
            false,
            true,
        ),
        (
            "rebase_merges_long_onto",
            &[
                "rebase",
                "--rebase-merges",
                "--onto",
                "origin/main",
                "origin/oldbase",
                "topic",
            ],
            false,
            true,
        ),
        (
            "no_rebase_merges_onto",
            &[
                "rebase",
                "--no-rebase-merges",
                "--onto",
                "origin/main",
                "origin/oldbase",
                "topic",
            ],
            false,
            true,
        ),
    ];

    for (name, args, checkout_main, onto_fixture) in cases {
        let source = if onto_fixture {
            rebase_merges_onto_fixture_repo()
        } else {
            rebase_merges_fixture_repo()
        };
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["checkout", "-B", "topic", "origin/topic"]);
        git(
            zmin_repo.path(),
            ["checkout", "-B", "topic", "origin/topic"],
        );
        if checkout_main {
            git(git_repo.path(), ["checkout", "main"]);
            git(zmin_repo.path(), ["checkout", "main"]);
        }

        let git_output = normalize_rebase_progress(command_output_with_env(
            "git",
            git_repo.path(),
            args,
            &SEQUENCER_ENV,
            "git",
        ));
        let zmin_output = normalize_rebase_progress(command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            args,
            &SEQUENCER_ENV,
            "zmin",
        ));

        assert_eq!(zmin_output, git_output, "{name} output");
        assert_eq!(
            git(zmin_repo.path(), ["rev-parse", "HEAD^{tree}"]),
            git(git_repo.path(), ["rev-parse", "HEAD^{tree}"]),
            "{name} tree"
        );
        assert_eq!(
            git(zmin_repo.path(), ["log", "--format=%s", "--max-count=4"]),
            git(git_repo.path(), ["log", "--format=%s", "--max-count=4"]),
            "{name} log"
        );
        assert_eq!(
            git(
                zmin_repo.path(),
                ["rev-list", "--parents", "--max-count=4", "HEAD"],
            ),
            git(
                git_repo.path(),
                ["rev-list", "--parents", "--max-count=4", "HEAD"],
            ),
            "{name} parents"
        );
        assert_eq!(
            git(zmin_repo.path(), ["symbolic-ref", "--short", "HEAD"]),
            git(git_repo.path(), ["symbolic-ref", "--short", "HEAD"]),
            "{name} branch"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            git(git_repo.path(), ["status", "--short"]),
            "{name} status"
        );
    }
}
