mod common;

use common::{
    command_any_output_with_stdin, configure_identity, git, git_init, git_with_env, zmin_bin,
};

#[test]
fn cat_file_batch_unknown_atom_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    std::fs::write(repo.path().join("a.txt"), "a\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);
    let head = git(repo.path(), ["rev-parse", "HEAD"]);
    let stdin = format!("{head}\n");

    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--batch=%(bad)"],
            &stdin,
            "zmin"
        ),
        command_any_output_with_stdin(
            "git",
            repo.path(),
            &["cat-file", "--batch=%(bad)"],
            &stdin,
            "git"
        )
    );
}
