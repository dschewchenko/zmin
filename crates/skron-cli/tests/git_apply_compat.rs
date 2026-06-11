mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    command_any_output_with_stdin, command_stdout_bytes, configure_identity, git, git_init,
    git_status_with_stdin, git_with_env, git_with_stdin, git_with_stdin_args,
    run_skron_status_with_stdin, run_skron_with_stdin, run_skron_with_stdin_args, skron_bin,
    write_file,
};

fn apply_base_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    write_file(repo.path(), "b.txt", "bee\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    repo
}

#[test]
fn patch_id_matches_stock_git_for_diff_and_log_patches() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), "one\n").expect("write first");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    fs::write(repo.path().join("a.txt"), "two \n").expect("write second");
    let diff =
        String::from_utf8(command_stdout_bytes("git", repo.path(), &["diff"])).expect("diff utf8");

    for args in [
        ["patch-id"].as_slice(),
        ["patch-id", "--stable"].as_slice(),
        ["patch-id", "--unstable"].as_slice(),
        ["patch-id", "--verbatim"].as_slice(),
    ] {
        assert_eq!(
            run_skron_with_stdin_args(repo.path(), args, &diff),
            git_with_stdin_args(repo.path(), args, &diff),
            "args: {args:?}"
        );
    }

    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    fs::write(repo.path().join("b.txt"), "new\n").expect("write third");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "third"]);
    let log_patch = String::from_utf8(command_stdout_bytes(
        "git",
        repo.path(),
        &["log", "--format=%H", "-p", "--max-count=2"],
    ))
    .expect("log patch utf8");

    for args in [
        ["patch-id", "--stable"].as_slice(),
        ["patch-id", "--unstable"].as_slice(),
    ] {
        assert_eq!(
            run_skron_with_stdin_args(repo.path(), args, &log_patch),
            git_with_stdin_args(repo.path(), args, &log_patch),
            "args: {args:?}"
        );
    }
}

