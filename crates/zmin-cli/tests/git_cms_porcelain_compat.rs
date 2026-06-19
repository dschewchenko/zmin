mod common;

use std::fs;

use common::{
    clone_repo_fixture, configure_identity, git, git_init, run_zmin, run_zmin_failure_output,
    write_file,
};

#[test]
fn cms_changes_and_save_compose_existing_git_operations() {
    let repo = git_init();
    configure_identity(repo.path());

    assert_eq!(run_zmin(repo.path(), ["changes"]), "No changes.");
    assert_eq!(
        run_zmin(repo.path(), ["save", "empty"]),
        "Nothing to save."
    );

    write_file(repo.path(), "page.md", "hello\n");
    assert_eq!(
        run_zmin(repo.path(), ["changes"]),
        "Changes:\nnew: page.md"
    );

    assert_eq!(
        run_zmin(repo.path(), ["save", "updated home page"]),
        "Saved: updated home page"
    );
    assert_eq!(
        git(repo.path(), ["log", "--format=%s", "-n1"]),
        "updated home page"
    );
    assert_eq!(run_zmin(repo.path(), ["changes"]), "No changes.");

    write_file(repo.path(), "page.md", "hello again\n");
    assert_eq!(
        run_zmin(repo.path(), ["changes"]),
        "Changes:\nmodified: page.md"
    );

    fs::remove_file(repo.path().join("page.md")).expect("remove page");
    assert_eq!(
        run_zmin(repo.path(), ["changes"]),
        "Changes:\ndeleted: page.md"
    );
}

#[test]
fn cms_undo_reverts_last_logged_save_only_when_safe() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "page.md", "base\n");
    git(repo.path(), ["add", "page.md"]);
    git(repo.path(), ["commit", "-m", "base"]);
    let base_id = git(repo.path(), ["rev-parse", "HEAD"]);

    write_file(repo.path(), "page.md", "edited\n");
    assert_eq!(
        run_zmin(repo.path(), ["save", "edit page"]),
        "Saved: edit page"
    );
    let saved_id = git(repo.path(), ["rev-parse", "HEAD"]);
    assert_ne!(saved_id, base_id);
    let operation_log =
        fs::read_to_string(repo.path().join(".git/zmin/operations.log")).expect("operation log");
    assert!(operation_log.contains(&format!("save\t{saved_id}\tedit page")));

    write_file(repo.path(), "scratch.txt", "dirty\n");
    let dirty_undo = run_zmin_failure_output(repo.path(), &["undo"]);
    assert_eq!(dirty_undo.0, 1);
    assert!(dirty_undo.2.contains("save or discard changes before undo"));
    fs::remove_file(repo.path().join("scratch.txt")).expect("remove scratch");

    assert_eq!(run_zmin(repo.path(), ["undo"]), "Undid save: edit page");
    assert_eq!(git(repo.path(), ["rev-parse", "HEAD"]), base_id);
    assert_eq!(
        fs::read_to_string(repo.path().join("page.md")).expect("read page"),
        "edited\n"
    );
    assert_eq!(
        run_zmin(repo.path(), ["changes"]),
        "Changes:\nmodified: page.md"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join(".git/zmin/operations.log"))
            .expect("operation log after undo"),
        ""
    );

    let nothing = run_zmin_failure_output(repo.path(), &["undo"]);
    assert_eq!(nothing.0, 1);
    assert!(nothing.2.contains("save or discard changes before undo"));
}

#[test]
fn cms_publish_and_update_use_safe_remote_operations() {
    let seed = git_init();
    configure_identity(seed.path());
    write_file(seed.path(), "page.md", "first\n");
    git(seed.path(), ["add", "page.md"]);
    git(seed.path(), ["commit", "-m", "first"]);

    let remote = tempfile::TempDir::new().expect("remote");
    git(remote.path(), ["init", "--bare"]);
    git(
        seed.path(),
        [
            "remote",
            "add",
            "origin",
            remote.path().to_str().expect("remote path"),
        ],
    );
    git(seed.path(), ["push", "-u", "origin", "HEAD"]);

    let editor = clone_repo_fixture(remote.path());
    configure_identity(editor.path());
    let client = clone_repo_fixture(remote.path());
    configure_identity(client.path());

    write_file(client.path(), "draft.md", "local\n");
    let dirty_publish = run_zmin_failure_output(client.path(), &["publish"]);
    assert_eq!(dirty_publish.0, 1);
    assert!(
        dirty_publish
            .2
            .contains("save or discard changes before publish")
    );
    run_zmin(client.path(), ["save", "local draft"]);
    assert_eq!(run_zmin(client.path(), ["publish"]), "Published.");
    assert_eq!(
        git(remote.path(), ["log", "--format=%s", "-n1", "HEAD"]),
        "local draft"
    );

    git(editor.path(), ["pull", "--ff-only"]);
    write_file(editor.path(), "page.md", "remote\n");
    git(editor.path(), ["add", "page.md"]);
    git(editor.path(), ["commit", "-m", "remote update"]);
    git(editor.path(), ["push"]);

    write_file(client.path(), "scratch.txt", "dirty\n");
    let dirty_update = run_zmin_failure_output(client.path(), &["update"]);
    assert_eq!(dirty_update.0, 1);
    assert!(
        dirty_update
            .2
            .contains("save or discard changes before update")
    );
    fs::remove_file(client.path().join("scratch.txt")).expect("remove dirty scratch");

    assert_eq!(run_zmin(client.path(), ["update"]), "Updated.");
    assert_eq!(
        fs::read_to_string(client.path().join("page.md")).expect("read updated page"),
        "remote\n"
    );
    assert_eq!(
        git(client.path(), ["log", "--format=%s", "-n1", "HEAD"]),
        "remote update"
    );
}

#[test]
fn cms_timeline_and_recover_are_safe_human_aliases() {
    let repo = git_init();
    configure_identity(repo.path());

    assert_eq!(run_zmin(repo.path(), ["timeline"]), "No history.");

    write_file(repo.path(), "page.md", "base\n");
    git(repo.path(), ["add", "page.md"]);
    git(repo.path(), ["commit", "-m", "first page"]);
    let first = git(repo.path(), ["rev-parse", "--short", "HEAD"]);

    write_file(repo.path(), "about.md", "about\n");
    git(repo.path(), ["add", "about.md"]);
    git(repo.path(), ["commit", "-m", "about page"]);
    let second = git(repo.path(), ["rev-parse", "--short", "HEAD"]);

    assert_eq!(
        run_zmin(repo.path(), ["timeline"]),
        format!("History:\n{second}  about page\n{first}  first page")
    );

    write_file(repo.path(), "page.md", "draft\n");
    assert_eq!(
        run_zmin(repo.path(), ["recover", "page.md"]),
        "Recovered: page.md"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("page.md")).expect("read recovered page"),
        "base\n"
    );
    assert_eq!(run_zmin(repo.path(), ["changes"]), "No changes.");

    write_file(repo.path(), "page.md", "staged\n");
    git(repo.path(), ["add", "page.md"]);
    let staged = run_zmin_failure_output(repo.path(), &["recover", "page.md"]);
    assert_eq!(staged.0, 1);
    assert!(
        staged
            .2
            .contains("refusing to recover staged changes in page.md")
    );
}
