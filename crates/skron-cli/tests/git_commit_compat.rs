mod common;

use std::fs;
use std::path::Path;

use common::{
    command_failure_output_with_env, command_output_with_env, configure_identity, git, git_init,
    git_with_env, run_skron, run_skron_with_env, skron_bin,
};

const COMMIT_ENV: [(&str, &str); 6] = [
    ("GIT_AUTHOR_NAME", "Bench"),
    ("GIT_AUTHOR_EMAIL", "bench@example.test"),
    ("GIT_AUTHOR_DATE", "1700000000 +0000"),
    ("GIT_COMMITTER_NAME", "Bench"),
    ("GIT_COMMITTER_EMAIL", "bench@example.test"),
    ("GIT_COMMITTER_DATE", "1700000000 +0000"),
];

#[test]
fn commit_amend_matches_stock_git_state() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"initial\n").expect("write git a");
    fs::write(skron_repo.path().join("a.txt"), b"initial\n").expect("write skron a");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "initial"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "initial"]);

    fs::write(git_repo.path().join("a.txt"), b"amended\n").expect("amend git a");
    fs::write(skron_repo.path().join("a.txt"), b"amended\n").expect("amend skron a");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "--amend", "-m", "amended"]);
    run_skron_with_env(skron_repo.path(), ["commit", "--amend", "-m", "amended"]);

    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["rev-list", "--parents", "HEAD"]),
        git(git_repo.path(), ["rev-list", "--parents", "HEAD"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["rev-list", "--count", "HEAD"]),
        git(git_repo.path(), ["rev-list", "--count", "HEAD"])
    );

    git_with_env(git_repo.path(), ["commit", "--amend", "-m", "message only"]);
    run_skron_with_env(
        skron_repo.path(),
        ["commit", "--amend", "-m", "message only"],
    );

    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    fs::write(git_repo.path().join("b.txt"), b"no edit\n").expect("write git b");
    fs::write(skron_repo.path().join("b.txt"), b"no edit\n").expect("write skron b");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "--amend", "--no-edit"]);
    run_skron_with_env(skron_repo.path(), ["commit", "--amend", "--no-edit"]);

    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
}

#[test]
fn commit_pathspec_only_and_fixup_match_stock_git_state() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("a.txt"), b"a1\n").expect("write a");
        fs::write(repo.join("b.txt"), b"b1\n").expect("write b");
    }
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "base"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "base"]);

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("a.txt"), b"a2\n").expect("write a2");
        fs::write(repo.join("b.txt"), b"b2\n").expect("write b2");
    }
    git(git_repo.path(), ["add", "b.txt"]);
    run_skron(skron_repo.path(), ["add", "b.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "path", "--", "a.txt"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "path", "--", "a.txt"]);
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain"]),
        git(git_repo.path(), ["status", "--porcelain"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("a.txt"), b"a3\n").expect("write a3");
        fs::write(repo.join("b.txt"), b"b3\n").expect("write b3");
    }
    git(git_repo.path(), ["add", "b.txt"]);
    run_skron(skron_repo.path(), ["add", "b.txt"]);
    git_with_env(
        git_repo.path(),
        ["commit", "--only", "-m", "only path", "a.txt"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["commit", "--only", "-m", "only path", "a.txt"],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain"]),
        git(git_repo.path(), ["status", "--porcelain"])
    );

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("a.txt"), b"a4\n").expect("write a4");
    }
    git(git_repo.path(), ["add", "a.txt"]);
    run_skron(skron_repo.path(), ["add", "a.txt"]);
    git_with_env(git_repo.path(), ["commit", "--fixup", "HEAD"]);
    run_skron_with_env(skron_repo.path(), ["commit", "--fixup", "HEAD"]);
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("a.txt"), b"a5\n").expect("write a5");
    }
    git(git_repo.path(), ["add", "a.txt"]);
    run_skron(skron_repo.path(), ["add", "a.txt"]);
    git_with_env(
        git_repo.path(),
        ["commit", "--fixup", "HEAD", "-m", "fixup detail"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["commit", "--fixup", "HEAD", "-m", "fixup detail"],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    install_capture_commit_editor(git_repo.path());
    install_capture_commit_editor(skron_repo.path());
    let git_editor = git_repo.path().join(".git/editor.sh");
    let skron_editor = skron_repo.path().join(".git/editor.sh");
    let mut git_env = COMMIT_ENV.to_vec();
    git_env.push(("GIT_EDITOR", git_editor.to_str().expect("git editor path")));
    let mut skron_env = COMMIT_ENV.to_vec();
    skron_env.push((
        "GIT_EDITOR",
        skron_editor.to_str().expect("skron editor path"),
    ));

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("c.txt"), b"fixup amend\n").expect("write fixup amend");
    }
    git(git_repo.path(), ["add", "c.txt"]);
    run_skron(skron_repo.path(), ["add", "c.txt"]);
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "--fixup=amend:HEAD"],
            &skron_env,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "--fixup=amend:HEAD"],
            &git_env,
            "git"
        )
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join(".git/editor-input.txt"))
            .expect("read skron fixup amend editor input"),
        fs::read_to_string(git_repo.path().join(".git/editor-input.txt"))
            .expect("read git fixup amend editor input")
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    for repo in [git_repo.path(), skron_repo.path()] {
        let _ = fs::remove_file(repo.join(".git/editor-input.txt"));
        fs::write(repo.join("d.txt"), b"fixup reword staged\n").expect("write fixup reword");
    }
    git(git_repo.path(), ["add", "d.txt"]);
    run_skron(skron_repo.path(), ["add", "d.txt"]);
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "--fixup=reword:HEAD"],
            &skron_env,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "--fixup=reword:HEAD"],
            &git_env,
            "git"
        )
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join(".git/editor-input.txt"))
            .expect("read skron fixup reword editor input"),
        fs::read_to_string(git_repo.path().join(".git/editor-input.txt"))
            .expect("read git fixup reword editor input")
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["status", "--porcelain"]),
        git(git_repo.path(), ["status", "--porcelain"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["ls-files", "--stage"]),
        git(git_repo.path(), ["ls-files", "--stage"])
    );
}

