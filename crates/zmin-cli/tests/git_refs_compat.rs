mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    command_any_output, command_any_output_with_stdin, command_failure_output_with_env,
    command_output_with_env, configure_identity, git, git_args, git_failure_output, git_init,
    git_status, git_with_env, run_zmin, run_zmin_args, run_zmin_failure_output, run_zmin_status,
    run_zmin_with_env, write_file, zmin_bin,
};

fn committed_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    run_zmin(repo.path(), ["add", "-A"]);
    run_zmin_with_env(repo.path(), ["commit", "-m", "initial"]);
    repo
}

#[test]
fn update_ref_and_symbolic_ref_match_stock_git_state() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();

    git(
        git_repo.path(),
        ["update-ref", "refs/heads/plumbing", "HEAD"],
    );
    run_zmin(
        zmin_repo.path(),
        ["update-ref", "refs/heads/plumbing", "HEAD"],
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["show-ref", "--heads"]),
        git(git_repo.path(), ["show-ref", "--heads"])
    );

    git(
        git_repo.path(),
        ["symbolic-ref", "HEAD", "refs/heads/plumbing"],
    );
    run_zmin(
        zmin_repo.path(),
        ["symbolic-ref", "HEAD", "refs/heads/plumbing"],
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["symbolic-ref", "HEAD"]),
        git(git_repo.path(), ["symbolic-ref", "HEAD"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["symbolic-ref", "--short", "HEAD"]),
        git(git_repo.path(), ["symbolic-ref", "--short", "HEAD"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["branch", "--show-current"]),
        git(git_repo.path(), ["branch", "--show-current"])
    );

    git(git_repo.path(), ["update-ref", "-d", "refs/heads/plumbing"]);
    run_zmin(
        zmin_repo.path(),
        ["update-ref", "-d", "refs/heads/plumbing"],
    );
    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["symbolic-ref", "-q", "HEAD"]),
        git_status(git_repo.path(), ["symbolic-ref", "-q", "HEAD"])
    );
}

#[test]
fn symbolic_ref_read_modes_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(
            repo,
            ["symbolic-ref", "refs/heads/inner", "refs/heads/main"],
        );
        git(
            repo,
            ["symbolic-ref", "refs/heads/outer", "refs/heads/inner"],
        );
    }

    for args in [
        ["symbolic-ref", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "--no-recurse", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "--short", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "-q", "refs/heads/inner"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin symbolic-ref"),
            command_any_output("git", git_repo.path(), args, "git symbolic-ref"),
            "args: {args:?}"
        );
    }
}

#[test]
fn symbolic_ref_delete_recurse_and_message_modes_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(
            repo,
            ["symbolic-ref", "refs/heads/inner", "refs/heads/main"],
        );
        git(
            repo,
            ["symbolic-ref", "refs/heads/outer", "refs/heads/inner"],
        );
        git(repo, ["branch", "other"]);
    }

    for args in [
        ["symbolic-ref", "--recurse", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "--delete", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "-d", "refs/heads/inner"].as_slice(),
        ["symbolic-ref", "-m", "reason", "HEAD", "refs/heads/other"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin symbolic-ref"),
            command_any_output("git", git_repo.path(), args, "git symbolic-ref"),
            "args: {args:?}"
        );
    }

    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/HEAD")).expect("read zmin HEAD"),
        fs::read_to_string(git_repo.path().join(".git/HEAD")).expect("read git HEAD")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/logs/HEAD")).expect("read zmin HEAD reflog"),
        fs::read_to_string(git_repo.path().join(".git/logs/HEAD")).expect("read git HEAD reflog")
    );

    let zmin_delete = command_any_output(
        zmin_bin(),
        zmin_repo.path(),
        &["symbolic-ref", "--delete", "-q", "refs/heads/missing"],
        "zmin symbolic-ref",
    );
    let git_delete = command_any_output(
        "git",
        git_repo.path(),
        &["symbolic-ref", "--delete", "-q", "refs/heads/missing"],
        "git symbolic-ref",
    );
    assert_eq!(zmin_delete, git_delete);
}

