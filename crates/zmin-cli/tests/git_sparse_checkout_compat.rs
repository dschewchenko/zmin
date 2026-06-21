mod common;

use std::{fs, path::Path};

use tempfile::TempDir;

use common::{
    clone_repo_fixture, command_any_output, configure_identity, git, git_failure_output, git_init,
    git_with_env, git_with_stdin, run_zmin, run_zmin_failure_output, run_zmin_with_stdin,
    visible_worktree_files, write_file, zmin_bin,
};

fn sparse_checkout_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "README.md", "readme\n");
    fs::create_dir_all(repo.path().join("docs")).expect("create docs");
    fs::create_dir_all(repo.path().join("src")).expect("create src");
    write_file(repo.path(), "docs/guide.md", "guide\n");
    write_file(repo.path(), "src/main.rs", "fn main() {}\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    repo
}

fn git_config_get(repo: &Path, key: &str) -> (i32, String, String) {
    command_any_output("git", repo, &["config", "--get", key], "git")
}

#[test]
fn sparse_checkout_set_list_disable_matches_stock_git_files() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    git(git_repo.path(), ["sparse-checkout", "set", "docs"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "set", "docs"]);
    assert_eq!(
        visible_worktree_files(zmin_repo.path()),
        visible_worktree_files(git_repo.path())
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        "docs"
    );

    git(git_repo.path(), ["sparse-checkout", "add", "src"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "add", "src"]);
    assert_eq!(
        visible_worktree_files(zmin_repo.path()),
        visible_worktree_files(git_repo.path())
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        "docs\nsrc"
    );

    fs::remove_file(zmin_repo.path().join("docs/guide.md")).expect("remove zmin sparse file");
    fs::remove_file(git_repo.path().join("docs/guide.md")).expect("remove git sparse file");
    git(git_repo.path(), ["sparse-checkout", "reapply"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "reapply"]);
    assert_eq!(
        visible_worktree_files(zmin_repo.path()),
        visible_worktree_files(git_repo.path())
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );

    git(git_repo.path(), ["sparse-checkout", "disable"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "disable"]);
    assert_eq!(
        visible_worktree_files(zmin_repo.path()),
        visible_worktree_files(git_repo.path())
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );
}

#[test]
fn sparse_checkout_pathspec_edge_cases_match_stock_git() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["sparse-checkout", "set", ".."]),
        git_failure_output(git_repo.path(), &["sparse-checkout", "set", ".."])
    );

    git(git_repo.path(), ["sparse-checkout", "set", "a\\b"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "set", "a\\b"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        git(git_repo.path(), ["sparse-checkout", "list"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["sparse-checkout", "add", ".."]),
        git_failure_output(git_repo.path(), &["sparse-checkout", "add", ".."])
    );
}

#[test]
fn sparse_checkout_skip_checks_option_matches_stock_git() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    let git_output = command_any_output(
        "git",
        git_repo.path(),
        &["sparse-checkout", "set", "--skip-checks", "docs"],
        "git",
    );
    let zmin_output = command_any_output(
        zmin_bin(),
        zmin_repo.path(),
        &["sparse-checkout", "set", "--skip-checks", "docs"],
        "zmin",
    );
    assert_eq!(zmin_output, git_output);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        git(git_repo.path(), ["sparse-checkout", "list"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );
    assert_eq!(
        visible_worktree_files(zmin_repo.path()),
        visible_worktree_files(git_repo.path())
    );
}

#[test]
fn sparse_checkout_non_sparse_failures_match_stock_git() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    for args in [
        ["sparse-checkout", "list"].as_slice(),
        ["sparse-checkout", "add", "docs"].as_slice(),
        ["sparse-checkout", "add", "--bad"].as_slice(),
        ["sparse-checkout", "reapply"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), args),
            git_failure_output(git_repo.path(), args)
        );
    }
}

#[test]
fn sparse_checkout_unknown_subcommand_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["sparse-checkout", "bogus"]),
        git_failure_output(git_repo.path(), &["sparse-checkout", "bogus"]),
    );
}

#[test]
fn sparse_checkout_set_unknown_option_matches_stock_git_usage() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["sparse-checkout", "set", "--bad"]),
        git_failure_output(git_repo.path(), &["sparse-checkout", "set", "--bad"]),
    );
}

#[test]
fn sparse_checkout_init_unknown_option_matches_stock_git_usage() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["sparse-checkout", "init", "--bad"]),
        git_failure_output(git_repo.path(), &["sparse-checkout", "init", "--bad"]),
    );
}

