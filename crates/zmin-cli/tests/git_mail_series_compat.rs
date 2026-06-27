mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    clone_repo_fixture, command_any_output, command_output_with_env, configure_identity, git,
    git_args, git_failure_output, git_init, git_status, git_with_env, read_named_files, run_zmin,
    run_zmin_args, run_zmin_failure_output, run_zmin_status, run_zmin_with_env, write_file,
    zmin_bin,
};

fn format_patch_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    write_file(repo.path(), "alpha.txt", "alpha\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "add alpha"]);
    write_file(repo.path(), "alpha.txt", "alpha\nbeta\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "update alpha"]);
    repo
}

fn range_diff_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["checkout", "-b", "old"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "add one"]);
    write_file(repo.path(), "b.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "add two"]);
    git(repo.path(), ["checkout", "main"]);
    git(repo.path(), ["checkout", "-b", "new"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "add one"]);
    write_file(repo.path(), "c.txt", "three\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "add three"]);
    repo
}

fn normalize_format_patch_version(output: &str) -> String {
    let mut normalized = Vec::new();
    let mut version_line = false;
    for line in output.lines() {
        if version_line {
            normalized.push("git-version");
            version_line = false;
            continue;
        }
        if line.starts_with("Content-Type: multipart/mixed; boundary=\"------------") {
            normalized.push("Content-Type: multipart/mixed; boundary=\"------------git-version\"");
        } else if line.starts_with("--------------") {
            let suffix = if line.ends_with("--") { "--" } else { "" };
            normalized.push(if suffix.is_empty() {
                "--------------git-version"
            } else {
                "--------------git-version--"
            });
        } else {
            normalized.push(line);
        }
        version_line = line == "-- ";
    }
    normalized.join("\n")
}

#[test]
fn format_patch_emits_stock_applicable_mail_patches() {
    let repo = format_patch_fixture_repo();
    let base = git(repo.path(), ["rev-parse", "HEAD~2"]);
    let expected_tree = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);

    let output = run_zmin(
        repo.path(),
        ["format-patch", "-o", "patches", "HEAD~2..HEAD"],
    );
    let patch_names = read_named_files(&repo.path().join("patches"))
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    assert_eq!(
        patch_names,
        vec!["0001-add-alpha.patch", "0002-update-alpha.patch"]
    );
    assert_eq!(
        output,
        "patches/0001-add-alpha.patch\npatches/0002-update-alpha.patch"
    );

    let apply_repo = clone_repo_fixture(repo.path());
    configure_identity(apply_repo.path());
    git(apply_repo.path(), ["reset", "--hard", &base]);
    for patch in patch_names {
        let path = repo.path().join("patches").join(patch);
        let path = path.to_str().expect("patch path utf8");
        git(apply_repo.path(), ["am", path]);
    }
    assert_eq!(
        git(apply_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        expected_tree
    );

    let stdout_patch = run_zmin(repo.path(), ["format-patch", "--stdout", "-1", "HEAD"]);
    assert!(stdout_patch.contains("Subject: [PATCH] update alpha"));
    assert!(stdout_patch.contains("diff --git a/alpha.txt b/alpha.txt"));

    let no_numbered = run_zmin(
        repo.path(),
        ["format-patch", "--stdout", "--no-numbered", "HEAD~2..HEAD"],
    );
    assert!(no_numbered.contains("Subject: [PATCH] add alpha"));
    assert!(no_numbered.contains("Subject: [PATCH] update alpha"));
    assert!(!no_numbered.contains("[PATCH 1/2]"));

    let prefixed_zmin = run_zmin(
        repo.path(),
        [
            "format-patch",
            "--inline",
            "--stdout",
            "--subject-prefix=TESTCASE",
            "HEAD~2..HEAD",
        ],
    );
    let prefixed_stock = git(
        repo.path(),
        [
            "format-patch",
            "--inline",
            "--stdout",
            "--subject-prefix=TESTCASE",
            "HEAD~2..HEAD",
        ],
    );
    assert_eq!(
        normalize_format_patch_version(&prefixed_zmin),
        normalize_format_patch_version(&prefixed_stock)
    );

    git(
        repo.path(),
        ["config", "format.subjectprefix", "DIFFERENT_PREFIX"],
    );
    let configured_zmin = run_zmin(
        repo.path(),
        ["format-patch", "--inline", "--stdout", "-1", "HEAD"],
    );
    let configured_stock = git(
        repo.path(),
        ["format-patch", "--inline", "--stdout", "-1", "HEAD"],
    );
    assert_eq!(
        normalize_format_patch_version(&configured_zmin),
        normalize_format_patch_version(&configured_stock)
    );

    let cover_zmin = run_zmin_with_env(
        repo.path(),
        [
            "format-patch",
            "--stdout",
            "--cover-letter",
            "-n",
            "HEAD~2..HEAD",
        ],
    );
    let cover_stock = git_with_env(
        repo.path(),
        [
            "format-patch",
            "--stdout",
            "--cover-letter",
            "-n",
            "HEAD~2..HEAD",
        ],
    );
    assert_eq!(
        normalize_format_patch_version(&cover_zmin),
        normalize_format_patch_version(&cover_stock)
    );
}

#[test]
fn format_patch_handles_merge_commit_like_stock_git_first_parent_patch() {
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

    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "main.txt", "main\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main"]);
    git(repo.path(), ["merge", "--no-ff", "-m", "merge", "side"]);

    let zmin = run_zmin(repo.path(), ["format-patch", "--stdout", "-1", "HEAD"]);
    let stock = git_args(repo.path(), &["format-patch", "--stdout", "-1", "HEAD"]);
    assert_eq!(
        normalize_format_patch_version(&zmin),
        normalize_format_patch_version(&stock)
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["format-patch", "--stdout", "-1", "HEAD"]),
        git_status(repo.path(), ["format-patch", "--stdout", "-1", "HEAD"])
    );
}