#[test]
fn symbolic_ref_option_combinations_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(
            repo,
            ["symbolic-ref", "refs/heads/inner", "refs/heads/main"],
        );
        git(
            repo,
            ["symbolic-ref", "refs/heads/outer", "refs/heads/inner"],
        );
    }

    for args in [
        ["symbolic-ref", "--no-quiet", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-short", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-quiet", "--no-short", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-short", "--no-quiet", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-delete", "--no-quiet", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-quiet", "--no-delete", "HEAD"].as_slice(),
        ["symbolic-ref", "--quiet", "--short", "HEAD"].as_slice(),
        ["symbolic-ref", "--short", "--quiet", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-quiet", "--short", "HEAD"].as_slice(),
        ["symbolic-ref", "--short", "--no-quiet", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-delete", "--short", "HEAD"].as_slice(),
        ["symbolic-ref", "--short", "--no-delete", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-delete", "--no-recurse", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-recurse", "--no-delete", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-quiet", "--no-recurse", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-recurse", "--no-quiet", "HEAD"].as_slice(),
        ["symbolic-ref", "--short", "--no-short", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-short", "--short", "HEAD"].as_slice(),
        ["symbolic-ref", "--quiet", "--quiet", "HEAD"].as_slice(),
        [
            "symbolic-ref",
            "--no-recurse",
            "--short",
            "refs/heads/outer",
        ]
        .as_slice(),
        [
            "symbolic-ref",
            "--short",
            "--no-recurse",
            "refs/heads/outer",
        ]
        .as_slice(),
        ["symbolic-ref", "--recurse", "--short", "refs/heads/outer"].as_slice(),
        [
            "symbolic-ref",
            "--recurse",
            "--no-recurse",
            "--short",
            "refs/heads/outer",
        ]
        .as_slice(),
        [
            "symbolic-ref",
            "--no-recurse",
            "--recurse",
            "--short",
            "refs/heads/outer",
        ]
        .as_slice(),
        ["symbolic-ref", "-q", "--no-quiet", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "--no-quiet", "-q", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "--no-quiet", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "--no-quiet", "--short", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "--short", "--no-quiet", "refs/heads/outer"].as_slice(),
        [
            "symbolic-ref",
            "--no-short",
            "--no-quiet",
            "refs/heads/outer",
        ]
        .as_slice(),
        [
            "symbolic-ref",
            "--no-quiet",
            "--no-short",
            "refs/heads/outer",
        ]
        .as_slice(),
        [
            "symbolic-ref",
            "--no-delete",
            "--no-quiet",
            "refs/heads/outer",
        ]
        .as_slice(),
        [
            "symbolic-ref",
            "--no-quiet",
            "--no-delete",
            "refs/heads/outer",
        ]
        .as_slice(),
        ["symbolic-ref", "--quiet", "--no-recurse", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-recurse", "--quiet", "HEAD"].as_slice(),
        ["symbolic-ref", "--short", "--short", "refs/heads/outer"].as_slice(),
        [
            "symbolic-ref",
            "--no-recurse",
            "--no-recurse",
            "refs/heads/outer",
        ]
        .as_slice(),
        ["symbolic-ref", "--no-short", "refs/heads/outer"].as_slice(),
        [
            "symbolic-ref",
            "--no-short",
            "--no-recurse",
            "refs/heads/outer",
        ]
        .as_slice(),
        [
            "symbolic-ref",
            "--no-recurse",
            "--no-short",
            "refs/heads/outer",
        ]
        .as_slice(),
        ["symbolic-ref", "--short", "--no-short", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "--no-short", "--short", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "--delete", "--delete", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "--no-delete", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-delete", "refs/heads/outer"].as_slice(),
        [
            "symbolic-ref",
            "--no-delete",
            "--no-short",
            "refs/heads/outer",
        ]
        .as_slice(),
        [
            "symbolic-ref",
            "--no-short",
            "--no-delete",
            "refs/heads/outer",
        ]
        .as_slice(),
        ["symbolic-ref", "--no-delete", "--short", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "--short", "--no-delete", "refs/heads/outer"].as_slice(),
        [
            "symbolic-ref",
            "--no-delete",
            "--no-recurse",
            "refs/heads/outer",
        ]
        .as_slice(),
        [
            "symbolic-ref",
            "--no-recurse",
            "--no-delete",
            "refs/heads/outer",
        ]
        .as_slice(),
        ["symbolic-ref", "--no-delete", "refs/heads/inner"].as_slice(),
        [
            "symbolic-ref",
            "--no-delete",
            "--no-quiet",
            "refs/heads/inner",
        ]
        .as_slice(),
        [
            "symbolic-ref",
            "--no-quiet",
            "--no-delete",
            "refs/heads/inner",
        ]
        .as_slice(),
        [
            "symbolic-ref",
            "--no-delete",
            "--no-delete",
            "refs/heads/outer",
        ]
        .as_slice(),
        [
            "symbolic-ref",
            "--delete",
            "--no-delete",
            "refs/heads/outer",
        ]
        .as_slice(),
        [
            "symbolic-ref",
            "--no-delete",
            "--delete",
            "refs/heads/outer",
        ]
        .as_slice(),
        ["symbolic-ref", "-d", "--no-delete", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "--no-delete", "-d", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "-d", "-d", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "--delete", "-d", "refs/heads/outer"].as_slice(),
        ["symbolic-ref", "-d", "--delete", "refs/heads/outer"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin symbolic-ref"),
            command_any_output("git", git_repo.path(), args, "git symbolic-ref"),
            "args: {args:?}"
        );
    }

    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/HEAD")).expect("read zmin HEAD"),
        fs::read_to_string(git_repo.path().join(".git/HEAD")).expect("read git HEAD")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/logs/HEAD")).expect("read zmin HEAD reflog"),
        fs::read_to_string(git_repo.path().join(".git/logs/HEAD")).expect("read git HEAD reflog")
    );

    let git_detached = committed_repo();
    let zmin_detached = committed_repo();
    for repo in [git_detached.path(), zmin_detached.path()] {
        let head = git(repo, ["rev-parse", "HEAD"]);
        git(repo, ["checkout", "-q", head.trim()]);
    }

    for args in [
        ["symbolic-ref", "--quiet", "--no-quiet", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-quiet", "--quiet", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-quiet", "--no-quiet", "HEAD"].as_slice(),
        ["symbolic-ref", "-q", "--no-quiet", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-quiet", "-q", "HEAD"].as_slice(),
        ["symbolic-ref", "-q", "-q", "HEAD"].as_slice(),
        ["symbolic-ref", "--short", "--short", "HEAD"].as_slice(),
        ["symbolic-ref", "--no-short", "--no-short", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), zmin_detached.path(), args, "zmin symbolic-ref"),
            command_any_output("git", git_detached.path(), args, "git symbolic-ref"),
            "args: {args:?}"
        );
    }
}

#[test]
fn show_ref_exists_and_quiet_verify_failure_modes_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["branch", "feature"]);
        git(repo, ["tag", "-a", "v1", "-m", "v1"]);
    }

    for args in [
        ["show-ref", "--exists", "refs/heads/missing"].as_slice(),
        ["show-ref", "--exists", "refs/tags/v1"].as_slice(),
        ["show-ref", "--exists", "HEAD"].as_slice(),
        ["show-ref", "--exists"].as_slice(),
        ["show-ref", "--exists", "refs/heads/main", "refs/tags/v1"].as_slice(),
        ["show-ref", "-q", "--verify", "refs/heads/missing"].as_slice(),
        ["show-ref", "-q", "--verify", "refs/heads/main"].as_slice(),
        ["show-ref", "--verify", "--hash", "-q", "refs/heads/main"].as_slice(),
        ["show-ref", "--verify", "--hash", "refs/heads/main"].as_slice(),
        ["show-ref", "--verify", "--hash", "refs/heads/missing"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin show-ref"),
            command_any_output("git", git_repo.path(), args, "git show-ref"),
            "args: {args:?}"
        );
    }
}

#[test]
fn show_ref_option_combinations_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["branch", "feature"]);
        git(repo, ["tag", "-a", "v1", "-m", "v1"]);
    }

    for args in [
        ["show-ref", "--head", "--heads", "--hash"].as_slice(),
        ["show-ref", "--hash", "--head", "--heads"].as_slice(),
        ["show-ref", "--tags", "--hash=12"].as_slice(),
        ["show-ref", "--heads", "--hash=12"].as_slice(),
        ["show-ref", "--branches", "--hash=12"].as_slice(),
        ["show-ref", "--dereference", "--hash"].as_slice(),
        ["show-ref", "--hash", "--dereference"].as_slice(),
        ["show-ref", "--head", "--heads", "--hash=12"].as_slice(),
        ["show-ref", "--hash=12", "--head", "--heads"].as_slice(),
        ["show-ref", "--verify", "--hash=12", "refs/heads/main"].as_slice(),
        ["show-ref", "--verify", "--hash=12", "refs/tags/v1"].as_slice(),
        ["show-ref", "--head", "--branches", "--hash"].as_slice(),
        ["show-ref", "--hash", "--head", "--branches"].as_slice(),
        ["show-ref", "--heads", "--tags", "--hash"].as_slice(),
        ["show-ref", "--hash", "--heads", "--tags"].as_slice(),
        ["show-ref", "--heads", "--tags", "--hash=12"].as_slice(),
        ["show-ref", "--head", "--tags", "--hash"].as_slice(),
        ["show-ref", "--hash", "--head", "--tags"].as_slice(),
        ["show-ref", "--head", "--tags", "--hash=12"].as_slice(),
        ["show-ref", "--hash=12", "--head", "--tags"].as_slice(),
        ["show-ref", "--head", "--heads", "--tags", "--hash"].as_slice(),
        ["show-ref", "--hash", "--head", "--heads", "--tags"].as_slice(),
        ["show-ref", "--head", "--heads", "--tags", "--hash=12"].as_slice(),
        [
            "show-ref",
            "--verify",
            "--head",
            "--hash",
            "refs/heads/main",
        ]
        .as_slice(),
        [
            "show-ref",
            "--verify",
            "--head",
            "--hash=12",
            "refs/heads/main",
        ]
        .as_slice(),
        [
            "show-ref",
            "--verify",
            "--tags",
            "--hash=12",
            "refs/tags/v1",
        ]
        .as_slice(),
        ["show-ref", "--head", "--branches", "--tags", "--hash"].as_slice(),
        ["show-ref", "--hash", "--head", "--branches", "--tags"].as_slice(),
        ["show-ref", "--head", "--branches", "--tags", "--hash=12"].as_slice(),
        ["show-ref", "--hash=12", "--head", "--branches", "--tags"].as_slice(),
        [
            "show-ref",
            "--verify",
            "--heads",
            "--hash",
            "refs/heads/main",
        ]
        .as_slice(),
        [
            "show-ref",
            "--verify",
            "--heads",
            "--hash=12",
            "refs/heads/main",
        ]
        .as_slice(),
        [
            "show-ref",
            "--verify",
            "--branches",
            "--hash=12",
            "refs/heads/main",
        ]
        .as_slice(),
        [
            "show-ref",
            "--verify",
            "--head",
            "--heads",
            "--hash",
            "refs/heads/main",
        ]
        .as_slice(),
        [
            "show-ref",
            "--verify",
            "--head",
            "--heads",
            "--hash=12",
            "refs/heads/main",
        ]
        .as_slice(),
        [
            "show-ref",
            "--verify",
            "--head",
            "--tags",
            "--hash=12",
            "refs/tags/v1",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin show-ref"),
            command_any_output("git", git_repo.path(), args, "git show-ref"),
            "args: {args:?}"
        );
    }
}

