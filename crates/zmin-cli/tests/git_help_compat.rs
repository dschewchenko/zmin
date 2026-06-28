mod common;

use common::{command_output_with_env, stock_git_bin, zmin_bin};
use tempfile::TempDir;

const HELP_ENVS: &[(&str, &str)] = &[
    ("GIT_PAGER", "cat"),
    ("PAGER", "cat"),
    ("MANPAGER", "cat"),
    ("GIT_MAN_VIEWER", "cat"),
    ("GIT_EDITOR", "true"),
];

#[test]
fn help_documented_option_family_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let stock = stock_git_bin().to_str().expect("stock git path");
    let cases: &[&[&str]] = &[
        &["help", "-a"],
        &["help", "--all"],
        &["help", "-g"],
        &["help", "--guides"],
        &["help", "-c"],
        &["help", "--config"],
        &["help", "-m"],
        &["help", "--man"],
        &["help", "-i"],
        &["help", "--info"],
        &["help", "-w"],
        &["help", "--web"],
        &["help", "--user-interfaces"],
        &["help", "--developer-interfaces"],
        &["help", "--all", "--no-aliases"],
        &["help", "--all", "--no-external-commands"],
        &["help", "--all", "--verbose"],
    ];

    for args in cases {
        let git = command_output_with_env(stock, dir.path(), args, HELP_ENVS, "stock git help");
        let zmin = command_output_with_env(zmin_bin(), dir.path(), args, HELP_ENVS, "zmin help");
        assert_eq!(zmin, git, "args: {:?}", args);
    }
}