#[test]
fn commit_messages_match_stock_git_object() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"multi\n").expect("write git a");
    fs::write(skron_repo.path().join("a.txt"), b"multi\n").expect("write skron a");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "subject", "-m", "body"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "subject", "-m", "body"]);
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    fs::write(git_repo.path().join("message.txt"), b"from file\n\nbody")
        .expect("write git message");
    fs::write(skron_repo.path().join("message.txt"), b"from file\n\nbody")
        .expect("write skron message");
    fs::write(git_repo.path().join("file-message.txt"), b"file message\n")
        .expect("write git file message");
    fs::write(
        skron_repo.path().join("file-message.txt"),
        b"file message\n",
    )
    .expect("write skron file message");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-F", "message.txt"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-F", "message.txt"]);
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    fs::write(git_repo.path().join("empty.txt"), b"empty\n").expect("write git empty");
    fs::write(skron_repo.path().join("empty.txt"), b"empty\n").expect("write skron empty");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        ["commit", "--allow-empty-message", "-m", ""],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["commit", "--allow-empty-message", "-m", ""],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    fs::write(git_repo.path().join("amend.txt"), b"amend\n").expect("write git amend");
    fs::write(skron_repo.path().join("amend.txt"), b"amend\n").expect("write skron amend");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        ["commit", "--amend", "-m", "amended", "-m", "details"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["commit", "--amend", "-m", "amended", "-m", "details"],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
}

#[test]
fn commit_reuse_message_matches_stock_git_object() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"base\n").expect("write git a");
    fs::write(skron_repo.path().join("a.txt"), b"base\n").expect("write skron a");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "base", "-m", "body"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "base", "-m", "body"]);

    fs::write(git_repo.path().join("b.txt"), b"reuse\n").expect("write git b");
    fs::write(skron_repo.path().join("b.txt"), b"reuse\n").expect("write skron b");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-C", "HEAD"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-C", "HEAD"]);
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    fs::write(git_repo.path().join("c.txt"), b"override\n").expect("write git c");
    fs::write(skron_repo.path().join("c.txt"), b"override\n").expect("write skron c");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        [
            "commit",
            "-C",
            "HEAD~1",
            "--author",
            "Alice Example <alice@example.test>",
            "--date",
            "1700001234 +0000",
        ],
    );
    run_skron_with_env(
        skron_repo.path(),
        [
            "commit",
            "-C",
            "HEAD~1",
            "--author",
            "Alice Example <alice@example.test>",
            "--date",
            "1700001234 +0000",
        ],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    fs::write(git_repo.path().join("d.txt"), b"reedit\n").expect("write git d");
    fs::write(skron_repo.path().join("d.txt"), b"reedit\n").expect("write skron d");
    install_capture_commit_editor(git_repo.path());
    install_capture_commit_editor(skron_repo.path());
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    let git_editor = git_repo.path().join(".git/editor.sh");
    let skron_editor = skron_repo.path().join(".git/editor.sh");
    let mut git_env = COMMIT_ENV.to_vec();
    git_env.push(("GIT_EDITOR", git_editor.to_str().expect("git editor path")));
    let mut skron_env = COMMIT_ENV.to_vec();
    skron_env.push((
        "GIT_EDITOR",
        skron_editor.to_str().expect("skron editor path"),
    ));
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "-c", "HEAD~2"],
            &skron_env,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "-c", "HEAD~2"],
            &git_env,
            "git"
        )
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join(".git/editor-input.txt"))
            .expect("read skron editor input"),
        fs::read_to_string(git_repo.path().join(".git/editor-input.txt"))
            .expect("read git editor input")
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    fs::write(git_repo.path().join("e.txt"), b"reedit unchanged\n").expect("write git e");
    fs::write(skron_repo.path().join("e.txt"), b"reedit unchanged\n").expect("write skron e");
    install_custom_commit_editor(
        git_repo.path(),
        "cp \"$1\" .git/editor-input.txt\n# keep reused message unchanged\n",
    );
    install_custom_commit_editor(
        skron_repo.path(),
        "cp \"$1\" .git/editor-input.txt\n# keep reused message unchanged\n",
    );
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "-c", "HEAD~3"],
            &skron_env,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "-c", "HEAD~3"],
            &git_env,
            "git"
        )
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
}

