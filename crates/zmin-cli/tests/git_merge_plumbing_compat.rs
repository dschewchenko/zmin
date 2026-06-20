mod common;

use std::process::Command;
use std::{fs, path::Path};

use tempfile::TempDir;

use common::{
    configure_identity, git, git_args, git_init, git_status, git_with_env, git_with_stdin,
    run_zmin, run_zmin_args, run_zmin_status, zmin_bin, write_file,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandTextOutput {
    status: i32,
    stdout: String,
}

fn run_zmin_output<const N: usize>(cwd: &Path, args: [&str; N]) -> CommandTextOutput {
    command_text_output(zmin_bin(), cwd, &args)
}

fn git_output_for_args<const N: usize>(cwd: &Path, args: [&str; N]) -> CommandTextOutput {
    command_text_output("git", cwd, &args)
}

fn command_text_output(command: &str, cwd: &Path, args: &[&str]) -> CommandTextOutput {
    let output = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    CommandTextOutput {
        status: output.status.code().expect("process exited by signal"),
        stdout: String::from_utf8(output.stdout)
            .expect("stdout utf8")
            .trim_end_matches('\n')
            .to_owned(),
    }
}

fn command_all_output(command: &str, cwd: &Path, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout)
            .expect("stdout utf8")
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .expect("stderr utf8")
            .trim_end_matches('\n')
            .to_owned(),
    )
}

fn merge_one_file_fixture() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "file.txt", "a\nbase\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    repo
}

fn mergetool_conflict_fixture() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "f.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["checkout", "-b", "feature"]);
    write_file(repo.path(), "f.txt", "feature\n");
    git_with_env(repo.path(), ["commit", "-am", "feature"]);
    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "f.txt", "main\n");
    git_with_env(repo.path(), ["commit", "-am", "main"]);
    assert_ne!(git_status(repo.path(), ["merge", "feature"]), 0);
    repo
}

fn rerere_conflict_fixture() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "rerere.enabled", "true"]);
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "f.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["checkout", "-b", "feature"]);
    write_file(repo.path(), "f.txt", "feature\n");
    git_with_env(repo.path(), ["commit", "-am", "feature"]);
    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "f.txt", "main\n");
    git_with_env(repo.path(), ["commit", "-am", "main"]);
    assert_ne!(git_status(repo.path(), ["merge", "feature"]), 0);
    repo
}

fn rerere_multi_conflict_fixture() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "rerere.enabled", "true"]);
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "base\n");
    write_file(repo.path(), "b.md", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["checkout", "-b", "feature"]);
    write_file(repo.path(), "a.txt", "feature\n");
    write_file(repo.path(), "b.md", "feature\n");
    git_with_env(repo.path(), ["commit", "-am", "feature"]);
    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "a.txt", "main\n");
    write_file(repo.path(), "b.md", "main\n");
    git_with_env(repo.path(), ["commit", "-am", "main"]);
    assert_ne!(git_status(repo.path(), ["merge", "feature"]), 0);
    repo
}

#[test]
fn merge_file_matches_stock_git_for_clean_and_conflict_cases() {
    let dir = TempDir::new().expect("temp dir");
    write_file(dir.path(), "base.txt", "a\nb\nc\n");
    write_file(dir.path(), "ours.txt", "a\nours\nc\n");
    write_file(dir.path(), "same-base.txt", "a\nb\nc\n");
    write_file(dir.path(), "theirs.txt", "a\ntheirs\nc\n");

    assert_eq!(
        run_zmin(
            dir.path(),
            ["merge-file", "-p", "ours.txt", "base.txt", "same-base.txt"]
        ),
        git(
            dir.path(),
            ["merge-file", "-p", "ours.txt", "base.txt", "same-base.txt"]
        )
    );
    let zmin_conflict = run_zmin_output(
        dir.path(),
        [
            "merge-file",
            "-p",
            "-L",
            "CURRENT",
            "-L",
            "BASE",
            "-L",
            "OTHER",
            "ours.txt",
            "base.txt",
            "theirs.txt",
        ],
    );
    let git_conflict = git_output_for_args(
        dir.path(),
        [
            "merge-file",
            "-p",
            "-L",
            "CURRENT",
            "-L",
            "BASE",
            "-L",
            "OTHER",
            "ours.txt",
            "base.txt",
            "theirs.txt",
        ],
    );
    assert_eq!(zmin_conflict.status, git_conflict.status);
    assert_eq!(zmin_conflict.stdout, git_conflict.stdout);

    write_file(dir.path(), "git-inplace.txt", "a\nours\nc\n");
    write_file(dir.path(), "zmin-inplace.txt", "a\nours\nc\n");
    let git_status = git_status(
        dir.path(),
        [
            "merge-file",
            "-L",
            "CURRENT",
            "-L",
            "BASE",
            "-L",
            "OTHER",
            "git-inplace.txt",
            "base.txt",
            "theirs.txt",
        ],
    );
    let zmin_status = run_zmin_status(
        dir.path(),
        [
            "merge-file",
            "-L",
            "CURRENT",
            "-L",
            "BASE",
            "-L",
            "OTHER",
            "zmin-inplace.txt",
            "base.txt",
            "theirs.txt",
        ],
    );
    assert_eq!(zmin_status, git_status);
    assert_eq!(
        fs::read_to_string(dir.path().join("zmin-inplace.txt")).expect("read zmin merge file"),
        fs::read_to_string(dir.path().join("git-inplace.txt")).expect("read git merge file")
    );
}

