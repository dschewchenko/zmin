mod common;

use tempfile::TempDir;

use common::{
    clone_repo_fixture, command_any_output, configure_identity, git, git_init, git_with_env,
    run_skron, run_skron_with_env, skron_bin, write_file,
};

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

#[test]
fn cherry_pick_and_revert_match_stock_git_for_clean_single_commit() {
    let git_repo = sequencer_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    let feature_commit = git(git_repo.path(), ["rev-parse", "feature"]);

    git(git_repo.path(), ["checkout", "main"]);
    git(skron_repo.path(), ["checkout", "main"]);
    git_with_env(git_repo.path(), ["cherry-pick", &feature_commit]);
    run_skron_with_env(skron_repo.path(), ["cherry-pick", &feature_commit]);
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(skron_repo.path(), ["log", "-1", "--format=%s"]),
        git(git_repo.path(), ["log", "-1", "--format=%s"])
    );
    assert_eq!(git(skron_repo.path(), ["status", "--short"]), "");

    git_with_env(git_repo.path(), ["revert", "HEAD"]);
    run_skron_with_env(skron_repo.path(), ["revert", "HEAD"]);
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(skron_repo.path(), ["log", "-1", "--format=%s"]),
        git(git_repo.path(), ["log", "-1", "--format=%s"])
    );
    assert_eq!(git(skron_repo.path(), ["status", "--short"]), "");
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
    let skron_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());
    git(
        git_repo.path(),
        ["checkout", "-B", "pick-mainline", &main_parent],
    );
    git(
        skron_repo.path(),
        ["checkout", "-B", "pick-mainline", &main_parent],
    );
    git_with_env(git_repo.path(), ["cherry-pick", "-m", "1", &merge_commit]);
    run_skron_with_env(skron_repo.path(), ["cherry-pick", "-m", "1", &merge_commit]);
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(git(skron_repo.path(), ["status", "--short"]), "");

    let git_repo = clone_repo_fixture(source.path());
    let skron_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());
    git_with_env(git_repo.path(), ["revert", "-m", "1", &merge_commit]);
    run_skron_with_env(skron_repo.path(), ["revert", "-m", "1", &merge_commit]);
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(git(skron_repo.path(), ["status", "--short"]), "");
}

#[test]
fn bisect_matches_stock_git_for_linear_history() {
    let git_repo = bisect_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());

    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_skron(skron_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
    git(git_repo.path(), ["bisect", "good"]);
    run_skron(skron_repo.path(), ["bisect", "good"]);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );

    git(git_repo.path(), ["bisect", "bad"]);
    run_skron(skron_repo.path(), ["bisect", "bad"]);
    let skron_log = run_skron(skron_repo.path(), ["bisect", "log"]);
    assert!(skron_log.contains("git bisect good"));
    assert!(skron_log.contains("git bisect bad"));

    git(git_repo.path(), ["bisect", "reset"]);
    run_skron(skron_repo.path(), ["bisect", "reset"]);
    assert_eq!(
        git(skron_repo.path(), ["symbolic-ref", "--short", "HEAD"]),
        git(git_repo.path(), ["symbolic-ref", "--short", "HEAD"])
    );
}