#[test]
fn commit_edit_and_no_edit_message_sources_match_stock_git() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("base.txt"), b"base\n").expect("write base");
    }
    git(git_repo.path(), ["add", "base.txt"]);
    run_skron(skron_repo.path(), ["add", "base.txt"]);
    git_with_env(
        git_repo.path(),
        ["commit", "-m", "Base subject", "-m", "Base body"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["commit", "-m", "Base subject", "-m", "Base body"],
    );

    for (idx, args) in [
        vec!["commit", "-e", "-m", "Msg subject", "-m", "Msg body"],
        vec!["commit", "-e", "-F", "msg.txt"],
        vec!["commit", "-e", "-C", "HEAD"],
        vec!["commit", "--no-edit", "-c", "HEAD"],
        vec!["commit", "--no-edit", "-m", "No edit subject"],
    ]
    .into_iter()
    .enumerate()
    {
        for repo in [git_repo.path(), skron_repo.path()] {
            fs::write(repo.join("msg.txt"), b"File subject\n\nFile body\n").expect("write msg");
            fs::write(repo.join(format!("edit-{idx}.txt")), b"edit\n").expect("write edit");
            let _ = fs::remove_file(repo.join(".git/editor-input.txt"));
            install_capture_commit_editor(repo);
        }
        git(git_repo.path(), ["add", "-A"]);
        run_skron(skron_repo.path(), ["add", "-A"]);
        let git_editor = git_repo.path().join(".git/editor.sh");
        let skron_editor = skron_repo.path().join(".git/editor.sh");
        let mut git_env = COMMIT_ENV.to_vec();
        git_env.push(("GIT_EDITOR", git_editor.to_str().expect("git editor path")));
        let mut skron_env = COMMIT_ENV.to_vec();
        skron_env.push((
            "GIT_EDITOR",
            skron_editor.to_str().expect("skron editor path"),
        ));

        assert_eq!(
            command_output_with_env(skron_bin(), skron_repo.path(), &args, &skron_env, "skron"),
            command_output_with_env("git", git_repo.path(), &args, &git_env, "git"),
            "edit args output mismatch: {args:?}"
        );
        assert_eq!(
            git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
            git(git_repo.path(), ["cat-file", "-p", "HEAD"]),
            "edit args object mismatch: {args:?}"
        );
        let skron_editor_input = skron_repo.path().join(".git/editor-input.txt");
        let git_editor_input = git_repo.path().join(".git/editor-input.txt");
        assert_eq!(
            skron_editor_input.exists(),
            git_editor_input.exists(),
            "editor invocation mismatch: {args:?}"
        );
        if skron_editor_input.exists() {
            assert_eq!(
                fs::read_to_string(skron_editor_input).expect("read skron editor input"),
                fs::read_to_string(git_editor_input).expect("read git editor input"),
                "editor input mismatch: {args:?}"
            );
        }
    }
}

#[test]
fn commit_author_matches_stock_git_object() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"author\n").expect("write git a");
    fs::write(skron_repo.path().join("a.txt"), b"author\n").expect("write skron a");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        [
            "commit",
            "--author",
            "Alice Example <alice@example.test>",
            "-m",
            "author",
        ],
    );
    run_skron_with_env(
        skron_repo.path(),
        [
            "commit",
            "--author",
            "Alice Example <alice@example.test>",
            "-m",
            "author",
        ],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
    assert_eq!(
        run_skron(
            skron_repo.path(),
            ["log", "--pretty=format:%an <%ae>", "--max-count", "1"]
        ),
        git(
            git_repo.path(),
            ["log", "--pretty=format:%an <%ae>", "--max-count", "1"]
        )
    );
}

#[test]
fn commit_date_matches_stock_git_object() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"date\n").expect("write git a");
    fs::write(skron_repo.path().join("a.txt"), b"date\n").expect("write skron a");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        ["commit", "--date", "1700001234 +0000", "-m", "date"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["commit", "--date", "1700001234 +0000", "-m", "date"],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
}

#[test]
fn commit_amend_author_date_options_match_stock_git_object() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"init\n").expect("write git a");
    fs::write(skron_repo.path().join("a.txt"), b"init\n").expect("write skron a");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "init"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "init"]);

    fs::write(git_repo.path().join("a.txt"), b"amend\n").expect("write git amend");
    fs::write(skron_repo.path().join("a.txt"), b"amend\n").expect("write skron amend");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        [
            "commit",
            "--amend",
            "--author",
            "Alice Example <alice@example.test>",
            "--date",
            "1700001234 +0000",
            "-m",
            "amended",
        ],
    );
    run_skron_with_env(
        skron_repo.path(),
        [
            "commit",
            "--amend",
            "--author",
            "Alice Example <alice@example.test>",
            "--date",
            "1700001234 +0000",
            "-m",
            "amended",
        ],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
}

#[test]
fn commit_reset_author_matches_stock_git_object() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"init\n").expect("write git a");
    fs::write(skron_repo.path().join("a.txt"), b"init\n").expect("write skron a");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        [
            "commit",
            "--author",
            "Alice Example <alice@example.test>",
            "--date",
            "1700001234 +0000",
            "-m",
            "init",
        ],
    );
    run_skron_with_env(
        skron_repo.path(),
        [
            "commit",
            "--author",
            "Alice Example <alice@example.test>",
            "--date",
            "1700001234 +0000",
            "-m",
            "init",
        ],
    );

    fs::write(git_repo.path().join("a.txt"), b"reset\n").expect("write git reset");
    fs::write(skron_repo.path().join("a.txt"), b"reset\n").expect("write skron reset");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        ["commit", "--amend", "--reset-author", "-m", "reset"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["commit", "--amend", "--reset-author", "-m", "reset"],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
}

#[test]
fn commit_summary_and_quiet_output_match_stock_git() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("a.txt"), b"one\n").expect("write a");
        fs::create_dir(repo.join("dir")).expect("create dir");
        fs::write(repo.join("dir/b.txt"), b"two\n").expect("write b");
    }
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "-m", "initial"],
            &COMMIT_ENV,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "-m", "initial"],
            &COMMIT_ENV,
            "git"
        )
    );

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("a.txt"), b"one\ntwo\n").expect("rewrite a");
    }
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "-m", "second"],
            &COMMIT_ENV,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "-m", "second"],
            &COMMIT_ENV,
            "git"
        )
    );

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::remove_file(repo.join("dir/b.txt")).expect("remove b");
    }
    git(git_repo.path(), ["rm", "dir/b.txt"]);
    run_skron(skron_repo.path(), ["rm", "dir/b.txt"]);
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "-m", "delete"],
            &COMMIT_ENV,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "-m", "delete"],
            &COMMIT_ENV,
            "git"
        )
    );

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("quiet.txt"), b"quiet\n").expect("write quiet");
    }
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "--quiet", "-m", "quiet"],
            &COMMIT_ENV,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "--quiet", "-m", "quiet"],
            &COMMIT_ENV,
            "git"
        )
    );
}

