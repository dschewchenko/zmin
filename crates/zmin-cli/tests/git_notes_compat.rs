mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    command_any_output, command_failure_output_with_env, command_output, command_output_with_env,
    command_stdout_bytes, configure_identity, git, git_failure_output, git_init, git_status,
    git_with_env, git_with_stdin, git_with_stdin_args, run_zmin, run_zmin_failure_output,
    run_zmin_status, run_zmin_with_env, run_zmin_with_stdin_args, write_file, zmin_bin,
};

fn notes_base_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    repo
}

#[test]
fn notes_add_list_show_remove_match_stock_git() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();
    let git_head = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let zmin_head = git(zmin_repo.path(), ["rev-parse", "HEAD"]);
    assert_eq!(zmin_head, git_head);

    git_with_env(git_repo.path(), ["notes", "add", "-m", "note text", "HEAD"]);
    run_zmin_with_env(
        zmin_repo.path(),
        ["notes", "add", "-m", "note text", "HEAD"],
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "get-ref"]),
        git(git_repo.path(), ["notes", "get-ref"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "list"]),
        git(git_repo.path(), ["notes", "list"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "show", "HEAD"]),
        git(git_repo.path(), ["notes", "show", "HEAD"])
    );

    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "remove", "HEAD"]),
        git(git_repo.path(), ["notes", "remove", "HEAD"])
    );
    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["notes", "show", "HEAD"]),
        git_status(git_repo.path(), ["notes", "show", "HEAD"])
    );

    write_file(git_repo.path(), "note.txt", "from file\nsecond\n");
    write_file(zmin_repo.path(), "note.txt", "from file\nsecond\n");
    git_with_env(git_repo.path(), ["notes", "add", "-F", "note.txt", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-F", "note.txt", "HEAD"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "show", "HEAD"]),
        git(git_repo.path(), ["notes", "show", "HEAD"])
    );

    git_with_env(
        git_repo.path(),
        ["notes", "append", "-m", "appended", "HEAD"],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["notes", "append", "-m", "appended", "HEAD"],
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "show", "HEAD"]),
        git(git_repo.path(), ["notes", "show", "HEAD"])
    );

    git_with_env(git_repo.path(), ["notes", "remove", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "remove", "HEAD"]);
    write_file(git_repo.path(), "append-note.txt", "fresh\nfile\n");
    write_file(zmin_repo.path(), "append-note.txt", "fresh\nfile\n");
    git_with_env(
        git_repo.path(),
        ["notes", "append", "-F", "append-note.txt", "HEAD"],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["notes", "append", "-F", "append-note.txt", "HEAD"],
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "show", "HEAD"]),
        git(git_repo.path(), ["notes", "show", "HEAD"])
    );

    write_file(git_repo.path(), "second.txt", "second\n");
    write_file(zmin_repo.path(), "second.txt", "second\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "second"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "second"]);
    let git_previous = git(git_repo.path(), ["rev-parse", "HEAD~1"]);
    let zmin_previous = git(zmin_repo.path(), ["rev-parse", "HEAD~1"]);
    let git_current = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let zmin_current = git(zmin_repo.path(), ["rev-parse", "HEAD"]);
    assert_ne!(zmin_previous, zmin_current);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "list"]),
        git(git_repo.path(), ["notes", "list"])
    );
    assert!(!run_zmin(zmin_repo.path(), ["notes", "list"]).contains(&zmin_current));
    git_with_env(
        git_repo.path(),
        ["notes", "copy", &git_previous, &git_current],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["notes", "copy", &zmin_previous, &zmin_current],
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "show", "HEAD"]),
        git(git_repo.path(), ["notes", "show", "HEAD"])
    );
    let zmin_duplicate = run_zmin_failure_output(
        zmin_repo.path(),
        &["notes", "copy", &zmin_previous, &zmin_current],
    );
    let git_duplicate = git_failure_output(
        git_repo.path(),
        &["notes", "copy", &git_previous, &git_current],
    );
    assert_eq!(zmin_duplicate, git_duplicate);
    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "copy", "-f", &zmin_previous, &zmin_current],
            "zmin",
        )
        .0,
        command_output(
            "git",
            git_repo.path(),
            &["notes", "copy", "-f", &git_previous, &git_current],
            "git",
        )
        .0
    );
}

#[test]
fn notes_unknown_subcommand_matches_stock_git_usage() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["notes", "frobnicate"]),
        git_failure_output(git_repo.path(), &["notes", "frobnicate"])
    );
}

#[test]
fn notes_edit_matches_stock_git_for_update_and_empty_remove() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();
    git_with_env(git_repo.path(), ["notes", "add", "-m", "old", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", "old", "HEAD"]);

    let git_editor = git_repo.path().join("edit-note.sh");
    let zmin_editor = zmin_repo.path().join("edit-note.sh");
    fs::write(&git_editor, "#!/bin/sh\nprintf 'edited\\n' > \"$1\"\n").expect("write git editor");
    fs::write(&zmin_editor, "#!/bin/sh\nprintf 'edited\\n' > \"$1\"\n").expect("write zmin editor");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for editor in [&git_editor, &zmin_editor] {
            let mut permissions = fs::metadata(editor).expect("editor metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(editor, permissions).expect("chmod editor");
        }
    }
    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "edit", "HEAD"],
            &[("GIT_EDITOR", zmin_editor.to_str().expect("editor path"))],
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["notes", "edit", "HEAD"],
            &[("GIT_EDITOR", git_editor.to_str().expect("editor path"))],
            "git",
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "show", "HEAD"]),
        git(git_repo.path(), ["notes", "show", "HEAD"])
    );

    fs::write(&git_editor, "#!/bin/sh\n: > \"$1\"\n").expect("write git empty editor");
    fs::write(&zmin_editor, "#!/bin/sh\n: > \"$1\"\n").expect("write zmin empty editor");
    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "edit", "HEAD"],
            &[("GIT_EDITOR", zmin_editor.to_str().expect("editor path"))],
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["notes", "edit", "HEAD"],
            &[("GIT_EDITOR", git_editor.to_str().expect("editor path"))],
            "git",
        )
    );
    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["notes", "show", "HEAD"]),
        git_status(git_repo.path(), ["notes", "show", "HEAD"])
    );
}