#[test]
fn show_ref_exclude_existing_matches_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["branch", "feature"]);
        git(repo, ["tag", "-a", "v1", "-m", "v1"]);
    }

    let stdin = "refs/heads/main\nrefs/heads/missing\nrefs/tags/v1\nabc\n deadbeef refs/heads/other\nrefs/heads/missing^{}\n";
    for args in [
        ["show-ref", "--exclude-existing"].as_slice(),
        ["show-ref", "--exclude-existing=refs/heads/"].as_slice(),
        ["show-ref", "--exclude-existing=refs/tags/"].as_slice(),
    ] {
        assert_eq!(
            command_any_output_with_stdin(
                zmin_bin(),
                zmin_repo.path(),
                args,
                stdin,
                "zmin show-ref"
            ),
            command_any_output_with_stdin("git", git_repo.path(), args, stdin, "git show-ref"),
            "args: {args:?}"
        );
    }
}

#[test]
fn update_ref_pseudoref_matches_stock_git_and_resolves_revision() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();

    git(git_repo.path(), ["update-ref", "REVERSE", "HEAD"]);
    run_zmin(zmin_repo.path(), ["update-ref", "REVERSE", "HEAD"]);

    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git").join("REVERSE"))
            .expect("read zmin pseudo-ref"),
        fs::read_to_string(git_repo.path().join(".git").join("REVERSE"))
            .expect("read git pseudo-ref")
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["rev-parse", "REVERSE"]),
        git(git_repo.path(), ["rev-parse", "REVERSE"])
    );
}

#[test]
fn refs_verify_matches_stock_git_for_healthy_repository() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"refs verify\n").expect("write file");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["branch", "feature"]);
    git(repo.path(), ["tag", "v1"]);

    for args in [
        ["refs", "verify"].as_slice(),
        ["refs", "verify", "--verbose"].as_slice(),
        ["refs", "verify", "--strict"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), repo.path(), args, "zmin refs verify"),
            command_any_output("git", repo.path(), args, "git refs verify"),
            "args: {args:?}"
        );
    }
}

#[test]
fn refs_verify_toggle_combinations_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"refs verify toggles\n").expect("write file");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["branch", "feature"]);
    git(repo.path(), ["tag", "v1"]);

    for args in [
        ["refs", "verify", "--no-verbose"].as_slice(),
        ["refs", "verify", "--no-strict"].as_slice(),
        ["refs", "verify", "--strict", "--verbose"].as_slice(),
        ["refs", "verify", "--verbose", "--strict"].as_slice(),
        ["refs", "verify", "--strict", "--no-strict"].as_slice(),
        ["refs", "verify", "--no-strict", "--strict"].as_slice(),
        ["refs", "verify", "--verbose", "--no-verbose"].as_slice(),
        ["refs", "verify", "--no-verbose", "--verbose"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), repo.path(), args, "zmin refs verify"),
            command_any_output("git", repo.path(), args, "git refs verify"),
            "args: {args:?}"
        );
    }
}

#[test]
fn refs_verify_invalid_option_failures_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"refs verify invalid\n").expect("write file");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    for args in [
        ["refs", "verify", "--dry-run"].as_slice(),
        ["refs", "verify", "--ref-format=files"].as_slice(),
        ["refs", "verify", "--ref-format=reftable"].as_slice(),
        ["refs", "verify", "--ref-format="].as_slice(),
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), repo.path(), args, "zmin refs verify"),
            command_any_output("git", repo.path(), args, "git refs verify"),
            "args: {args:?}"
        );
    }
}

#[test]
fn update_ref_stdin_batch_transactions_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();
    let git_head = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let zmin_head = git(zmin_repo.path(), ["rev-parse", "HEAD"]);
    assert_eq!(zmin_head, git_head);

    let batch = format!(
        "update refs/heads/batch-a {git_head}\n\
         create refs/heads/batch-b {git_head}\n\
         verify refs/heads/missing 0000000000000000000000000000000000000000\n\
         delete refs/heads/delete-missing\n"
    );
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin"],
            &batch,
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin"],
            &batch,
            "git"
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["show-ref", "--heads"]),
        git(git_repo.path(), ["show-ref", "--heads"])
    );

    let zero = "0000000000000000000000000000000000000000";
    let nul_batch = format!(
        "update refs/heads/nul-a\0{git_head}\0\0\
         create refs/heads/nul-b\0{git_head}\0\
         verify refs/heads/nul-missing\0{zero}\0\
         delete refs/heads/delete-nul-missing\0\0"
    );
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin", "-z"],
            &nul_batch,
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin", "-z"],
            &nul_batch,
            "git",
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["show-ref", "--heads"]),
        git(git_repo.path(), ["show-ref", "--heads"])
    );

    let quoted_batch = format!(
        "create \"refs/heads/quoted-ref\" \"{git_head}\"\n\
         update \"refs/heads/quoted-octal-\\162ef\" \"{git_head}\" \"{zero}\"\n"
    );
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin"],
            &quoted_batch,
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin"],
            &quoted_batch,
            "git",
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["show-ref", "--heads"]),
        git(git_repo.path(), ["show-ref", "--heads"])
    );

    let badly_quoted = format!("create \"refs/heads/bad {git_head}\n");
    let zmin = command_any_output_with_stdin(
        zmin_bin(),
        zmin_repo.path(),
        &["update-ref", "--stdin"],
        &badly_quoted,
        "zmin",
    );
    let git_output = command_any_output_with_stdin(
        "git",
        git_repo.path(),
        &["update-ref", "--stdin"],
        &badly_quoted,
        "git",
    );
    assert_eq!(zmin.0, git_output.0);
    assert_eq!(zmin.1, git_output.1);
    assert_eq!(zmin.2.lines().next(), git_output.2.lines().next());

    let transaction = format!(
        "start\n\
         update refs/heads/transaction-a {git_head}\n\
         prepare\n\
         commit\n"
    );
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin"],
            &transaction,
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin"],
            &transaction,
            "git",
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["show-ref", "--heads"]),
        git(git_repo.path(), ["show-ref", "--heads"])
    );

    let bad_old = "1111111111111111111111111111111111111111";
    let rejected = format!(
        "start\n\
         update refs/heads/should-not-exist {git_head}\n\
         verify refs/heads/{default_branch} {bad_old}\n\
         prepare\n\
         commit\n",
        default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"])
    );
    let zmin = command_any_output_with_stdin(
        zmin_bin(),
        zmin_repo.path(),
        &["update-ref", "--stdin"],
        &rejected,
        "zmin",
    );
    let git = command_any_output_with_stdin(
        "git",
        git_repo.path(),
        &["update-ref", "--stdin"],
        &rejected,
        "git",
    );
    assert_eq!(zmin.0, git.0);
    assert_eq!(zmin.1, git.1);
    assert_eq!(zmin.2.lines().next(), git.2.lines().next());

    let bad_option = "option nope\n";
    let zmin = command_any_output_with_stdin(
        zmin_bin(),
        zmin_repo.path(),
        &["update-ref", "--stdin"],
        bad_option,
        "zmin",
    );
    let git = command_any_output_with_stdin(
        "git",
        git_repo.path(),
        &["update-ref", "--stdin"],
        bad_option,
        "git",
    );
    assert_eq!(zmin.0, git.0);
    assert_eq!(zmin.1, git.1);
    assert_eq!(zmin.2.lines().next(), git.2.lines().next());

    assert_eq!(
        run_zmin_status(
            zmin_repo.path(),
            ["show-ref", "--verify", "refs/heads/should-not-exist"]
        ),
        git_status(
            git_repo.path(),
            ["show-ref", "--verify", "refs/heads/should-not-exist"]
        )
    );
}