#[test]
fn commit_hooks_match_stock_git_flow() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());
    install_commit_hook_set(git_repo.path());
    install_commit_hook_set(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"hooks\n").expect("write git hooks");
    fs::write(skron_repo.path().join("a.txt"), b"hooks\n").expect("write skron hooks");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "-m", "subject"],
            &COMMIT_ENV,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "-m", "subject"],
            &COMMIT_ENV,
            "git"
        )
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("hook.log")).expect("read skron hook log"),
        fs::read_to_string(git_repo.path().join("hook.log")).expect("read git hook log")
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    fs::write(git_repo.path().join("b.txt"), b"skip\n").expect("write git skip");
    fs::write(skron_repo.path().join("b.txt"), b"skip\n").expect("write skron skip");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "--no-verify", "-m", "skip"],
            &COMMIT_ENV,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "--no-verify", "-m", "skip"],
            &COMMIT_ENV,
            "git"
        )
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("hook.log")).expect("read skron hook log"),
        fs::read_to_string(git_repo.path().join("hook.log")).expect("read git hook log")
    );
}

#[test]
fn commit_prepare_and_post_rewrite_hooks_match_stock_git_flow() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());
    install_prepare_commit_msg_hook(git_repo.path());
    install_prepare_commit_msg_hook(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"prepare\n").expect("write git prepare");
    fs::write(skron_repo.path().join("a.txt"), b"prepare\n").expect("write skron prepare");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "--no-verify", "-m", "subject"],
            &COMMIT_ENV,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "--no-verify", "-m", "subject"],
            &COMMIT_ENV,
            "git"
        )
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("hook.log")).expect("read skron hook log"),
        fs::read_to_string(git_repo.path().join("hook.log")).expect("read git hook log")
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    install_post_rewrite_hook(git_repo.path());
    install_post_rewrite_hook(skron_repo.path());
    fs::write(git_repo.path().join("b.txt"), b"amend\n").expect("write git amend");
    fs::write(skron_repo.path().join("b.txt"), b"amend\n").expect("write skron amend");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "--amend", "-m", "amended"],
            &COMMIT_ENV,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "--amend", "-m", "amended"],
            &COMMIT_ENV,
            "git"
        )
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join("rewrite.log")).expect("read skron rewrite log"),
        fs::read_to_string(git_repo.path().join("rewrite.log")).expect("read git rewrite log")
    );
}

#[test]
fn commit_hook_failures_match_stock_git_flow() {
    for hook_name in ["pre-commit", "commit-msg"] {
        let git_repo = git_init();
        let skron_repo = git_init();
        configure_identity(git_repo.path());
        configure_identity(skron_repo.path());
        install_failing_commit_hook(git_repo.path(), hook_name);
        install_failing_commit_hook(skron_repo.path(), hook_name);
        fs::write(git_repo.path().join("a.txt"), b"fail\n").expect("write git fail");
        fs::write(skron_repo.path().join("a.txt"), b"fail\n").expect("write skron fail");
        git(git_repo.path(), ["add", "-A"]);
        run_skron(skron_repo.path(), ["add", "-A"]);
        assert_eq!(
            command_failure_output_with_env(
                skron_bin(),
                skron_repo.path(),
                &["commit", "-m", "fail"],
                &COMMIT_ENV,
                "skron"
            ),
            command_failure_output_with_env(
                "git",
                git_repo.path(),
                &["commit", "-m", "fail"],
                &COMMIT_ENV,
                "git"
            ),
            "failure output mismatch for {hook_name}"
        );
        assert_eq!(
            command_failure_output_with_env(
                skron_bin(),
                skron_repo.path(),
                &["rev-parse", "--verify", "HEAD"],
                &[],
                "skron"
            )
            .0,
            command_failure_output_with_env(
                "git",
                git_repo.path(),
                &["rev-parse", "--verify", "HEAD"],
                &[],
                "git"
            )
            .0
        );
    }

    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());
    install_failing_commit_hook(git_repo.path(), "post-commit");
    install_failing_commit_hook(skron_repo.path(), "post-commit");
    fs::write(git_repo.path().join("a.txt"), b"post\n").expect("write git post");
    fs::write(skron_repo.path().join("a.txt"), b"post\n").expect("write skron post");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "-m", "post"],
            &COMMIT_ENV,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "-m", "post"],
            &COMMIT_ENV,
            "git"
        )
    );
}