#[test]
fn notes_edit_message_source_options_match_stock_git() {
    for case in [
        "short-message",
        "long-message",
        "short-file",
        "long-file",
        "compact-file",
        "short-reuse",
        "long-reuse",
        "compact-reuse",
        "short-reedit",
        "long-reedit",
        "compact-reedit",
        "compact-message",
    ] {
        let git_repo = notes_base_repo();
        let zmin_repo = notes_base_repo();
        git_with_env(git_repo.path(), ["notes", "add", "-m", "old", "HEAD"]);
        run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", "old", "HEAD"]);
        write_file(git_repo.path(), "note.txt", "from file\n");
        write_file(zmin_repo.path(), "note.txt", "from file\n");
        let git_blob = git_with_stdin(
            git_repo.path(),
            ["hash-object", "-w", "--stdin"],
            "blob note\n",
        );
        let zmin_blob = git_with_stdin(
            zmin_repo.path(),
            ["hash-object", "-w", "--stdin"],
            "blob note\n",
        );
        assert_eq!(zmin_blob, git_blob);

        let git_editor = git_repo.path().join("edit-source-note.sh");
        let zmin_editor = zmin_repo.path().join("edit-source-note.sh");
        fs::write(
            &git_editor,
            "#!/bin/sh\nprintf 'edited-source\\n' > \"$1\"\n",
        )
        .expect("write git editor");
        fs::write(
            &zmin_editor,
            "#!/bin/sh\nprintf 'edited-source\\n' > \"$1\"\n",
        )
        .expect("write zmin editor");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for editor in [&git_editor, &zmin_editor] {
                let mut permissions = fs::metadata(editor).expect("editor metadata").permissions();
                permissions.set_mode(0o755);
                fs::set_permissions(editor, permissions).expect("chmod editor");
            }
        }

        let (git_args, zmin_args) = match case {
            "short-message" => (
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "-m".to_owned(),
                    "msg".to_owned(),
                    "HEAD".to_owned(),
                ],
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "-m".to_owned(),
                    "msg".to_owned(),
                    "HEAD".to_owned(),
                ],
            ),
            "long-message" => (
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "--message=long".to_owned(),
                    "HEAD".to_owned(),
                ],
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "--message=long".to_owned(),
                    "HEAD".to_owned(),
                ],
            ),
            "short-file" => (
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "-F".to_owned(),
                    "note.txt".to_owned(),
                    "HEAD".to_owned(),
                ],
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "-F".to_owned(),
                    "note.txt".to_owned(),
                    "HEAD".to_owned(),
                ],
            ),
            "long-file" => (
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "--file=note.txt".to_owned(),
                    "HEAD".to_owned(),
                ],
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "--file=note.txt".to_owned(),
                    "HEAD".to_owned(),
                ],
            ),
            "compact-file" => (
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "-Fnote.txt".to_owned(),
                    "HEAD".to_owned(),
                ],
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "-Fnote.txt".to_owned(),
                    "HEAD".to_owned(),
                ],
            ),
            "short-reuse" => (
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "-C".to_owned(),
                    git_blob.clone(),
                    "HEAD".to_owned(),
                ],
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "-C".to_owned(),
                    zmin_blob.clone(),
                    "HEAD".to_owned(),
                ],
            ),
            "long-reuse" => (
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    format!("--reuse-message={git_blob}"),
                    "HEAD".to_owned(),
                ],
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    format!("--reuse-message={zmin_blob}"),
                    "HEAD".to_owned(),
                ],
            ),
            "compact-reuse" => (
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    format!("-C{git_blob}"),
                    "HEAD".to_owned(),
                ],
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    format!("-C{zmin_blob}"),
                    "HEAD".to_owned(),
                ],
            ),
            "short-reedit" => (
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "-c".to_owned(),
                    git_blob.clone(),
                    "HEAD".to_owned(),
                ],
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "-c".to_owned(),
                    zmin_blob.clone(),
                    "HEAD".to_owned(),
                ],
            ),
            "long-reedit" => (
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    format!("--reedit-message={git_blob}"),
                    "HEAD".to_owned(),
                ],
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    format!("--reedit-message={zmin_blob}"),
                    "HEAD".to_owned(),
                ],
            ),
            "compact-reedit" => (
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    format!("-c{git_blob}"),
                    "HEAD".to_owned(),
                ],
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    format!("-c{zmin_blob}"),
                    "HEAD".to_owned(),
                ],
            ),
            "compact-message" => (
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "-mcompact".to_owned(),
                    "HEAD".to_owned(),
                ],
                vec![
                    "notes".to_owned(),
                    "edit".to_owned(),
                    "-mcompact".to_owned(),
                    "HEAD".to_owned(),
                ],
            ),
            _ => unreachable!("unknown notes edit message source case"),
        };
        let git_args = git_args.iter().map(String::as_str).collect::<Vec<_>>();
        let zmin_args = zmin_args.iter().map(String::as_str).collect::<Vec<_>>();

        assert_eq!(
            command_output_with_env(
                zmin_bin(),
                zmin_repo.path(),
                &zmin_args,
                &[("GIT_EDITOR", zmin_editor.to_str().expect("editor path"))],
                "zmin",
            ),
            command_output_with_env(
                "git",
                git_repo.path(),
                &git_args,
                &[("GIT_EDITOR", git_editor.to_str().expect("editor path"))],
                "git",
            ),
            "notes edit output should match for {case}",
        );
        assert_eq!(
            command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
            command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"]),
            "notes edit content should match for {case}",
        );
    }
}

#[test]
fn notes_allow_empty_matches_stock_git_for_add_and_append() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();

    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "add", "--allow-empty", "HEAD"],
            &[("GIT_EDITOR", "true")],
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["notes", "add", "--allow-empty", "HEAD"],
            &[("GIT_EDITOR", "true")],
            "git",
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "show", "HEAD"]),
        git(git_repo.path(), ["notes", "show", "HEAD"])
    );

    git_with_env(git_repo.path(), ["notes", "remove", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "remove", "HEAD"]);
    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "append", "--allow-empty", "HEAD"],
            &[("GIT_EDITOR", "true")],
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["notes", "append", "--allow-empty", "HEAD"],
            &[("GIT_EDITOR", "true")],
            "git",
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "show", "HEAD"]),
        git(git_repo.path(), ["notes", "show", "HEAD"])
    );

    git_with_env(
        git_repo.path(),
        ["notes", "add", "-f", "-m", "kept", "HEAD"],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["notes", "add", "-f", "-m", "kept", "HEAD"],
    );
    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "append", "--allow-empty", "HEAD"],
            &[("GIT_EDITOR", "true")],
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["notes", "append", "--allow-empty", "HEAD"],
            &[("GIT_EDITOR", "true")],
            "git",
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "show", "HEAD"]),
        git(git_repo.path(), ["notes", "show", "HEAD"])
    );
}