#[test]
fn update_ref_stdin_batch_updates_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();
    let head = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let bad_old = "1111111111111111111111111111111111111111";

    for name in [
        "refs/heads/existing-create",
        "refs/heads/existing-update",
        "refs/heads/existing-tx",
        "refs/heads/existing-z",
    ] {
        git(git_repo.path(), ["update-ref", name, &head]);
        run_zmin(zmin_repo.path(), ["update-ref", name, &head]);
    }

    let batch = format!(
        "update refs/heads/batch-ok {head}\n\
         create refs/heads/existing-create {head}\n\
         update refs/heads/existing-update {head} {bad_old}\n\
         verify refs/heads/missing-batch {bad_old}\n\
         update refs/heads/batch-after {head}\n"
    );
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin", "--batch-updates"],
            &batch,
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin", "--batch-updates"],
            &batch,
            "git",
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["show-ref", "--heads"]),
        git(git_repo.path(), ["show-ref", "--heads"])
    );

    let transaction = format!(
        "start\n\
         update refs/heads/batch-tx-ok {head}\n\
         update refs/heads/existing-tx {head} {bad_old}\n\
         prepare\n\
         commit\n"
    );
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin", "--batch-updates"],
            &transaction,
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin", "--batch-updates"],
            &transaction,
            "git",
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["show-ref", "--heads"]),
        git(git_repo.path(), ["show-ref", "--heads"])
    );

    let nul_batch = format!(
        "update refs/heads/batch-z-ok\0{head}\0\0\
         update refs/heads/existing-z\0{head}\0{bad_old}\0\
         update refs/heads/batch-z-after\0{head}\0\0"
    );
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin", "-z", "--batch-updates"],
            &nul_batch,
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin", "-z", "--batch-updates"],
            &nul_batch,
            "git",
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["show-ref", "--heads"]),
        git(git_repo.path(), ["show-ref", "--heads"])
    );

    let repeated = format!(
        "update refs/heads/repeated {head}\n\
         update refs/heads/repeated {head}\n"
    );
    let zmin = command_any_output_with_stdin(
        zmin_bin(),
        zmin_repo.path(),
        &["update-ref", "--stdin", "--batch-updates"],
        &repeated,
        "zmin",
    );
    let git = command_any_output_with_stdin(
        "git",
        git_repo.path(),
        &["update-ref", "--stdin", "--batch-updates"],
        &repeated,
        "git",
    );
    assert_eq!(zmin.0, git.0);
    assert_eq!(zmin.1, git.1);
    assert_eq!(zmin.2.lines().next(), git.2.lines().next());
}

#[test]
fn update_ref_reflog_updates_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();
    let head = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let env = [
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000100 +0000"),
    ];

    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "update-ref",
                "-m",
                "create via update-ref",
                "refs/heads/reflogged",
                &head,
            ],
            &env,
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &[
                "update-ref",
                "-m",
                "create via update-ref",
                "refs/heads/reflogged",
                &head,
            ],
            &env,
            "git",
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/logs/refs/heads/reflogged"))
            .expect("read zmin branch reflog"),
        fs::read_to_string(git_repo.path().join(".git/logs/refs/heads/reflogged"))
            .expect("read git branch reflog")
    );

    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "update-ref",
                "--create-reflog",
                "-m",
                "custom namespace",
                "refs/custom/reflogged",
                &head,
            ],
            &env,
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &[
                "update-ref",
                "--create-reflog",
                "-m",
                "custom namespace",
                "refs/custom/reflogged",
                &head,
            ],
            &env,
            "git",
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/logs/refs/custom/reflogged"))
            .expect("read zmin custom reflog"),
        fs::read_to_string(git_repo.path().join(".git/logs/refs/custom/reflogged"))
            .expect("read git custom reflog")
    );

    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "-d", "refs/heads/reflogged"],
            &env,
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["update-ref", "-d", "refs/heads/reflogged"],
            &env,
            "git",
        )
    );
    assert_eq!(
        zmin_repo
            .path()
            .join(".git/logs/refs/heads/reflogged")
            .exists(),
        git_repo
            .path()
            .join(".git/logs/refs/heads/reflogged")
            .exists()
    );
}