#[test]
fn commit_template_editor_matches_stock_git_object() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"template\n").expect("write git template");
    fs::write(skron_repo.path().join("a.txt"), b"template\n").expect("write skron template");
    fs::write(
        git_repo.path().join("template.txt"),
        b"template subject\n\n# comment\n",
    )
    .expect("write git template message");
    fs::write(
        skron_repo.path().join("template.txt"),
        b"template subject\n\n# comment\n",
    )
    .expect("write skron template message");
    fs::write(
        git_repo.path().join("editor.sh"),
        "#!/bin/sh\nprintf 'edited subject\\n\\nedited body\\n' > \"$1\"\n",
    )
    .expect("write git editor");
    fs::write(
        skron_repo.path().join("editor.sh"),
        "#!/bin/sh\nprintf 'edited subject\\n\\nedited body\\n' > \"$1\"\n",
    )
    .expect("write skron editor");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for editor in [
            git_repo.path().join("editor.sh"),
            skron_repo.path().join("editor.sh"),
        ] {
            let mut permissions = fs::metadata(&editor)
                .expect("editor metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&editor, permissions).expect("chmod editor");
        }
    }

    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    let git_editor = git_repo.path().join("editor.sh");
    let skron_editor = skron_repo.path().join("editor.sh");
    let env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
    ];
    let git_editor_path = git_editor.to_str().expect("git editor path");
    let skron_editor_path = skron_editor.to_str().expect("skron editor path");
    let mut git_env = env.to_vec();
    git_env.push(("GIT_EDITOR", git_editor_path));
    let mut skron_env = env.to_vec();
    skron_env.push(("GIT_EDITOR", skron_editor_path));
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "--template", "template.txt"],
            &skron_env,
            "skron"
        )
        .0,
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "--template", "template.txt"],
            &git_env,
            "git"
        )
        .0
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    fs::write(git_repo.path().join("b.txt"), b"config template\n").expect("write git b");
    fs::write(skron_repo.path().join("b.txt"), b"config template\n").expect("write skron b");
    fs::write(
        git_repo.path().join("config-template.txt"),
        b"config template subject\n",
    )
    .expect("write git config template");
    fs::write(
        skron_repo.path().join("config-template.txt"),
        b"config template subject\n",
    )
    .expect("write skron config template");
    git(
        git_repo.path(),
        ["config", "commit.template", "config-template.txt"],
    );
    run_skron(
        skron_repo.path(),
        ["config", "commit.template", "config-template.txt"],
    );
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit"],
            &skron_env,
            "skron"
        )
        .0,
        command_output_with_env("git", git_repo.path(), &["commit"], &git_env, "git").0
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
}

#[test]
fn commit_editor_without_message_matches_stock_git_buffer() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());
    install_capture_commit_editor(git_repo.path());
    install_capture_commit_editor(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"normal editor\n").expect("write git a");
    fs::write(skron_repo.path().join("a.txt"), b"normal editor\n").expect("write skron a");
    git(git_repo.path(), ["add", "a.txt"]);
    run_skron(skron_repo.path(), ["add", "a.txt"]);

    let git_editor = git_repo.path().join(".git/editor.sh");
    let skron_editor = skron_repo.path().join(".git/editor.sh");
    let mut git_env = COMMIT_ENV.to_vec();
    git_env.push(("GIT_EDITOR", git_editor.to_str().expect("git editor path")));
    let mut skron_env = COMMIT_ENV.to_vec();
    skron_env.push((
        "GIT_EDITOR",
        skron_editor.to_str().expect("skron editor path"),
    ));

    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit"],
            &skron_env,
            "skron"
        ),
        command_output_with_env("git", git_repo.path(), &["commit"], &git_env, "git")
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join(".git/editor-input.txt"))
            .expect("read skron editor input"),
        fs::read_to_string(git_repo.path().join(".git/editor-input.txt"))
            .expect("read git editor input")
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
}

#[test]
fn commit_status_option_and_config_match_stock_git_buffer() {
    for (config_status, args) in [
        (None, vec!["commit", "--no-status"]),
        (Some("false"), vec!["commit"]),
        (Some("false"), vec!["commit", "--status"]),
        (None, vec!["commit", "-v", "--no-status"]),
    ] {
        let git_repo = git_init();
        let skron_repo = git_init();
        configure_identity(git_repo.path());
        configure_identity(skron_repo.path());
        install_capture_commit_editor(git_repo.path());
        install_capture_commit_editor(skron_repo.path());
        if let Some(value) = config_status {
            git(git_repo.path(), ["config", "commit.status", value]);
            run_skron(skron_repo.path(), ["config", "commit.status", value]);
        }

        fs::write(git_repo.path().join("a.txt"), b"status option\n").expect("write git a");
        fs::write(skron_repo.path().join("a.txt"), b"status option\n").expect("write skron a");
        git(git_repo.path(), ["add", "a.txt"]);
        run_skron(skron_repo.path(), ["add", "a.txt"]);

        let git_editor = git_repo.path().join(".git/editor.sh");
        let skron_editor = skron_repo.path().join(".git/editor.sh");
        let mut git_env = COMMIT_ENV.to_vec();
        git_env.push(("GIT_EDITOR", git_editor.to_str().expect("git editor path")));
        let mut skron_env = COMMIT_ENV.to_vec();
        skron_env.push((
            "GIT_EDITOR",
            skron_editor.to_str().expect("skron editor path"),
        ));

        assert_eq!(
            command_output_with_env(skron_bin(), skron_repo.path(), &args, &skron_env, "skron"),
            command_output_with_env("git", git_repo.path(), &args, &git_env, "git"),
            "commit status args mismatch: {args:?} config={config_status:?}"
        );
        assert_eq!(
            fs::read_to_string(skron_repo.path().join(".git/editor-input.txt"))
                .expect("read skron editor input"),
            fs::read_to_string(git_repo.path().join(".git/editor-input.txt"))
                .expect("read git editor input"),
            "commit status editor mismatch: {args:?} config={config_status:?}"
        );
        assert_eq!(
            git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
            git(git_repo.path(), ["cat-file", "-p", "HEAD"]),
            "commit status object mismatch: {args:?} config={config_status:?}"
        );
    }
}