#[test]
fn merge_one_file_matches_stock_git_for_clean_text_merge() {
    let git_repo = merge_one_file_fixture();
    let zmin_repo = merge_one_file_fixture();
    let base = git(git_repo.path(), ["rev-parse", "HEAD:file.txt"]);
    let ours = base.clone();
    let theirs = git_with_stdin(
        git_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "a\ntheirs\n",
    );
    git_with_stdin(
        zmin_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "a\ntheirs\n",
    );
    let index_info = format!(
        "100644 {base} 1\tfile.txt\n100644 {ours} 2\tfile.txt\n100644 {theirs} 3\tfile.txt\n"
    );
    git_with_stdin(
        git_repo.path(),
        ["update-index", "--index-info"],
        &index_info,
    );
    git_with_stdin(
        zmin_repo.path(),
        ["update-index", "--index-info"],
        &index_info,
    );

    let args = [
        "merge-one-file",
        &base,
        &ours,
        &theirs,
        "file.txt",
        "100644",
        "100644",
        "100644",
    ];
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &args),
        git_args(git_repo.path(), &args)
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("file.txt")).expect("read zmin merge result"),
        fs::read_to_string(git_repo.path().join("file.txt")).expect("read git merge result")
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );
}

#[test]
fn merge_one_file_matches_stock_git_for_identical_add_add() {
    let git_repo = merge_one_file_fixture();
    let zmin_repo = merge_one_file_fixture();
    let added = git_with_stdin(git_repo.path(), ["hash-object", "-w", "--stdin"], "added\n");
    git_with_stdin(
        zmin_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "added\n",
    );
    let index_info = format!("100644 {added} 2\tadded.txt\n100644 {added} 3\tadded.txt\n");
    git_with_stdin(
        git_repo.path(),
        ["update-index", "--index-info"],
        &index_info,
    );
    git_with_stdin(
        zmin_repo.path(),
        ["update-index", "--index-info"],
        &index_info,
    );

    let args = [
        "merge-one-file",
        "",
        &added,
        &added,
        "added.txt",
        "",
        "100644",
        "100644",
    ];
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &args),
        git_args(git_repo.path(), &args)
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("added.txt")).expect("read zmin add/add"),
        fs::read_to_string(git_repo.path().join("added.txt")).expect("read git add/add")
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage", "added.txt"]),
        git(git_repo.path(), ["ls-files", "--stage", "added.txt"])
    );
}

#[test]
fn merge_index_runs_merge_one_file_for_unmerged_entries() {
    let git_repo = merge_one_file_fixture();
    let zmin_repo = merge_one_file_fixture();
    let base = git(git_repo.path(), ["rev-parse", "HEAD:file.txt"]);
    let ours = base.clone();
    let theirs = git_with_stdin(
        git_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "a\ntheirs\n",
    );
    git_with_stdin(
        zmin_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "a\ntheirs\n",
    );
    let index_info = format!(
        "100644 {base} 1\tfile.txt\n100644 {ours} 2\tfile.txt\n100644 {theirs} 3\tfile.txt\n"
    );
    git_with_stdin(
        git_repo.path(),
        ["update-index", "--index-info"],
        &index_info,
    );
    git_with_stdin(
        zmin_repo.path(),
        ["update-index", "--index-info"],
        &index_info,
    );

    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["merge-index", "git-merge-one-file", "-a"]
        ),
        git(git_repo.path(), ["merge-index", "git-merge-one-file", "-a"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("file.txt")).expect("read zmin merge result"),
        fs::read_to_string(git_repo.path().join("file.txt")).expect("read git merge result")
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );
}

