mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{
    clone_repo_fixture, command_any_output, command_stdout_bytes, command_stdout_bytes_with_stdin,
    configure_identity, git, git_args, git_init, git_status, git_status_args, git_with_env,
    git_with_stdin, git_with_stdin_args, git_with_stdin_bytes, run_zmin, run_zmin_args,
    run_zmin_status, run_zmin_with_env, run_zmin_with_stdin_args, run_zmin_with_stdin_bytes,
    write_file, zmin_bin,
};
use tempfile::TempDir;
use zmin_git_core::{GitHashAlgorithm, GitObjectHash};

#[test]
fn fsck_matches_stock_git_for_healthy_repo_and_detects_corrupt_object() {
    let repo = committed_repo();
    assert_eq!(run_zmin(repo.path(), ["fsck"]), git(repo.path(), ["fsck"]));
    assert_eq!(
        run_zmin(repo.path(), ["fsck", "--name-objects"]),
        git(repo.path(), ["fsck", "--name-objects"])
    );

    let blob = git(repo.path(), ["rev-parse", "HEAD:a.txt"]);
    fs::write(loose_object_path(repo.path(), &blob), b"not a zlib object")
        .expect("corrupt loose object");
    assert_eq!(
        run_zmin_status(repo.path(), ["fsck"]),
        git_status(repo.path(), ["fsck"])
    );
}

#[test]
fn fsck_missing_email_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let tree = git_with_stdin(repo.path(), ["mktree"], "");
    let commit = format!(
        "tree {tree}\nauthor Missing Email 1700000000 +0000\ncommitter Valid <valid@example.invalid> 1700000000 +0000\n\nbad author\n"
    );
    let commit_path = repo.path().join("bad-commit.txt");
    fs::write(&commit_path, commit).expect("write malformed commit");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "commit",
            "-w",
            commit_path.to_str().expect("commit path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/heads/main", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.missingEmail=warn", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.missingEmail=warn", "fsck"],
            "git",
        )
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.missingEmail=ignore", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.missingEmail=ignore", "fsck"],
            "git",
        )
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.missingEmail=bogus", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.missingEmail=bogus", "fsck"],
            "git",
        )
    );
}

#[test]
fn fsck_connectivity_only_skips_message_validation_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let tree = git_with_stdin(repo.path(), ["mktree"], "");
    let commit = format!(
        "tree {tree}\nauthor Missing Email 1700000000 +0000\ncommitter Valid <valid@example.invalid> 1700000000 +0000\n\nbad author\n"
    );
    let commit_path = repo.path().join("bad-commit.txt");
    fs::write(&commit_path, commit).expect("write malformed commit");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "commit",
            "-w",
            commit_path.to_str().expect("commit path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/heads/main", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["fsck", "--connectivity-only"],
            "zmin",
        ),
        command_any_output("git", repo.path(), &["fsck", "--connectivity-only"], "git")
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["fsck", "--connectivity-only", "--no-dangling"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["fsck", "--connectivity-only", "--no-dangling"],
            "git",
        )
    );
}

#[test]
fn fsck_bad_email_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let tree = git_with_stdin(repo.path(), ["mktree"], "");
    let commit = format!(
        "tree {tree}\nauthor Bad Email <bad@example.invalid 1700000000 +0000\ncommitter Valid <valid@example.invalid> 1700000000 +0000\n\nbad email\n"
    );
    let commit_path = repo.path().join("bad-commit.txt");
    fs::write(&commit_path, commit).expect("write malformed commit");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "commit",
            "-w",
            commit_path.to_str().expect("commit path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/heads/main", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.badEmail={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "badEmail={severity}"
        );
    }
}

#[test]
fn fsck_missing_author_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let tree = git_with_stdin(repo.path(), ["mktree"], "");
    let commit = format!(
        "tree {tree}\ncommitter Valid <valid@example.invalid> 1700000000 +0000\n\nmissing author\n"
    );
    let commit_path = repo.path().join("bad-commit.txt");
    fs::write(&commit_path, commit).expect("write malformed commit");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "commit",
            "-w",
            commit_path.to_str().expect("commit path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/heads/main", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.missingAuthor={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "missingAuthor={severity}"
        );
    }
}

#[test]
fn fsck_missing_committer_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let tree = git_with_stdin(repo.path(), ["mktree"], "");
    let commit = format!(
        "tree {tree}\nauthor Valid <valid@example.invalid> 1700000000 +0000\n\nmissing committer\n"
    );
    let commit_path = repo.path().join("bad-commit.txt");
    fs::write(&commit_path, commit).expect("write malformed commit");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "commit",
            "-w",
            commit_path.to_str().expect("commit path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/heads/main", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.missingCommitter={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "missingCommitter={severity}"
        );
    }
}

#[test]
fn fsck_missing_tagger_entry_severity_config_matches_stock_git() {
    let repo = committed_repo();
    let blob = git_with_stdin(
        repo.path(),
        ["hash-object", "-w", "--stdin"],
        "tag target\n",
    );
    let tag = format!("object {blob}\ntype blob\ntag v1\n\nmissing tagger\n");
    let tag_path = repo.path().join("bad-tag.txt");
    fs::write(&tag_path, tag).expect("write malformed tag");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tag",
            "-w",
            tag_path.to_str().expect("tag path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/tags/v1", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.missingTaggerEntry={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "missingTaggerEntry={severity}"
        );
    }
}

#[test]
fn fsck_bad_tag_name_severity_config_matches_stock_git() {
    let repo = committed_repo();
    let blob = git_with_stdin(
        repo.path(),
        ["hash-object", "-w", "--stdin"],
        "tag target\n",
    );
    let tag = format!(
        "object {blob}\ntype blob\ntag bad..name\ntagger Test <test@example.invalid> 1700000000 +0000\n\nbad tag name\n"
    );
    let tag_path = repo.path().join("bad-tag.txt");
    fs::write(&tag_path, tag).expect("write malformed tag");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tag",
            "-w",
            tag_path.to_str().expect("tag path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/tags/good-ref", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.badTagName={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "badTagName={severity}"
        );
    }
}

#[test]
fn fsck_bad_date_tagger_severity_config_matches_stock_git() {
    let repo = committed_repo();
    let blob = git_with_stdin(
        repo.path(),
        ["hash-object", "-w", "--stdin"],
        "tag target\n",
    );
    let tag = format!(
        "object {blob}\ntype blob\ntag bad-date\ntagger Bad Date <bad@example.invalid> abc +0000\n\ntag bad date\n"
    );
    let tag_path = repo.path().join("bad-tag.txt");
    fs::write(&tag_path, tag).expect("write malformed tag");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tag",
            "-w",
            tag_path.to_str().expect("tag path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/tags/bad-date", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.badDate={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "badDate tagger {severity}"
        );
    }
}

#[test]
fn fsck_missing_email_tagger_severity_config_matches_stock_git() {
    let repo = committed_repo();
    let blob = git_with_stdin(
        repo.path(),
        ["hash-object", "-w", "--stdin"],
        "tag target\n",
    );
    let tag = format!(
        "object {blob}\ntype blob\ntag missing-email\ntagger Missing Email 1700000000 +0000\n\ntag missing email\n"
    );
    let tag_path = repo.path().join("bad-tag.txt");
    fs::write(&tag_path, tag).expect("write malformed tag");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tag",
            "-w",
            tag_path.to_str().expect("tag path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/tags/missing-email", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.missingEmail={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "missingEmail tagger {severity}"
        );
    }
}

#[test]
fn fsck_bad_email_tagger_severity_config_matches_stock_git() {
    let repo = committed_repo();
    let blob = git_with_stdin(
        repo.path(),
        ["hash-object", "-w", "--stdin"],
        "tag target\n",
    );
    let tag = format!(
        "object {blob}\ntype blob\ntag bad-email\ntagger Bad Email <bad@example.invalid 1700000000 +0000\n\ntag bad email\n"
    );
    let tag_path = repo.path().join("bad-tag.txt");
    fs::write(&tag_path, tag).expect("write malformed tag");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tag",
            "-w",
            tag_path.to_str().expect("tag path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/tags/bad-email", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.badEmail={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "badEmail tagger {severity}"
        );
    }
}

#[test]
fn fsck_missing_name_before_email_tagger_severity_config_matches_stock_git() {
    let repo = committed_repo();
    let blob = git_with_stdin(
        repo.path(),
        ["hash-object", "-w", "--stdin"],
        "tag target\n",
    );
    let tag = format!(
        "object {blob}\ntype blob\ntag missing-name-email\ntagger <bad@example.invalid> 1700000000 +0000\n\ntag missing name before email\n"
    );
    let tag_path = repo.path().join("bad-tag.txt");
    fs::write(&tag_path, tag).expect("write malformed tag");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tag",
            "-w",
            tag_path.to_str().expect("tag path"),
        ],
    );
    git(
        repo.path(),
        ["update-ref", "refs/tags/missing-name-email", &bad],
    );

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.missingNameBeforeEmail={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "missingNameBeforeEmail tagger {severity}"
        );
    }
}

#[test]
fn fsck_missing_space_before_email_tagger_severity_config_matches_stock_git() {
    let repo = committed_repo();
    let blob = git_with_stdin(
        repo.path(),
        ["hash-object", "-w", "--stdin"],
        "tag target\n",
    );
    let tag = format!(
        "object {blob}\ntype blob\ntag missing-space-email\ntagger Missing Space<bad@example.invalid> 1700000000 +0000\n\ntag missing space before email\n"
    );
    let tag_path = repo.path().join("bad-tag.txt");
    fs::write(&tag_path, tag).expect("write malformed tag");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tag",
            "-w",
            tag_path.to_str().expect("tag path"),
        ],
    );
    git(
        repo.path(),
        ["update-ref", "refs/tags/missing-space-email", &bad],
    );

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.missingSpaceBeforeEmail={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "missingSpaceBeforeEmail tagger {severity}"
        );
    }
}