#[test]
fn sparse_checkout_add_unknown_option_matches_stock_git_usage_when_enabled() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    git(git_repo.path(), ["sparse-checkout", "set", "docs"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "set", "docs"]);

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["sparse-checkout", "add", "--bad"]),
        git_failure_output(git_repo.path(), &["sparse-checkout", "add", "--bad"]),
    );
}

#[test]
fn sparse_checkout_add_stdin_ignores_positional_patterns_like_stock_git() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    git(git_repo.path(), ["sparse-checkout", "set", "docs"]);
    run_zmin(zmin_repo.path(), ["sparse-checkout", "set", "docs"]);
    git_with_stdin(
        git_repo.path(),
        ["sparse-checkout", "add", "--stdin", "missing"],
        "src\n",
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        ["sparse-checkout", "add", "--stdin", "missing"],
        "src\n",
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        git(git_repo.path(), ["sparse-checkout", "list"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );
}

#[test]
fn sparse_checkout_stdin_and_config_options_match_stock_git() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    git_with_stdin(
        git_repo.path(),
        ["sparse-checkout", "set", "--stdin"],
        "docs\n",
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        ["sparse-checkout", "set", "--stdin"],
        "docs\n",
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        git(git_repo.path(), ["sparse-checkout", "list"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );

    git_with_stdin(
        git_repo.path(),
        ["sparse-checkout", "add", "--stdin"],
        "src\n",
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        ["sparse-checkout", "add", "--stdin"],
        "src\n",
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        git(git_repo.path(), ["sparse-checkout", "list"])
    );

    git_with_stdin(
        git_repo.path(),
        ["sparse-checkout", "set", "--stdin", "docs"],
        "src\n",
    );
    run_zmin_with_stdin(
        zmin_repo.path(),
        ["sparse-checkout", "set", "--stdin", "docs"],
        "src\n",
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        git(git_repo.path(), ["sparse-checkout", "list"])
    );
    assert_eq!(
        git(zmin_repo.path(), ["ls-files", "-t"]),
        git(git_repo.path(), ["ls-files", "-t"])
    );

    git(
        git_repo.path(),
        ["sparse-checkout", "set", "--no-cone", "docs"],
    );
    run_zmin(
        zmin_repo.path(),
        ["sparse-checkout", "set", "--no-cone", "docs"],
    );
    assert_eq!(
        git_config_get(zmin_repo.path(), "core.sparseCheckoutCone"),
        git_config_get(git_repo.path(), "core.sparseCheckoutCone")
    );
    assert_eq!(
        git_config_get(zmin_repo.path(), "index.sparse"),
        git_config_get(git_repo.path(), "index.sparse")
    );

    let git_init_repo = git_init();
    let zmin_init_repo = git_init();
    git(
        git_init_repo.path(),
        ["sparse-checkout", "init", "--no-cone", "--no-sparse-index"],
    );
    run_zmin(
        zmin_init_repo.path(),
        ["sparse-checkout", "init", "--no-cone", "--no-sparse-index"],
    );
    for key in [
        "core.sparseCheckout",
        "core.sparseCheckoutCone",
        "index.sparse",
    ] {
        assert_eq!(
            git_config_get(zmin_init_repo.path(), key),
            git_config_get(git_init_repo.path(), key)
        );
    }
}

#[test]
fn sparse_checkout_sparse_index_options_match_stock_git() {
    let git_repo = sparse_checkout_fixture_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());

    git(
        git_repo.path(),
        ["sparse-checkout", "set", "--sparse-index", "docs"],
    );
    run_zmin(
        zmin_repo.path(),
        ["sparse-checkout", "set", "--sparse-index", "docs"],
    );
    for key in [
        "core.sparseCheckout",
        "core.sparseCheckoutCone",
        "index.sparse",
    ] {
        assert_eq!(
            git_config_get(zmin_repo.path(), key),
            git_config_get(git_repo.path(), key)
        );
    }
    assert_eq!(
        run_zmin(zmin_repo.path(), ["sparse-checkout", "list"]),
        git(git_repo.path(), ["sparse-checkout", "list"])
    );
    assert_eq!(
        visible_worktree_files(zmin_repo.path()),
        visible_worktree_files(git_repo.path())
    );

    let git_init_repo = git_init();
    let zmin_init_repo = git_init();
    git(
        git_init_repo.path(),
        ["sparse-checkout", "init", "--sparse-index", "--no-cone"],
    );
    run_zmin(
        zmin_init_repo.path(),
        ["sparse-checkout", "init", "--sparse-index", "--no-cone"],
    );
    for key in [
        "core.sparseCheckout",
        "core.sparseCheckoutCone",
        "index.sparse",
    ] {
        assert_eq!(
            git_config_get(zmin_init_repo.path(), key),
            git_config_get(git_init_repo.path(), key)
        );
    }
}