#[test]
fn format_patch_binary_summary_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    let mut content = vec![b'a'; 16 * 1024];
    content[128] = 0;
    fs::write(repo.path().join("blob.bin"), content).expect("write binary blob");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "add binary blob"]);
    git(repo.path(), ["repack", "-ad", "--depth=0"]);
    let base = git(repo.path(), ["rev-parse", "HEAD~1"]);
    let expected_tree = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);

    let zmin = run_zmin(repo.path(), ["format-patch", "--stdout", "-1", "HEAD"]);

    assert!(zmin.contains("GIT binary patch"));
    assert!(!zmin.contains("Binary files /dev/null and b/blob.bin differ"));

    let apply_repo = clone_repo_fixture(repo.path());
    configure_identity(apply_repo.path());
    git(apply_repo.path(), ["reset", "--hard", &base]);
    let patch_path = repo.path().join("binary.patch");
    fs::write(&patch_path, zmin).expect("write zmin binary patch");
    git(
        apply_repo.path(),
        ["am", patch_path.to_str().expect("patch path utf8")],
    );
    assert_eq!(
        git(apply_repo.path(), ["rev-parse", "HEAD^{tree}"]),
        expected_tree
    );
}

#[test]
fn am_applies_stock_format_patch_mail_like_stock_git() {
    let repo = format_patch_fixture_repo();
    let base = git(repo.path(), ["rev-parse", "HEAD~2"]);
    git(
        repo.path(),
        ["format-patch", "-o", "stock-patches", "HEAD~2..HEAD"],
    );
    let patch_names = read_named_files(&repo.path().join("stock-patches"))
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>();

    let git_apply = clone_repo_fixture(repo.path());
    let zmin_apply = clone_repo_fixture(repo.path());
    configure_identity(git_apply.path());
    configure_identity(zmin_apply.path());
    git(git_apply.path(), ["reset", "--hard", &base]);
    git(zmin_apply.path(), ["reset", "--hard", &base]);

    for patch in patch_names {
        let path = repo.path().join("stock-patches").join(patch);
        let path = path.to_str().expect("patch path utf8");
        git(git_apply.path(), ["am", path]);
        run_zmin_with_env(zmin_apply.path(), ["am", path]);
    }

    assert_eq!(
        git(zmin_apply.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_apply.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(zmin_apply.path(), ["log", "--format=%an <%ae>%n%s", "-2"]),
        git(git_apply.path(), ["log", "--format=%an <%ae>%n%s", "-2"])
    );
    assert_eq!(git(zmin_apply.path(), ["status", "--short"]), "");
}

fn am_single_patch_fixture() -> (TempDir, String, String) {
    let repo = format_patch_fixture_repo();
    let base = git(repo.path(), ["rev-parse", "HEAD~1"]);
    git(repo.path(), ["format-patch", "-o", "stock-patches", "HEAD~1"]);
    let patch_name = read_named_files(&repo.path().join("stock-patches"))
        .into_iter()
        .map(|(name, _)| name)
        .next()
        .expect("single am patch");
    let patch_path = repo.path().join("stock-patches").join(patch_name);
    (repo, base, patch_path.to_string_lossy().into_owned())
}

fn am_conflict_patch_fixture() -> (TempDir, String, String) {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\nbase\n");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    write_file(repo.path(), "a.txt", "one\nupstream\n");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "upstream"]);
    git(repo.path(), ["format-patch", "-o", "patches", "HEAD~1"]);
    let base = git(repo.path(), ["rev-parse", "HEAD~1"]);
    let patch_name = read_named_files(&repo.path().join("patches"))
        .into_iter()
        .map(|(name, _)| name)
        .next()
        .expect("conflict patch");
    let patch_path = repo.path().join("patches").join(patch_name);
    (repo, base, patch_path.to_string_lossy().into_owned())
}

#[test]
fn am_option_surface_batch_matches_stock_git() {
    let (source, base, patch_path) = am_single_patch_fixture();
    let patch = patch_path.as_str();
    let success_cases: [(&str, &[&str]); 33] = [
        ("--quiet", &["am", "--quiet", patch]),
        ("-q", &["am", "-q", patch]),
        ("--utf8", &["am", "--utf8", patch]),
        ("-u", &["am", "-u", patch]),
        ("--no-utf8", &["am", "--no-utf8", patch]),
        ("--keep", &["am", "--keep", patch]),
        ("-k", &["am", "-k", patch]),
        ("--keep-non-patch", &["am", "--keep-non-patch", patch]),
        ("--signoff", &["am", "--signoff", patch]),
        ("-s", &["am", "-s", patch]),
        ("--keep-cr", &["am", "--keep-cr", patch]),
        ("--no-keep-cr", &["am", "--no-keep-cr", patch]),
        ("--message-id", &["am", "--message-id", patch]),
        ("-m", &["am", "-m", patch]),
        ("--no-message-id", &["am", "--no-message-id", patch]),
        ("--scissors", &["am", "--scissors", patch]),
        ("-c", &["am", "-c", patch]),
        ("--no-scissors", &["am", "--no-scissors", patch]),
        ("--quoted-cr=strip", &["am", "--quoted-cr=strip", patch]),
        ("--3way", &["am", "--3way", patch]),
        ("-3", &["am", "-3", patch]),
        ("--no-3way", &["am", "--no-3way", patch]),
        ("--ignore-space-change", &["am", "--ignore-space-change", patch]),
        ("--ignore-whitespace", &["am", "--ignore-whitespace", patch]),
        ("--whitespace=warn", &["am", "--whitespace=warn", patch]),
        ("-C1", &["am", "-C1", patch]),
        ("-p1", &["am", "-p1", patch]),
        ("--patch-format=mboxrd", &["am", "--patch-format=mboxrd", patch]),
        ("--empty=stop", &["am", "--empty=stop", patch]),
        ("--empty=drop", &["am", "--empty=drop", patch]),
        ("--reject", &["am", "--reject", patch]),
        ("--no-verify", &["am", "--no-verify", patch]),
        ("-n", &["am", "-n", patch]),
    ];
    for (label, args) in success_cases {
        let git_apply = clone_repo_fixture(source.path());
        let zmin_apply = clone_repo_fixture(source.path());
        configure_identity(git_apply.path());
        configure_identity(zmin_apply.path());
        git(git_apply.path(), ["reset", "--hard", &base]);
        git(zmin_apply.path(), ["reset", "--hard", &base]);

        let git_result = command_any_output("git", git_apply.path(), args, "git");
        let zmin_result = command_any_output(zmin_bin(), zmin_apply.path(), args, "zmin");

        assert_eq!(zmin_result, git_result, "args: {args:?}");
        assert_eq!(
            git(zmin_apply.path(), ["rev-parse", "HEAD^{tree}"]),
            git(git_apply.path(), ["rev-parse", "HEAD^{tree}"]),
            "tree args: {args:?}"
        );
        assert_eq!(
            git(zmin_apply.path(), ["log", "--format=%s%n%B", "-1"]),
            git(git_apply.path(), ["log", "--format=%s%n%B", "-1"]),
            "log args: {args:?}"
        );
        assert_eq!(
            git(zmin_apply.path(), ["status", "--short"]),
            git(git_apply.path(), ["status", "--short"]),
            "status args: {args:?}"
        );
        assert!(!label.is_empty());
    }
}

#[test]
fn am_committer_date_is_author_date_matches_stock_git() {
    let (source, base, patch_path) = am_single_patch_fixture();
    let patch = patch_path.as_str();
    let args = ["am", "--committer-date-is-author-date", patch];
    let envs = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000200 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000300 +0000"),
    ];

    let git_apply = clone_repo_fixture(source.path());
    let zmin_apply = clone_repo_fixture(source.path());
    configure_identity(git_apply.path());
    configure_identity(zmin_apply.path());
    git(git_apply.path(), ["reset", "--hard", &base]);
    git(zmin_apply.path(), ["reset", "--hard", &base]);

    let git_result = command_output_with_env("git", git_apply.path(), &args, &envs, "git");
    let zmin_result = command_output_with_env(zmin_bin(), zmin_apply.path(), &args, &envs, "zmin");
    assert_eq!(zmin_result, git_result);
    assert_eq!(
        git(
            zmin_apply.path(),
            ["log", "-1", "--format=%ad%n%cd%n%s%n%B", "--date=raw"]
        ),
        git(
            git_apply.path(),
            ["log", "-1", "--format=%ad%n%cd%n%s%n%B", "--date=raw"]
        )
    );
    assert_eq!(
        git(zmin_apply.path(), ["rev-parse", "HEAD^{tree}"]),
        git(git_apply.path(), ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(zmin_apply.path(), ["status", "--short"]),
        git(git_apply.path(), ["status", "--short"])
    );
}

#[test]
fn am_resume_only_flags_without_session_match_stock_git() {
    let (_source, _base, patch_path) = am_single_patch_fixture();
    let patch = patch_path.as_str();
    let failure_cases: [(&str, &[&str]); 10] = [
        ("--allow-empty", &["am", "--allow-empty", patch]),
        ("--abort", &["am", "--abort"]),
        ("--quit", &["am", "--quit"]),
        ("--skip", &["am", "--skip"]),
        ("--continue", &["am", "--continue"]),
        ("--resolved", &["am", "--resolved"]),
        ("-r", &["am", "-r"]),
        ("--retry", &["am", "--retry"]),
        ("--show-current-patch=raw", &["am", "--show-current-patch=raw"]),
        ("--show-current-patch=diff", &["am", "--show-current-patch=diff"]),
    ];
    for (label, args) in failure_cases {
        let repo = git_init();
        configure_identity(repo.path());
        write_file(repo.path(), "a.txt", "base\n");
        git(repo.path(), ["add", "a.txt"]);
        git_with_env(repo.path(), ["commit", "-m", "base"]);

        let git_result = git_failure_output(repo.path(), args);
        let zmin_result = run_zmin_failure_output(repo.path(), args);
        assert_eq!(zmin_result, git_result, "args: {args:?}");
        assert_eq!(git(repo.path(), ["status", "--short"]), "");
        assert!(!label.is_empty());
    }
}

#[test]
fn am_conflict_session_family_matches_stock_git() {
    let (source, base, patch_path) = am_conflict_patch_fixture();
    let patch = patch_path.as_str();

    let conflict_cases: [(&str, &[&str]); 7] = [
        ("initial", &["am", patch]),
        ("show-raw", &["am", "--show-current-patch=raw"]),
        ("show-diff", &["am", "--show-current-patch=diff"]),
        ("retry", &["am", "--retry"]),
        ("continue", &["am", "--continue"]),
        ("resolved", &["am", "--resolved"]),
        ("-r", &["am", "-r"]),
    ];

    let git_apply = clone_repo_fixture(source.path());
    let zmin_apply = clone_repo_fixture(source.path());
    configure_identity(git_apply.path());
    configure_identity(zmin_apply.path());
    git(git_apply.path(), ["reset", "--hard", &base]);
    git(zmin_apply.path(), ["reset", "--hard", &base]);
    write_file(git_apply.path(), "a.txt", "one\nlocal\n");
    write_file(zmin_apply.path(), "a.txt", "one\nlocal\n");
    git(git_apply.path(), ["add", "a.txt"]);
    git(zmin_apply.path(), ["add", "a.txt"]);
    git_with_env(git_apply.path(), ["commit", "-m", "local"]);
    git_with_env(zmin_apply.path(), ["commit", "-m", "local"]);

    for (label, args) in conflict_cases {
        let git_result = command_any_output("git", git_apply.path(), args, "git");
        let zmin_result = command_any_output(zmin_bin(), zmin_apply.path(), args, "zmin");
        assert_eq!(zmin_result, git_result, "case {label}: args {args:?}");
        assert_eq!(
            git(zmin_apply.path(), ["status", "--short"]),
            git(git_apply.path(), ["status", "--short"]),
            "status case {label}"
        );
    }

    for (label, cleanup_args) in [
        ("skip", ["am", "--skip"].as_slice()),
        ("abort", ["am", "--abort"].as_slice()),
        ("quit", ["am", "--quit"].as_slice()),
    ] {
        let git_repo = clone_repo_fixture(source.path());
        let zmin_repo = clone_repo_fixture(source.path());
        configure_identity(git_repo.path());
        configure_identity(zmin_repo.path());
        git(git_repo.path(), ["reset", "--hard", &base]);
        git(zmin_repo.path(), ["reset", "--hard", &base]);
        write_file(git_repo.path(), "a.txt", "one\nlocal\n");
        write_file(zmin_repo.path(), "a.txt", "one\nlocal\n");
        git(git_repo.path(), ["add", "a.txt"]);
        git(zmin_repo.path(), ["add", "a.txt"]);
        git_with_env(git_repo.path(), ["commit", "-m", "local"]);
        git_with_env(zmin_repo.path(), ["commit", "-m", "local"]);
        let _ = command_any_output("git", git_repo.path(), &["am", patch], "git");
        let _ = command_any_output(zmin_bin(), zmin_repo.path(), &["am", patch], "zmin");

        let git_result = command_any_output("git", git_repo.path(), cleanup_args, "git");
        let zmin_result = command_any_output(zmin_bin(), zmin_repo.path(), cleanup_args, "zmin");
        assert_eq!(zmin_result, git_result, "case {label}: args {cleanup_args:?}");
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            git(git_repo.path(), ["status", "--short"]),
            "status case {label}"
        );
    }
}

#[test]
fn range_diff_matches_stock_git_for_patch_equivalence() {
    let repo = range_diff_fixture_repo();
    for args in [
        ["range-diff", "main..old", "main..new"].as_slice(),
        ["range-diff", "main", "old", "new"].as_slice(),
        ["range-diff", "--no-dual-color", "main..old", "main..new"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}