#[test]
fn fsck_missing_space_before_date_tagger_severity_config_matches_stock_git() {
    let repo = committed_repo();
    let blob = git_with_stdin(
        repo.path(),
        ["hash-object", "-w", "--stdin"],
        "tag target\n",
    );
    let tag = format!(
        "object {blob}\ntype blob\ntag missing-space-date\ntagger No Space <bad@example.invalid>1700000000 +0000\n\ntag missing space before date\n"
    );
    let tag_path = repo.path().join("bad-tag.txt");
    fs::write(&tag_path, tag).expect("write malformed tag");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tag",
            "-w",
            tag_path.to_str().expect("tag path"),
        ],
    );
    git(
        repo.path(),
        ["update-ref", "refs/tags/missing-space-date", &bad],
    );

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.missingSpaceBeforeDate={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "missingSpaceBeforeDate tagger {severity}"
        );
    }
}

#[test]
fn fsck_zero_padded_date_tagger_severity_config_matches_stock_git() {
    let repo = committed_repo();
    let blob = git_with_stdin(
        repo.path(),
        ["hash-object", "-w", "--stdin"],
        "tag target\n",
    );
    let tag = format!(
        "object {blob}\ntype blob\ntag zero-padded-date\ntagger Zero Date <bad@example.invalid> 01700000000 +0000\n\nzero-padded date\n"
    );
    let tag_path = repo.path().join("bad-tag.txt");
    fs::write(&tag_path, tag).expect("write malformed tag");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tag",
            "-w",
            tag_path.to_str().expect("tag path"),
        ],
    );
    git(
        repo.path(),
        ["update-ref", "refs/tags/zero-padded-date", &bad],
    );

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.zeroPaddedDate={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "zeroPaddedDate tagger {severity}"
        );
    }
}

#[test]
fn fsck_bad_date_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let tree = git_with_stdin(repo.path(), ["mktree"], "");
    let commit = format!(
        "tree {tree}\nauthor Bad Date <bad@example.invalid> abc +0000\ncommitter Valid <valid@example.invalid> 1700000000 +0000\n\nbad author date\n"
    );
    let commit_path = repo.path().join("bad-commit.txt");
    fs::write(&commit_path, commit).expect("write malformed commit");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "commit",
            "-w",
            commit_path.to_str().expect("commit path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/heads/main", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.badDate={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "badDate={severity}"
        );
    }
}

#[test]
fn fsck_bad_timezone_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let tree = git_with_stdin(repo.path(), ["mktree"], "");
    let commit = format!(
        "tree {tree}\nauthor Bad Timezone <bad@example.invalid> 1700000000 ZZZZ\ncommitter Valid <valid@example.invalid> 1700000000 +0000\n\nbad author timezone\n"
    );
    let commit_path = repo.path().join("bad-commit.txt");
    fs::write(&commit_path, commit).expect("write malformed commit");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "commit",
            "-w",
            commit_path.to_str().expect("commit path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/heads/main", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.badTimezone={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "badTimezone={severity}"
        );
    }
}

#[test]
fn fsck_missing_space_before_email_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let tree = git_with_stdin(repo.path(), ["mktree"], "");
    let commit = format!(
        "tree {tree}\nauthor Missing Space<bad@example.invalid> 1700000000 +0000\ncommitter Valid <valid@example.invalid> 1700000000 +0000\n\nmissing space before email\n"
    );
    let commit_path = repo.path().join("bad-commit.txt");
    fs::write(&commit_path, commit).expect("write malformed commit");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "commit",
            "-w",
            commit_path.to_str().expect("commit path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/heads/main", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.missingSpaceBeforeEmail={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "missingSpaceBeforeEmail={severity}"
        );
    }
}

#[test]
fn fsck_missing_name_before_email_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let tree = git_with_stdin(repo.path(), ["mktree"], "");
    let commit = format!(
        "tree {tree}\nauthor <bad@example.invalid> 1700000000 +0000\ncommitter Valid <valid@example.invalid> 1700000000 +0000\n\nmissing name before email\n"
    );
    let commit_path = repo.path().join("bad-commit.txt");
    fs::write(&commit_path, commit).expect("write malformed commit");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "commit",
            "-w",
            commit_path.to_str().expect("commit path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/heads/main", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.missingNameBeforeEmail={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "missingNameBeforeEmail={severity}"
        );
    }
}

#[test]
fn fsck_missing_space_before_date_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let tree = git_with_stdin(repo.path(), ["mktree"], "");
    let commit = format!(
        "tree {tree}\nauthor Missing Date Space <bad@example.invalid>1700000000 +0000\ncommitter Valid <valid@example.invalid> 1700000000 +0000\n\nmissing space before date\n"
    );
    let commit_path = repo.path().join("bad-commit.txt");
    fs::write(&commit_path, commit).expect("write malformed commit");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "commit",
            "-w",
            commit_path.to_str().expect("commit path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/heads/main", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.missingSpaceBeforeDate={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "missingSpaceBeforeDate={severity}"
        );
    }
}

#[test]
fn fsck_zero_padded_date_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let tree = git_with_stdin(repo.path(), ["mktree"], "");
    let commit = format!(
        "tree {tree}\nauthor Zero Date <bad@example.invalid> 01700000000 +0000\ncommitter Valid <valid@example.invalid> 1700000000 +0000\n\nzero-padded date\n"
    );
    let commit_path = repo.path().join("bad-commit.txt");
    fs::write(&commit_path, commit).expect("write malformed commit");
    let bad = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "commit",
            "-w",
            commit_path.to_str().expect("commit path"),
        ],
    );
    git(repo.path(), ["update-ref", "refs/heads/main", &bad]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.zeroPaddedDate={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "zeroPaddedDate={severity}"
        );
    }
}

#[test]
fn fsck_zero_padded_filemode_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let blob = git_with_stdin(repo.path(), ["hash-object", "-w", "--stdin"], "x\n");
    let mut tree = b"0100644 a.txt\0".to_vec();
    tree.extend_from_slice(&hex_to_bytes(&blob));
    let tree_path = repo.path().join("bad-tree.bin");
    fs::write(&tree_path, tree).expect("write malformed tree");
    let tree = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tree",
            "-w",
            tree_path.to_str().expect("tree path"),
        ],
    );
    let commit = git(repo.path(), ["commit-tree", &tree, "-m", "bad tree"]);
    git(repo.path(), ["update-ref", "refs/heads/main", &commit]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.zeroPaddedFilemode=error", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.zeroPaddedFilemode=error", "fsck"],
            "git",
        )
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.zeroPaddedFilemode=warn", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.zeroPaddedFilemode=warn", "fsck"],
            "git",
        )
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.zeroPaddedFilemode=ignore", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.zeroPaddedFilemode=ignore", "fsck"],
            "git",
        )
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.zeroPaddedFilemode=bogus", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.zeroPaddedFilemode=bogus", "fsck"],
            "git",
        )
    );
}

#[test]
fn fsck_bad_filemode_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let blob = git_with_stdin(repo.path(), ["hash-object", "-w", "--stdin"], "x\n");
    let mut tree = b"100777 a.txt\0".to_vec();
    tree.extend_from_slice(&hex_to_bytes(&blob));
    let tree_path = repo.path().join("bad-tree.bin");
    fs::write(&tree_path, tree).expect("write malformed tree");
    let tree = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tree",
            "-w",
            tree_path.to_str().expect("tree path"),
        ],
    );
    let commit = git(repo.path(), ["commit-tree", &tree, "-m", "bad tree"]);
    git(repo.path(), ["update-ref", "refs/heads/main", &commit]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.badFilemode=error", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.badFilemode=error", "fsck"],
            "git",
        )
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.badFilemode=warn", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.badFilemode=warn", "fsck"],
            "git",
        )
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.badFilemode=ignore", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.badFilemode=ignore", "fsck"],
            "git",
        )
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.badFilemode=bogus", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.badFilemode=bogus", "fsck"],
            "git",
        )
    );
}

#[test]
fn fsck_tree_not_sorted_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let blob = git_with_stdin(repo.path(), ["hash-object", "-w", "--stdin"], "x\n");
    let blob_bytes = hex_to_bytes(&blob);
    let mut tree = b"100644 b.txt\0".to_vec();
    tree.extend_from_slice(&blob_bytes);
    tree.extend_from_slice(b"100644 a.txt\0");
    tree.extend_from_slice(&blob_bytes);
    let tree_path = repo.path().join("bad-tree.bin");
    fs::write(&tree_path, tree).expect("write unsorted tree");
    let tree = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tree",
            "-w",
            tree_path.to_str().expect("tree path"),
        ],
    );
    let commit = git(repo.path(), ["commit-tree", &tree, "-m", "bad tree"]);
    git(repo.path(), ["update-ref", "refs/heads/main", &commit]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.treeNotSorted=error", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.treeNotSorted=error", "fsck"],
            "git",
        )
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.treeNotSorted=warn", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.treeNotSorted=warn", "fsck"],
            "git",
        )
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.treeNotSorted=ignore", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.treeNotSorted=ignore", "fsck"],
            "git",
        )
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["-c", "fsck.treeNotSorted=bogus", "fsck"],
            "zmin",
        ),
        command_any_output(
            "git",
            repo.path(),
            &["-c", "fsck.treeNotSorted=bogus", "fsck"],
            "git",
        )
    );
}

#[test]
fn fsck_special_tree_name_severity_config_matches_stock_git() {
    for (name, config_key) in [(".", "hasDot"), ("..", "hasDotdot"), (".git", "hasDotgit")] {
        let repo = git_init();
        configure_identity(repo.path());
        git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
        let blob = git_with_stdin(repo.path(), ["hash-object", "-w", "--stdin"], "x\n");
        let mut tree = b"100644 ".to_vec();
        tree.extend_from_slice(name.as_bytes());
        tree.push(0);
        tree.extend_from_slice(&hex_to_bytes(&blob));
        let tree_path = repo.path().join("bad-tree.bin");
        fs::write(&tree_path, tree).expect("write malformed tree");
        let tree = git(
            repo.path(),
            [
                "hash-object",
                "--literally",
                "-t",
                "tree",
                "-w",
                tree_path.to_str().expect("tree path"),
            ],
        );
        let commit = git(repo.path(), ["commit-tree", &tree, "-m", "bad tree"]);
        git(repo.path(), ["update-ref", "refs/heads/main", &commit]);

        assert_eq!(
            command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
            command_any_output("git", repo.path(), &["fsck"], "git")
        );
        for severity in ["error", "warn", "ignore", "bogus"] {
            let config = format!("fsck.{config_key}={severity}");
            assert_eq!(
                command_any_output(
                    zmin_bin(),
                    repo.path(),
                    &["-c", config.as_str(), "fsck"],
                    "zmin",
                ),
                command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
                "{config_key}={severity}"
            );
        }
    }
}