#[test]
fn bisect_terms_next_skip_and_replay_match_stock_git_state() {
    let git_repo = bisect_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());

    assert_eq!(
        command_any_output("git", git_repo.path(), &["bisect", "terms"], "git"),
        command_any_output(
            skron_bin(),
            skron_repo.path(),
            &["bisect", "terms"],
            "skron"
        )
    );

    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_skron(skron_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    assert_eq!(
        git(git_repo.path(), ["bisect", "terms"]),
        run_skron(skron_repo.path(), ["bisect", "terms"])
    );
    git(git_repo.path(), ["bisect", "skip"]);
    run_skron(skron_repo.path(), ["bisect", "skip"]);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
    git(git_repo.path(), ["bisect", "next"]);
    run_skron(skron_repo.path(), ["bisect", "next"]);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
    let skron_log = run_skron(skron_repo.path(), ["bisect", "log"]);
    assert!(skron_log.contains("git bisect skip"));

    let replay_source = bisect_fixture_repo();
    let replay_log_source = clone_repo_fixture(replay_source.path());
    let replay_git = clone_repo_fixture(replay_source.path());
    let replay_skron = clone_repo_fixture(replay_source.path());
    git(
        replay_log_source.path(),
        ["bisect", "start", "HEAD", "HEAD~4"],
    );
    let log = git(replay_log_source.path(), ["bisect", "log"]);
    std::fs::write(replay_git.path().join("bisect.log"), &log).expect("write git replay log");
    std::fs::write(replay_skron.path().join("bisect.log"), &log).expect("write skron replay log");
    git(replay_git.path(), ["bisect", "replay", "bisect.log"]);
    run_skron(replay_skron.path(), ["bisect", "replay", "bisect.log"]);
    assert!(run_skron(replay_skron.path(), ["bisect", "log"]).contains("git bisect start"));
}