#[test]
fn notes_add_allow_empty_edits_empty_message_like_stock_git() {
    for args in [
        vec!["notes", "add", "--allow-empty", "HEAD"],
        vec!["notes", "add", "--allow-empty", "--no-edit", "HEAD"],
    ] {
        let git_repo = notes_base_repo();
        let zmin_repo = notes_base_repo();
        let git_editor = git_repo.path().join("allow-empty-editor.sh");
        let zmin_editor = zmin_repo.path().join("allow-empty-editor.sh");
        fs::write(&git_editor, "#!/bin/sh\nprintf 'edited-note\\n' > \"$1\"\n")
            .expect("write git editor");
        fs::write(
            &zmin_editor,
            "#!/bin/sh\nprintf 'edited-note\\n' > \"$1\"\n",
        )
        .expect("write zmin editor");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for editor in [&git_editor, &zmin_editor] {
                let mut permissions = fs::metadata(editor).expect("editor metadata").permissions();
                permissions.set_mode(0o755);
                fs::set_permissions(editor, permissions).expect("chmod editor");
            }
        }

        assert_eq!(
            command_output_with_env(
                zmin_bin(),
                zmin_repo.path(),
                &args,
                &[("GIT_EDITOR", zmin_editor.to_str().expect("editor path"))],
                "zmin",
            ),
            command_output_with_env(
                "git",
                git_repo.path(),
                &args,
                &[("GIT_EDITOR", git_editor.to_str().expect("editor path"))],
                "git",
            ),
            "notes add output should match for {args:?}",
        );
        assert_eq!(
            command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
            command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"]),
            "notes add content should match for {args:?}",
        );
    }
}

#[test]
fn notes_reuse_message_matches_stock_git_for_add_and_append() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();
    let git_blob = git_with_stdin(
        git_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "blob note",
    );
    let zmin_blob = git_with_stdin(
        zmin_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "blob note",
    );
    assert_eq!(zmin_blob, git_blob);

    command_output(
        "git",
        git_repo.path(),
        &["notes", "add", "-m", "literal", "-C", &git_blob, "HEAD"],
        "git",
    );
    command_output(
        zmin_bin(),
        zmin_repo.path(),
        &["notes", "add", "-m", "literal", "-C", &zmin_blob, "HEAD"],
        "zmin",
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );

    git_with_env(git_repo.path(), ["notes", "remove", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "remove", "HEAD"]);
    git_with_env(git_repo.path(), ["notes", "add", "-m", "old", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", "old", "HEAD"]);
    command_output(
        "git",
        git_repo.path(),
        &["notes", "append", "--reuse-message", &git_blob, "HEAD"],
        "git",
    );
    command_output(
        zmin_bin(),
        zmin_repo.path(),
        &["notes", "append", "--reuse-message", &zmin_blob, "HEAD"],
        "zmin",
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );
}

#[test]
fn notes_reedit_message_matches_stock_git_for_add_and_append() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();
    let git_blob = git_with_stdin(
        git_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "seed note\n",
    );
    let zmin_blob = git_with_stdin(
        zmin_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "seed note\n",
    );
    assert_eq!(zmin_blob, git_blob);

    let git_editor = git_repo.path().join("reedit-note.sh");
    let zmin_editor = zmin_repo.path().join("reedit-note.sh");
    fs::write(&git_editor, "#!/bin/sh\nprintf 'edited\\n' > \"$1\"\n").expect("write git editor");
    fs::write(&zmin_editor, "#!/bin/sh\nprintf 'edited\\n' > \"$1\"\n").expect("write zmin editor");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for editor in [&git_editor, &zmin_editor] {
            let mut permissions = fs::metadata(editor).expect("editor metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(editor, permissions).expect("chmod editor");
        }
    }

    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "add", "-c", &zmin_blob, "HEAD"],
            &[("GIT_EDITOR", zmin_editor.to_str().expect("editor path"))],
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["notes", "add", "-c", &git_blob, "HEAD"],
            &[("GIT_EDITOR", git_editor.to_str().expect("editor path"))],
            "git",
        )
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );

    git_with_env(git_repo.path(), ["notes", "remove", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "remove", "HEAD"]);
    git_with_env(git_repo.path(), ["notes", "add", "-m", "old", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", "old", "HEAD"]);
    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "append", "--reedit-message", &zmin_blob, "HEAD"],
            &[("GIT_EDITOR", zmin_editor.to_str().expect("editor path"))],
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["notes", "append", "--reedit-message", &git_blob, "HEAD"],
            &[("GIT_EDITOR", git_editor.to_str().expect("editor path"))],
            "git",
        )
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );
}

#[test]
fn notes_copy_stdin_matches_stock_git_for_pair_stream() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();
    git_with_env(git_repo.path(), ["notes", "add", "-m", "note", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", "note", "HEAD"]);

    write_file(git_repo.path(), "b.txt", "two\n");
    write_file(zmin_repo.path(), "b.txt", "two\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "second"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "second"]);

    let git_from = git(git_repo.path(), ["rev-parse", "HEAD~1"]);
    let git_to = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let zmin_from = git(zmin_repo.path(), ["rev-parse", "HEAD~1"]);
    let zmin_to = git(zmin_repo.path(), ["rev-parse", "HEAD"]);
    assert_eq!(zmin_from, git_from);
    assert_eq!(zmin_to, git_to);

    assert_eq!(
        run_zmin_with_stdin_args(
            zmin_repo.path(),
            &["notes", "copy", "--stdin"],
            &format!("{zmin_from} {zmin_to}\n"),
        ),
        git_with_stdin_args(
            git_repo.path(),
            &["notes", "copy", "--stdin"],
            &format!("{git_from} {git_to}\n"),
        )
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );
}

#[test]
fn notes_copy_for_rewrite_matches_stock_git_config_gate() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();
    git_with_env(git_repo.path(), ["notes", "add", "-m", "note", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", "note", "HEAD"]);

    write_file(git_repo.path(), "b.txt", "two\n");
    write_file(zmin_repo.path(), "b.txt", "two\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "second"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "second"]);

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "notes.rewriteRef", "refs/notes/commits"]);
    }

    let git_from = git(git_repo.path(), ["rev-parse", "HEAD~1"]);
    let git_to = git(git_repo.path(), ["rev-parse", "HEAD"]);
    let zmin_from = git(zmin_repo.path(), ["rev-parse", "HEAD~1"]);
    let zmin_to = git(zmin_repo.path(), ["rev-parse", "HEAD"]);
    assert_eq!(zmin_from, git_from);
    assert_eq!(zmin_to, git_to);

    assert_eq!(
        run_zmin_with_stdin_args(
            zmin_repo.path(),
            &["notes", "copy", "--for-rewrite=rebase"],
            &format!("{zmin_from} {zmin_to}\n"),
        ),
        git_with_stdin_args(
            git_repo.path(),
            &["notes", "copy", "--for-rewrite=rebase"],
            &format!("{git_from} {git_to}\n"),
        )
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );

    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["notes", "remove", "--ignore-missing", "HEAD"]);
        git(repo, ["config", "notes.rewrite.rebase", "false"]);
    }
    assert_eq!(
        run_zmin_with_stdin_args(
            zmin_repo.path(),
            &["notes", "copy", "--for-rewrite", "rebase"],
            &format!("{zmin_from} {zmin_to}\n"),
        ),
        git_with_stdin_args(
            git_repo.path(),
            &["notes", "copy", "--for-rewrite", "rebase"],
            &format!("{git_from} {git_to}\n"),
        )
    );
    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["notes", "show", "HEAD"]).0,
        git_failure_output(git_repo.path(), &["notes", "show", "HEAD"]).0
    );
}