#[test]
fn mergetool_runs_configured_tool_and_stages_resolution_like_stock_git() {
    let git_repo = mergetool_conflict_fixture();
    let zmin_repo = mergetool_conflict_fixture();
    let command = "printf 'B:'; cat \"$BASE\"; printf 'L:'; cat \"$LOCAL\"; printf 'R:'; cat \"$REMOTE\"; printf 'resolved\\n' > \"$MERGED\"";
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "mergetool.zmintest.cmd", command]);
        git(
            repo,
            ["config", "mergetool.zmintest.trustExitCode", "true"],
        );
    }

    let args = ["mergetool", "--tool=zmintest", "--no-prompt", "f.txt"];
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &args),
        git_args(git_repo.path(), &args)
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("f.txt")).expect("read zmin merged file"),
        fs::read_to_string(git_repo.path().join("f.txt")).expect("read git merged file")
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );
}

#[test]
fn mergetool_uses_configured_default_tool_like_stock_git() {
    let git_repo = mergetool_conflict_fixture();
    let zmin_repo = mergetool_conflict_fixture();
    let command = "printf 'B:'; cat \"$BASE\"; printf 'L:'; cat \"$LOCAL\"; printf 'R:'; cat \"$REMOTE\"; printf 'resolved\\n' > \"$MERGED\"";
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "merge.tool", "zmintest"]);
        git(repo, ["config", "mergetool.zmintest.cmd", command]);
        git(
            repo,
            ["config", "mergetool.zmintest.trustExitCode", "true"],
        );
        git(repo, ["config", "mergetool.prompt", "false"]);
    }

    let args = ["mergetool", "--no-prompt", "f.txt"];
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &args),
        git_args(git_repo.path(), &args)
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("f.txt")).expect("read zmin merged file"),
        fs::read_to_string(git_repo.path().join("f.txt")).expect("read git merged file")
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
}

#[test]
fn rerere_status_and_remaining_match_stock_git_for_unmerged_paths() {
    let git_repo = rerere_conflict_fixture();
    let zmin_repo = rerere_conflict_fixture();

    for args in [["rerere", "status"], ["rerere", "remaining"]] {
        assert_eq!(
            run_zmin(zmin_repo.path(), args),
            git(git_repo.path(), args),
            "args: {args:?}"
        );
    }

    let disabled = mergetool_conflict_fixture();
    assert_eq!(run_zmin(disabled.path(), ["rerere", "status"]), "");
    assert_eq!(run_zmin(disabled.path(), ["rerere", "remaining"]), "");
}

#[test]
fn rerere_diff_default_forget_and_invalid_record_match_stock_git() {
    let git_repo = rerere_conflict_fixture();
    let zmin_repo = rerere_conflict_fixture();

    assert_eq!(
        command_all_output(zmin_bin(), zmin_repo.path(), &["rerere", "diff"]),
        command_all_output("git", git_repo.path(), &["rerere", "diff"])
    );

    write_file(git_repo.path(), "f.txt", "resolved\n");
    write_file(zmin_repo.path(), "f.txt", "resolved\n");
    assert_eq!(
        command_all_output(zmin_bin(), zmin_repo.path(), &["rerere"]),
        command_all_output("git", git_repo.path(), &["rerere"])
    );
    assert_eq!(
        command_all_output(
            zmin_bin(),
            zmin_repo.path(),
            &["rerere", "--rerere-autoupdate"]
        ),
        command_all_output("git", git_repo.path(), &["rerere", "--rerere-autoupdate"])
    );
    assert!(
        zmin_repo
            .path()
            .join(".git/rr-cache")
            .read_dir()
            .expect("zmin rr-cache")
            .flatten()
            .any(|entry| entry.path().join("postimage").is_file())
    );

    assert_eq!(
        command_all_output(
            zmin_bin(),
            zmin_repo.path(),
            &["rerere", "forget", "f.txt"]
        ),
        command_all_output("git", git_repo.path(), &["rerere", "forget", "f.txt"])
    );
    assert!(
        !zmin_repo
            .path()
            .join(".git/rr-cache")
            .read_dir()
            .expect("zmin rr-cache")
            .flatten()
            .any(|entry| entry.path().join("postimage").is_file())
    );

    assert_eq!(
        command_all_output(zmin_bin(), zmin_repo.path(), &["rerere", "record"]),
        command_all_output("git", git_repo.path(), &["rerere", "record"])
    );
    assert_eq!(
        command_all_output(zmin_bin(), zmin_repo.path(), &["rerere", "gc"]),
        command_all_output("git", git_repo.path(), &["rerere", "gc"])
    );
}