#[test]
fn fsck_has_dotgit_platform_variants_match_stock_git() {
    for name in [
        ".GIT",
        ".git.",
        ".GIT...",
        "git~1",
        "GIT~1",
        ".git/config",
        ".GIT/config",
        ".git./config",
        "git~1/config",
    ] {
        let repo = git_init();
        configure_identity(repo.path());
        git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
        let blob = git_with_stdin(repo.path(), ["hash-object", "-w", "--stdin"], "x\n");
        let mut tree = b"100644 ".to_vec();
        tree.extend_from_slice(name.as_bytes());
        tree.push(0);
        tree.extend_from_slice(&hex_to_bytes(&blob));
        let tree_path = repo.path().join("bad-tree.bin");
        fs::write(&tree_path, tree).expect("write malformed tree");
        let tree = git(
            repo.path(),
            [
                "hash-object",
                "--literally",
                "-t",
                "tree",
                "-w",
                tree_path.to_str().expect("tree path"),
            ],
        );
        let commit = git(repo.path(), ["commit-tree", &tree, "-m", "bad tree"]);
        git(repo.path(), ["update-ref", "refs/heads/main", &commit]);

        assert_eq!(
            command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
            command_any_output("git", repo.path(), &["fsck"], "git"),
            "hasDotgit variant {name}"
        );
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", "fsck.hasDotgit=ignore", "fsck"],
                "zmin",
            ),
            command_any_output(
                "git",
                repo.path(),
                &["-c", "fsck.hasDotgit=ignore", "fsck"],
                "git",
            ),
            "hasDotgit ignore variant {name}"
        );
    }
}

#[test]
fn fsck_full_pathname_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let blob = git_with_stdin(repo.path(), ["hash-object", "-w", "--stdin"], "x\n");
    let mut tree = b"100644 a/b\0".to_vec();
    tree.extend_from_slice(&hex_to_bytes(&blob));
    let tree_path = repo.path().join("bad-tree.bin");
    fs::write(&tree_path, tree).expect("write malformed tree");
    let tree = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tree",
            "-w",
            tree_path.to_str().expect("tree path"),
        ],
    );
    let commit = git(repo.path(), ["commit-tree", &tree, "-m", "bad tree"]);
    git(repo.path(), ["update-ref", "refs/heads/main", &commit]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.fullPathname={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "fullPathname={severity}"
        );
    }
}

#[test]
fn fsck_duplicate_entries_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let blob = git_with_stdin(repo.path(), ["hash-object", "-w", "--stdin"], "x\n");
    let blob_bytes = hex_to_bytes(&blob);
    let mut tree = b"100644 a.txt\0".to_vec();
    tree.extend_from_slice(&blob_bytes);
    tree.extend_from_slice(b"100644 a.txt\0");
    tree.extend_from_slice(&blob_bytes);
    let tree_path = repo.path().join("bad-tree.bin");
    fs::write(&tree_path, tree).expect("write malformed tree");
    let tree = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tree",
            "-w",
            tree_path.to_str().expect("tree path"),
        ],
    );
    let commit = git(repo.path(), ["commit-tree", &tree, "-m", "bad tree"]);
    git(repo.path(), ["update-ref", "refs/heads/main", &commit]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.duplicateEntries={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "duplicateEntries={severity}"
        );
    }
}

#[test]
fn fsck_null_sha1_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let mut tree = b"100644 zero\0".to_vec();
    tree.extend_from_slice(&[0; 20]);
    let tree_path = repo.path().join("bad-tree.bin");
    fs::write(&tree_path, tree).expect("write malformed tree");
    let tree = git(
        repo.path(),
        [
            "hash-object",
            "--literally",
            "-t",
            "tree",
            "-w",
            tree_path.to_str().expect("tree path"),
        ],
    );
    let commit = git(repo.path(), ["commit-tree", &tree, "-m", "null sha1"]);
    git(repo.path(), ["update-ref", "refs/heads/main", &commit]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.nullSha1={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "nullSha1={severity}"
        );
    }
}

#[test]
fn fsck_gitmodules_parse_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    write_file(
        repo.path(),
        "nested/.GITMODULES",
        "[submodule \"nested\"\n\tpath = deps/nested\n\turl = https://example.invalid/nested.git\n",
    );
    git(repo.path(), ["add", "nested/.GITMODULES"]);
    git_with_env(repo.path(), ["commit", "-m", "bad gitmodules"]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.gitmodulesParse={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "gitmodulesParse={severity}"
        );
    }
}

#[test]
fn fsck_gitmodules_blob_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let empty_tree = git_with_stdin(repo.path(), ["mktree"], "");
    let tree = git_with_stdin(
        repo.path(),
        ["mktree"],
        &format!("040000 tree {empty_tree}\t.gitmodules\n"),
    );
    let commit = git_with_stdin(repo.path(), ["commit-tree", &tree], "bad gitmodules blob\n");
    git(repo.path(), ["update-ref", "refs/heads/main", &commit]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.gitmodulesBlob={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "gitmodulesBlob={severity}"
        );
    }
}

#[test]
fn fsck_gitmodules_missing_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let missing = "0123456789012345678901234567890123456789";
    let tree = git_with_stdin(
        repo.path(),
        ["mktree", "--missing"],
        &format!("100644 blob {missing}\t.gitmodules\n"),
    );
    let commit = git_with_stdin(repo.path(), ["commit-tree", &tree], "missing gitmodules\n");
    git(repo.path(), ["update-ref", "refs/heads/main", &commit]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.gitmodulesMissing={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "gitmodulesMissing={severity}"
        );
    }
}

#[test]
fn fsck_gitmodules_name_config_validation_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    write_file(
        repo.path(),
        ".gitmodules",
        "[submodule \"-bad\"]\n\tpath = deps/bad\n\turl = https://example.invalid/bad.git\n",
    );
    git(repo.path(), ["add", ".gitmodules"]);
    git_with_env(repo.path(), ["commit", "-m", "gitmodules name"]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.gitmodulesName={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "gitmodulesName={severity}"
        );
    }
}

#[test]
fn fsck_gitmodules_url_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    write_file(
        repo.path(),
        ".gitmodules",
        "[submodule \"bad-url\"]\n\tpath = deps/bad-url\n\turl = -bad\n",
    );
    git(repo.path(), ["add", ".gitmodules"]);
    git_with_env(repo.path(), ["commit", "-m", "bad gitmodules url"]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.gitmodulesUrl={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "gitmodulesUrl={severity}"
        );
    }
}

#[test]
fn fsck_gitmodules_path_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    write_file(
        repo.path(),
        ".gitmodules",
        "[submodule \"bad-path\"]\n\tpath = -bad\n\turl = https://example.invalid/bad.git\n",
    );
    git(repo.path(), ["add", ".gitmodules"]);
    git_with_env(repo.path(), ["commit", "-m", "bad gitmodules path"]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.gitmodulesPath={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "gitmodulesPath={severity}"
        );
    }
}

#[test]
fn fsck_gitmodules_update_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    write_file(
        repo.path(),
        ".gitmodules",
        "[submodule \"bad-update\"]\n\tpath = deps/bad-update\n\turl = https://example.invalid/bad.git\n\tupdate = !cmd\n",
    );
    git(repo.path(), ["add", ".gitmodules"]);
    git_with_env(repo.path(), ["commit", "-m", "bad gitmodules update"]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.gitmodulesUpdate={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "gitmodulesUpdate={severity}"
        );
    }
}

#[test]
fn fsck_gitmodules_symlink_severity_config_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["symbolic-ref", "HEAD", "refs/heads/main"]);
    let blob = git_with_stdin(repo.path(), ["hash-object", "-w", "--stdin"], "real");
    let tree = git_with_stdin(
        repo.path(),
        ["mktree"],
        &format!("120000 blob {blob}\t.gitmodules\n"),
    );
    let commit = git_with_stdin(repo.path(), ["commit-tree", &tree], "bad symlink\n");
    git(repo.path(), ["update-ref", "refs/heads/main", &commit]);

    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &["fsck"], "zmin"),
        command_any_output("git", repo.path(), &["fsck"], "git")
    );
    for severity in ["error", "warn", "ignore", "bogus"] {
        let config = format!("fsck.gitmodulesSymlink={severity}");
        assert_eq!(
            command_any_output(
                zmin_bin(),
                repo.path(),
                &["-c", config.as_str(), "fsck"],
                "zmin",
            ),
            command_any_output("git", repo.path(), &["-c", config.as_str(), "fsck"], "git",),
            "gitmodulesSymlink={severity}"
        );
    }
}

#[test]
fn fsck_lost_found_writes_dangling_objects_like_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    let blob = git_with_stdin(
        git_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "dangling blob\n",
    );
    git_with_stdin(
        zmin_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "dangling blob\n",
    );

    assert_eq!(
        run_zmin(zmin_repo.path(), ["fsck", "--lost-found"]),
        git(git_repo.path(), ["fsck", "--lost-found"])
    );
    assert_eq!(
        fs::read(zmin_repo.path().join(".git/lost-found/other").join(&blob)).expect("zmin blob"),
        fs::read(git_repo.path().join(".git/lost-found/other").join(&blob)).expect("git blob")
    );
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).expect("hex byte"))
        .collect()
}