#[test]
fn notes_list_show_extra_arguments_match_stock_git_failures() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();
    git_with_env(git_repo.path(), ["notes", "add", "-m", "note", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", "note", "HEAD"]);

    for args in [
        ["notes", "list", "HEAD", "HEAD"].as_slice(),
        ["notes", "show", "HEAD", "HEAD"].as_slice(),
        ["notes", "get-ref", "HEAD"].as_slice(),
    ] {
        let zmin = run_zmin_failure_output(zmin_repo.path(), args);
        let git = git_failure_output(git_repo.path(), args);
        assert_eq!(zmin.0, git.0, "exit status should match for {args:?}");
        assert_eq!(zmin.1, git.1, "stdout should match for {args:?}");
    }
}

#[test]
fn notes_edit_flag_matches_stock_git_for_add_and_append() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();

    let git_editor = git_repo.path().join("edit-flag-note.sh");
    let zmin_editor = zmin_repo.path().join("edit-flag-note.sh");
    fs::write(&git_editor, "#!/bin/sh\nprintf 'edited\\n' > \"$1\"\n").expect("write git editor");
    fs::write(&zmin_editor, "#!/bin/sh\nprintf 'edited\\n' > \"$1\"\n").expect("write zmin editor");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for editor in [&git_editor, &zmin_editor] {
            let mut permissions = fs::metadata(editor).expect("editor metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(editor, permissions).expect("chmod editor");
        }
    }

    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "add", "-e", "HEAD"],
            &[("GIT_EDITOR", zmin_editor.to_str().expect("editor path"))],
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["notes", "add", "-e", "HEAD"],
            &[("GIT_EDITOR", git_editor.to_str().expect("editor path"))],
            "git",
        )
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );

    git_with_env(git_repo.path(), ["notes", "remove", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "remove", "HEAD"]);
    git_with_env(git_repo.path(), ["notes", "add", "-m", "old", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", "old", "HEAD"]);
    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "append", "-m", "seed", "-e", "HEAD"],
            &[("GIT_EDITOR", zmin_editor.to_str().expect("editor path"))],
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["notes", "append", "-m", "seed", "-e", "HEAD"],
            &[("GIT_EDITOR", git_editor.to_str().expect("editor path"))],
            "git",
        )
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );
}

#[test]
fn notes_separator_options_match_stock_git_for_message_blocks() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();

    for args in [
        ["notes", "add", "-m", "one", "-m", "two", "HEAD"].as_slice(),
        [
            "notes",
            "add",
            "--no-separator",
            "-m",
            "one",
            "-m",
            "two",
            "HEAD",
        ]
        .as_slice(),
        [
            "notes",
            "add",
            "--separator=|",
            "-m",
            "one",
            "-m",
            "two",
            "HEAD",
        ]
        .as_slice(),
    ] {
        command_output("git", git_repo.path(), args, "git");
        command_output(zmin_bin(), zmin_repo.path(), args, "zmin");
        assert_eq!(
            command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
            command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"]),
            "notes add output should match for {args:?}",
        );
        git_with_env(git_repo.path(), ["notes", "remove", "HEAD"]);
        run_zmin_with_env(zmin_repo.path(), ["notes", "remove", "HEAD"]);
    }

    git_with_env(git_repo.path(), ["notes", "add", "-m", "base", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", "base", "HEAD"]);
    command_output(
        "git",
        git_repo.path(),
        &[
            "notes",
            "append",
            "--separator=|",
            "-m",
            "one",
            "-m",
            "two",
            "HEAD",
        ],
        "git",
    );
    command_output(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "notes",
            "append",
            "--separator=|",
            "-m",
            "one",
            "-m",
            "two",
            "HEAD",
        ],
        "zmin",
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );
}

#[test]
fn notes_stripspace_options_match_stock_git_for_text_and_reused_messages() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();
    let git_blob = git_with_stdin(
        git_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "blob  \n\n\ntail  ",
    );
    let zmin_blob = git_with_stdin(
        zmin_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "blob  \n\n\ntail  ",
    );
    assert_eq!(zmin_blob, git_blob);

    for (git_args, zmin_args) in [
        (
            vec![
                "notes", "add", "-m", "  one  ", "-m", "", "-m", " two ", "HEAD",
            ],
            vec![
                "notes", "add", "-m", "  one  ", "-m", "", "-m", " two ", "HEAD",
            ],
        ),
        (
            vec![
                "notes",
                "add",
                "--no-stripspace",
                "-m",
                "  one  ",
                "-m",
                "",
                "-m",
                " two ",
                "HEAD",
            ],
            vec![
                "notes",
                "add",
                "--no-stripspace",
                "-m",
                "  one  ",
                "-m",
                "",
                "-m",
                " two ",
                "HEAD",
            ],
        ),
        (
            vec!["notes", "add", "-C", &git_blob, "HEAD"],
            vec!["notes", "add", "-C", &zmin_blob, "HEAD"],
        ),
        (
            vec!["notes", "add", "--stripspace", "-C", &git_blob, "HEAD"],
            vec!["notes", "add", "--stripspace", "-C", &zmin_blob, "HEAD"],
        ),
    ] {
        command_output("git", git_repo.path(), &git_args, "git");
        command_output(zmin_bin(), zmin_repo.path(), &zmin_args, "zmin");
        assert_eq!(
            command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
            command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"]),
            "notes stripspace output should match for {git_args:?}",
        );
        git_with_env(git_repo.path(), ["notes", "remove", "HEAD"]);
        run_zmin_with_env(zmin_repo.path(), ["notes", "remove", "HEAD"]);
    }
}