#[test]
fn packed_refs_are_resolved_updated_and_deleted_like_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();
    let first = git(git_repo.path(), ["rev-parse", "HEAD"]);
    assert_eq!(git(zmin_repo.path(), ["rev-parse", "HEAD"]), first);

    write_file(git_repo.path(), "second.txt", "second\n");
    git(git_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "second"]);
    write_file(zmin_repo.path(), "second.txt", "second\n");
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "second"]);
    let second = git(git_repo.path(), ["rev-parse", "HEAD"]);
    assert_eq!(git(zmin_repo.path(), ["rev-parse", "HEAD"]), second);

    for ref_name in ["refs/heads/packed-stale", "refs/heads/packed-delete"] {
        git(git_repo.path(), ["update-ref", ref_name, &first]);
        git(zmin_repo.path(), ["update-ref", ref_name, &first]);
    }
    git(git_repo.path(), ["tag", "packed-light", &first]);
    git(zmin_repo.path(), ["tag", "packed-light", &first]);
    git(git_repo.path(), ["pack-refs", "--all", "--prune"]);
    git(zmin_repo.path(), ["pack-refs", "--all", "--prune"]);

    assert_eq!(
        run_zmin(zmin_repo.path(), ["show-ref", "--heads"]),
        git(git_repo.path(), ["show-ref", "--heads"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["branch", "--list"]),
        git(git_repo.path(), ["branch", "--list"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["branch", "--list", "packed-*"]),
        git(git_repo.path(), ["branch", "--list", "packed-*"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["tag", "--list"]),
        git(git_repo.path(), ["tag", "--list"])
    );

    git(
        git_repo.path(),
        ["update-ref", "refs/heads/packed-stale", &second],
    );
    run_zmin(
        zmin_repo.path(),
        ["update-ref", "refs/heads/packed-stale", &second],
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["show-ref", "--verify", "refs/heads/packed-stale"]
        ),
        git(
            git_repo.path(),
            ["show-ref", "--verify", "refs/heads/packed-stale"]
        )
    );

    git(
        git_repo.path(),
        ["update-ref", "-d", "refs/heads/packed-delete"],
    );
    run_zmin(
        zmin_repo.path(),
        ["update-ref", "-d", "refs/heads/packed-delete"],
    );
    assert_eq!(
        run_zmin_status(
            zmin_repo.path(),
            ["show-ref", "--verify", "refs/heads/packed-delete"]
        ),
        git_status(
            git_repo.path(),
            ["show-ref", "--verify", "refs/heads/packed-delete"]
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/packed-refs"))
            .expect("read zmin packed-refs"),
        fs::read_to_string(git_repo.path().join(".git/packed-refs")).expect("read git packed-refs")
    );
}

#[test]
fn update_ref_no_deref_modes_match_stock_git_head_storage() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();
    let first = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(git(zmin_repo.path(), ["rev-parse", "HEAD"]), first);

    write_file(git_repo.path(), "second.txt", "second\n");
    git(git_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "second"]);
    write_file(zmin_repo.path(), "second.txt", "second\n");
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "second"]);
    let second = git(git_repo.path(), ["rev-parse", "HEAD"]);
    assert_eq!(git(zmin_repo.path(), ["rev-parse", "HEAD"]), second);

    git(git_repo.path(), ["update-ref", "HEAD", &first]);
    run_zmin(zmin_repo.path(), ["update-ref", "HEAD", &first]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/HEAD")).expect("read zmin HEAD"),
        fs::read_to_string(git_repo.path().join(".git/HEAD")).expect("read git HEAD")
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["rev-parse", "HEAD"]),
        git(git_repo.path(), ["rev-parse", "HEAD"])
    );

    git(
        git_repo.path(),
        ["update-ref", "--no-deref", "HEAD", &second],
    );
    run_zmin(
        zmin_repo.path(),
        ["update-ref", "--no-deref", "HEAD", &second],
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/HEAD")).expect("read zmin HEAD"),
        fs::read_to_string(git_repo.path().join(".git/HEAD")).expect("read git HEAD")
    );

    git(
        git_repo.path(),
        [
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{default_branch}"),
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{default_branch}"),
        ],
    );
    git(
        git_repo.path(),
        [
            "update-ref",
            &format!("refs/heads/{default_branch}"),
            &second,
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "update-ref",
            &format!("refs/heads/{default_branch}"),
            &second,
        ],
    );
    let stdin = format!("option no-deref\nupdate HEAD {first}\n");
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin"],
            &stdin,
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin"],
            &stdin,
            "git",
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/HEAD")).expect("read zmin HEAD"),
        fs::read_to_string(git_repo.path().join(".git/HEAD")).expect("read git HEAD")
    );

    git(
        git_repo.path(),
        [
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{default_branch}"),
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{default_branch}"),
        ],
    );
    git(
        git_repo.path(),
        [
            "update-ref",
            &format!("refs/heads/{default_branch}"),
            &second,
        ],
    );
    run_zmin(
        zmin_repo.path(),
        [
            "update-ref",
            &format!("refs/heads/{default_branch}"),
            &second,
        ],
    );
    let nul_stdin = format!("option no-deref\0update HEAD\0{first}\0\0");
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin", "-z"],
            &nul_stdin,
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin", "-z"],
            &nul_stdin,
            "git",
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/HEAD")).expect("read zmin HEAD"),
        fs::read_to_string(git_repo.path().join(".git/HEAD")).expect("read git HEAD")
    );

    let delete_git_repo = committed_repo();
    let delete_zmin_repo = committed_repo();
    let delete_branch = git(
        delete_git_repo.path(),
        ["rev-parse", "--abbrev-ref", "HEAD"],
    );
    assert_eq!(
        run_zmin_status(delete_zmin_repo.path(), ["update-ref", "-d", "HEAD"]),
        git_status(delete_git_repo.path(), ["update-ref", "-d", "HEAD"])
    );
    assert_eq!(
        fs::read_to_string(delete_zmin_repo.path().join(".git/HEAD"))
            .expect("read zmin symbolic HEAD"),
        fs::read_to_string(delete_git_repo.path().join(".git/HEAD"))
            .expect("read git symbolic HEAD")
    );
    assert_eq!(
        delete_zmin_repo
            .path()
            .join(".git/refs/heads")
            .join(&delete_branch)
            .exists(),
        delete_git_repo
            .path()
            .join(".git/refs/heads")
            .join(&delete_branch)
            .exists()
    );

    let no_deref_delete_git_repo = committed_repo();
    let no_deref_delete_zmin_repo = committed_repo();
    assert_eq!(
        run_zmin_status(
            no_deref_delete_zmin_repo.path(),
            ["update-ref", "--no-deref", "-d", "HEAD"],
        ),
        git_status(
            no_deref_delete_git_repo.path(),
            ["update-ref", "--no-deref", "-d", "HEAD"],
        )
    );
    assert_eq!(
        no_deref_delete_zmin_repo.path().join(".git/HEAD").exists(),
        no_deref_delete_git_repo.path().join(".git/HEAD").exists()
    );
}

#[test]
fn update_ref_stdin_symref_commands_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();
    let zero = "0000000000000000000000000000000000000000";

    let create = "symref-create refs/heads/sym refs/heads/main\n";
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin"],
            create,
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin"],
            create,
            "git",
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/refs/heads/sym")).expect("read zmin symref"),
        fs::read_to_string(git_repo.path().join(".git/refs/heads/sym")).expect("read git symref")
    );

    let verify = "option no-deref\nsymref-verify refs/heads/sym refs/heads/main\n";
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin"],
            verify,
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin"],
            verify,
            "git",
        )
    );

    let update =
        "option no-deref\nsymref-update refs/heads/sym refs/heads/other ref refs/heads/main\n";
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin"],
            update,
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin"],
            update,
            "git",
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/refs/heads/sym"))
            .expect("read zmin updated symref"),
        fs::read_to_string(git_repo.path().join(".git/refs/heads/sym"))
            .expect("read git updated symref")
    );

    let z_update =
        b"option no-deref\0symref-update refs/heads/sym\0refs/heads/main\0ref\0refs/heads/other\0";
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin", "-z"],
            std::str::from_utf8(z_update).expect("z update utf8"),
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin", "-z"],
            std::str::from_utf8(z_update).expect("z update utf8"),
            "git",
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/refs/heads/sym"))
            .expect("read zmin z symref"),
        fs::read_to_string(git_repo.path().join(".git/refs/heads/sym")).expect("read git z symref")
    );

    let delete = "option no-deref\nsymref-delete refs/heads/sym refs/heads/main\n";
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin"],
            delete,
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin"],
            delete,
            "git",
        )
    );
    assert_eq!(
        zmin_repo.path().join(".git/refs/heads/sym").exists(),
        git_repo.path().join(".git/refs/heads/sym").exists()
    );

    let create_with_oid_zero =
        format!("option no-deref\nsymref-update refs/heads/sym refs/heads/main oid {zero}\n");
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin"],
            &create_with_oid_zero,
            "zmin",
        ),
        command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin"],
            &create_with_oid_zero,
            "git",
        )
    );

    let repeated = "symref-create refs/heads/repeat refs/heads/main\nsymref-update refs/heads/repeat refs/heads/other\n";
    let zmin = command_any_output_with_stdin(
        zmin_bin(),
        zmin_repo.path(),
        &["update-ref", "--stdin"],
        repeated,
        "zmin",
    );
    let git = command_any_output_with_stdin(
        "git",
        git_repo.path(),
        &["update-ref", "--stdin"],
        repeated,
        "git",
    );
    assert_eq!(zmin.0, git.0);
    assert_eq!(zmin.1, git.1);
    assert_eq!(zmin.2.lines().next(), git.2.lines().next());

    let deref_verify = "symref-verify refs/heads/sym refs/heads/main\n";
    let zmin = command_any_output_with_stdin(
        zmin_bin(),
        zmin_repo.path(),
        &["update-ref", "--stdin"],
        deref_verify,
        "zmin",
    );
    let git = command_any_output_with_stdin(
        "git",
        git_repo.path(),
        &["update-ref", "--stdin"],
        deref_verify,
        "git",
    );
    assert_eq!(zmin.0, git.0);
    assert_eq!(zmin.1, git.1);
    assert_eq!(zmin.2.lines().next(), git.2.lines().next());
}

