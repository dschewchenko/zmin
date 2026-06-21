mod common;

use common::{
    command_any_output, command_output, configure_identity, git, git_with_env, write_file,
};
use tempfile::TempDir;

#[test]
fn clone_ref_format_reftable_is_open_gap_against_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    write_file(&source, "README.md", "hello\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "init"]);

    let stock = command_output(
        "git",
        dir.path(),
        &[
            "clone",
            "--ref-format=reftable",
            source.to_str().expect("source path"),
            "git-reftable",
        ],
        "git clone --ref-format=reftable",
    );
    assert_eq!(stock.0, 0);
    assert_eq!(
        command_output(
            "git",
            &dir.path().join("git-reftable"),
            &["rev-parse", "--show-ref-format"],
            "git rev-parse --show-ref-format"
        )
        .1,
        "reftable"
    );

    let zmin = command_any_output(
        common::zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--ref-format=reftable",
            source.to_str().expect("source path"),
            "zmin-reftable",
        ],
        "zmin clone --ref-format=reftable",
    );
    assert_eq!(zmin.0, 128);
    assert!(zmin.1.is_empty());
    assert!(zmin.2.contains("reftable ref storage is not supported yet"));
    assert!(!dir.path().join("zmin-reftable").exists());
}