#[test]
fn verify_pack_matches_stock_git_for_default_and_stats() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "file.txt", "content\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "pack me"]);
    git(repo.path(), ["repack", "-adq"]);
    let idx = first_pack_index(repo.path());
    let idx = idx.to_str().expect("idx path");

    assert_eq!(
        run_zmin(repo.path(), ["verify-pack", idx]),
        git(repo.path(), ["verify-pack", idx])
    );
    assert_eq!(
        run_zmin(repo.path(), ["verify-pack", "-v", idx]),
        git(repo.path(), ["verify-pack", "-v", idx])
    );
    assert_eq!(
        run_zmin(repo.path(), ["verify-pack", "-s", idx]),
        git(repo.path(), ["verify-pack", "-s", idx])
    );

    let corrupt_repo = git_init();
    configure_identity(corrupt_repo.path());
    write_file(corrupt_repo.path(), "file.txt", "content\n");
    git(corrupt_repo.path(), ["add", "-A"]);
    git_with_env(corrupt_repo.path(), ["commit", "-m", "pack me"]);
    git(corrupt_repo.path(), ["repack", "-adq"]);
    let corrupt_idx = first_pack_index(corrupt_repo.path());
    let corrupt_pack = corrupt_idx.with_extension("pack");
    flip_last_byte(&corrupt_pack);
    let zmin_corrupt = command_any_output(
        zmin_bin(),
        corrupt_repo.path(),
        &["verify-pack", corrupt_idx.to_str().expect("idx path")],
        "zmin",
    );
    let git_corrupt = command_any_output(
        "git",
        corrupt_repo.path(),
        &["verify-pack", corrupt_idx.to_str().expect("idx path")],
        "git",
    );
    assert_eq!(zmin_corrupt.0, git_corrupt.0);
    assert_ne!(zmin_corrupt.0, 0);

    let corrupt_idx_repo = git_init();
    configure_identity(corrupt_idx_repo.path());
    write_file(corrupt_idx_repo.path(), "file.txt", "content\n");
    git(corrupt_idx_repo.path(), ["add", "-A"]);
    git_with_env(corrupt_idx_repo.path(), ["commit", "-m", "pack me"]);
    git(corrupt_idx_repo.path(), ["repack", "-adq"]);
    let corrupt_idx = first_pack_index(corrupt_idx_repo.path());
    flip_last_byte(&corrupt_idx);
    assert_eq!(
        command_any_output(
            zmin_bin(),
            corrupt_idx_repo.path(),
            &["verify-pack", corrupt_idx.to_str().expect("idx path")],
            "zmin"
        ),
        command_any_output(
            "git",
            corrupt_idx_repo.path(),
            &["verify-pack", corrupt_idx.to_str().expect("idx path")],
            "git"
        )
    );
}

#[test]
fn verify_pack_rejects_unsupported_pack_index_version_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "file.txt", "content\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "pack me"]);
    git(repo.path(), ["repack", "-adq"]);
    let idx = first_pack_index(repo.path());
    set_pack_index_version(&idx, 3);

    let args = ["verify-pack", idx.to_str().expect("idx path")];
    assert_eq!(
        command_any_output(zmin_bin(), repo.path(), &args, "zmin"),
        command_any_output("git", repo.path(), &args, "git")
    );
}

#[test]
fn pack_redundant_matches_stock_git_for_redundant_pack_set() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "one.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    let first_commit = git(repo.path(), ["rev-parse", "HEAD"]);
    let first_objects = git(
        repo.path(),
        [
            "rev-list",
            "--objects",
            "--no-object-names",
            first_commit.as_str(),
        ],
    );
    git_with_stdin_args(
        repo.path(),
        &["pack-objects", ".git/objects/pack/pack"],
        &first_objects,
    );

    write_file(repo.path(), "two.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    let all_objects = git(
        repo.path(),
        ["rev-list", "--objects", "--no-object-names", "--all"],
    );
    git_with_stdin_args(
        repo.path(),
        &["pack-objects", ".git/objects/pack/pack"],
        &all_objects,
    );

    assert_eq!(
        run_zmin(
            repo.path(),
            ["pack-redundant", "--i-still-use-this", "--all"]
        ),
        git(
            repo.path(),
            ["pack-redundant", "--i-still-use-this", "--all"]
        )
    );
}

#[test]
fn verify_commit_and_tag_match_stock_git_for_unsigned_and_signed_fixtures() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "file.txt", "content\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "unsigned"]);
    git(repo.path(), ["tag", "-a", "-m", "tag", "v1"]);
    git(repo.path(), ["tag", "lightweight"]);

    assert_eq!(
        run_zmin_status(repo.path(), ["verify-commit", "HEAD"]),
        git_status(repo.path(), ["verify-commit", "HEAD"])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["verify-tag", "v1"]),
        git_status(repo.path(), ["verify-tag", "v1"])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["verify-tag", "lightweight"]),
        git_status(repo.path(), ["verify-tag", "lightweight"])
    );

    let head = git(repo.path(), ["rev-parse", "HEAD"]);
    let tag_content = format!(
        "object {head}\n\
         type commit\n\
         tag signed-fixture\n\
         tagger Bench <bench@example.test> 1700000000 +0000\n\
         \n\
         signed fixture\n\
         -----BEGIN PGP SIGNATURE-----\n\
         \n\
         ZmFrZQo=\n\
         -----END PGP SIGNATURE-----\n"
    );
    let signed_tag = git_with_stdin_args(
        repo.path(),
        &["hash-object", "-t", "tag", "-w", "--stdin"],
        &tag_content,
    );
    git(
        repo.path(),
        ["update-ref", "refs/tags/signed-fixture", &signed_tag],
    );

    let gpg_dir = TempDir::new().expect("fake gpg dir");
    let good_gpg = write_fake_gpg(gpg_dir.path(), "fake-gpg-good", FakeGpgMode::Good);
    git(
        repo.path(),
        [
            "config",
            "gpg.program",
            good_gpg.to_str().expect("gpg path"),
        ],
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["verify-tag", "--raw", "signed-fixture"],
            "zmin"
        ),
        command_any_output(
            "git",
            repo.path(),
            &["verify-tag", "--raw", "signed-fixture"],
            "git"
        )
    );

    let bad_gpg = write_fake_gpg(gpg_dir.path(), "fake-gpg-bad", FakeGpgMode::Bad);
    git(
        repo.path(),
        ["config", "gpg.program", bad_gpg.to_str().expect("gpg path")],
    );
    let zmin_bad = command_any_output(
        zmin_bin(),
        repo.path(),
        &["verify-tag", "--raw", "signed-fixture"],
        "zmin",
    );
    let git_bad = command_any_output(
        "git",
        repo.path(),
        &["verify-tag", "--raw", "signed-fixture"],
        "git",
    );
    assert_eq!(zmin_bad, git_bad);
    assert_ne!(zmin_bad.0, 0);

    let tree = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);
    let signed_commit_content = format!(
        "tree {tree}\n\
         author Bench <bench@example.test> 1700000000 +0000\n\
         committer Bench <bench@example.test> 1700000000 +0000\n\
         gpgsig -----BEGIN PGP SIGNATURE-----\n\
         \n\
          ZmFrZQo=\n\
          -----END PGP SIGNATURE-----\n\
         \n\
         signed commit fixture\n"
    );
    let signed_commit = git_with_stdin_args(
        repo.path(),
        &["hash-object", "-t", "commit", "-w", "--stdin"],
        &signed_commit_content,
    );
    git(
        repo.path(),
        [
            "update-ref",
            "refs/heads/signed-commit-fixture",
            &signed_commit,
        ],
    );
    git(
        repo.path(),
        [
            "config",
            "gpg.program",
            good_gpg.to_str().expect("gpg path"),
        ],
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["verify-commit", "--raw", "signed-commit-fixture"],
            "zmin"
        ),
        command_any_output(
            "git",
            repo.path(),
            &["verify-commit", "--raw", "signed-commit-fixture"],
            "git"
        )
    );
    git(
        repo.path(),
        ["config", "gpg.program", bad_gpg.to_str().expect("gpg path")],
    );
    let zmin_bad_commit = command_any_output(
        zmin_bin(),
        repo.path(),
        &["verify-commit", "--raw", "signed-commit-fixture"],
        "zmin",
    );
    let git_bad_commit = command_any_output(
        "git",
        repo.path(),
        &["verify-commit", "--raw", "signed-commit-fixture"],
        "git",
    );
    assert_eq!(zmin_bad_commit, git_bad_commit);
    assert_ne!(zmin_bad_commit.0, 0);
}

#[test]
fn unpack_objects_writes_pack_objects_readable_by_stock_git() {
    let source = git_init();
    configure_identity(source.path());
    write_file(
        source.path(),
        "delta.txt",
        &format!("{}\nbase\n", "same\n".repeat(1_000)),
    );
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "base"]);
    write_file(
        source.path(),
        "delta.txt",
        &format!("{}\nchanged\n", "same\n".repeat(1_000)),
    );
    write_file(source.path(), "extra.txt", "extra\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "changed"]);
    let pack = command_stdout_bytes_with_stdin(
        "git",
        source.path(),
        &["pack-objects", "--stdout", "--revs"],
        b"HEAD\n",
    );

    let target = git_init();
    run_zmin_with_stdin_bytes(target.path(), ["unpack-objects", "-q"], &pack);
    let objects = git(source.path(), ["rev-list", "--objects", "HEAD"]);
    for line in objects.lines() {
        let id = line.split_whitespace().next().expect("object id");
        assert_eq!(
            git(target.path(), ["cat-file", "-t", id]),
            git(source.path(), ["cat-file", "-t", id]),
            "object type for {id}"
        );
        assert_eq!(
            command_stdout_bytes("git", target.path(), &["cat-file", "-p", id]),
            command_stdout_bytes("git", source.path(), &["cat-file", "-p", id]),
            "object content for {id}"
        );
    }
}

#[test]
fn unpack_objects_strict_accepts_stock_git_pack() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    write_file(source.path(), "a.txt", "two\n");
    write_file(source.path(), "b.txt", "bee\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "two"]);
    let pack = command_stdout_bytes_with_stdin(
        "git",
        source.path(),
        &["pack-objects", "--stdout", "--revs"],
        b"HEAD\n",
    );

    let git_target = git_init();
    let zmin_target = git_init();
    git_with_stdin_bytes(
        git_target.path(),
        ["unpack-objects", "--strict", "-q"],
        &pack,
    );
    run_zmin_with_stdin_bytes(
        zmin_target.path(),
        ["unpack-objects", "--strict", "-q"],
        &pack,
    );

    let objects = git(source.path(), ["rev-list", "--objects", "HEAD"]);
    for line in objects.lines() {
        let id = line.split_whitespace().next().expect("object id");
        assert_eq!(
            git(zmin_target.path(), ["cat-file", "-t", id]),
            git(git_target.path(), ["cat-file", "-t", id]),
            "object type for {id}"
        );
        assert_eq!(
            command_stdout_bytes("git", zmin_target.path(), &["cat-file", "-p", id]),
            command_stdout_bytes("git", git_target.path(), &["cat-file", "-p", id]),
            "object content for {id}"
        );
    }
}