#[test]
fn update_ref_invalid_refname_failures_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();
    let head = git(git_repo.path(), ["rev-parse", "HEAD"]);

    for ref_name in [
        "refs/heads/bad..name",
        "refs/heads/bad.lock",
        "refs/heads/bad/name.lock",
        "refs/heads/bad~name",
    ] {
        let zmin = command_any_output(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", ref_name, &head],
            "zmin",
        );
        let git = command_any_output(
            "git",
            git_repo.path(),
            &["update-ref", ref_name, &head],
            "git",
        );
        assert_eq!(zmin.0, git.0, "status for {ref_name}");
        assert_eq!(zmin.1, git.1, "stdout for {ref_name}");
        assert_eq!(
            zmin.2.lines().next(),
            git.2.lines().next(),
            "stderr for {ref_name}"
        );
    }

    let zmin = command_any_output(
        zmin_bin(),
        zmin_repo.path(),
        &["update-ref", "-d", "refs/heads/bad..name"],
        "zmin",
    );
    let git = command_any_output(
        "git",
        git_repo.path(),
        &["update-ref", "-d", "refs/heads/bad..name"],
        "git",
    );
    assert_eq!(zmin, git);

    for input in [
        format!("create refs/heads/bad..name {head}\n"),
        "symref-create refs/heads/sym refs/heads/bad..target\n".to_owned(),
    ] {
        let zmin = command_any_output_with_stdin(
            zmin_bin(),
            zmin_repo.path(),
            &["update-ref", "--stdin"],
            &input,
            "zmin",
        );
        let git = command_any_output_with_stdin(
            "git",
            git_repo.path(),
            &["update-ref", "--stdin"],
            &input,
            "git",
        );
        assert_eq!(zmin.0, git.0);
        assert_eq!(zmin.1, git.1);
        assert_eq!(zmin.2.lines().next(), git.2.lines().next());
    }
}

#[test]
fn branch_create_list_delete_and_rename_match_stock_git_state() {
    let repo = committed_repo();

    assert_eq!(
        run_zmin(repo.path(), ["branch", "--show-current"]),
        git(repo.path(), ["branch", "--show-current"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["branch", "--show-current", "ignored"]),
        git(repo.path(), ["branch", "--show-current", "ignored"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]),
        git(repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["rev-parse", "--symbolic-full-name", "HEAD"]),
        git(repo.path(), ["rev-parse", "--symbolic-full-name", "HEAD"])
    );
    run_zmin(repo.path(), ["branch", "feature"]);
    assert_eq!(
        run_zmin(
            repo.path(),
            ["rev-parse", "--symbolic-full-name", "feature"]
        ),
        git(
            repo.path(),
            ["rev-parse", "--symbolic-full-name", "feature"]
        )
    );
    assert_eq!(
        run_zmin(repo.path(), ["branch"]),
        git(repo.path(), ["branch"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["show-ref", "--heads"]),
        git(repo.path(), ["show-ref", "--heads"])
    );

    run_zmin(repo.path(), ["branch", "-d", "feature"]);
    assert_eq!(
        run_zmin(repo.path(), ["show-ref", "--heads"]),
        git(repo.path(), ["show-ref", "--heads"])
    );

    run_zmin(repo.path(), ["branch", "force-delete"]);
    run_zmin(repo.path(), ["branch", "-D", "force-delete"]);
    assert_eq!(
        run_zmin(repo.path(), ["show-ref", "--heads"]),
        git(repo.path(), ["show-ref", "--heads"])
    );

    run_zmin(repo.path(), ["branch", "rename-source"]);
    run_zmin(
        repo.path(),
        ["branch", "-m", "rename-source", "rename-target"],
    );
    assert_eq!(
        run_zmin(repo.path(), ["show-ref", "--heads"]),
        git(repo.path(), ["show-ref", "--heads"])
    );
    run_zmin(repo.path(), ["checkout", "rename-target"]);
    run_zmin(repo.path(), ["branch", "-m", "rename-current"]);
    assert_eq!(
        git(repo.path(), ["symbolic-ref", "HEAD"]),
        "refs/heads/rename-current"
    );
    run_zmin(repo.path(), ["branch", "force-source"]);
    run_zmin(repo.path(), ["branch", "force-dest"]);
    run_zmin(repo.path(), ["branch", "-M", "force-source", "force-dest"]);
    assert_eq!(
        run_zmin(repo.path(), ["show-ref", "--heads"]),
        git(repo.path(), ["show-ref", "--heads"])
    );

    git(repo.path(), ["switch", "--detach", "HEAD"]);
    assert_eq!(
        run_zmin(repo.path(), ["branch", "--show-current"]),
        git(repo.path(), ["branch", "--show-current"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]),
        git(repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"])
    );
}

#[test]
fn branch_upstream_config_matches_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);
    let remote_key = format!("branch.{default_branch}.remote");
    let merge_key = format!("branch.{default_branch}.merge");

    git(
        git_repo.path(),
        ["remote", "add", "origin", "../remote.git"],
    );
    git(
        zmin_repo.path(),
        ["remote", "add", "origin", "../remote.git"],
    );
    git(
        git_repo.path(),
        ["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    git(
        zmin_repo.path(),
        ["update-ref", "refs/remotes/origin/main", "HEAD"],
    );

    git(git_repo.path(), ["branch", "-u", "origin/main"]);
    run_zmin(zmin_repo.path(), ["branch", "-u", "origin/main"]);
    assert_eq!(
        git(zmin_repo.path(), ["config", "--get", &remote_key]),
        git(git_repo.path(), ["config", "--get", &remote_key])
    );
    assert_eq!(
        git(zmin_repo.path(), ["config", "--get", &merge_key]),
        git(git_repo.path(), ["config", "--get", &merge_key])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["status", "--porcelain=v1", "--branch"]),
        git(git_repo.path(), ["status", "--porcelain=v1", "--branch"])
    );

    git(git_repo.path(), ["branch", "--unset-upstream"]);
    run_zmin(zmin_repo.path(), ["branch", "--unset-upstream"]);
    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["config", "--get", &remote_key]),
        git_status(git_repo.path(), ["config", "--get", &remote_key])
    );
    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["config", "--get", &merge_key]),
        git_status(git_repo.path(), ["config", "--get", &merge_key])
    );

    git(git_repo.path(), ["branch", "feature"]);
    run_zmin(zmin_repo.path(), ["branch", "feature"]);
    git(
        git_repo.path(),
        ["branch", "--set-upstream-to=feature", &default_branch],
    );
    run_zmin(
        zmin_repo.path(),
        ["branch", "--set-upstream-to=feature", &default_branch],
    );
    assert_eq!(
        git(zmin_repo.path(), ["config", "--get", &remote_key]),
        git(git_repo.path(), ["config", "--get", &remote_key])
    );
    assert_eq!(
        git(zmin_repo.path(), ["config", "--get", &merge_key]),
        git(git_repo.path(), ["config", "--get", &merge_key])
    );
}