#[test]
fn commit_editor_empty_and_unchanged_abort_like_stock_git() {
    for editor_body in [
        "cp \"$1\" .git/editor-input.txt\n",
        "cp \"$1\" .git/editor-input.txt\n: > \"$1\"\n",
    ] {
        let git_repo = git_init();
        let skron_repo = git_init();
        configure_identity(git_repo.path());
        configure_identity(skron_repo.path());
        install_custom_commit_editor(git_repo.path(), editor_body);
        install_custom_commit_editor(skron_repo.path(), editor_body);

        fs::write(git_repo.path().join("a.txt"), b"abort editor\n").expect("write git a");
        fs::write(skron_repo.path().join("a.txt"), b"abort editor\n").expect("write skron a");
        git(git_repo.path(), ["add", "a.txt"]);
        run_skron(skron_repo.path(), ["add", "a.txt"]);

        let git_editor = git_repo.path().join(".git/editor.sh");
        let skron_editor = skron_repo.path().join(".git/editor.sh");
        let mut git_env = COMMIT_ENV.to_vec();
        git_env.push(("GIT_EDITOR", git_editor.to_str().expect("git editor path")));
        let mut skron_env = COMMIT_ENV.to_vec();
        skron_env.push((
            "GIT_EDITOR",
            skron_editor.to_str().expect("skron editor path"),
        ));

        assert_eq!(
            command_failure_output_with_env(
                skron_bin(),
                skron_repo.path(),
                &["commit"],
                &skron_env,
                "skron"
            ),
            command_failure_output_with_env("git", git_repo.path(), &["commit"], &git_env, "git")
        );
        assert_eq!(
            fs::read_to_string(skron_repo.path().join(".git/editor-input.txt"))
                .expect("read skron editor input"),
            fs::read_to_string(git_repo.path().join(".git/editor-input.txt"))
                .expect("read git editor input")
        );
    }
}

#[test]
fn commit_verbose_template_editor_matches_stock_git_buffer() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());
    install_capture_commit_editor(git_repo.path());
    install_capture_commit_editor(skron_repo.path());

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("a.txt"), b"old\n").expect("write base");
        fs::write(
            repo.join(".git/template.txt"),
            b"Template subject\n\nTemplate body\n",
        )
        .expect("write template");
    }
    git(git_repo.path(), ["add", "a.txt"]);
    run_skron(skron_repo.path(), ["add", "a.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "base"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "base"]);

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("a.txt"), b"new\n").expect("write modified");
        fs::write(repo.join("b.txt"), b"created\n").expect("write created");
    }
    git(git_repo.path(), ["add", "a.txt", "b.txt"]);
    run_skron(skron_repo.path(), ["add", "a.txt", "b.txt"]);

    let git_editor = git_repo.path().join(".git/editor.sh");
    let skron_editor = skron_repo.path().join(".git/editor.sh");
    let mut git_env = COMMIT_ENV.to_vec();
    git_env.push(("GIT_EDITOR", git_editor.to_str().expect("git editor path")));
    let mut skron_env = COMMIT_ENV.to_vec();
    skron_env.push((
        "GIT_EDITOR",
        skron_editor.to_str().expect("skron editor path"),
    ));

    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "--template", ".git/template.txt", "-v"],
            &skron_env,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "--template", ".git/template.txt", "-v"],
            &git_env,
            "git"
        )
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join(".git/editor-input.txt"))
            .expect("read skron editor input"),
        fs::read_to_string(git_repo.path().join(".git/editor-input.txt"))
            .expect("read git editor input")
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
}

#[test]
fn commit_verbose_verbose_template_editor_matches_stock_git_buffer() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());
    install_capture_commit_editor(git_repo.path());
    install_capture_commit_editor(skron_repo.path());

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("a.txt"), b"old\n").expect("write base");
        fs::write(
            repo.join(".git/template.txt"),
            b"Template subject\n\nTemplate body\n",
        )
        .expect("write template");
    }
    git(git_repo.path(), ["add", "a.txt"]);
    run_skron(skron_repo.path(), ["add", "a.txt"]);
    git_with_env(git_repo.path(), ["commit", "-m", "base"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "base"]);

    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("a.txt"), b"new\n").expect("write modified");
        fs::write(repo.join("b.txt"), b"created\n").expect("write created");
    }
    git(git_repo.path(), ["add", "a.txt", "b.txt"]);
    run_skron(skron_repo.path(), ["add", "a.txt", "b.txt"]);
    for repo in [git_repo.path(), skron_repo.path()] {
        fs::write(repo.join("a.txt"), b"new\nunstaged\n").expect("write unstaged");
    }

    let git_editor = git_repo.path().join(".git/editor.sh");
    let skron_editor = skron_repo.path().join(".git/editor.sh");
    let mut git_env = COMMIT_ENV.to_vec();
    git_env.push(("GIT_EDITOR", git_editor.to_str().expect("git editor path")));
    let mut skron_env = COMMIT_ENV.to_vec();
    skron_env.push((
        "GIT_EDITOR",
        skron_editor.to_str().expect("skron editor path"),
    ));

    assert_eq!(
        command_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "--template", ".git/template.txt", "-vv"],
            &skron_env,
            "skron"
        ),
        command_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "--template", ".git/template.txt", "-vv"],
            &git_env,
            "git"
        )
    );
    assert_eq!(
        fs::read_to_string(skron_repo.path().join(".git/editor-input.txt"))
            .expect("read skron editor input"),
        fs::read_to_string(git_repo.path().join(".git/editor-input.txt"))
            .expect("read git editor input")
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
}

#[test]
fn commit_template_requires_editor_change_like_stock_git() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"template\n").expect("write git template");
    fs::write(skron_repo.path().join("a.txt"), b"template\n").expect("write skron template");
    fs::write(git_repo.path().join("template.txt"), b"template subject\n")
        .expect("write git template message");
    fs::write(
        skron_repo.path().join("template.txt"),
        b"template subject\n",
    )
    .expect("write skron template message");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);

    assert_eq!(
        command_failure_output_with_env(
            skron_bin(),
            skron_repo.path(),
            &["commit", "--template", "template.txt"],
            &[("GIT_EDITOR", "true")],
            "skron"
        ),
        command_failure_output_with_env(
            "git",
            git_repo.path(),
            &["commit", "--template", "template.txt"],
            &[("GIT_EDITOR", "true")],
            "git"
        )
    );
}