#[test]
fn pack_objects_stdout_writes_pack_readable_by_stock_git() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    write_file(source.path(), "a.txt", "two\n");
    write_file(source.path(), "b.txt", "bee\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "two"]);

    let pack = command_stdout_bytes_with_stdin(
        zmin_bin(),
        source.path(),
        &["pack-objects", "--stdout", "--revs"],
        b"HEAD\n",
    );
    let target = git_init();
    git_with_stdin_bytes(target.path(), ["unpack-objects", "-q"], &pack);

    let objects = git(source.path(), ["rev-list", "--objects", "HEAD"]);
    for line in objects.lines() {
        let id = line.split_whitespace().next().expect("object id");
        assert_eq!(
            git(target.path(), ["cat-file", "-t", id]),
            git(source.path(), ["cat-file", "-t", id]),
            "object type for {id}"
        );
        assert_eq!(
            command_stdout_bytes("git", target.path(), &["cat-file", "-p", id]),
            command_stdout_bytes("git", source.path(), &["cat-file", "-p", id]),
            "object content for {id}"
        );
    }
}

#[test]
fn pack_objects_progress_flags_write_stock_readable_pack() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    write_file(source.path(), "b.txt", "two\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "two"]);

    for flag in ["--progress", "--no-progress"] {
        let pack = command_stdout_bytes_with_stdin(
            zmin_bin(),
            source.path(),
            &["pack-objects", "--stdout", "--revs", flag],
            b"HEAD\n",
        );
        let target = git_init();
        git_with_stdin_bytes(target.path(), ["unpack-objects", "-q"], &pack);
        for line in git(source.path(), ["rev-list", "--objects", "HEAD"]).lines() {
            let id = line.split_whitespace().next().expect("object id");
            assert_eq!(
                command_stdout_bytes("git", target.path(), &["cat-file", "-p", id]),
                command_stdout_bytes("git", source.path(), &["cat-file", "-p", id]),
                "flag: {flag}; object: {id}"
            );
        }
    }
}

#[test]
fn pack_objects_undeltified_compat_flags_write_stock_readable_pack() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    write_file(source.path(), "b.txt", "two\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "two"]);

    for flag in [
        "--index-version=2",
        "--no-reuse-delta",
        "--no-reuse-object",
        "--delta-base-offset",
        "--window=0",
        "--depth=0",
    ] {
        let pack = command_stdout_bytes_with_stdin(
            zmin_bin(),
            source.path(),
            &["pack-objects", "--stdout", "--revs", flag],
            b"HEAD\n",
        );
        let target = git_init();
        git_with_stdin_bytes(target.path(), ["unpack-objects", "-q"], &pack);
        for line in git(source.path(), ["rev-list", "--objects", "HEAD"]).lines() {
            let id = line.split_whitespace().next().expect("object id");
            assert_eq!(
                command_stdout_bytes("git", target.path(), &["cat-file", "-p", id]),
                command_stdout_bytes("git", source.path(), &["cat-file", "-p", id]),
                "flag: {flag}; object: {id}"
            );
        }
    }
}

#[test]
fn pack_objects_window_depth_writes_stock_readable_delta_pack() {
    let source = git_init();
    let base_content = format!("{}\nbase\n", "shared line\n".repeat(2_000));
    let changed_content = format!("{}\nchanged\n", "shared line\n".repeat(2_000));
    let base = git_with_stdin(
        source.path(),
        ["hash-object", "-w", "--stdin"],
        &base_content,
    );
    let changed = git_with_stdin(
        source.path(),
        ["hash-object", "-w", "--stdin"],
        &changed_content,
    );
    let pack_input = format!("{base}\n{changed}\n");
    let pack_id = run_zmin_with_stdin_args(
        source.path(),
        &[
            "pack-objects",
            "--window=10",
            "--depth=10",
            ".git/objects/pack/pack-zmin",
        ],
        &pack_input,
    );
    let idx = source
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-zmin-{pack_id}.idx"));

    let verify = git_args(
        source.path(),
        &["verify-pack", "-v", idx.to_str().expect("idx path")],
    );
    assert!(
        verify
            .lines()
            .any(|line| { line.starts_with(&changed) && line.split_whitespace().count() >= 7 }),
        "expected changed blob to be stored as a delta:\n{verify}"
    );
    let target = git_init();
    let pack = idx.with_extension("pack");
    let indexed = git_with_stdin_bytes(
        target.path(),
        ["index-pack", "--stdin"],
        &fs::read(pack).expect("read delta pack"),
    );
    assert!(indexed.starts_with("pack\t"));
    assert_eq!(
        command_stdout_bytes("git", target.path(), &["cat-file", "-p", &base]),
        base_content.as_bytes()
    );
    assert_eq!(
        command_stdout_bytes("git", target.path(), &["cat-file", "-p", &changed]),
        changed_content.as_bytes()
    );
}

#[test]
fn pack_objects_base_name_writes_stock_readable_pack_and_index() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    write_file(source.path(), "a.txt", "two\n");
    write_file(source.path(), "b.txt", "bee\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "two"]);
    let objects = git(
        source.path(),
        ["rev-list", "--objects", "--no-object-names", "HEAD"],
    );

    let pack_id = run_zmin_with_stdin_args(
        source.path(),
        &["pack-objects", ".git/objects/pack/pack-zmin"],
        &objects,
    );
    let pack_path = source
        .path()
        .join(format!(".git/objects/pack/pack-zmin-{pack_id}.pack"));
    let index_path = source
        .path()
        .join(format!(".git/objects/pack/pack-zmin-{pack_id}.idx"));
    assert!(pack_path.exists(), "pack file was not written");
    assert!(index_path.exists(), "pack index was not written");
    let verify = git(
        source.path(),
        ["verify-pack", "-v", index_path.to_str().expect("idx path")],
    );
    let object_count = objects.lines().filter(|line| !line.is_empty()).count();
    let verified_objects = verify
        .lines()
        .filter(|line| {
            line.split_whitespace().next().is_some_and(|token| {
                token.len() == 40 && token.chars().all(|char| char.is_ascii_hexdigit())
            })
        })
        .count();
    assert_eq!(verified_objects, object_count, "{verify}");
    assert!(verify.ends_with(".pack: ok"), "{verify}");

    let target = git_init();
    git_with_stdin_bytes(
        target.path(),
        ["unpack-objects", "-q"],
        &fs::read(pack_path).expect("read zmin pack"),
    );
    for line in objects.lines() {
        let id = line.split_whitespace().next().expect("object id");
        assert_eq!(
            command_stdout_bytes("git", target.path(), &["cat-file", "-p", id]),
            command_stdout_bytes("git", source.path(), &["cat-file", "-p", id]),
            "object content for {id}"
        );
    }
}

#[test]
fn unpack_objects_recover_accepts_valid_stock_git_pack() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    write_file(source.path(), "a.txt", "two\n");
    git(source.path(), ["commit", "-am", "two"]);
    let pack = command_stdout_bytes_with_stdin(
        "git",
        source.path(),
        &["pack-objects", "--stdout", "--revs"],
        b"HEAD\n",
    );

    let git_target = git_init();
    let zmin_target = git_init();
    git_with_stdin_bytes(git_target.path(), ["unpack-objects", "-r", "-q"], &pack);
    run_zmin_with_stdin_bytes(zmin_target.path(), ["unpack-objects", "-r", "-q"], &pack);

    let objects = git(source.path(), ["rev-list", "--objects", "HEAD"]);
    for line in objects.lines() {
        let id = line.split_whitespace().next().expect("object id");
        assert_eq!(
            command_stdout_bytes("git", zmin_target.path(), &["cat-file", "-p", id]),
            command_stdout_bytes("git", git_target.path(), &["cat-file", "-p", id]),
            "object content for {id}"
        );
    }
}