#[test]
fn notes_file_dash_reads_message_from_stdin_like_stock_git() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();
    let message = "from stdin  \n\nsecond\n";

    assert_eq!(
        run_zmin_with_stdin_args(
            zmin_repo.path(),
            &["notes", "add", "-F", "-", "HEAD"],
            message
        ),
        git_with_stdin_args(
            git_repo.path(),
            &["notes", "add", "-F", "-", "HEAD"],
            message
        )
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );

    git_with_env(git_repo.path(), ["notes", "remove", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "remove", "HEAD"]);
    git_with_env(git_repo.path(), ["notes", "add", "-m", "base", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", "base", "HEAD"]);
    assert_eq!(
        run_zmin_with_stdin_args(
            zmin_repo.path(),
            &["notes", "append", "-F", "-", "HEAD"],
            "appended\n"
        ),
        git_with_stdin_args(
            git_repo.path(),
            &["notes", "append", "-F", "-", "HEAD"],
            "appended\n"
        )
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );
}

#[test]
fn notes_long_message_and_file_options_match_stock_git() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "add", "--message=long form", "HEAD"],
            "zmin",
        ),
        command_output(
            "git",
            git_repo.path(),
            &["notes", "add", "--message=long form", "HEAD"],
            "git",
        )
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );

    git_with_env(git_repo.path(), ["notes", "remove", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "remove", "HEAD"]);
    write_file(git_repo.path(), "long-note.txt", "from long file\n");
    write_file(zmin_repo.path(), "long-note.txt", "from long file\n");
    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "append", "--file=long-note.txt", "HEAD"],
            "zmin",
        ),
        command_output(
            "git",
            git_repo.path(),
            &["notes", "append", "--file=long-note.txt", "HEAD"],
            "git",
        )
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );
}

#[test]
fn notes_no_option_toggles_match_stock_git_for_add_and_append() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "notes",
                "add",
                "--no-edit",
                "--no-force",
                "--message",
                "toggle",
                "HEAD",
            ],
            "zmin",
        ),
        command_output(
            "git",
            git_repo.path(),
            &[
                "notes",
                "add",
                "--no-edit",
                "--no-force",
                "--message",
                "toggle",
                "HEAD",
            ],
            "git",
        )
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "notes",
                "append",
                "--allow-empty",
                "--no-allow-empty",
                "--message",
                "next",
                "HEAD",
            ],
            "zmin",
        ),
        command_output(
            "git",
            git_repo.path(),
            &[
                "notes",
                "append",
                "--allow-empty",
                "--no-allow-empty",
                "--message",
                "next",
                "HEAD",
            ],
            "git",
        )
    );
    assert_eq!(
        command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["notes", "show", "HEAD"]),
        command_stdout_bytes("git", git_repo.path(), &["notes", "show", "HEAD"])
    );
}

#[test]
fn notes_edit_no_allow_empty_toggle_matches_stock_git() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();

    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "edit", "--allow-empty", "--no-allow-empty", "HEAD"],
            &[("GIT_EDITOR", "true")],
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["notes", "edit", "--allow-empty", "--no-allow-empty", "HEAD"],
            &[("GIT_EDITOR", "true")],
            "git",
        )
    );
    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["notes", "show", "HEAD"]),
        git_status(git_repo.path(), ["notes", "show", "HEAD"])
    );
}

#[test]
fn notes_other_no_option_toggles_match_stock_git() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();
    write_file(git_repo.path(), "second.txt", "second\n");
    write_file(zmin_repo.path(), "second.txt", "second\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "second"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "second"]);
    let first = git(git_repo.path(), ["rev-parse", "HEAD~1"]);
    let second = git(git_repo.path(), ["rev-parse", "HEAD"]);

    git_with_env(git_repo.path(), ["notes", "add", "-m", "first", &first]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", "first", &first]);
    git_with_env(git_repo.path(), ["notes", "add", "-m", "second", &second]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", "second", &second]);
    assert_eq!(
        run_zmin_failure_output(
            zmin_repo.path(),
            &["notes", "copy", "--force", "--no-force", &first, &second]
        ),
        git_failure_output(
            git_repo.path(),
            &["notes", "copy", "--force", "--no-force", &first, &second]
        )
    );
    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &[
                "notes",
                "prune",
                "--dry-run",
                "--no-dry-run",
                "--verbose",
                "--no-verbose"
            ],
            "zmin",
        ),
        command_output(
            "git",
            git_repo.path(),
            &[
                "notes",
                "prune",
                "--dry-run",
                "--no-dry-run",
                "--verbose",
                "--no-verbose"
            ],
            "git",
        )
    );

    let git_merge_repo = notes_base_repo();
    let zmin_merge_repo = notes_base_repo();
    let object = git(git_merge_repo.path(), ["rev-parse", "HEAD"]);
    git_with_env(
        git_merge_repo.path(),
        ["notes", "--ref=left", "add", "-m", "left", &object],
    );
    run_zmin_with_env(
        zmin_merge_repo.path(),
        ["notes", "--ref=left", "add", "-m", "left", &object],
    );
    git_with_env(
        git_merge_repo.path(),
        ["notes", "--ref=right", "add", "-m", "right", &object],
    );
    run_zmin_with_env(
        zmin_merge_repo.path(),
        ["notes", "--ref=right", "add", "-m", "right", &object],
    );
    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_merge_repo.path(),
            &[
                "notes",
                "--ref=left",
                "merge",
                "--quiet",
                "--no-quiet",
                "-s",
                "ours",
                "right"
            ],
            "zmin",
        ),
        command_output(
            "git",
            git_merge_repo.path(),
            &[
                "notes",
                "--ref=left",
                "merge",
                "--quiet",
                "--no-quiet",
                "-s",
                "ours",
                "right"
            ],
            "git",
        )
    );
}

#[test]
fn notes_remove_ignore_missing_and_stdin_match_stock_git() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["notes", "remove", "HEAD"]),
        git_failure_output(git_repo.path(), &["notes", "remove", "HEAD"])
    );
    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "remove", "--ignore-missing", "HEAD"],
            "zmin",
        ),
        command_output(
            "git",
            git_repo.path(),
            &["notes", "remove", "--ignore-missing", "HEAD"],
            "git",
        )
    );

    git_with_env(git_repo.path(), ["notes", "add", "-m", "note", "HEAD"]);
    run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", "note", "HEAD"]);
    assert_eq!(
        git_with_stdin(git_repo.path(), ["notes", "remove", "--stdin"], "HEAD\n"),
        run_zmin_with_stdin_args(zmin_repo.path(), &["notes", "remove", "--stdin"], "HEAD\n",)
    );
    assert_eq!(
        run_zmin_status(zmin_repo.path(), ["notes", "show", "HEAD"]),
        git_status(git_repo.path(), ["notes", "show", "HEAD"])
    );
}