#[test]
fn rerere_forget_matches_stock_git_for_pathspecs_and_duplicate_preimages() {
    let git_repo = rerere_multi_conflict_fixture();
    let zmin_repo = rerere_multi_conflict_fixture();
    write_file(git_repo.path(), "a.txt", "resolved a\n");
    write_file(git_repo.path(), "b.md", "resolved b\n");
    write_file(zmin_repo.path(), "a.txt", "resolved a\n");
    write_file(zmin_repo.path(), "b.md", "resolved b\n");
    assert_eq!(
        command_all_output("git", git_repo.path(), &["rerere"]),
        command_all_output(zmin_bin(), zmin_repo.path(), &["rerere"])
    );

    assert_eq!(
        command_all_output("git", git_repo.path(), &["rerere", "forget", "*.txt"]),
        command_all_output(
            zmin_bin(),
            zmin_repo.path(),
            &["rerere", "forget", "*.txt"]
        )
    );
    assert_eq!(
        git(git_repo.path(), ["rerere", "diff"]),
        run_zmin(zmin_repo.path(), ["rerere", "diff"])
    );
}

#[test]
fn rerere_reuses_recorded_resolution_and_autoupdates_index_like_stock_git() {
    let git_repo = rerere_conflict_fixture();
    let zmin_repo = rerere_conflict_fixture();
    let git_conflict = fs::read_to_string(git_repo.path().join("f.txt")).expect("git conflict");
    let zmin_conflict =
        fs::read_to_string(zmin_repo.path().join("f.txt")).expect("zmin conflict");
    write_file(git_repo.path(), "f.txt", "resolved\n");
    write_file(zmin_repo.path(), "f.txt", "resolved\n");
    assert_eq!(
        command_all_output("git", git_repo.path(), &["rerere"]),
        command_all_output(zmin_bin(), zmin_repo.path(), &["rerere"])
    );

    write_file(git_repo.path(), "f.txt", &git_conflict);
    write_file(zmin_repo.path(), "f.txt", &zmin_conflict);
    assert_eq!(
        command_all_output("git", git_repo.path(), &["rerere"]),
        command_all_output(zmin_bin(), zmin_repo.path(), &["rerere"])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("f.txt")).expect("zmin resolved"),
        fs::read_to_string(git_repo.path().join("f.txt")).expect("git resolved")
    );

    write_file(git_repo.path(), "f.txt", &git_conflict);
    write_file(zmin_repo.path(), "f.txt", &zmin_conflict);
    assert_eq!(
        command_all_output("git", git_repo.path(), &["rerere", "--rerere-autoupdate"]),
        command_all_output(
            zmin_bin(),
            zmin_repo.path(),
            &["rerere", "--rerere-autoupdate"]
        )
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-u"]),
        git(git_repo.path(), ["ls-files", "-u"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["status", "--short"]),
        git(git_repo.path(), ["status", "--short"])
    );
}

#[test]
#[cfg(unix)]
fn rerere_gc_expiry_matches_stock_git_for_resolved_and_unresolved_cache() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        for key in [
            "1111111111111111111111111111111111111111",
            "2222222222222222222222222222222222222222",
            "3333333333333333333333333333333333333333",
        ] {
            fs::create_dir_all(repo.join(".git/rr-cache").join(key)).expect("create rr-cache dir");
        }
        write_file(
            repo,
            ".git/rr-cache/1111111111111111111111111111111111111111/preimage",
            "pre\n",
        );
        write_file(
            repo,
            ".git/rr-cache/1111111111111111111111111111111111111111/postimage",
            "post\n",
        );
        write_file(
            repo,
            ".git/rr-cache/2222222222222222222222222222222222222222/preimage",
            "pre\n",
        );
        write_file(
            repo,
            ".git/rr-cache/3333333333333333333333333333333333333333/preimage",
            "pre\n",
        );
        write_file(
            repo,
            ".git/rr-cache/3333333333333333333333333333333333333333/postimage",
            "post\n",
        );
        Command::new("touch")
            .args([
                "-t",
                "202001010000",
                ".git/rr-cache/1111111111111111111111111111111111111111/preimage",
                ".git/rr-cache/1111111111111111111111111111111111111111/postimage",
                ".git/rr-cache/2222222222222222222222222222222222222222/preimage",
            ])
            .current_dir(repo)
            .status()
            .expect("touch rr-cache files");
    }

    assert_eq!(
        command_all_output("git", git_repo.path(), &["rerere", "gc"]),
        command_all_output(zmin_bin(), zmin_repo.path(), &["rerere", "gc"])
    );
    let list = |repo: &Path| {
        let mut files = fs::read_dir(repo.join(".git/rr-cache"))
            .expect("read rr-cache")
            .flatten()
            .flat_map(|entry| {
                fs::read_dir(entry.path())
                    .expect("read rr-cache entry")
                    .flatten()
                    .map(move |file| {
                        file.path()
                            .strip_prefix(repo.join(".git/rr-cache"))
                            .expect("strip rr-cache")
                            .to_string_lossy()
                            .to_string()
                    })
            })
            .collect::<Vec<_>>();
        files.sort();
        files
    };
    assert_eq!(list(zmin_repo.path()), list(git_repo.path()));
}