#[test]
fn for_each_ref_upstream_atoms_match_stock_git() {
    let repo = committed_repo();
    let default_branch = git(repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);
    let format = "%(refname)%00%(objectname)%00%(upstream)%00%(upstream:short)%00%(upstream:track)%00%(upstream:trackshort)%00%(HEAD)%00%(committerdate:unix)";
    let branch_format = "%(refname)%00%(upstream:short)%00%(upstream:track)";

    git(repo.path(), ["remote", "add", "origin", "../remote.git"]);
    git(
        repo.path(),
        ["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    git(
        repo.path(),
        ["branch", "-u", "origin/main", &default_branch],
    );

    assert_eq!(
        run_zmin(
            repo.path(),
            ["for-each-ref", "--format", format, "refs/heads"]
        ),
        git(
            repo.path(),
            ["for-each-ref", "--format", format, "refs/heads"]
        )
    );
    assert_eq!(
        run_zmin(repo.path(), ["branch", "--format", branch_format, "--list"]),
        git(repo.path(), ["branch", "--format", branch_format, "--list"])
    );

    write_file(repo.path(), "ahead.txt", "ahead\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "ahead"]);
    assert_eq!(
        run_zmin(
            repo.path(),
            ["for-each-ref", "--format", format, "refs/heads"]
        ),
        git(
            repo.path(),
            ["for-each-ref", "--format", format, "refs/heads"]
        )
    );
    assert_eq!(
        run_zmin(repo.path(), ["branch", "--format", branch_format, "--list"]),
        git(repo.path(), ["branch", "--format", branch_format, "--list"])
    );

    git(
        repo.path(),
        ["update-ref", "-d", "refs/remotes/origin/main"],
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            ["for-each-ref", "--format", format, "refs/heads"]
        ),
        git(
            repo.path(),
            ["for-each-ref", "--format", format, "refs/heads"]
        )
    );
}

#[test]
fn branch_contains_merged_and_no_merged_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);
    let base = git(git_repo.path(), ["rev-parse", "HEAD"]);

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["branch", "topic"]);
        git(repo, ["checkout", "topic"]);
        write_file(repo, "topic.txt", "topic\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "topic"]);
        git(repo, ["checkout", &default_branch]);
        write_file(repo, "main.txt", "main\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "main"]);
        git(repo, ["branch", "merged-at-base", &base]);
        git(repo, ["update-ref", "refs/remotes/origin/topic", "topic"]);
    }

    for args in [
        vec!["branch", "--contains", &base],
        vec!["branch", "--merged"],
        vec!["branch", "--merged", "HEAD"],
        vec!["branch", "--no-merged"],
        vec!["branch", "--contains", &base, "-a"],
        vec!["branch", "--no-merged", "HEAD", "-a"],
        vec!["branch", "--contains", &base, "--merged", "HEAD"],
        vec!["branch", "--contains", &base, "--no-merged", "HEAD"],
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), &args),
            git_args(git_repo.path(), &args),
            "branch filter output should match for {args:?}"
        );
    }

    for args in [
        ["branch", "--contains", "missing"].as_slice(),
        ["branch", "--merged", "missing"].as_slice(),
        ["branch", "--no-merged", "missing"].as_slice(),
    ] {
        let zmin = run_zmin_failure_output(zmin_repo.path(), args);
        let git = git_failure_output(git_repo.path(), args);
        assert_eq!(zmin.0, git.0, "exit status should match for {args:?}");
        assert_eq!(zmin.1, git.1, "stdout should match for {args:?}");
        assert_eq!(
            zmin.2.lines().next(),
            git.2.lines().next(),
            "primary stderr line should match for {args:?}"
        );
    }
}

#[test]
fn tag_contains_merged_and_no_merged_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();
    let default_branch = git(git_repo.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);
    let base = git(git_repo.path(), ["rev-parse", "HEAD"]);

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["tag", "base-light"]);
        git_with_env(repo, ["tag", "-a", "base-ann", "-m", "base-ann"]);
        git(repo, ["checkout", "-b", "topic"]);
        write_file(repo, "topic.txt", "topic\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "topic"]);
        git(repo, ["tag", "topic-light"]);
        git_with_env(repo, ["tag", "-a", "topic-ann", "-m", "topic-ann"]);
        git(repo, ["checkout", &default_branch]);
        write_file(repo, "main.txt", "main\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "main"]);
        git(repo, ["tag", "main-light"]);
    }
    for (name, message, timestamp) in [
        ("date-old", "Old subject", "1700000100 +0000"),
        ("date-new", "New subject", "1700000200 +0000"),
    ] {
        let env = [
            ("GIT_COMMITTER_NAME", "Bench"),
            ("GIT_COMMITTER_EMAIL", "bench@example.test"),
            ("GIT_COMMITTER_DATE", timestamp),
        ];
        command_output_with_env(
            "git",
            git_repo.path(),
            &["tag", "-a", name, "-m", message],
            &env,
            "git",
        );
        command_output_with_env(
            "git",
            zmin_repo.path(),
            &["tag", "-a", name, "-m", message],
            &env,
            "git",
        );
    }

    for args in [
        vec!["tag", "--contains", &base],
        vec!["tag", "--no-contains", "HEAD"],
        vec!["tag", "--merged", "HEAD"],
        vec!["tag", "--no-merged", "HEAD"],
        vec!["tag", "--contains", &base, "--merged", "HEAD"],
        vec!["tag", "--contains", &base, "--no-merged", "HEAD"],
        vec!["tag", "--contains", &base, "topic-*"],
        vec!["tag", "--sort=-refname"],
        vec!["tag", "--sort=refname", "--format=%(refname:short)"],
        vec!["tag", "--sort=refname", "--format=%(objecttype):%(subject)"],
        vec![
            "tag",
            "--list",
            "date-*",
            "--sort=-taggerdate",
            "--format=%(refname:short)|%(objectname:short)|%(objecttype)|%(contents:subject)|%(taggername)|%(taggeremail)|%(taggerdate:unix)",
        ],
        vec![
            "tag",
            "--contains",
            &base,
            "--sort=-refname",
            "--format=%(refname:short)",
        ],
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), &args),
            git_args(git_repo.path(), &args),
            "tag filter output should match for {args:?}"
        );
    }

    for args in [
        ["tag", "--contains", "missing"].as_slice(),
        ["tag", "--no-contains", "missing"].as_slice(),
        ["tag", "--merged", "missing"].as_slice(),
        ["tag", "--no-merged", "missing"].as_slice(),
        ["tag", "--sort=nope"].as_slice(),
        ["tag", "--format=%(nope)"].as_slice(),
    ] {
        let zmin = run_zmin_failure_output(zmin_repo.path(), args);
        let git = git_failure_output(git_repo.path(), args);
        assert_eq!(zmin.0, git.0, "exit status should match for {args:?}");
        assert_eq!(zmin.1, git.1, "stdout should match for {args:?}");
        assert_eq!(
            zmin.2.lines().next(),
            git.2.lines().next(),
            "primary stderr line should match for {args:?}"
        );
    }
}

#[test]
fn tag_create_list_delete_and_annotated_objects_match_stock_git_state() {
    let repo = committed_repo();

    run_zmin(repo.path(), ["tag", "v1.0.0"]);
    assert_eq!(run_zmin(repo.path(), ["tag"]), git(repo.path(), ["tag"]));
    assert_eq!(
        run_zmin(repo.path(), ["tag", "--list", "v1.*"]),
        git(repo.path(), ["tag", "--list", "v1.*"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["show-ref", "--tags"]),
        git(repo.path(), ["show-ref", "--tags"])
    );

    run_zmin(repo.path(), ["tag", "-d", "v1.0.0"]);
    assert_eq!(run_zmin(repo.path(), ["tag"]), git(repo.path(), ["tag"]));

    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    write_file(git_repo.path(), "a.txt", "hello\n");
    write_file(zmin_repo.path(), "a.txt", "hello\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "initial"]);

    git_with_env(git_repo.path(), ["tag", "-a", "v1.0.0", "-m", "release"]);
    run_zmin_with_env(zmin_repo.path(), ["tag", "-a", "v1.0.0", "-m", "release"]);

    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-t", "v1.0.0"]),
        git(git_repo.path(), ["cat-file", "-t", "v1.0.0"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "refs/tags/v1.0.0"]),
        git(git_repo.path(), ["cat-file", "-p", "refs/tags/v1.0.0"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["show-ref", "--tags"]),
        git(git_repo.path(), ["show-ref", "--tags"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["rev-parse", "--verify", "v1.0.0^{tag}"]),
        git(git_repo.path(), ["rev-parse", "--verify", "v1.0.0^{tag}"])
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["rev-parse", "--verify", "v1.0.0^{object}"],
        ),
        git(
            git_repo.path(),
            ["rev-parse", "--verify", "v1.0.0^{object}"],
        )
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["rev-parse", "--verify", "v1.0.0^{commit}"],
        ),
        git(
            git_repo.path(),
            ["rev-parse", "--verify", "v1.0.0^{commit}"],
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["rev-parse", "--verify", "v1.0.0^{}"]),
        git(git_repo.path(), ["rev-parse", "--verify", "v1.0.0^{}"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["rev-parse", "--verify", "HEAD^{commit}"],),
        git(git_repo.path(), ["rev-parse", "--verify", "HEAD^{commit}"])
    );

    git_with_env(
        git_repo.path(),
        ["tag", "v1.0.1", "-m", "implicit annotated"],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["tag", "v1.0.1", "-m", "implicit annotated"],
    );
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "refs/tags/v1.0.1"]),
        git(git_repo.path(), ["cat-file", "-p", "refs/tags/v1.0.1"])
    );
}

