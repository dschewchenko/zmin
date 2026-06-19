mod common;

use tempfile::TempDir;

#[test]
fn compatibility_profile_v2_32_json_is_usable() {
    let dir = TempDir::new().expect("temp dir");
    let (code, stdout, stderr) = common::command_any_output(
        common::zmin_bin(),
        dir.path(),
        &["compatibility", "--profile", "v2-32", "--format", "json"],
        "zmin",
    );

    assert_eq!(code, 0);
    assert!(stderr.trim().is_empty());
    assert!(
        stdout.contains(r#""profile": "v2-32""#),
        "expected json profile marker: {stdout}"
    );
    assert!(
        stdout.contains(r#""missing": ["#),
        "expected missing section: {stdout}"
    );
    assert!(
        stdout.contains(r#""commands":"#),
        "expected commands section in json: {stdout}"
    );
}

#[test]
fn compatibility_profile_v2_47_keeps_current_acceptance_gate() {
    let dir = TempDir::new().expect("temp dir");
    let (code, stdout, stderr) = common::command_any_output(
        common::zmin_bin(),
        dir.path(),
        &["compatibility", "--profile", "v2-47", "--format", "json"],
        "zmin",
    );

    assert_eq!(code, 0);
    assert!(stderr.trim().is_empty());
    assert!(
        stdout.contains(
            r#""counts": {"implemented": 203, "matching_baseline": 151, "missing": 0, "extra": 52}"#
        ),
        "compatibility counts changed; update the acceptance docs with the new live report: {stdout}"
    );
    assert!(
        stdout.contains(r#""explicit_not_ready": 0"#),
        "expected no explicit not-ready commands in the current scope: {stdout}"
    );
    assert!(
        stdout.contains(r#""missing": []"#),
        "supported Git baseline must not have missing commands: {stdout}"
    );

    let submodule_start = stdout
        .find(r#""name": "git-submodule""#)
        .expect("git-submodule command should be present in compatibility report");
    let submodule_block = &stdout[submodule_start..];
    assert!(
        submodule_block.contains(r#""ready": true"#),
        "git-submodule should be ready after stable subcommand parity coverage"
    );

    let http_fetch_start = stdout
        .find(r#""name": "git-http-fetch""#)
        .expect("git-http-fetch command should be present in compatibility report");
    let http_fetch_block = &stdout[http_fetch_start..];
    assert!(
        http_fetch_block.contains(r#""ready": true"#),
        "git-http-fetch should be ready after dumb HTTP and direct packfile parity coverage"
    );

    for command in [
        "git-scalar-list",
        "git-scalar-delete",
        "git-scalar-diagnose",
        "git-scalar-clone",
        "git-scalar-reconfigure",
        "git-scalar-register",
        "git-scalar-run",
        "git-scalar-unregister",
        "git-scalar-version",
    ] {
        let start = stdout
            .find(&format!(r#""name": "{command}""#))
            .unwrap_or_else(|| {
                panic!("{command} command should be present in compatibility report")
            });
        let block = &stdout[start..];
        assert!(
            block.contains(r#""ready": true"#),
            "{command} should be ready after focused Scalar parity coverage"
        );
    }

    let scalar_help_start = stdout
        .find(r#""name": "git-scalar-help""#)
        .expect("git-scalar-help command should be present in compatibility report");
    let scalar_help_block = &stdout[scalar_help_start..];
    assert!(
        scalar_help_block.contains(r#""ready": true"#),
        "git-scalar-help should be ready after manual-style help parity coverage"
    );

    for command in ["git-gitk", "git-gitweb"] {
        let start = stdout
            .find(&format!(r#""name": "{command}""#))
            .unwrap_or_else(|| {
                panic!("{command} command should be present in compatibility report")
            });
        let block = &stdout[start..];
        assert!(
            block.contains(r#""ready": true"#),
            "{command} should be ready because current stock Git exposes only the unavailable-command failure shape on this supported surface"
        );
    }

    for command in ["git-fetch", "git-push", "git-status", "git-format-patch"] {
        let start = stdout
            .find(&format!(r#""name": "{command}""#))
            .unwrap_or_else(|| panic!("{command} should be present in compatibility report"));
        let block = &stdout[start..std::cmp::min(start + 260, stdout.len())];
        assert!(
            block.contains(r#""ready": true"#),
            "{command} must remain ready as shared primitive surface command"
        );
    }
}

#[test]
fn compatibility_profile_v2_47_has_no_explicit_not_ready_commands() {
    let dir = TempDir::new().expect("temp dir");
    let (code, stdout, stderr) = common::command_any_output(
        common::zmin_bin(),
        dir.path(),
        &["compatibility", "--profile", "v2-47", "--format", "json"],
        "zmin",
    );

    assert_eq!(code, 0);
    assert!(stderr.trim().is_empty());
    let read_ready = |json: &str, command: &str| -> bool {
        let name_marker = format!(r#""name": "{command}""#);
        let start = json
            .find(&name_marker)
            .unwrap_or_else(|| panic!("{command} should be present in compatibility report"));
        let block = &json[start..std::cmp::min(start + 512, json.len())];
        let ready_pos = block
            .find(r#""ready":"#)
            .unwrap_or_else(|| panic!("{command} block misses ready flag"));
        let ready_text = &block[ready_pos..std::cmp::min(ready_pos + 64, block.len())];
        ready_text.contains(r#""ready": true"#)
    };

    assert!(
        stdout.contains(r#""explicit_not_ready": 0"#),
        "expected explicit_not_ready count to stay at 0"
    );

    for command in [
        "git-fetch",
        "git-push",
        "git-status",
        "git-format-patch",
        "git-diff",
        "git-commit",
        "git-clone",
        "git-maintenance",
        "git-scalar",
    ] {
        assert!(
            read_ready(&stdout, command),
            "{command} should stay ready in the current compatibility gate"
        );
    }
}

#[test]
fn compatibility_profile_text_prints_readiness_and_command_blocklist() {
    let dir = TempDir::new().expect("temp dir");
    let (code, stdout, stderr) = common::command_any_output(
        common::zmin_bin(),
        dir.path(),
        &["compat", "--profile", "v2-47"],
        "zmin",
    );

    assert_eq!(code, 0);
    assert!(stderr.trim().is_empty());
    assert!(
        stdout.contains("Compatibility profile: v2-47"),
        "expected profile header: {stdout}"
    );
    assert!(
        stdout.contains("Commands: expected"),
        "expected count summary"
    );
    assert!(
        stdout.contains("Additional commands"),
        "expected command delta section: {stdout}"
    );
}
