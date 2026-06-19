mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    command_output_with_env, configure_identity, git, git_args, git_init, git_status_args,
    git_with_env, git_with_stdin, run_zmin_args, run_zmin_status_args, run_zmin_with_stdin,
    zmin_bin, write_file,
};

fn show_branch_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    git(repo.path(), ["checkout", "-b", "feature"]);
    write_file(repo.path(), "a.txt", "feature\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "feature"]);
    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "a.txt", "main2\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "main2"]);
    repo
}

#[test]
fn show_branch_matches_stock_git_for_local_branch_graphs() {
    let repo = show_branch_fixture_repo();
    git(
        repo.path(),
        ["update-ref", "refs/remotes/origin/main", "refs/heads/main"],
    );
    git(
        repo.path(),
        [
            "update-ref",
            "refs/remotes/origin/feature",
            "refs/heads/feature",
        ],
    );
    for args in [
        ["show-branch"].as_slice(),
        ["show-branch", "main", "feature"].as_slice(),
        ["show-branch", "--sha1-name", "main", "feature"].as_slice(),
        ["show-branch", "--all"].as_slice(),
        ["show-branch", "--remotes"].as_slice(),
        ["show-branch", "--current"].as_slice(),
        ["show-branch", "--current", "main"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn name_rev_matches_stock_git_for_refs_tags_and_stdin_annotation() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    git_with_env(repo.path(), ["commit", "--allow-empty", "-m", "base"]);
    git(repo.path(), ["tag", "v1"]);
    write_file(repo.path(), "next.txt", "next\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "next"]);
    git(repo.path(), ["branch", "feature"]);
    let head = git(repo.path(), ["rev-parse", "HEAD"]);
    let base = git(repo.path(), ["rev-parse", "HEAD~1"]);

    for args in [
        ["name-rev", &head].as_slice(),
        ["name-rev", "--name-only", &head].as_slice(),
        ["name-rev", &base].as_slice(),
        ["name-rev", "--tags", &base].as_slice(),
        ["name-rev", "--refs=refs/heads/main", &head].as_slice(),
        ["name-rev", "--exclude=refs/heads/feature", &head].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    let input = format!("head {head} base {base}\n");
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["name-rev", "--annotate-stdin"], &input),
        git_with_stdin(repo.path(), ["name-rev", "--annotate-stdin"], &input)
    );
}

#[test]
fn replace_matches_stock_git_for_list_create_delete() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        git_with_env(repo, ["commit", "--allow-empty", "-m", "one"]);
        git_with_env(repo, ["commit", "--allow-empty", "-m", "two"]);
    }
    let one = git(git_repo.path(), ["rev-parse", "HEAD~1"]);
    let two = git(git_repo.path(), ["rev-parse", "HEAD"]);
    assert_eq!(git(zmin_repo.path(), ["rev-parse", "HEAD~1"]), one);
    assert_eq!(git(zmin_repo.path(), ["rev-parse", "HEAD"]), two);

    git_args(git_repo.path(), &["replace", &two, &one]);
    run_zmin_args(zmin_repo.path(), &["replace", &two, &one]);
    assert_eq!(
        fs::read_to_string(git_repo.path().join(".git/refs/replace").join(&two))
            .expect("read git replace ref"),
        fs::read_to_string(zmin_repo.path().join(".git/refs/replace").join(&two))
            .expect("read zmin replace ref")
    );

    for args in [
        ["replace"].as_slice(),
        ["replace", "-l", "*"].as_slice(),
        ["replace", "--format=medium"].as_slice(),
        ["replace", "--format=long"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(zmin_repo.path(), args),
            git_args(git_repo.path(), args),
            "args: {args:?}"
        );
    }

    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["replace", "-d", &two[..12]]),
        git_args(git_repo.path(), &["replace", "-d", &two[..12]])
    );
    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["replace"]),
        git_args(git_repo.path(), &["replace"])
    );
}

#[test]
fn replace_graft_matches_stock_git_for_reparenting_commit() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        git_with_env(repo, ["commit", "--allow-empty", "-m", "one"]);
        git_with_env(repo, ["commit", "--allow-empty", "-m", "two"]);
        git_with_env(repo, ["commit", "--allow-empty", "-m", "three"]);
    }
    let two = git(git_repo.path(), ["rev-parse", "HEAD~1"]);
    let one = git(git_repo.path(), ["rev-parse", "HEAD~2"]);
    assert_eq!(git(zmin_repo.path(), ["rev-parse", "HEAD~1"]), two);
    assert_eq!(git(zmin_repo.path(), ["rev-parse", "HEAD~2"]), one);

    git_args(git_repo.path(), &["replace", "--graft", &two]);
    run_zmin_args(zmin_repo.path(), &["replace", "--graft", &two]);
    assert_eq!(
        git_args(git_repo.path(), &["replace", "--format=long"]),
        run_zmin_args(zmin_repo.path(), &["replace", "--format=long"])
    );
    let git_replacement = fs::read_to_string(git_repo.path().join(".git/refs/replace").join(&two))
        .expect("git replacement");
    let zmin_replacement =
        fs::read_to_string(zmin_repo.path().join(".git/refs/replace").join(&two))
            .expect("zmin replacement");
    assert_eq!(git_replacement, zmin_replacement);
    assert_eq!(
        git_args(git_repo.path(), &["cat-file", "-p", git_replacement.trim()]),
        git_args(
            zmin_repo.path(),
            &["cat-file", "-p", zmin_replacement.trim()],
        )
    );

    assert_eq!(
        run_zmin_status_args(zmin_repo.path(), &["replace", "--graft", &two, &one]),
        git_status_args(git_repo.path(), &["replace", "--graft", &two, &one])
    );
}