#[test]
fn notes_ref_selection_matches_stock_git_env_and_explicit_refs() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();

    for (envs, args) in [
        (vec![], vec!["notes", "get-ref"]),
        (
            vec![("GIT_NOTES_REF", "refs/notes/review")],
            vec!["notes", "get-ref"],
        ),
        (vec![("GIT_NOTES_REF", "review")], vec!["notes", "get-ref"]),
        (
            vec![("GIT_NOTES_REF", "refs/notes/env")],
            vec!["notes", "--ref=arg", "get-ref"],
        ),
        (
            vec![("GIT_NOTES_REF", "refs/notes/env")],
            vec!["notes", "--ref=arg", "--no-ref", "get-ref"],
        ),
        (
            vec![("GIT_NOTES_REF", "refs/notes/env")],
            vec!["notes", "--no-ref", "--ref=arg", "get-ref"],
        ),
        (vec![], vec!["notes", "--ref=refs/heads/main", "get-ref"]),
        (vec![], vec!["notes", "--ref=", "get-ref"]),
    ] {
        assert_eq!(
            command_output_with_env(zmin_bin(), zmin_repo.path(), &args, &envs, "zmin"),
            command_output_with_env("git", git_repo.path(), &args, &envs, "git"),
            "notes ref selection should match stock Git for {args:?} with {envs:?}",
        );
    }

    git(
        git_repo.path(),
        ["config", "core.notesRef", "refs/notes/core"],
    );
    git(
        zmin_repo.path(),
        ["config", "core.notesRef", "refs/notes/core"],
    );
    for (envs, args) in [
        (vec![], vec!["notes", "get-ref"]),
        (
            vec![("GIT_NOTES_REF", "refs/notes/env")],
            vec!["notes", "get-ref"],
        ),
        (
            vec![("GIT_NOTES_REF", "refs/notes/env")],
            vec!["notes", "--ref=arg", "get-ref"],
        ),
        (
            vec![("GIT_NOTES_REF", "refs/notes/env")],
            vec!["notes", "--ref=arg", "--no-ref", "get-ref"],
        ),
    ] {
        assert_eq!(
            command_output_with_env(zmin_bin(), zmin_repo.path(), &args, &envs, "zmin"),
            command_output_with_env("git", git_repo.path(), &args, &envs, "git"),
            "core.notesRef precedence should match stock Git for {args:?} with {envs:?}",
        );
    }

    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "add", "-m", "core note", "HEAD"],
            &[],
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["notes", "add", "-m", "core note", "HEAD"],
            &[],
            "git",
        ),
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "--ref=core", "show", "HEAD"]),
        git(git_repo.path(), ["notes", "--ref=core", "show", "HEAD"])
    );

    git(git_repo.path(), ["config", "core.notesRef", "core"]);
    git(zmin_repo.path(), ["config", "core.notesRef", "core"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "get-ref"]),
        git(git_repo.path(), ["notes", "get-ref"])
    );
    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["notes", "add", "-m", "bad", "HEAD"]),
        git_failure_output(git_repo.path(), &["notes", "add", "-m", "bad", "HEAD"])
    );
    git(git_repo.path(), ["config", "--unset", "core.notesRef"]);
    git(zmin_repo.path(), ["config", "--unset", "core.notesRef"]);

    let envs = [("GIT_NOTES_REF", "refs/notes/review")];
    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "add", "-m", "review note", "HEAD"],
            &envs,
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["notes", "add", "-m", "review note", "HEAD"],
            &envs,
            "git",
        ),
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "list"]),
        git(git_repo.path(), ["notes", "list"])
    );
    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "show", "HEAD"],
            &envs,
            "zmin",
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["notes", "show", "HEAD"],
            &envs,
            "git"
        ),
    );

    for args in [
        vec!["notes", "add", "-m", "bad", "HEAD"],
        vec!["notes", "append", "-m", "bad", "HEAD"],
        vec!["notes", "copy", "HEAD", "HEAD"],
        vec!["notes", "remove", "HEAD"],
        vec!["notes", "prune"],
    ] {
        let envs = [("GIT_NOTES_REF", "refs/heads/main")];
        assert_eq!(
            command_failure_output_with_env(zmin_bin(), zmin_repo.path(), &args, &envs, "zmin"),
            command_failure_output_with_env("git", git_repo.path(), &args, &envs, "git"),
            "notes mutation failure should match stock Git for {args:?}",
        );
    }
}

#[test]
fn notes_tree_fanout_matches_stock_git_after_growth() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();

    for index in 1..=64 {
        let file = format!("file-{index}.txt");
        let content = format!("content {index}\n");
        let message = format!("commit {index}");
        let note = format!("note {index}");
        write_file(git_repo.path(), &file, &content);
        write_file(zmin_repo.path(), &file, &content);
        git(git_repo.path(), ["add", "-A"]);
        run_zmin(zmin_repo.path(), ["add", "-A"]);
        git_with_env(git_repo.path(), ["commit", "-m", &message]);
        run_zmin_with_env(zmin_repo.path(), ["commit", "-m", &message]);
        git_with_env(git_repo.path(), ["notes", "add", "-m", &note, "HEAD"]);
        run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", &note, "HEAD"]);
    }

    assert_eq!(
        git(zmin_repo.path(), ["ls-tree", "refs/notes/commits"]),
        git(git_repo.path(), ["ls-tree", "refs/notes/commits"])
    );
    assert!(!git(zmin_repo.path(), ["ls-tree", "refs/notes/commits"]).contains(" tree "));

    for index in 65..=148 {
        let file = format!("file-{index}.txt");
        let content = format!("content {index}\n");
        let message = format!("commit {index}");
        let note = format!("note {index}");
        write_file(git_repo.path(), &file, &content);
        write_file(zmin_repo.path(), &file, &content);
        git(git_repo.path(), ["add", "-A"]);
        run_zmin(zmin_repo.path(), ["add", "-A"]);
        git_with_env(git_repo.path(), ["commit", "-m", &message]);
        run_zmin_with_env(zmin_repo.path(), ["commit", "-m", &message]);
        git_with_env(git_repo.path(), ["notes", "add", "-m", &note, "HEAD"]);
        run_zmin_with_env(zmin_repo.path(), ["notes", "add", "-m", &note, "HEAD"]);
    }

    assert_eq!(
        git(zmin_repo.path(), ["ls-tree", "refs/notes/commits"]),
        git(git_repo.path(), ["ls-tree", "refs/notes/commits"])
    );
    assert!(git(zmin_repo.path(), ["ls-tree", "refs/notes/commits"]).contains(" tree "));
    assert_eq!(
        git(
            zmin_repo.path(),
            ["ls-tree", "-r", "--name-only", "refs/notes/commits"]
        ),
        git(
            git_repo.path(),
            ["ls-tree", "-r", "--name-only", "refs/notes/commits"]
        )
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "list"]),
        git(git_repo.path(), ["notes", "list"])
    );
}