#[test]
fn commit_cleanup_modes_match_stock_git_object() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"cleanup\n").expect("write git cleanup");
    fs::write(skron_repo.path().join("a.txt"), b"cleanup\n").expect("write skron cleanup");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);

    fs::write(
        git_repo.path().join("message.txt"),
        b"subject\n\n body  \n# comment\n\n \n  \n",
    )
    .expect("write git commit cleanup message");
    fs::write(
        skron_repo.path().join("message.txt"),
        b"subject\n\n body  \n# comment\n\n \n  \n",
    )
    .expect("write skron commit cleanup message");

    for (idx, args) in [
        ["commit", "--cleanup", "strip", "-F", "message.txt"],
        ["commit", "--cleanup", "whitespace", "-F", "message.txt"],
        ["commit", "--cleanup", "default", "-F", "message.txt"],
    ]
    .iter()
    .enumerate()
    {
        fs::write(
            git_repo.path().join("a.txt"),
            format!("cleanup {idx}\n").as_bytes(),
        )
        .expect("rewrite git cleanup target");
        fs::write(
            skron_repo.path().join("a.txt"),
            format!("cleanup {idx}\n").as_bytes(),
        )
        .expect("rewrite skron cleanup target");
        git(git_repo.path(), ["add", "-A"]);
        run_skron(skron_repo.path(), ["add", "-A"]);
        git_with_env(git_repo.path(), *args);
        run_skron_with_env(skron_repo.path(), *args);
        assert_eq!(
            git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
            git(git_repo.path(), ["cat-file", "-p", "HEAD"])
        );
    }
    fs::write(git_repo.path().join("a.txt"), b"cleanup 3\n").expect("rewrite git cleanup target");
    fs::write(skron_repo.path().join("a.txt"), b"cleanup 3\n")
        .expect("rewrite skron cleanup target");
    git_with_env(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        ["commit", "--no-cleanup", "-F", "message.txt"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["commit", "--no-cleanup", "-F", "message.txt"],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
}

#[test]
fn commit_signoff_matches_stock_git_object() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"signoff\n").expect("write git signoff");
    fs::write(skron_repo.path().join("a.txt"), b"signoff\n").expect("write skron signoff");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);

    fs::write(git_repo.path().join("a.txt"), b"signoff\nsignoff-empty\n")
        .expect("rewrite git signoff");
    fs::write(skron_repo.path().join("a.txt"), b"signoff\nsignoff-empty\n")
        .expect("rewrite skron signoff");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        ["commit", "--allow-empty-message", "--signoff", "-m", ""],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["commit", "--allow-empty-message", "--signoff", "-m", ""],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
    fs::write(git_repo.path().join("a.txt"), b"signoff\nsignoff-detail\n")
        .expect("rewrite git signoff");
    fs::write(
        skron_repo.path().join("a.txt"),
        b"signoff\nsignoff-detail\n",
    )
    .expect("rewrite skron signoff");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        ["commit", "--signoff", "-m", "subject", "-m", "details"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["commit", "--signoff", "-m", "subject", "-m", "details"],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
}

#[test]
fn commit_trailers_match_stock_git_object_and_editor_buffer() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"trailer\n").expect("write git trailer");
    fs::write(skron_repo.path().join("a.txt"), b"trailer\n").expect("write skron trailer");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        [
            "commit",
            "-m",
            "subject",
            "--trailer",
            "Reviewed-by: Alice <alice@example.test>",
        ],
    );
    run_skron_with_env(
        skron_repo.path(),
        [
            "commit",
            "-m",
            "subject",
            "--trailer",
            "Reviewed-by: Alice <alice@example.test>",
        ],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    fs::write(git_repo.path().join("message.txt"), b"from file\n\nbody\n")
        .expect("write git message");
    fs::write(
        skron_repo.path().join("message.txt"),
        b"from file\n\nbody\n",
    )
    .expect("write skron message");
    fs::write(git_repo.path().join("a.txt"), b"trailer\nfile\n").expect("rewrite git file trailer");
    fs::write(skron_repo.path().join("a.txt"), b"trailer\nfile\n")
        .expect("rewrite skron file trailer");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        ["commit", "-F", "message.txt", "--trailer", "Refs: #123"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["commit", "-F", "message.txt", "--trailer", "Refs: #123"],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    fs::write(git_repo.path().join("a.txt"), b"trailer\nmulti\n")
        .expect("rewrite git multi trailer");
    fs::write(skron_repo.path().join("a.txt"), b"trailer\nmulti\n")
        .expect("rewrite skron multi trailer");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        [
            "commit",
            "-m",
            "subject",
            "--trailer",
            "A: one",
            "--trailer",
            "B=two",
        ],
    );
    run_skron_with_env(
        skron_repo.path(),
        [
            "commit",
            "-m",
            "subject",
            "--trailer",
            "A: one",
            "--trailer",
            "B=two",
        ],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    fs::write(git_repo.path().join("a.txt"), b"trailer\nsignoff\n")
        .expect("rewrite git signoff trailer");
    fs::write(skron_repo.path().join("a.txt"), b"trailer\nsignoff\n")
        .expect("rewrite skron signoff trailer");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        [
            "commit",
            "--signoff",
            "-m",
            "subject",
            "--trailer",
            "Reviewed-by: Alice <alice@example.test>",
        ],
    );
    run_skron_with_env(
        skron_repo.path(),
        [
            "commit",
            "--signoff",
            "-m",
            "subject",
            "--trailer",
            "Reviewed-by: Alice <alice@example.test>",
        ],
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );

    install_capture_commit_editor(git_repo.path());
    install_capture_commit_editor(skron_repo.path());
    fs::write(git_repo.path().join("a.txt"), b"trailer\neditor\n")
        .expect("rewrite git editor trailer");
    fs::write(skron_repo.path().join("a.txt"), b"trailer\neditor\n")
        .expect("rewrite skron editor trailer");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    let git_editor = git_repo.path().join(".git/editor.sh");
    let skron_editor = skron_repo.path().join(".git/editor.sh");
    let mut git_env = COMMIT_ENV.to_vec();
    git_env.push(("GIT_EDITOR", git_editor.to_str().expect("git editor path")));
    let mut skron_env = COMMIT_ENV.to_vec();
    skron_env.push((
        "GIT_EDITOR",
        skron_editor.to_str().expect("skron editor path"),
    ));
    let skron_output = command_output_with_env(
        skron_bin(),
        skron_repo.path(),
        &[
            "commit",
            "--trailer",
            "Reviewed-by: Alice <alice@example.test>",
        ],
        &skron_env,
        "skron",
    );
    let git_output = command_output_with_env(
        "git",
        git_repo.path(),
        &[
            "commit",
            "--trailer",
            "Reviewed-by: Alice <alice@example.test>",
        ],
        &git_env,
        "git",
    );
    assert_eq!(skron_output.0, git_output.0);
    assert_eq!(skron_output.2, git_output.2);
    assert!(skron_output.1.contains("Final subject"));
    assert!(git_output.1.contains("Final subject"));
    assert_eq!(
        fs::read_to_string(skron_repo.path().join(".git/editor-input.txt"))
            .expect("read skron trailer editor input"),
        fs::read_to_string(git_repo.path().join(".git/editor-input.txt"))
            .expect("read git trailer editor input")
    );
    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
}

