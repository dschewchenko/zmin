mod common;

use common::{command_any_output as command_output, stock_git_bin, zmin_bin};
use tempfile::TempDir;

#[test]
fn version_command_options_match_stock_git_shape() {
    let dir = TempDir::new().expect("temp dir");

    let zmin = command_output(
        zmin_bin(),
        dir.path(),
        &["version", "--build-options"],
        "zmin",
    );
    assert_eq!(zmin.0, 0);
    assert!(zmin.1.starts_with("git version 2.47.1.zmin "), "{}", zmin.1);
    for expected in [
        "cpu:",
        "sizeof-long:",
        "sizeof-size_t:",
        "shell-path:",
        "zmin-version:",
    ] {
        assert!(
            zmin.1.contains(expected),
            "version --build-options missing {expected}: {}",
            zmin.1
        );
    }
    assert_eq!(zmin.2, "");

    let zmin_invalid = command_output(
        zmin_bin(),
        dir.path(),
        &["version", "--version"],
        "zmin invalid",
    );
    let stock_invalid = command_output(
        stock_git_bin().to_str().expect("stock git path"),
        dir.path(),
        &["version", "--version"],
        "git invalid",
    );
    assert_eq!(zmin_invalid.0, stock_invalid.0);
    assert_eq!(zmin_invalid.1, stock_invalid.1);
    assert_eq!(zmin_invalid.2, stock_invalid.2);
}