#[test]
fn notes_merge_matches_stock_git_for_clean_and_strategy_merges() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();
    write_file(git_repo.path(), "second.txt", "second\n");
    write_file(zmin_repo.path(), "second.txt", "second\n");
    git(git_repo.path(), ["add", "-A"]);
    run_zmin(zmin_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "second"]);
    run_zmin_with_env(zmin_repo.path(), ["commit", "-m", "second"]);
    let first = git(git_repo.path(), ["rev-parse", "HEAD~1"]);
    let second = git(git_repo.path(), ["rev-parse", "HEAD"]);

    git_with_env(
        git_repo.path(),
        ["notes", "--ref=left", "add", "-m", "left", &first],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["notes", "--ref=left", "add", "-m", "left", &first],
    );
    git_with_env(
        git_repo.path(),
        ["notes", "--ref=right", "add", "-m", "right", &second],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["notes", "--ref=right", "add", "-m", "right", &second],
    );
    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "--ref=left", "merge", "right"],
            "zmin",
        ),
        command_output(
            "git",
            git_repo.path(),
            &["notes", "--ref=left", "merge", "right"],
            "git",
        ),
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "--ref=left", "list"]),
        git(git_repo.path(), ["notes", "--ref=left", "list"])
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["log", "-1", "--format=%B", "refs/notes/left"]
        ),
        git(
            git_repo.path(),
            ["log", "-1", "--format=%B", "refs/notes/left"]
        )
    );

    for strategy in ["ours", "theirs", "union", "cat_sort_uniq"] {
        let git_repo = notes_base_repo();
        let zmin_repo = notes_base_repo();
        let object = git(git_repo.path(), ["rev-parse", "HEAD"]);
        git_with_env(
            git_repo.path(),
            ["notes", "--ref=left", "add", "-m", "left", &object],
        );
        run_zmin_with_env(
            zmin_repo.path(),
            ["notes", "--ref=left", "add", "-m", "left", &object],
        );
        git_with_env(
            git_repo.path(),
            ["notes", "--ref=right", "add", "-m", "right", &object],
        );
        run_zmin_with_env(
            zmin_repo.path(),
            ["notes", "--ref=right", "add", "-m", "right", &object],
        );

        assert_eq!(
            command_output(
                zmin_bin(),
                zmin_repo.path(),
                &["notes", "--ref=left", "merge", "-s", strategy, "right"],
                "zmin",
            ),
            command_output(
                "git",
                git_repo.path(),
                &["notes", "--ref=left", "merge", "-s", strategy, "right"],
                "git",
            ),
            "notes merge output should match for strategy {strategy}",
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["notes", "--ref=left", "show", &object]),
            git(git_repo.path(), ["notes", "--ref=left", "show", &object]),
            "merged note should match for strategy {strategy}",
        );
        assert_eq!(
            git(
                zmin_repo.path(),
                ["log", "-1", "--format=%B", "refs/notes/left"]
            ),
            git(
                git_repo.path(),
                ["log", "-1", "--format=%B", "refs/notes/left"]
            ),
            "merge commit message should match for strategy {strategy}",
        );
    }

    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();
    let object = git(git_repo.path(), ["rev-parse", "HEAD"]);
    git_with_env(
        git_repo.path(),
        ["notes", "--ref=left", "add", "-m", "left", &object],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["notes", "--ref=left", "add", "-m", "left", &object],
    );
    git_with_env(
        git_repo.path(),
        ["notes", "--ref=right", "add", "-m", "right", &object],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["notes", "--ref=right", "add", "-m", "right", &object],
    );
    let git_left_before = git(git_repo.path(), ["rev-parse", "refs/notes/left"]);
    let zmin_left_before = git(zmin_repo.path(), ["rev-parse", "refs/notes/left"]);
    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["notes", "--ref=left", "merge", "right"]),
        git_failure_output(git_repo.path(), &["notes", "--ref=left", "merge", "right"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "--ref=left", "show", &object]),
        git(git_repo.path(), ["notes", "--ref=left", "show", &object])
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git").join("NOTES_MERGE_REF"))
            .expect("read zmin state"),
        fs::read_to_string(git_repo.path().join(".git").join("NOTES_MERGE_REF"))
            .expect("read git state"),
    );
    let zmin_partial =
        fs::read_to_string(zmin_repo.path().join(".git").join("NOTES_MERGE_PARTIAL"))
            .expect("read zmin partial");
    let git_partial = fs::read_to_string(git_repo.path().join(".git").join("NOTES_MERGE_PARTIAL"))
        .expect("read git partial");
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-t", zmin_partial.trim()]),
        git(git_repo.path(), ["cat-file", "-t", git_partial.trim()])
    );
    assert_eq!(
        fs::read_to_string(
            zmin_repo
                .path()
                .join(".git")
                .join("NOTES_MERGE_WORKTREE")
                .join(&object),
        )
        .expect("read zmin conflict file"),
        fs::read_to_string(
            git_repo
                .path()
                .join(".git")
                .join("NOTES_MERGE_WORKTREE")
                .join(&object),
        )
        .expect("read git conflict file"),
    );
    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "--ref=left", "merge", "--abort"],
            "zmin",
        ),
        command_output(
            "git",
            git_repo.path(),
            &["notes", "--ref=left", "merge", "--abort"],
            "git",
        ),
    );
    assert_eq!(
        git(zmin_repo.path(), ["rev-parse", "refs/notes/left"]),
        zmin_left_before
    );
    assert_eq!(
        git(git_repo.path(), ["rev-parse", "refs/notes/left"]),
        git_left_before
    );

    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();
    let object = git(git_repo.path(), ["rev-parse", "HEAD"]);
    git_with_env(
        git_repo.path(),
        ["notes", "--ref=left", "add", "-m", "left", &object],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["notes", "--ref=left", "add", "-m", "left", &object],
    );
    git_with_env(
        git_repo.path(),
        ["notes", "--ref=right", "add", "-m", "right", &object],
    );
    run_zmin_with_env(
        zmin_repo.path(),
        ["notes", "--ref=right", "add", "-m", "right", &object],
    );
    let _ = git_failure_output(git_repo.path(), &["notes", "--ref=left", "merge", "right"]);
    let _ = run_zmin_failure_output(zmin_repo.path(), &["notes", "--ref=left", "merge", "right"]);
    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "--ref=left", "merge", "--commit"],
            "zmin",
        ),
        command_output(
            "git",
            git_repo.path(),
            &["notes", "--ref=left", "merge", "--commit"],
            "git",
        ),
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "--ref=left", "show", &object]),
        git(git_repo.path(), ["notes", "--ref=left", "show", &object])
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["log", "-1", "--format=%B", "refs/notes/left"]
        ),
        git(
            git_repo.path(),
            ["log", "-1", "--format=%B", "refs/notes/left"]
        ),
    );
}