#[test]
fn index_pack_stdin_writes_stock_readable_pack_index() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    write_file(source.path(), "a.txt", "two\n");
    write_file(source.path(), "b.txt", "bee\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "two"]);
    let pack = command_stdout_bytes_with_stdin(
        "git",
        source.path(),
        &["pack-objects", "--stdout", "--revs"],
        b"HEAD\n",
    );

    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output = git_with_stdin_bytes(
        git_repo.path(),
        ["index-pack", "--stdin", "--no-rev-index"],
        &pack,
    );
    let zmin_output = run_zmin_with_stdin_bytes(
        zmin_repo.path(),
        ["index-pack", "--stdin", "--no-rev-index"],
        &pack,
    );
    assert_eq!(zmin_output, git_output);
    assert_eq!(
        run_zmin_with_stdin_bytes(
            zmin_repo.path(),
            ["index-pack", "--stdin", "--no-rev-index"],
            &pack,
        ),
        git_output
    );
    let pack_id = git_output
        .strip_prefix("pack\t")
        .expect("index-pack output pack id");
    let idx_name = format!("pack-{pack_id}.idx");
    assert_eq!(
        fs::read(zmin_repo.path().join(".git/objects/pack").join(&idx_name))
            .expect("read zmin idx"),
        fs::read(git_repo.path().join(".git/objects/pack").join(&idx_name)).expect("read git idx")
    );
    let rev_name = format!("pack-{pack_id}.rev");
    assert!(
        !zmin_repo
            .path()
            .join(".git/objects/pack")
            .join(&rev_name)
            .exists()
    );
    assert!(
        !git_repo
            .path()
            .join(".git/objects/pack")
            .join(&rev_name)
            .exists()
    );

    let git_v1_repo = git_init();
    let zmin_v1_repo = git_init();
    let git_v1_output = git_with_stdin_bytes(
        git_v1_repo.path(),
        [
            "index-pack",
            "--stdin",
            "--no-rev-index",
            "--index-version=1",
        ],
        &pack,
    );
    let zmin_v1_output = run_zmin_with_stdin_bytes(
        zmin_v1_repo.path(),
        [
            "index-pack",
            "--stdin",
            "--no-rev-index",
            "--index-version=1",
        ],
        &pack,
    );
    assert_eq!(zmin_v1_output, git_v1_output);
    let v1_pack_id = git_v1_output
        .strip_prefix("pack\t")
        .expect("index-pack v1 output pack id");
    let v1_idx_name = format!("pack-{v1_pack_id}.idx");
    assert_eq!(
        fs::read(
            zmin_v1_repo
                .path()
                .join(".git/objects/pack")
                .join(&v1_idx_name)
        )
        .expect("read zmin v1 idx"),
        fs::read(
            git_v1_repo
                .path()
                .join(".git/objects/pack")
                .join(&v1_idx_name)
        )
        .expect("read git v1 idx")
    );
    assert!(
        !zmin_v1_repo
            .path()
            .join(".git/objects/pack")
            .join(format!("pack-{v1_pack_id}.rev"))
            .exists()
    );

    let git_rev_repo = git_init();
    let zmin_rev_repo = git_init();
    let git_rev_output =
        git_with_stdin_bytes(git_rev_repo.path(), ["index-pack", "--stdin"], &pack);
    let zmin_rev_output =
        run_zmin_with_stdin_bytes(zmin_rev_repo.path(), ["index-pack", "--stdin"], &pack);
    assert_eq!(zmin_rev_output, git_rev_output);
    let rev_pack_id = git_rev_output
        .strip_prefix("pack\t")
        .expect("index-pack default output pack id");
    let rev_name = format!("pack-{rev_pack_id}.rev");
    assert_eq!(
        fs::read(
            zmin_rev_repo
                .path()
                .join(".git/objects/pack")
                .join(&rev_name)
        )
        .expect("read zmin rev"),
        fs::read(
            git_rev_repo
                .path()
                .join(".git/objects/pack")
                .join(&rev_name)
        )
        .expect("read git rev")
    );

    let git_explicit_rev_repo = git_init();
    let zmin_explicit_rev_repo = git_init();
    let git_explicit_rev_output = git_with_stdin_bytes(
        git_explicit_rev_repo.path(),
        ["index-pack", "--stdin", "--rev-index"],
        &pack,
    );
    let zmin_explicit_rev_output = run_zmin_with_stdin_bytes(
        zmin_explicit_rev_repo.path(),
        ["index-pack", "--stdin", "--rev-index"],
        &pack,
    );
    assert_eq!(zmin_explicit_rev_output, git_explicit_rev_output);
    let explicit_rev_pack_id = git_explicit_rev_output
        .strip_prefix("pack\t")
        .expect("index-pack explicit rev output pack id");
    let explicit_rev_name = format!("pack-{explicit_rev_pack_id}.rev");
    assert_eq!(
        fs::read(
            zmin_explicit_rev_repo
                .path()
                .join(".git/objects/pack")
                .join(&explicit_rev_name)
        )
        .expect("read zmin explicit rev"),
        fs::read(
            git_explicit_rev_repo
                .path()
                .join(".git/objects/pack")
                .join(&explicit_rev_name)
        )
        .expect("read git explicit rev")
    );
    let zmin_explicit_pack = zmin_explicit_rev_repo
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{explicit_rev_pack_id}.pack"));
    flip_last_byte(&zmin_explicit_pack.with_extension("rev"));
    assert_eq!(
        command_any_output(
            zmin_bin(),
            zmin_explicit_rev_repo.path(),
            &[
                "index-pack",
                "--verify",
                zmin_explicit_pack.to_str().expect("zmin pack path")
            ],
            "zmin",
        ),
        command_any_output(
            "git",
            zmin_explicit_rev_repo.path(),
            &[
                "index-pack",
                "--verify",
                zmin_explicit_pack.to_str().expect("zmin pack path")
            ],
            "git",
        )
    );

    let git_corrupt_idx_repo = git_init();
    let zmin_corrupt_idx_repo = git_init();
    let git_corrupt_idx_output = git_with_stdin_bytes(
        git_corrupt_idx_repo.path(),
        ["index-pack", "--stdin"],
        &pack,
    );
    let zmin_corrupt_idx_output = run_zmin_with_stdin_bytes(
        zmin_corrupt_idx_repo.path(),
        ["index-pack", "--stdin"],
        &pack,
    );
    assert_eq!(zmin_corrupt_idx_output, git_corrupt_idx_output);
    let corrupt_idx_pack_id = git_corrupt_idx_output
        .strip_prefix("pack\t")
        .expect("index-pack corrupt idx output pack id");
    let zmin_corrupt_idx_pack = zmin_corrupt_idx_repo
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{corrupt_idx_pack_id}.pack"));
    flip_last_byte(&zmin_corrupt_idx_pack.with_extension("idx"));
    assert_eq!(
        command_any_output(
            zmin_bin(),
            zmin_corrupt_idx_repo.path(),
            &[
                "index-pack",
                "--verify",
                zmin_corrupt_idx_pack
                    .to_str()
                    .expect("zmin corrupt idx pack path")
            ],
            "zmin",
        ),
        command_any_output(
            "git",
            zmin_corrupt_idx_repo.path(),
            &[
                "index-pack",
                "--verify",
                zmin_corrupt_idx_pack
                    .to_str()
                    .expect("zmin corrupt idx pack path")
            ],
            "git",
        )
    );
    let head = git(source.path(), ["rev-parse", "HEAD"]);
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", &head]),
        git(source.path(), ["cat-file", "-p", &head])
    );

    let pack_path = zmin_repo
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{pack_id}.pack"));
    assert_eq!(
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "index-pack",
                "--verify",
                pack_path.to_str().expect("pack path")
            ],
            "zmin"
        ),
        command_any_output(
            "git",
            git_repo.path(),
            &[
                "index-pack",
                "--verify",
                pack_path.to_str().expect("pack path")
            ],
            "git"
        )
    );
    assert_eq!(
        command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &["index-pack", "--verify", "--stdin"],
            "zmin"
        )
        .0,
        command_any_output(
            "git",
            git_repo.path(),
            &["index-pack", "--verify", "--stdin"],
            "git"
        )
        .0
    );

    let corrupt_repo = git_init();
    configure_identity(corrupt_repo.path());
    write_file(corrupt_repo.path(), "file.txt", "content\n");
    git(corrupt_repo.path(), ["add", "-A"]);
    git_with_env(corrupt_repo.path(), ["commit", "-m", "pack me"]);
    git(corrupt_repo.path(), ["repack", "-adq"]);
    let corrupt_idx = first_pack_index(corrupt_repo.path());
    let corrupt_pack = corrupt_idx.with_extension("pack");
    flip_last_byte(&corrupt_pack);
    assert_eq!(
        command_any_output(
            zmin_bin(),
            corrupt_repo.path(),
            &[
                "index-pack",
                "--verify",
                corrupt_pack.to_str().expect("pack path")
            ],
            "zmin"
        ),
        command_any_output(
            "git",
            corrupt_repo.path(),
            &[
                "index-pack",
                "--verify",
                corrupt_pack.to_str().expect("pack path")
            ],
            "git"
        )
    );
}

#[test]
fn index_pack_file_does_not_require_git_repository_like_stock_git() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    write_file(source.path(), "a.txt", "two\n");
    git(source.path(), ["commit", "-am", "two"]);
    let pack = command_stdout_bytes_with_stdin(
        "git",
        source.path(),
        &["pack-objects", "--stdout", "--revs"],
        b"HEAD\n",
    );

    let git_target = TempDir::new().expect("git standalone dir");
    let zmin_target = TempDir::new().expect("zmin standalone dir");
    let git_pack = git_target.path().join("input.pack");
    let zmin_pack = zmin_target.path().join("input.pack");
    fs::write(&git_pack, &pack).expect("write git pack");
    fs::write(&zmin_pack, &pack).expect("write zmin pack");

    assert_eq!(
        command_any_output(
            zmin_bin(),
            zmin_target.path(),
            &["index-pack", "input.pack"],
            "zmin",
        ),
        command_any_output(
            "git",
            git_target.path(),
            &["index-pack", "input.pack"],
            "git"
        )
    );
    assert!(zmin_target.path().join("input.idx").is_file());
    assert_eq!(
        fs::read(zmin_target.path().join("input.idx")).expect("read zmin standalone idx"),
        fs::read(git_target.path().join("input.idx")).expect("read git standalone idx")
    );
}

#[test]
fn index_pack_keep_and_index_version_match_stock_git_output() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    let pack = command_stdout_bytes_with_stdin(
        "git",
        source.path(),
        &["pack-objects", "--stdout", "--revs"],
        b"HEAD\n",
    );

    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output = git_with_stdin_bytes(
        git_repo.path(),
        ["index-pack", "--stdin", "--keep=", "--index-version=2"],
        &pack,
    );
    let zmin_output = run_zmin_with_stdin_bytes(
        zmin_repo.path(),
        ["index-pack", "--stdin", "--keep=", "--index-version=2"],
        &pack,
    );
    assert_eq!(zmin_output, git_output);
    let pack_id = git_output
        .strip_prefix("keep\t")
        .expect("index-pack keep output");
    assert!(
        zmin_repo
            .path()
            .join(format!(".git/objects/pack/pack-{pack_id}.keep"))
            .exists()
    );
    assert_eq!(git_status(zmin_repo.path(), ["fsck", "--strict"]), 0);
}

