mod common;

use common::{
    command_any_output, git_args, git_failure_output, git_status_args, run_zmin_args,
    run_zmin_failure_output, run_zmin_status_args,
};

#[test]
fn check_ref_format_matches_stock_git_for_common_modes() {
    let repo = common::git_init();

    for args in [
        ["check-ref-format", "refs/heads/main"].as_slice(),
        ["check-ref-format", "--allow-onelevel", "main"].as_slice(),
        ["check-ref-format", "--no-allow-onelevel", "refs/heads/main"].as_slice(),
        ["check-ref-format", "--normalize", "/refs//heads/main"].as_slice(),
        [
            "check-ref-format",
            "--allow-onelevel",
            "--normalize",
            "main",
        ]
        .as_slice(),
        ["check-ref-format", "--branch", "main"].as_slice(),
        ["check-ref-format", "--branch", "refs/heads/main"].as_slice(),
        ["check-ref-format", "--refspec-pattern", "foo/bar*baz"].as_slice(),
        ["check-ref-format", "--refspec-pattern", "foo/bar*/baz"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args)
        );
    }

    for args in [
        ["check-ref-format", "main"].as_slice(),
        ["check-ref-format", "refs/heads/bad..name"].as_slice(),
        ["check-ref-format", "refs/heads/.hidden"].as_slice(),
        ["check-ref-format", "refs/heads/trailing."].as_slice(),
        ["check-ref-format", "--normalize", "refs/heads/main/"].as_slice(),
        ["check-ref-format", "--branch", "bad..name"].as_slice(),
        [
            "check-ref-format",
            "--refspec-pattern",
            "refs/heads/*:refs/remotes/origin/*",
        ]
        .as_slice(),
        ["check-ref-format", "--refspec-pattern", "foo/*/bar/*"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_status_args(repo.path(), args),
            git_status_args(repo.path(), args)
        );
    }
}

#[test]
fn check_ref_format_option_precedence_and_branch_modes_match_stock_git() {
    let repo = common::git_init();

    common::configure_identity(repo.path());
    common::write_file(repo.path(), "file.txt", "one\n");
    common::git(repo.path(), ["add", "file.txt"]);
    common::git_with_env(repo.path(), ["commit", "-m", "one"]);

    for args in [
        [
            "check-ref-format",
            "--allow-onelevel",
            "--no-allow-onelevel",
            "main",
        ]
        .as_slice(),
        [
            "check-ref-format",
            "--no-allow-onelevel",
            "--allow-onelevel",
            "main",
        ]
        .as_slice(),
        [
            "check-ref-format",
            "--allow-onelevel",
            "--refspec-pattern",
            "foo*",
        ]
        .as_slice(),
        [
            "check-ref-format",
            "--normalize",
            "--refspec-pattern",
            "/refs//heads/*",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            command_any_output(common::zmin_bin(), repo.path(), args, "zmin"),
            command_any_output("git", repo.path(), args, "git")
        );
    }

    common::git(repo.path(), ["checkout", "-b", "side"]);
    common::git(repo.path(), ["checkout", "main"]);
    assert_eq!(
        command_any_output(
            common::zmin_bin(),
            repo.path(),
            &["check-ref-format", "--branch", "@{-1}"],
            "zmin"
        ),
        command_any_output(
            "git",
            repo.path(),
            &["check-ref-format", "--branch", "@{-1}"],
            "git"
        )
    );

    common::git(repo.path(), ["checkout", "--detach", "HEAD"]);
    common::git(repo.path(), ["checkout", "-b", "topic"]);
    for args in [
        ["check-ref-format", "--branch", "@{-1}"].as_slice(),
        ["check-ref-format", "--branch", "@{-2}"].as_slice(),
        ["check-ref-format", "--branch", "@{-3}"].as_slice(),
    ] {
        assert_eq!(
            command_any_output(common::zmin_bin(), repo.path(), args, "zmin"),
            command_any_output("git", repo.path(), args, "git")
        );
    }

    for args in [
        ["check-ref-format", "--branch", "-bad"].as_slice(),
        ["check-ref-format", "--branch", "@{-99}"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(repo.path(), args),
            git_failure_output(repo.path(), args)
        );
    }
}