#[test]
fn apply_matches_stock_git_for_worktree_check_and_cached_modes() {
    let patch_repo = apply_base_repo();
    write_file(patch_repo.path(), "a.txt", "one\ntwo\n");
    fs::remove_file(patch_repo.path().join("b.txt")).expect("remove b");
    write_file(patch_repo.path(), "c.txt", "see\n");
    let patch = String::from_utf8(command_stdout_bytes("git", patch_repo.path(), &["diff"]))
        .expect("diff utf8");

    let git_repo = apply_base_repo();
    let skron_repo = apply_base_repo();
    assert_eq!(
        run_skron_status_with_stdin(skron_repo.path(), ["apply", "--check"], &patch),
        git_status_with_stdin(git_repo.path(), ["apply", "--check"], &patch)
    );
    git_with_stdin(git_repo.path(), ["apply"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply"], &patch);
    assert_eq!(
        git(skron_repo.path(), ["diff"]),
        git(git_repo.path(), ["diff"])
    );

    let git_repo = apply_base_repo();
    let skron_repo = apply_base_repo();
    git_with_stdin(git_repo.path(), ["apply", "--cached"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply", "--cached"], &patch);
    assert_eq!(
        git(skron_repo.path(), ["diff", "--cached"]),
        git(git_repo.path(), ["diff", "--cached"])
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("a.txt")).expect("read skron worktree a"),
        "one\n"
    );

    let git_repo = apply_base_repo();
    let skron_repo = apply_base_repo();
    git_with_stdin(git_repo.path(), ["apply", "--index"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply", "--index"], &patch);
    assert_eq!(
        git(skron_repo.path(), ["diff", "--cached"]),
        git(git_repo.path(), ["diff", "--cached"])
    );
    assert_eq!(
        git(skron_repo.path(), ["diff"]),
        git(git_repo.path(), ["diff"])
    );
}

#[test]
fn apply_binary_literal_patch_matches_stock_git() {
    let patch_repo = git_init();
    configure_identity(patch_repo.path());
    fs::write(patch_repo.path().join("bin.dat"), b"abc\0old\n").expect("write binary");
    git(patch_repo.path(), ["add", "-A"]);
    git_with_env(patch_repo.path(), ["commit", "-m", "base"]);
    fs::write(patch_repo.path().join("bin.dat"), b"abc\0new\n").expect("rewrite binary");
    let patch = String::from_utf8(command_stdout_bytes(
        "git",
        patch_repo.path(),
        &["diff", "--binary"],
    ))
    .expect("binary patch utf8");

    let git_repo = git_init();
    let skron_repo = git_init();
    for repo in [git_repo.path(), skron_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("bin.dat"), b"abc\0old\n").expect("write base binary");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "base"]);
    }
    git_with_stdin(git_repo.path(), ["apply"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply"], &patch);
    assert_eq!(
        fs::read(skron_repo.path().join("bin.dat")).expect("read skron binary"),
        fs::read(git_repo.path().join("bin.dat")).expect("read git binary")
    );

    git_with_stdin(git_repo.path(), ["apply", "-R"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply", "-R"], &patch);
    assert_eq!(
        fs::read(skron_repo.path().join("bin.dat")).expect("read reversed skron binary"),
        fs::read(git_repo.path().join("bin.dat")).expect("read reversed git binary")
    );
}

#[test]
fn apply_binary_delta_patch_matches_stock_git() {
    let patch_repo = git_init();
    configure_identity(patch_repo.path());
    fs::write(patch_repo.path().join("bin.dat"), b"abcd\0".repeat(10_000)).expect("write binary");
    git(patch_repo.path(), ["add", "-A"]);
    git_with_env(patch_repo.path(), ["commit", "-m", "base"]);
    let mut updated = fs::read(patch_repo.path().join("bin.dat")).expect("read binary");
    updated.splice(100..160, b"XYZ\0".repeat(15));
    fs::write(patch_repo.path().join("bin.dat"), &updated).expect("rewrite binary");
    let patch = String::from_utf8(command_stdout_bytes(
        "git",
        patch_repo.path(),
        &["diff", "--binary"],
    ))
    .expect("binary patch utf8");
    assert!(patch.contains("delta "));

    let git_repo = git_init();
    let skron_repo = git_init();
    for repo in [git_repo.path(), skron_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("bin.dat"), b"abcd\0".repeat(10_000)).expect("write base binary");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "base"]);
    }

    git_with_stdin(git_repo.path(), ["apply"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply"], &patch);
    assert_eq!(
        fs::read(skron_repo.path().join("bin.dat")).expect("read skron binary"),
        fs::read(git_repo.path().join("bin.dat")).expect("read git binary")
    );

    git_with_stdin(git_repo.path(), ["apply", "-R"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply", "-R"], &patch);
    assert_eq!(
        fs::read(skron_repo.path().join("bin.dat")).expect("read reversed skron binary"),
        fs::read(git_repo.path().join("bin.dat")).expect("read reversed git binary")
    );
}

#[test]
fn skron_diff_binary_patch_applies_with_stock_git_and_skron() {
    let patch_repo = git_init();
    configure_identity(patch_repo.path());
    fs::write(patch_repo.path().join("bin.dat"), b"abc\0old\n").expect("write binary");
    git(patch_repo.path(), ["add", "-A"]);
    git_with_env(patch_repo.path(), ["commit", "-m", "base"]);
    fs::write(patch_repo.path().join("bin.dat"), b"abc\0new\n\xff").expect("rewrite binary");
    let patch = String::from_utf8(command_stdout_bytes(
        skron_bin(),
        patch_repo.path(),
        &["diff", "--binary"],
    ))
    .expect("skron binary patch utf8");
    assert!(patch.contains("GIT binary patch"));

    let git_repo = git_init();
    let skron_repo = git_init();
    for repo in [git_repo.path(), skron_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("bin.dat"), b"abc\0old\n").expect("write base binary");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "base"]);
    }

    git_with_stdin(git_repo.path(), ["apply"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply"], &patch);
    assert_eq!(
        fs::read(skron_repo.path().join("bin.dat")).expect("read skron binary"),
        fs::read(git_repo.path().join("bin.dat")).expect("read git binary")
    );

    git_with_stdin(git_repo.path(), ["apply", "-R"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply", "-R"], &patch);
    assert_eq!(
        fs::read(skron_repo.path().join("bin.dat")).expect("read reversed skron binary"),
        fs::read(git_repo.path().join("bin.dat")).expect("read reversed git binary")
    );
}

#[test]
fn apply_rename_patches_match_stock_git() {
    let pure_patch_repo = apply_base_repo();
    git(pure_patch_repo.path(), ["mv", "b.txt", "moved.txt"]);
    let pure_patch = String::from_utf8(command_stdout_bytes(
        "git",
        pure_patch_repo.path(),
        &["diff", "HEAD", "--find-renames"],
    ))
    .expect("pure rename patch utf8");
    let git_repo = apply_base_repo();
    let skron_repo = apply_base_repo();
    git_with_stdin(git_repo.path(), ["apply", "--index"], &pure_patch);
    run_skron_with_stdin(skron_repo.path(), ["apply", "--index"], &pure_patch);
    assert_eq!(
        git(skron_repo.path(), ["diff", "--cached", "--find-renames"]),
        git(git_repo.path(), ["diff", "--cached", "--find-renames"])
    );

    let patch_repo = apply_base_repo();
    git(patch_repo.path(), ["mv", "b.txt", "renamed.txt"]);
    write_file(patch_repo.path(), "renamed.txt", "bee\nrenamed\n");
    let patch = String::from_utf8(command_stdout_bytes(
        "git",
        patch_repo.path(),
        &["diff", "HEAD", "--find-renames"],
    ))
    .expect("rename patch utf8");

    let git_repo = apply_base_repo();
    let skron_repo = apply_base_repo();
    git_with_stdin(git_repo.path(), ["apply"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply"], &patch);
    assert_eq!(
        git(skron_repo.path(), ["diff", "--find-renames"]),
        git(git_repo.path(), ["diff", "--find-renames"])
    );

    let git_repo = apply_base_repo();
    let skron_repo = apply_base_repo();
    git_with_stdin(git_repo.path(), ["apply", "--cached"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply", "--cached"], &patch);
    assert_eq!(
        git(skron_repo.path(), ["diff", "--cached", "--find-renames"]),
        git(git_repo.path(), ["diff", "--cached", "--find-renames"])
    );
    assert!(skron_repo.path().join("b.txt").exists());
    assert!(!skron_repo.path().join("renamed.txt").exists());

    let git_repo = apply_base_repo();
    let skron_repo = apply_base_repo();
    git_with_stdin(git_repo.path(), ["apply", "--index"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply", "--index"], &patch);
    assert_eq!(
        git(skron_repo.path(), ["diff", "--cached", "--find-renames"]),
        git(git_repo.path(), ["diff", "--cached", "--find-renames"])
    );
    assert_eq!(
        git(skron_repo.path(), ["diff", "--find-renames"]),
        git(git_repo.path(), ["diff", "--find-renames"])
    );
}

#[cfg(unix)]
#[test]
fn apply_mode_only_patches_match_stock_git() {
    let patch_repo = apply_base_repo();
    let mut perms = fs::metadata(patch_repo.path().join("a.txt"))
        .expect("metadata")
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    fs::set_permissions(patch_repo.path().join("a.txt"), perms).expect("chmod");
    let patch = String::from_utf8(command_stdout_bytes(
        "git",
        patch_repo.path(),
        &["diff", "HEAD"],
    ))
    .expect("mode patch utf8");

    let git_repo = apply_base_repo();
    let skron_repo = apply_base_repo();
    git_with_stdin(git_repo.path(), ["apply", "--index"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply", "--index"], &patch);
    assert_eq!(
        git(skron_repo.path(), ["diff", "--cached"]),
        git(git_repo.path(), ["diff", "--cached"])
    );
    assert_eq!(
        git(skron_repo.path(), ["diff"]),
        git(git_repo.path(), ["diff"])
    );
}

#[test]
fn apply_empty_file_delete_patches_match_stock_git() {
    let patch_repo = git_init();
    configure_identity(patch_repo.path());
    fs::write(patch_repo.path().join("empty.txt"), "").expect("write empty file");
    git(patch_repo.path(), ["add", "-A"]);
    git_with_env(patch_repo.path(), ["commit", "-m", "base"]);
    fs::remove_file(patch_repo.path().join("empty.txt")).expect("remove empty file");
    let patch = String::from_utf8(command_stdout_bytes(
        "git",
        patch_repo.path(),
        &["diff", "HEAD"],
    ))
    .expect("empty delete patch utf8");
    assert!(patch.contains("deleted file mode"));
    assert!(!patch.contains("@@ "));

    let git_repo = git_init();
    let skron_repo = git_init();
    for repo in [git_repo.path(), skron_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("empty.txt"), "").expect("write base empty file");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "base"]);
    }

    git_with_stdin(git_repo.path(), ["apply"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply"], &patch);
    assert_eq!(
        git(skron_repo.path(), ["diff"]),
        git(git_repo.path(), ["diff"])
    );

    git_with_stdin(git_repo.path(), ["apply", "-R"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply", "-R"], &patch);
    assert_eq!(
        git(skron_repo.path(), ["diff"]),
        git(git_repo.path(), ["diff"])
    );
    assert_eq!(
        fs::read(skron_repo.path().join("empty.txt")).expect("read restored skron empty file"),
        fs::read(git_repo.path().join("empty.txt")).expect("read restored git empty file")
    );

    let git_repo = git_init();
    let skron_repo = git_init();
    for repo in [git_repo.path(), skron_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("empty.txt"), "").expect("write base empty file");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "base"]);
    }

    git_with_stdin(git_repo.path(), ["apply", "--cached"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply", "--cached"], &patch);
    assert_eq!(
        git(skron_repo.path(), ["diff", "--cached"]),
        git(git_repo.path(), ["diff", "--cached"])
    );

    let git_repo = git_init();
    let skron_repo = git_init();
    for repo in [git_repo.path(), skron_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("empty.txt"), "").expect("write base empty file");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "base"]);
    }

    git_with_stdin(git_repo.path(), ["apply", "--index"], &patch);
    run_skron_with_stdin(skron_repo.path(), ["apply", "--index"], &patch);
    assert_eq!(
        git(skron_repo.path(), ["diff", "--cached"]),
        git(git_repo.path(), ["diff", "--cached"])
    );
    assert_eq!(
        git(skron_repo.path(), ["diff"]),
        git(git_repo.path(), ["diff"])
    );
}

#[test]
fn apply_header_only_patch_failure_matches_stock_git() {
    let git_repo = apply_base_repo();
    let skron_repo = apply_base_repo();
    let patch = "diff --git a/a.txt b/a.txt\n";

    let git_output =
        command_any_output_with_stdin("git", git_repo.path(), &["apply"], patch, "git apply");
    let skron_output =
        command_any_output_with_stdin(skron_bin(), skron_repo.path(), &["apply"], patch, "skron");

    assert_eq!(skron_output.0, git_output.0);
    assert_eq!(skron_output.1, git_output.1);
    assert!(
        skron_output.2.contains("No valid patches in input"),
        "unexpected skron stderr: {}",
        skron_output.2
    );
    assert!(
        git_output.2.contains("No valid patches in input"),
        "unexpected git stderr: {}",
        git_output.2
    );
}