#[test]
fn index_pack_version_1_matches_stock_git_output_and_writes_v1_index() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    let pack = command_stdout_bytes_with_stdin(
        "git",
        source.path(),
        &["pack-objects", "--stdout", "--revs"],
        b"HEAD\n",
    );

    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output = git_with_stdin_bytes(
        git_repo.path(),
        ["index-pack", "--stdin", "--keep=", "--index-version=1"],
        &pack,
    );
    let zmin_output = run_zmin_with_stdin_bytes(
        zmin_repo.path(),
        ["index-pack", "--stdin", "--keep=", "--index-version=1"],
        &pack,
    );
    assert_eq!(zmin_output, git_output);
    let pack_id = git_output
        .strip_prefix("keep\t")
        .expect("index-pack keep output");
    let zmin_idx = zmin_repo
        .path()
        .join(format!(".git/objects/pack/pack-{pack_id}.idx"));
    let git_idx = git_repo
        .path()
        .join(format!(".git/objects/pack/pack-{pack_id}.idx"));
    let zmin_bytes = fs::read(&zmin_idx).expect("read zmin v1 idx");
    let git_bytes = fs::read(&git_idx).expect("read git v1 idx");
    assert_eq!(zmin_bytes, git_bytes);
    assert_eq!(&zmin_bytes[..4], &[0, 0, 0, 0]);
    assert_eq!(git_status(zmin_repo.path(), ["fsck", "--strict"]), 0);
}

#[test]
fn pack_objects_index_version_1_writes_stock_compatible_v1_index() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);

    let git_repo = clone_repo_fixture(source.path());
    let zmin_repo = clone_repo_fixture(source.path());

    let git_prefix = git_repo.path().join("out/pack");
    let zmin_prefix = zmin_repo.path().join("out/pack");
    fs::create_dir_all(git_prefix.parent().expect("git parent")).expect("mkdir git out");
    fs::create_dir_all(zmin_prefix.parent().expect("zmin parent")).expect("mkdir zmin out");

    let _git_output = git_with_stdin_args(
        git_repo.path(),
        &[
            "pack-objects",
            git_prefix.to_str().expect("git prefix"),
            "--revs",
            "--index-version=1",
        ],
        "HEAD\n",
    );
    let zmin_output = run_zmin_with_stdin_args(
        zmin_repo.path(),
        &[
            "pack-objects",
            zmin_prefix.to_str().expect("zmin prefix"),
            "--revs",
            "--index-version=1",
        ],
        "HEAD\n",
    );
    let _ = zmin_output;
    let git_idx = fs::read_dir(git_repo.path().join("out"))
        .expect("read git out dir")
        .find_map(|entry| {
            let path = entry.expect("git out entry").path();
            (path.extension().and_then(|value| value.to_str()) == Some("idx")).then_some(path)
        })
        .expect("git idx path");
    let zmin_idx = fs::read_dir(zmin_repo.path().join("out"))
        .expect("read zmin out dir")
        .find_map(|entry| {
            let path = entry.expect("zmin out entry").path();
            (path.extension().and_then(|value| value.to_str()) == Some("idx")).then_some(path)
        })
        .expect("zmin idx path");
    let zmin_pack = zmin_idx.with_extension("pack");
    assert_eq!(
        &fs::read(&git_idx).expect("read git idx")[..4],
        &[0, 0, 0, 0]
    );
    assert_eq!(
        &fs::read(&zmin_idx).expect("read zmin idx")[..4],
        &[0, 0, 0, 0]
    );
    assert_eq!(
        command_any_output(
            "git",
            zmin_repo.path(),
            &[
                "verify-pack",
                "-v",
                zmin_idx.to_str().expect("zmin idx path")
            ],
            "git",
        )
        .0,
        0
    );
    let target = git_init();
    git_with_stdin_bytes(
        target.path(),
        ["index-pack", "--stdin", "--index-version=1"],
        &fs::read(&zmin_pack).expect("read zmin pack"),
    );
    for line in git(source.path(), ["rev-list", "--objects", "HEAD"]).lines() {
        let id = line.split_whitespace().next().expect("object id");
        assert_eq!(
            command_stdout_bytes("git", target.path(), &["cat-file", "-p", id]),
            command_stdout_bytes("git", source.path(), &["cat-file", "-p", id]),
            "object: {id}"
        );
    }
}

#[test]
fn index_pack_strict_and_fsck_objects_accept_valid_stock_pack() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    let pack = command_stdout_bytes_with_stdin(
        "git",
        source.path(),
        &["pack-objects", "--stdout", "--revs"],
        b"HEAD\n",
    );

    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output = git_with_stdin_bytes(
        git_repo.path(),
        ["index-pack", "--stdin", "--strict", "--fsck-objects"],
        &pack,
    );
    let zmin_output = run_zmin_with_stdin_bytes(
        zmin_repo.path(),
        ["index-pack", "--stdin", "--strict", "--fsck-objects"],
        &pack,
    );
    assert_eq!(zmin_output, git_output);
    let pack_id = git_output
        .strip_prefix("pack\t")
        .expect("index-pack output pack id");
    assert!(
        zmin_repo
            .path()
            .join(format!(".git/objects/pack/pack-{pack_id}.idx"))
            .exists()
    );
}

#[test]
fn index_pack_strict_rejects_malformed_commit_like_stock_git() {
    let source = git_init();
    let tree = git_with_stdin(source.path(), ["mktree"], "");
    fs::write(
        source.path().join("bad.commit"),
        format!("tree {tree}\n\nmsg\n"),
    )
    .expect("write malformed commit");
    let bad = git(
        source.path(),
        [
            "hash-object",
            "-t",
            "commit",
            "-w",
            "--literally",
            "bad.commit",
        ],
    );
    let pack_input = format!("{bad}\n");
    let pack = command_stdout_bytes_with_stdin(
        "git",
        source.path(),
        &["pack-objects", "--stdout"],
        pack_input.as_bytes(),
    );

    for args in [
        ["index-pack", "--strict"],
        ["index-pack", "--fsck-objects"],
        ["index-pack", "--strict=missingAuthor=warn"],
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let git_pack = git_repo.path().join("bad.pack");
        let zmin_pack = zmin_repo.path().join("bad.pack");
        fs::write(&git_pack, &pack).expect("write git malformed pack");
        fs::write(&zmin_pack, &pack).expect("write zmin malformed pack");

        let mut git_args = args.to_vec();
        let mut zmin_args = args.to_vec();
        git_args.push(git_pack.to_str().expect("git pack path"));
        zmin_args.push(zmin_pack.to_str().expect("zmin pack path"));
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), &zmin_args, "zmin"),
            command_any_output("git", git_repo.path(), &git_args, "git"),
            "args: {args:?}"
        );
    }
}

#[test]
fn index_pack_fix_thin_repairs_stock_git_thin_pack() {
    let source = git_init();
    configure_identity(source.path());
    write_file(
        source.path(),
        "delta.txt",
        &format!("{}\nbase\n", "shared line\n".repeat(4_000)),
    );
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "base"]);
    let base = git(source.path(), ["rev-parse", "HEAD"]);
    write_file(
        source.path(),
        "delta.txt",
        &format!("{}\nchanged\n", "shared line\n".repeat(4_000)),
    );
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "changed"]);
    let head = git(source.path(), ["rev-parse", "HEAD"]);
    let pack_input = format!("HEAD\n^{base}\n");
    let thin_pack = command_stdout_bytes_with_stdin(
        "git",
        source.path(),
        &[
            "pack-objects",
            "--stdout",
            "--thin",
            "--window=50",
            "--depth=50",
            "--revs",
        ],
        pack_input.as_bytes(),
    );

    let git_target = git_init();
    let zmin_target = git_init();
    let source_path = source.path().to_str().expect("source path");
    let fetch_ref = format!("{base}:refs/heads/base");
    git_args(git_target.path(), &["fetch", source_path, &fetch_ref]);
    git_args(zmin_target.path(), &["fetch", source_path, &fetch_ref]);

    let git_output = git_with_stdin_bytes(
        git_target.path(),
        ["index-pack", "--stdin", "--fix-thin"],
        &thin_pack,
    );
    let zmin_output = run_zmin_with_stdin_bytes(
        zmin_target.path(),
        ["index-pack", "--stdin", "--fix-thin"],
        &thin_pack,
    );
    assert!(git_output.starts_with("pack\t"));
    assert!(zmin_output.starts_with("pack\t"));
    let zmin_pack_id = zmin_output
        .strip_prefix("pack\t")
        .expect("zmin index-pack output pack id");

    let zmin_idx = zmin_target
        .path()
        .join(".git/objects/pack")
        .join(format!("pack-{zmin_pack_id}.idx"));
    assert_eq!(
        command_any_output(
            "git",
            zmin_target.path(),
            &["verify-pack", zmin_idx.to_str().expect("idx path")],
            "git"
        )
        .0,
        0
    );
    let objects = git(source.path(), ["rev-list", "--objects", &head]);
    for line in objects.lines() {
        let id = line.split_whitespace().next().expect("object id");
        assert_eq!(
            command_stdout_bytes("git", zmin_target.path(), &["cat-file", "-p", id]),
            command_stdout_bytes("git", source.path(), &["cat-file", "-p", id])
        );
    }
}