#[test]
fn bisect_custom_terms_and_skip_range_match_stock_git_state() {
    let git_repo = bisect_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());

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
    run_skron(
        skron_repo.path(),
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
        run_skron(skron_repo.path(), ["bisect", "terms"]),
        git(git_repo.path(), ["bisect", "terms"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["bisect", "terms", "--term-good"]),
        git(git_repo.path(), ["bisect", "terms", "--term-good"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["bisect", "terms", "--term-bad"]),
        git(git_repo.path(), ["bisect", "terms", "--term-bad"])
    );
    git(git_repo.path(), ["bisect", "fixed"]);
    run_skron(skron_repo.path(), ["bisect", "fixed"]);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );

    let git_repo = bisect_fixture_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_skron(skron_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    git(git_repo.path(), ["bisect", "skip", "HEAD~2..HEAD"]);
    run_skron(skron_repo.path(), ["bisect", "skip", "HEAD~2..HEAD"]);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
}

#[test]
#[cfg(not(windows))]
fn bisect_run_marks_commits_from_command_exit_code() {
    let repo = bisect_fixture_repo();
    run_skron(repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_skron(
        repo.path(),
        ["bisect", "run", "sh", "-c", "test \"$(cat a.txt)\" -lt 3"],
    );
    let log = run_skron(repo.path(), ["bisect", "log"]);
    assert!(log.contains("git bisect good"));
    assert!(log.contains("git bisect bad"));
}

#[test]
#[cfg(not(windows))]
fn bisect_run_aborts_on_exit_code_128_like_stock_git() {
    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let skron_repo = clone_repo_fixture(source.path());
    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_skron(skron_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);

    let git_output = command_any_output(
        "git",
        git_repo.path(),
        &["bisect", "run", "sh", "-c", "exit 128"],
        "git",
    );
    let skron_output = command_any_output(
        skron_bin(),
        skron_repo.path(),
        &["bisect", "run", "sh", "-c", "exit 128"],
        "skron",
    );

    assert_eq!(skron_output, git_output);
    let skron_log = run_skron(skron_repo.path(), ["bisect", "log"]);
    assert!(skron_log.contains("git bisect start"));
    assert!(!skron_log.contains("git bisect good"));
    assert!(!skron_log.contains("git bisect bad"));
    assert!(!skron_log.contains("git bisect skip"));
}

#[test]
#[cfg(not(windows))]
fn bisect_run_detects_bogus_126_on_known_good_like_stock_git() {
    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let skron_repo = clone_repo_fixture(source.path());
    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_skron(skron_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);

    let git_output = command_any_output(
        "git",
        git_repo.path(),
        &["bisect", "run", "sh", "-c", "exit 126"],
        "git",
    );
    let skron_output = command_any_output(
        skron_bin(),
        skron_repo.path(),
        &["bisect", "run", "sh", "-c", "exit 126"],
        "skron",
    );

    assert_eq!(skron_output.0, git_output.0);
    assert_eq!(skron_output.2, git_output.2);
    assert!(skron_output.1.contains("running 'sh' '-c' 'exit 126'"));
    assert!(
        run_skron(skron_repo.path(), ["bisect", "log"]).contains("git bisect bad"),
        "first run should still mark the tested commit bad before known-good validation"
    );
}

#[test]
fn bisect_skip_reports_skipped_only_candidates_like_stock_git() {
    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let skron_repo = clone_repo_fixture(source.path());
    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~2"]);
    run_skron(skron_repo.path(), ["bisect", "start", "HEAD", "HEAD~2"]);

    let git_output = command_any_output("git", git_repo.path(), &["bisect", "skip"], "git");
    let skron_output =
        command_any_output(skron_bin(), skron_repo.path(), &["bisect", "skip"], "skron");

    assert_eq!(skron_output, git_output);
    let skron_log = run_skron(skron_repo.path(), ["bisect", "log"]);
    assert!(skron_log.contains("# only skipped commits left to test"));
    assert!(skron_log.contains("# possible first bad commit:"));
}

#[test]
fn bisect_help_view_no_checkout_and_pathspec_cover_stable_modes() {
    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let skron_repo = clone_repo_fixture(source.path());

    assert_eq!(
        command_any_output("git", git_repo.path(), &["bisect", "-h"], "git"),
        command_any_output(skron_bin(), skron_repo.path(), &["bisect", "-h"], "skron")
    );
    assert_eq!(
        command_any_output("git", git_repo.path(), &["bisect", "help"], "git"),
        command_any_output(skron_bin(), skron_repo.path(), &["bisect", "help"], "skron")
    );

    git(git_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    run_skron(skron_repo.path(), ["bisect", "start", "HEAD", "HEAD~4"]);
    assert_eq!(
        command_any_output("git", git_repo.path(), &["bisect", "help"], "git"),
        command_any_output(skron_bin(), skron_repo.path(), &["bisect", "help"], "skron")
    );
    assert_eq!(
        command_any_output(
            "git",
            git_repo.path(),
            &["bisect", "view", "--oneline"],
            "git"
        ),
        command_any_output(
            skron_bin(),
            skron_repo.path(),
            &["bisect", "view", "--oneline"],
            "skron"
        )
    );

    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let skron_repo = clone_repo_fixture(source.path());
    let git_head = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let skron_head = git(skron_repo.path(), ["rev-parse", "HEAD"]);
    git(
        git_repo.path(),
        ["bisect", "start", "--no-checkout", "HEAD", "HEAD~4"],
    );
    run_skron(
        skron_repo.path(),
        ["bisect", "start", "--no-checkout", "HEAD", "HEAD~4"],
    );
    assert_eq!(git(git_repo.path(), ["rev-parse", "HEAD"]), git_head);
    assert_eq!(git(skron_repo.path(), ["rev-parse", "HEAD"]), skron_head);
    assert_eq!(
        std::fs::read_to_string(git_repo.path().join(".git/BISECT_HEAD")).expect("git BISECT_HEAD"),
        std::fs::read_to_string(skron_repo.path().join(".git/BISECT_HEAD"))
            .expect("skron BISECT_HEAD")
    );
    git(git_repo.path(), ["bisect", "skip"]);
    run_skron(skron_repo.path(), ["bisect", "skip"]);
    assert_eq!(
        std::fs::read_to_string(git_repo.path().join(".git/BISECT_HEAD"))
            .expect("git BISECT_HEAD after skip"),
        std::fs::read_to_string(skron_repo.path().join(".git/BISECT_HEAD"))
            .expect("skron BISECT_HEAD after skip")
    );

    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let skron_repo = clone_repo_fixture(source.path());
    git(
        git_repo.path(),
        ["bisect", "start", "HEAD", "HEAD~4", "--", "a.txt"],
    );
    run_skron(
        skron_repo.path(),
        ["bisect", "start", "HEAD", "HEAD~4", "--", "a.txt"],
    );
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );

    let source = bisect_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let skron_repo = clone_repo_fixture(source.path());
    let git_output = command_any_output(
        "git",
        git_repo.path(),
        &["bisect", "start", "HEAD", "HEAD~4", "--", "missing.txt"],
        "git",
    );
    let skron_output = command_any_output(
        skron_bin(),
        skron_repo.path(),
        &["bisect", "start", "HEAD", "HEAD~4", "--", "missing.txt"],
        "skron",
    );
    assert_eq!(skron_output, git_output);
    assert!(!skron_repo.path().join(".git/BISECT_START").exists());
}

#[test]
fn bisect_first_parent_limits_candidates_to_first_parent_chain() {
    let source = bisect_first_parent_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let skron_repo = clone_repo_fixture(source.path());

    git(
        git_repo.path(),
        ["bisect", "start", "--first-parent", "HEAD", "HEAD~2"],
    );
    run_skron(
        skron_repo.path(),
        ["bisect", "start", "--first-parent", "HEAD", "HEAD~2"],
    );
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );
    assert_eq!(
        std::fs::read_to_string(skron_repo.path().join(".git/BISECT_FIRST_PARENT"))
            .expect("skron first-parent state"),
        "1\n"
    );
}

#[test]
fn rebase_replays_linear_topic_like_stock_git() {
    let source = rebase_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let skron_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    git_with_env(git_repo.path(), ["rebase", "origin/main"]);
    run_skron_with_env(skron_repo.path(), ["rebase", "origin/main"]);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_repo.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(skron_repo.path(), ["log", "--format=%s", "--max-count=3"]),
        git(git_repo.path(), ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(git(skron_repo.path(), ["status", "--short"]), "");
}

#[test]
fn rebase_uses_configured_upstream_like_stock_git() {
    let source = rebase_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let skron_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());
    git(git_repo.path(), ["branch", "-u", "origin/main"]);
    git(skron_repo.path(), ["branch", "-u", "origin/main"]);

    git_with_env(git_repo.path(), ["rebase"]);
    run_skron_with_env(skron_repo.path(), ["rebase"]);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_repo.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(skron_repo.path(), ["log", "--format=%s", "--max-count=3"]),
        git(git_repo.path(), ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(git(skron_repo.path(), ["status", "--short"]), "");
}

#[test]
fn rebase_with_branch_argument_checks_out_and_replays_like_stock_git() {
    let source = rebase_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let skron_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());
    git(git_repo.path(), ["checkout", "main"]);
    git(skron_repo.path(), ["checkout", "main"]);

    git_with_env(git_repo.path(), ["rebase", "origin/main", "topic"]);
    run_skron_with_env(skron_repo.path(), ["rebase", "origin/main", "topic"]);
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_repo.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(skron_repo.path(), ["log", "--format=%s", "--max-count=3"]),
        git(git_repo.path(), ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        git(skron_repo.path(), ["symbolic-ref", "--short", "HEAD"]),
        git(git_repo.path(), ["symbolic-ref", "--short", "HEAD"])
    );
    assert_eq!(git(skron_repo.path(), ["status", "--short"]), "");
}

#[test]
fn rebase_onto_replays_topic_like_stock_git() {
    let source = rebase_onto_fixture_repo();
    let git_repo = clone_repo_fixture(source.path());
    let skron_repo = clone_repo_fixture(source.path());
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());
    git(git_repo.path(), ["checkout", "-B", "topic", "origin/topic"]);
    git(
        skron_repo.path(),
        ["checkout", "-B", "topic", "origin/topic"],
    );

    git_with_env(
        git_repo.path(),
        ["rebase", "--onto", "origin/main", "origin/oldbase", "topic"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["rebase", "--onto", "origin/main", "origin/oldbase", "topic"],
    );
    assert_eq!(
        git(skron_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_repo.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(skron_repo.path(), ["log", "--format=%s", "--max-count=3"]),
        git(git_repo.path(), ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        git(skron_repo.path(), ["symbolic-ref", "--short", "HEAD"]),
        git(git_repo.path(), ["symbolic-ref", "--short", "HEAD"])
    );
    assert_eq!(git(skron_repo.path(), ["status", "--short"]), "");
}
