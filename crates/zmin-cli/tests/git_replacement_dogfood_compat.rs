mod common;

use std::path::Path;
use std::process::Command;

use common::{stock_git_bin, zmin_bin};

#[test]
fn replacement_dogfood_smoke_script_passes_with_current_zmin_binary() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let script = workspace_root.join("tools/git-replacement-dogfood-smoke.sh");

    let output = Command::new("bash")
        .arg(&script)
        .current_dir(workspace_root)
        .env("ZMIN_BIN", zmin_bin())
        .env("ZMIN_STOCK_GIT", stock_git_bin())
        .output()
        .expect("run git replacement dogfood smoke");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("git_replacement_dogfood_smoke=ok"),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