#[test]
fn notes_merge_quiet_and_verbose_options_match_stock_git() {
    for verbosity in ["-q", "-v"] {
        let git_repo = notes_base_repo();
        let zmin_repo = notes_base_repo();
        let object = git(git_repo.path(), ["rev-parse", "HEAD"]);
        git_with_env(
            git_repo.path(),
            ["notes", "--ref=left", "add", "-m", "left", &object],
        );
        run_zmin_with_env(
            zmin_repo.path(),
            ["notes", "--ref=left", "add", "-m", "left", &object],
        );
        git_with_env(
            git_repo.path(),
            ["notes", "--ref=right", "add", "-m", "right", &object],
        );
        run_zmin_with_env(
            zmin_repo.path(),
            ["notes", "--ref=right", "add", "-m", "right", &object],
        );

        assert_eq!(
            command_output(
                zmin_bin(),
                zmin_repo.path(),
                &[
                    "notes",
                    "--ref=left",
                    "merge",
                    verbosity,
                    "-s",
                    "ours",
                    "right",
                ],
                "zmin",
            ),
            command_output(
                "git",
                git_repo.path(),
                &[
                    "notes",
                    "--ref=left",
                    "merge",
                    verbosity,
                    "-s",
                    "ours",
                    "right",
                ],
                "git",
            ),
            "notes merge output should match for {verbosity}",
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["notes", "--ref=left", "show", &object]),
            git(git_repo.path(), ["notes", "--ref=left", "show", &object])
        );
    }
}

#[test]
fn notes_merge_no_strategy_toggle_matches_stock_git_order() {
    fn setup_conflicting_refs() -> (TempDir, TempDir, String) {
        let git_repo = notes_base_repo();
        let zmin_repo = notes_base_repo();
        let object = git(git_repo.path(), ["rev-parse", "HEAD"]);
        command_output(
            "git",
            git_repo.path(),
            &["notes", "--ref=left", "add", "-m", "left", &object],
            "git",
        );
        command_output(
            "git",
            git_repo.path(),
            &["notes", "--ref=right", "add", "-m", "right", &object],
            "git",
        );
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "--ref=left", "add", "-m", "left", &object],
            "zmin",
        );
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "--ref=right", "add", "-m", "right", &object],
            "zmin",
        );
        (git_repo, zmin_repo, object)
    }

    for (name, args) in [
        (
            "manual",
            vec!["notes", "--ref=left", "merge", "--no-strategy", "right"],
        ),
        (
            "reset",
            vec![
                "notes",
                "--ref=left",
                "merge",
                "--strategy=ours",
                "--no-strategy",
                "right",
            ],
        ),
        (
            "override",
            vec![
                "notes",
                "--ref=left",
                "merge",
                "--no-strategy",
                "--strategy=ours",
                "right",
            ],
        ),
    ] {
        let (git_repo, zmin_repo, object) = setup_conflicting_refs();
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), &args, "zmin"),
            command_any_output("git", git_repo.path(), &args, "git"),
            "notes merge output should match for {name}",
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["notes", "--ref=left", "show", &object]),
            git(git_repo.path(), ["notes", "--ref=left", "show", &object]),
            "notes merge content should match for {name}",
        );
    }

    for (name, args) in [
        (
            "commit-no-strategy",
            vec!["notes", "merge", "--commit", "--no-strategy"],
        ),
        (
            "commit-reset-strategy",
            vec!["notes", "merge", "-s", "ours", "--no-strategy", "--commit"],
        ),
        (
            "abort-no-strategy",
            vec!["notes", "merge", "--abort", "--no-strategy"],
        ),
        (
            "abort-reset-strategy",
            vec!["notes", "merge", "-s", "ours", "--no-strategy", "--abort"],
        ),
    ] {
        let git_repo = notes_base_repo();
        let zmin_repo = notes_base_repo();
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), &args, "zmin"),
            command_any_output("git", git_repo.path(), &args, "git"),
            "notes merge state failure should match for {name}",
        );
    }
}

#[test]
fn notes_merge_negative_state_and_argument_failures_match_stock_git() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();

    for args in [
        ["notes", "merge", "--commit"].as_slice(),
        ["notes", "merge", "--abort"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), args),
            git_failure_output(git_repo.path(), args),
            "notes merge state failure should match for {args:?}"
        );
    }

    for args in [
        ["notes", "merge", "--commit", "-s", "ours"].as_slice(),
        ["notes", "merge", "--abort", "topic"].as_slice(),
        ["notes", "merge", "-s", "nope", "refs/notes/x"].as_slice(),
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
fn notes_prune_matches_stock_git_for_missing_objects() {
    let git_repo = notes_base_repo();
    let zmin_repo = notes_base_repo();
    let missing = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    for repo in [git_repo.path(), zmin_repo.path()] {
        let note = git_with_stdin(repo, ["hash-object", "-w", "--stdin"], "orphan note\n");
        let tree = git_with_stdin_args(
            repo,
            &["mktree"],
            &format!("100644 blob {note}\t{missing}\n"),
        );
        let commit = git_with_stdin_args(repo, &["commit-tree", &tree], "notes\n");
        git(repo, ["update-ref", "refs/notes/commits", &commit]);
    }

    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "prune", "-n"],
            "zmin",
        ),
        command_output("git", git_repo.path(), &["notes", "prune", "-n"], "git")
    );
    assert_eq!(
        command_output(
            zmin_bin(),
            zmin_repo.path(),
            &["notes", "prune", "-nv"],
            "zmin",
        ),
        command_output("git", git_repo.path(), &["notes", "prune", "-nv"], "git")
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "list"]),
        git(git_repo.path(), ["notes", "list"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "prune", "-v"]),
        git(git_repo.path(), ["notes", "prune", "-v"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["notes", "list"]),
        git(git_repo.path(), ["notes", "list"])
    );
}