#[test]
fn replace_edit_matches_stock_git_for_commit_message_edit() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "a.txt", "one\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "one"]);
    }
    let git_oid = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let zmin_oid = git(zmin_repo.path(), ["rev-parse", "HEAD"]);

    let git_editor = git_repo.path().join("replace-edit.sh");
    let zmin_editor = zmin_repo.path().join("replace-edit.sh");
    fs::write(&git_editor, "#!/bin/sh\nperl -0pi -e 's/one/two/' \"$1\"\n")
        .expect("write git editor");
    fs::write(
        &zmin_editor,
        "#!/bin/sh\nperl -0pi -e 's/one/two/' \"$1\"\n",
    )
    .expect("write zmin editor");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_editor, fs::Permissions::from_mode(0o755))
            .expect("chmod git editor");
        fs::set_permissions(&zmin_editor, fs::Permissions::from_mode(0o755))
            .expect("chmod zmin editor");
    }

    assert_eq!(
        command_output_with_env(
            "git",
            git_repo.path(),
            &["replace", "--edit", &git_oid],
            &[("GIT_EDITOR", git_editor.to_str().expect("git editor path"))],
            "git",
        ),
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["replace", "--edit", &zmin_oid],
            &[(
                "GIT_EDITOR",
                zmin_editor.to_str().expect("zmin editor path")
            )],
            "zmin",
        )
    );

    let git_replacement =
        fs::read_to_string(git_repo.path().join(".git/refs/replace").join(&git_oid))
            .expect("git replacement");
    let zmin_replacement =
        fs::read_to_string(zmin_repo.path().join(".git/refs/replace").join(&zmin_oid))
            .expect("zmin replacement");
    assert_eq!(
        git_args(git_repo.path(), &["cat-file", "-p", git_replacement.trim()]),
        git_args(
            zmin_repo.path(),
            &["cat-file", "-p", zmin_replacement.trim()],
        )
    );
}

#[test]
fn replace_convert_graft_file_matches_stock_git_for_reparenting_entries() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        git(repo, ["checkout", "-b", "main"]);
        git_with_env(repo, ["commit", "--allow-empty", "-m", "one"]);
        let one = git(repo, ["rev-parse", "HEAD"]);
        git_with_env(repo, ["commit", "--allow-empty", "-m", "two"]);
        git(repo, ["checkout", "-b", "side", &one]);
        git_with_env(repo, ["commit", "--allow-empty", "-m", "side"]);
        git(repo, ["checkout", "main"]);
    }

    let git_two = git(git_repo.path(), ["rev-parse", "main"]);
    let git_side = git(git_repo.path(), ["rev-parse", "side"]);
    let zmin_two = git(zmin_repo.path(), ["rev-parse", "main"]);
    let zmin_side = git(zmin_repo.path(), ["rev-parse", "side"]);
    fs::create_dir_all(git_repo.path().join(".git/info")).expect("git info dir");
    fs::create_dir_all(zmin_repo.path().join(".git/info")).expect("zmin info dir");
    fs::write(
        git_repo.path().join(".git/info/grafts"),
        format!("{git_two} {git_side}\n"),
    )
    .expect("write git grafts");
    fs::write(
        zmin_repo.path().join(".git/info/grafts"),
        format!("{zmin_two} {zmin_side}\n"),
    )
    .expect("write zmin grafts");

    assert_eq!(
        command_output_with_env(
            "git",
            git_repo.path(),
            &["replace", "--convert-graft-file"],
            &[],
            "git"
        ),
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["replace", "--convert-graft-file"],
            &[],
            "zmin",
        )
    );

    assert!(!git_repo.path().join(".git/info/grafts").exists());
    assert!(!zmin_repo.path().join(".git/info/grafts").exists());
    let git_replacement =
        fs::read_to_string(git_repo.path().join(".git/refs/replace").join(&git_two))
            .expect("git replacement");
    let zmin_replacement =
        fs::read_to_string(zmin_repo.path().join(".git/refs/replace").join(&zmin_two))
            .expect("zmin replacement");
    assert_eq!(
        git_args(git_repo.path(), &["cat-file", "-p", git_replacement.trim()]),
        git_args(
            zmin_repo.path(),
            &["cat-file", "-p", zmin_replacement.trim()],
        )
    );
}