#[test]
fn tag_documented_creation_flags_match_stock_git() {
    let cases = [
        ["tag", "--message", "release long", "v-msg"].as_slice(),
        ["tag", "--file", "msg.txt", "v-file"].as_slice(),
        ["tag", "-F", "msg.txt", "v-file-short"].as_slice(),
        ["tag", "--create-reflog", "v-log"].as_slice(),
    ];

    for args in cases {
        let git_repo = committed_repo();
        let zmin_repo = committed_repo();
        for repo in [git_repo.path(), zmin_repo.path()] {
            write_file(repo, "msg.txt", "msg from file\n");
            git(repo, ["tag", "base-light"]);
        }

        let git_output = command_any_output("git", git_repo.path(), args, "git tag");
        let zmin_output = command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin tag");
        assert_eq!(
            zmin_output, git_output,
            "tag args should match for {args:?}"
        );
        assert_eq!(
            git(
                zmin_repo.path(),
                [
                    "for-each-ref",
                    "--format=%(refname) %(objectname) %(objecttype)",
                    "refs/tags"
                ]
            ),
            git(
                git_repo.path(),
                [
                    "for-each-ref",
                    "--format=%(refname) %(objectname) %(objecttype)",
                    "refs/tags"
                ]
            ),
            "tag refs should match for {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["status", "--short"]),
            git(git_repo.path(), ["status", "--short"]),
            "status should match for {args:?}"
        );
        assert_eq!(
            fs::read_to_string(zmin_repo.path().join(".git/logs/refs/tags/v-log")).ok(),
            fs::read_to_string(git_repo.path().join(".git/logs/refs/tags/v-log")).ok(),
            "tag reflog state should match for {args:?}"
        );
    }
}

#[test]
fn tag_documented_listing_flags_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["tag", "alpha"]);
        git(repo, ["tag", "gamma"]);
        git_with_env(repo, ["tag", "--message", "release long", "beta"]);
    }

    let alpha = git(git_repo.path(), ["rev-parse", "alpha"]);
    for args in [
        vec!["tag", "--points-at", alpha.trim()],
        vec!["tag", "--format=", "--omit-empty"],
        vec!["tag", "--column"],
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), &args),
            git_args(git_repo.path(), &args),
            "tag listing output should match for {args:?}"
        );
        assert_eq!(
            git(
                zmin_repo.path(),
                [
                    "for-each-ref",
                    "--format=%(refname) %(objectname) %(objecttype)",
                    "refs/tags"
                ]
            ),
            git(
                git_repo.path(),
                [
                    "for-each-ref",
                    "--format=%(refname) %(objectname) %(objecttype)",
                    "refs/tags"
                ]
            ),
            "tag refs should remain unchanged for {args:?}"
        );
    }
}

#[test]
fn tag_remaining_documented_flags_match_stock_git() {
    let git_repo = committed_repo();
    let zmin_repo = committed_repo();

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["tag", "base"]);
    }

    let editor_env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ("GIT_EDITOR", ":"),
    ];

    for (args, tag_name) in [
        (
            vec!["tag", "--cleanup=verbatim", "v-clean", "-m", "msg"],
            "v-clean",
        ),
        (vec!["tag", "--edit", "v-edit", "-m", "msg"], "v-edit"),
        (vec!["tag", "--no-sign", "v-nosign", "-m", "msg"], "v-nosign"),
        (
            vec![
                "tag",
                "--trailer",
                "Signed-off-by: Me <me@example.com>",
                "v-trailer",
                "-m",
                "msg",
            ],
            "v-trailer",
        ),
        (vec!["tag", "-e", "v-edit-short", "-m", "msg"], "v-edit-short"),
    ] {
        assert_eq!(
            command_output_with_env("git", git_repo.path(), &args, &editor_env, "git tag"),
            command_output_with_env(zmin_bin(), zmin_repo.path(), &args, &editor_env, "zmin tag"),
            "tag args should match for {args:?}"
        );
        assert_eq!(
            git(zmin_repo.path(), ["cat-file", "-p", &format!("refs/tags/{tag_name}")]),
            git(git_repo.path(), ["cat-file", "-p", &format!("refs/tags/{tag_name}")]),
            "tag object payload should match for {args:?}"
        );
    }

    assert_eq!(
        command_output_with_env("git", git_repo.path(), &["tag", "-n"], &editor_env, "git tag -n"),
        command_output_with_env(zmin_bin(), zmin_repo.path(), &["tag", "-n"], &editor_env, "zmin tag -n"),
        "tag -n output should match stock Git"
    );

    let git_gnupg = TempDir::new().expect("git gnupg tempdir");
    let zmin_gnupg = TempDir::new().expect("zmin gnupg tempdir");
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(git_gnupg.path(), fs::Permissions::from_mode(0o700))
            .expect("chmod git gnupg");
        fs::set_permissions(zmin_gnupg.path(), fs::Permissions::from_mode(0o700))
            .expect("chmod zmin gnupg");
    }
    for gnupg_home in [git_gnupg.path(), zmin_gnupg.path()] {
        let output = std::process::Command::new("gpg")
            .arg("--list-keys")
            .env("GNUPGHOME", gnupg_home)
            .output()
            .expect("run gpg --list-keys");
        assert!(
            output.status.success(),
            "gpg --list-keys failed for {}: {}",
            gnupg_home.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let git_gnupg_home = git_gnupg.path().to_str().expect("git gnupg path");
    let zmin_gnupg_home = zmin_gnupg.path().to_str().expect("zmin gnupg path");
    let git_gpg_env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ("GNUPGHOME", git_gnupg_home),
    ];
    let zmin_gpg_env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ("GNUPGHOME", zmin_gnupg_home),
    ];

    for args in [
        vec!["tag", "--sign", "v-sign", "-m", "msg"],
        vec!["tag", "-s", "v-sign-short", "-m", "msg"],
        vec!["tag", "--local-user=test@example.com", "v-local-user", "-m", "msg"],
        vec!["tag", "-u", "test@example.com", "v-local-user-short", "-m", "msg"],
    ] {
        assert_eq!(
            command_failure_output_with_env("git", git_repo.path(), &args, &git_gpg_env, "git tag"),
            command_failure_output_with_env(
                zmin_bin(),
                zmin_repo.path(),
                &args,
                &zmin_gpg_env,
                "zmin tag",
            ),
            "tag signing failure should match for {args:?}"
        );
    }

    assert_eq!(
        git(
            zmin_repo.path(),
            ["for-each-ref", "--format=%(refname) %(objectname) %(objecttype)", "refs/tags"],
        ),
        git(
            git_repo.path(),
            ["for-each-ref", "--format=%(refname) %(objectname) %(objecttype)", "refs/tags"],
        ),
        "tag refs should match after remaining documented flag coverage"
    );
}