#[test]
fn bundle_create_list_heads_and_unbundle_are_stock_readable() {
    let source = git_init();
    configure_identity(source.path());
    write_file(
        source.path(),
        "a.txt",
        &format!("{}\nbase\n", "shared line\n".repeat(2_000)),
    );
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    write_file(
        source.path(),
        "a.txt",
        &format!("{}\nchanged\n", "shared line\n".repeat(2_000)),
    );
    write_file(source.path(), "b.txt", "bee\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "two"]);

    let bundle_dir = TempDir::new().expect("bundle dir");
    let bundle_path = bundle_dir.path().join("repo.bundle");
    let bundle = bundle_path.to_str().expect("bundle path");
    let branch = git(source.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);
    run_zmin_args(
        source.path(),
        &["bundle", "create", bundle, "HEAD", &branch],
    );

    assert_eq!(
        run_zmin_args(source.path(), &["bundle", "list-heads", bundle]),
        git_args(source.path(), &["bundle", "list-heads", bundle])
    );
    assert_eq!(
        run_zmin_args(
            source.path(),
            &["bundle", "list-heads", bundle, "refs/heads/main"]
        ),
        git_args(
            source.path(),
            &["bundle", "list-heads", bundle, "refs/heads/main"]
        )
    );
    assert_eq!(
        run_zmin_args(
            source.path(),
            &["bundle", "list-heads", bundle, "refs/heads/*"]
        ),
        git_args(
            source.path(),
            &["bundle", "list-heads", bundle, "refs/heads/*"]
        )
    );
    assert_eq!(
        git_status_args(source.path(), &["bundle", "verify", bundle]),
        0,
        "stock git should verify a zmin-created bundle"
    );

    let target = git_init();
    assert_eq!(
        run_zmin_args(target.path(), &["bundle", "unbundle", bundle]),
        git_args(source.path(), &["bundle", "list-heads", bundle])
    );
    let filtered_target = git_init();
    assert_eq!(
        run_zmin_args(
            filtered_target.path(),
            &["bundle", "unbundle", bundle, "refs/heads/*"]
        ),
        git_args(
            source.path(),
            &["bundle", "list-heads", bundle, "refs/heads/*"]
        )
    );
    let head = git(source.path(), ["rev-parse", "HEAD"]);
    assert_eq!(
        git(target.path(), ["cat-file", "-p", &head]),
        git(source.path(), ["cat-file", "-p", &head])
    );
    let target_idx = first_pack_index(target.path());
    let verify = git(
        target.path(),
        ["verify-pack", "-v", target_idx.to_str().expect("idx path")],
    );
    assert!(
        verify
            .lines()
            .any(|line| line.contains(" blob ") && line.split_whitespace().count() >= 7),
        "expected bundle pack to contain a delta:\n{verify}"
    );
}

#[test]
fn bundle_create_accepts_version_option_for_upstream_fetch_suite() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    write_file(source.path(), "a.txt", "two\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "two"]);

    let bundle_dir = TempDir::new().expect("bundle dir");
    let bundle_path = bundle_dir.path().join("versioned.bundle");
    let bundle = bundle_path.to_str().expect("bundle path");
    run_zmin_args(
        source.path(),
        &["bundle", "create", "--version=3", bundle, "main^..main"],
    );

    assert_eq!(
        git_status_args(source.path(), &["bundle", "verify", bundle]),
        0
    );
    let target = clone_repo_fixture(source.path());
    git(target.path(), ["reset", "--hard", "HEAD^"]);
    run_zmin_args(target.path(), &["fetch", bundle, "main:main"]);
    assert_eq!(
        git(target.path(), ["log", "-1", "--pretty=%s", "main"]),
        "two"
    );
}

#[test]
fn bundle_create_accepts_since_option_for_upstream_fetch_suite() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    write_file(source.path(), "a.txt", "two\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "two"]);
    let since = git(source.path(), ["log", "--no-walk", "--format=%ad", "HEAD"]);

    let bundle_dir = TempDir::new().expect("bundle dir");
    let bundle_path = bundle_dir.path().join("since.bundle");
    let bundle = bundle_path.to_str().expect("bundle path");
    run_zmin_args(
        source.path(),
        &[
            "bundle",
            "create",
            bundle,
            "main",
            &format!("--since={since}"),
        ],
    );

    assert_eq!(
        git_status_args(source.path(), &["bundle", "verify", bundle]),
        0
    );
}

#[test]
fn bundle_unbundle_accepts_prerequisite_bundles() {
    let source = git_init();
    configure_identity(source.path());
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    let base = git(source.path(), ["rev-parse", "HEAD"]);

    let git_target = clone_repo_fixture(source.path());
    let zmin_target = clone_repo_fixture(source.path());

    write_file(source.path(), "a.txt", "two\n");
    write_file(source.path(), "b.txt", "bee\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "two"]);
    let head = git(source.path(), ["rev-parse", "HEAD"]);

    let bundle_dir = TempDir::new().expect("bundle dir");
    let bundle_path = bundle_dir.path().join("incremental.bundle");
    let bundle = bundle_path.to_str().expect("bundle path");
    git(
        source.path(),
        ["bundle", "create", bundle, &format!("{base}..HEAD")],
    );

    assert_eq!(
        command_any_output(
            zmin_bin(),
            zmin_target.path(),
            &["bundle", "verify", bundle],
            "zmin"
        )
        .0,
        0
    );
    assert_eq!(
        run_zmin_args(zmin_target.path(), &["bundle", "unbundle", bundle]),
        git_args(git_target.path(), &["bundle", "unbundle", bundle])
    );
    assert_eq!(
        git(zmin_target.path(), ["cat-file", "-p", &head]),
        git(source.path(), ["cat-file", "-p", &head])
    );

    let missing_base = git_init();
    let zmin_missing = command_any_output(
        zmin_bin(),
        missing_base.path(),
        &["bundle", "unbundle", bundle],
        "zmin",
    );
    let git_missing = command_any_output(
        "git",
        missing_base.path(),
        &["bundle", "unbundle", bundle],
        "git",
    );
    assert_eq!(zmin_missing.0, git_missing.0);
    assert_ne!(zmin_missing.0, 0);
}

fn committed_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    run_zmin(repo.path(), ["add", "-A"]);
    run_zmin_with_env(repo.path(), ["commit", "-m", "initial"]);
    repo
}

fn loose_object_path(repo: &std::path::Path, id: &str) -> std::path::PathBuf {
    repo.join(".git/objects").join(&id[..2]).join(&id[2..])
}

fn first_pack_index(repo: &std::path::Path) -> std::path::PathBuf {
    let mut paths = fs::read_dir(repo.join(".git/objects/pack"))
        .expect("read pack dir")
        .map(|entry| entry.expect("pack entry").path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("idx"))
        .collect::<Vec<_>>();
    paths.sort();
    paths.into_iter().next().expect("pack index")
}

fn flip_last_byte(path: &std::path::Path) {
    let mut bytes = fs::read(path).expect("read file to corrupt");
    let last = bytes.last_mut().expect("non-empty file to corrupt");
    *last ^= 1;
    let mut permissions = fs::metadata(path)
        .expect("file metadata before corruption")
        .permissions();
    if permissions.readonly() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            permissions.set_mode(permissions.mode() | 0o200);
        }
        #[cfg(windows)]
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).expect("make file writable for corruption");
    }
    fs::write(path, bytes).expect("write corrupted file");
}

fn set_pack_index_version(path: &std::path::Path, version: u32) {
    let mut bytes = fs::read(path).expect("read pack index");
    bytes[4..8].copy_from_slice(&version.to_be_bytes());
    let checksum_offset = bytes.len() - GitHashAlgorithm::Sha1.digest_len();
    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update(&bytes[..checksum_offset]);
    let checksum = hasher.finalize();
    bytes[checksum_offset..].copy_from_slice(checksum.as_bytes());
    let mut permissions = fs::metadata(path)
        .expect("pack index metadata before version mutation")
        .permissions();
    if permissions.readonly() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            permissions.set_mode(permissions.mode() | 0o200);
        }
        #[cfg(windows)]
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).expect("make pack index writable");
    }
    fs::write(path, bytes).expect("write pack index version");
}

#[derive(Clone, Copy)]
enum FakeGpgMode {
    Good,
    Bad,
}

fn write_fake_gpg(dir: &Path, name: &str, mode: FakeGpgMode) -> PathBuf {
    let path = dir.join(fake_gpg_file_name(name));
    fs::write(&path, fake_gpg_script(mode)).expect("write fake gpg");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&path)
            .expect("fake gpg metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("chmod fake gpg");
    }
    path
}

#[cfg(unix)]
fn fake_gpg_file_name(name: &str) -> String {
    name.to_owned()
}

#[cfg(windows)]
fn fake_gpg_file_name(name: &str) -> String {
    format!("{name}.cmd")
}

#[cfg(unix)]
fn fake_gpg_script(mode: FakeGpgMode) -> String {
    let (status_line, code) = match mode {
        FakeGpgMode::Good => (
            "[GNUPG:] GOODSIG 0123456789ABCDEF Fixture Signer <fixture@example.test>",
            0,
        ),
        FakeGpgMode::Bad => (
            "[GNUPG:] BADSIG 0123456789ABCDEF Fixture Signer <fixture@example.test>",
            1,
        ),
    };
    format!(
        r#"#!/bin/sh
sig=""
payload=""
for arg in "$@"; do
    sig="$payload"
    payload="$arg"
done
if ! grep -q -- "BEGIN PGP SIGNATURE" "$sig"; then
    echo "missing signature payload" >&2
    exit 3
fi
payload_file="$(mktemp)"
cat > "$payload_file"
if ! grep -q -- "tag signed-fixture" "$payload_file" && ! grep -q -- "signed commit fixture" "$payload_file"; then
    rm -f "$payload_file"
    echo "missing signed tag payload" >&2
    exit 4
fi
rm -f "$payload_file"
echo "[GNUPG:] NEWSIG"
echo "{status_line}"
echo "[GNUPG:] VALIDSIG 0123456789ABCDEF0123456789ABCDEF01234567 2026-05-13 1747137600 0 4 0 1 10 00 0123456789ABCDEF"
echo "[GNUPG:] TRUST_FULLY 0 pgp"
exit {code}
"#
    )
}

#[cfg(windows)]
fn fake_gpg_script(mode: FakeGpgMode) -> String {
    let (status_line, code) = match mode {
        FakeGpgMode::Good => (
            "[GNUPG:] GOODSIG 0123456789ABCDEF Fixture Signer <fixture@example.test>",
            0,
        ),
        FakeGpgMode::Bad => (
            "[GNUPG:] BADSIG 0123456789ABCDEF Fixture Signer <fixture@example.test>",
            1,
        ),
    };
    let status_line = cmd_echo_text(status_line);
    format!(
        "@echo off\r\n\
         echo [GNUPG:] NEWSIG\r\n\
         echo {status_line}\r\n\
         echo [GNUPG:] VALIDSIG 0123456789ABCDEF0123456789ABCDEF01234567 2026-05-13 1747137600 0 4 0 1 10 00 0123456789ABCDEF\r\n\
         echo [GNUPG:] TRUST_FULLY 0 pgp\r\n\
         exit /b {code}\r\n"
    )
}

#[cfg(windows)]
fn cmd_echo_text(value: &str) -> String {
    value
        .replace('^', "^^")
        .replace('&', "^&")
        .replace('|', "^|")
        .replace('<', "^<")
        .replace('>', "^>")
}