#[test]
fn commit_squash_matches_stock_git_object() {
    let git_repo = git_init();
    let skron_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(skron_repo.path());

    fs::write(git_repo.path().join("a.txt"), b"squash\n").expect("write git squash base");
    fs::write(skron_repo.path().join("a.txt"), b"squash\n").expect("write skron squash base");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "base"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "base"]);

    fs::write(git_repo.path().join("a.txt"), b"squash target\n").expect("write git squash target");
    fs::write(skron_repo.path().join("a.txt"), b"squash target\n")
        .expect("write skron squash target");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(git_repo.path(), ["commit", "-m", "target"]);
    run_skron_with_env(skron_repo.path(), ["commit", "-m", "target"]);

    fs::write(git_repo.path().join("a.txt"), b"squash work\n").expect("write git squash work");
    fs::write(skron_repo.path().join("a.txt"), b"squash work\n").expect("write skron squash work");
    git(git_repo.path(), ["add", "-A"]);
    run_skron(skron_repo.path(), ["add", "-A"]);
    git_with_env(
        git_repo.path(),
        ["commit", "--squash", "HEAD~1", "-m", "work"],
    );
    run_skron_with_env(
        skron_repo.path(),
        ["commit", "--squash", "HEAD~1", "-m", "work"],
    );

    assert_eq!(
        git(skron_repo.path(), ["cat-file", "-p", "HEAD"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD"])
    );
}

fn install_commit_hook_set(repo: &Path) {
    let hooks = repo.join(".git/hooks");
    fs::create_dir_all(&hooks).expect("create hooks dir");
    fs::write(
        hooks.join("pre-commit"),
        "#!/bin/sh\necho pre:$#:$* >> hook.log\n",
    )
    .expect("write pre-commit");
    fs::write(
        hooks.join("commit-msg"),
        "#!/bin/sh\necho msg:$#:$*:$(cat \"$1\") >> hook.log\n",
    )
    .expect("write commit-msg");
    fs::write(
        hooks.join("post-commit"),
        "#!/bin/sh\necho post:$#:$* >> hook.log\n",
    )
    .expect("write post-commit");
    chmod_executable(&hooks.join("pre-commit"));
    chmod_executable(&hooks.join("commit-msg"));
    chmod_executable(&hooks.join("post-commit"));
}

fn install_failing_commit_hook(repo: &Path, hook_name: &str) {
    let hooks = repo.join(".git/hooks");
    fs::create_dir_all(&hooks).expect("create hooks dir");
    fs::write(
        hooks.join(hook_name),
        format!("#!/bin/sh\necho {hook_name}-fail >&2\nexit 42\n"),
    )
    .expect("write failing hook");
    chmod_executable(&hooks.join(hook_name));
}

fn install_prepare_commit_msg_hook(repo: &Path) {
    let hooks = repo.join(".git/hooks");
    fs::create_dir_all(&hooks).expect("create hooks dir");
    fs::write(
        hooks.join("prepare-commit-msg"),
        "#!/bin/sh\necho prepare:$#:$* >> hook.log\necho prepared >> \"$1\"\n",
    )
    .expect("write prepare-commit-msg");
    chmod_executable(&hooks.join("prepare-commit-msg"));
}

fn install_post_rewrite_hook(repo: &Path) {
    let hooks = repo.join(".git/hooks");
    fs::create_dir_all(&hooks).expect("create hooks dir");
    fs::write(
        hooks.join("post-rewrite"),
        "#!/bin/sh\necho post-rewrite:$#:$* >> rewrite.log\ncat >> rewrite.log\n",
    )
    .expect("write post-rewrite");
    chmod_executable(&hooks.join("post-rewrite"));
}

fn install_capture_commit_editor(repo: &Path) {
    install_custom_commit_editor(
        repo,
        "cp \"$1\" .git/editor-input.txt\nprintf 'Final subject\\n\\nFinal body\\n' > \"$1\"\n",
    );
}

fn install_custom_commit_editor(repo: &Path, body: &str) {
    fs::write(repo.join(".git/editor.sh"), format!("#!/bin/sh\n{body}"))
        .expect("write custom editor");
    chmod_executable(&repo.join(".git/editor.sh"));
}

fn chmod_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).expect("script metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod script");
    }
}
