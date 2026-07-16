mod common;

use common::{
    command_output_with_env, configure_identity, git, git_init, stock_git_bin, write_file,
};
use std::fs;
use std::process::Command;

#[test]
fn trace2_normal_env_emits_basic_lifecycle_for_version() {
    let repo = git_init();
    let trace_dir = tempfile::TempDir::new().expect("trace dir");
    let trace_path = trace_dir.path().join("normal.trace");

    let (_status, stdout, stderr) = command_output_with_env(
        common::zmin_bin(),
        repo.path(),
        &["version"],
        &[
            ("GIT_TRACE2", trace_path.to_str().expect("trace path utf8")),
            ("GIT_TRACE2_BRIEF", "1"),
        ],
        "zmin version with trace2 normal",
    );

    assert_eq!(stdout, "git version 2.47.1.zmin (zmin 0.1.0)");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");

    let trace = fs::read_to_string(&trace_path).expect("read normal trace");
    let lines = trace.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 5, "unexpected trace lines: {trace}");
    assert_eq!(lines[0], "version 2.47.1.zmin (zmin 0.1.0)");
    assert!(
        lines[1].contains(" start ") || lines[1].starts_with("start "),
        "missing start line: {trace}"
    );
    assert!(
        lines[1].ends_with(" version"),
        "unexpected argv render: {trace}"
    );
    assert_eq!(lines[2], "cmd_name version (version)");
    assert!(lines[3].starts_with("exit elapsed:"));
    assert!(lines[3].ends_with("code:0"));
    assert!(lines[4].starts_with("atexit elapsed:"));
    assert!(lines[4].ends_with("code:0"));
}

#[test]
fn trace2_perf_env_emits_basic_lifecycle_for_version() {
    let repo = git_init();
    let trace_dir = tempfile::TempDir::new().expect("trace dir");
    let trace_path = trace_dir.path().join("perf.trace");

    let (_status, stdout, stderr) = command_output_with_env(
        common::zmin_bin(),
        repo.path(),
        &["version"],
        &[
            (
                "GIT_TRACE2_PERF",
                trace_path.to_str().expect("trace path utf8"),
            ),
            ("GIT_TRACE2_PERF_BRIEF", "1"),
        ],
        "zmin version with trace2 perf",
    );

    assert_eq!(stdout, "git version 2.47.1.zmin (zmin 0.1.0)");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");

    let trace = fs::read_to_string(&trace_path).expect("read perf trace");
    let lines = trace.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 5, "unexpected trace lines: {trace}");
    assert!(lines[0].contains("| version"));
    assert!(lines[0].ends_with("| 2.47.1.zmin (zmin 0.1.0)"));
    assert!(lines[1].contains("| start"));
    assert!(
        lines[1].contains("/zmin version"),
        "unexpected start line: {trace}"
    );
    assert!(lines[2].contains("| cmd_name"));
    assert!(lines[2].ends_with("| version (version)"));
    assert!(lines[3].contains("| exit"));
    assert!(lines[3].ends_with("| code:0"));
    assert!(lines[4].contains("| atexit"));
    assert!(lines[4].ends_with("| code:0"));
}

#[test]
fn trace2_normal_uses_global_config_target_and_brief() {
    let repo = git_init();
    let config_dir = tempfile::TempDir::new().expect("config dir");
    let trace_path = config_dir.path().join("config.trace");
    let config_path = config_dir.path().join("gitconfig");

    fs::write(
        &config_path,
        format!(
            "[trace2]\n\tnormalTarget = {}\n\tnormalBrief = true\n",
            trace_path.display()
        ),
    )
    .expect("write global config");

    let (_status, stdout, stderr) = command_output_with_env(
        common::zmin_bin(),
        repo.path(),
        &["version"],
        &[(
            "GIT_CONFIG_GLOBAL",
            config_path.to_str().expect("config path utf8"),
        )],
        "zmin version with trace2 config",
    );

    assert_eq!(stdout, "git version 2.47.1.zmin (zmin 0.1.0)");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");

    let trace = fs::read_to_string(&trace_path).expect("read config trace");
    assert!(trace.contains("version 2.47.1.zmin (zmin 0.1.0)"));
    assert!(trace.contains("cmd_name version (version)"));
}

#[test]
fn trace2_emits_config_and_env_params_and_redacts_by_default() {
    let repo = git_init();
    let config_dir = tempfile::TempDir::new().expect("config dir");
    let config_path = config_dir.path().join("gitconfig");
    let trace_redacted = config_dir.path().join("redacted.trace");
    let trace_unredacted = config_dir.path().join("unredacted.trace");

    fs::write(
        &config_path,
        "[trace2]\n\tconfigParams = cfg.prop.*\n\tenvvars = ENV_PROP_FOO\n\tnormalBrief = true\n[cfg \"prop\"]\n\tfoo = red\n",
    )
    .expect("write trace2 config");

    let (_status, _stdout, stderr) = command_output_with_env(
        common::zmin_bin(),
        repo.path(),
        &["version"],
        &[
            (
                "GIT_CONFIG_GLOBAL",
                config_path.to_str().expect("config path utf8"),
            ),
            (
                "GIT_TRACE2",
                trace_redacted.to_str().expect("trace path utf8"),
            ),
            ("ENV_PROP_FOO", "https://user:pwd@example.com/token"),
        ],
        "zmin version with redacted trace2 params",
    );
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");

    let redacted = fs::read_to_string(&trace_redacted).expect("read redacted trace");
    assert!(redacted.contains("def_param cfg.prop.foo=red"));
    assert!(redacted.contains("def_param ENV_PROP_FOO=https://redacted@example.com/token"));
    assert!(
        !redacted.contains("user:pwd"),
        "trace leaked credentials: {redacted}"
    );

    let (_status, _stdout, stderr) = command_output_with_env(
        common::zmin_bin(),
        repo.path(),
        &["version"],
        &[
            (
                "GIT_CONFIG_GLOBAL",
                config_path.to_str().expect("config path utf8"),
            ),
            (
                "GIT_TRACE2",
                trace_unredacted.to_str().expect("trace path utf8"),
            ),
            ("GIT_TRACE2_REDACT", "0"),
            ("ENV_PROP_FOO", "https://user:pwd@example.com/token"),
        ],
        "zmin version with unredacted trace2 params",
    );
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");

    let unredacted = fs::read_to_string(&trace_unredacted).expect("read unredacted trace");
    assert!(unredacted.contains("def_param cfg.prop.foo=red"));
    assert!(unredacted.contains("def_param ENV_PROP_FOO=https://user:pwd@example.com/token"));
}

#[test]
fn trace2_clone_unredacted_start_and_remote_url_match_stock_git_shape() {
    let root = tempfile::TempDir::new().expect("temp root");
    let source = root.path().join("source");
    git(
        root.path(),
        ["init", "-b", "main", source.to_str().expect("source utf8")],
    );
    configure_identity(&source);
    write_file(&source, "README.md", "hi\n");
    git(&source, ["add", "README.md"]);
    git(&source, ["commit", "-m", "init"]);

    let home = root.path().join("home");
    fs::create_dir_all(&home).expect("create home");
    fs::write(
        home.join(".gitconfig"),
        format!(
            "[url \"{}/\"]\n\tinsteadOf = https://user:pwd@example.com/\n[trace2]\n\tconfigParams = core.*,remote.*.url\n",
            root.path().display()
        ),
    )
    .expect("write gitconfig");

    let stock_trace = root.path().join("stock.trace");
    let zmin_trace = root.path().join("zmin.trace");
    let typed_url = "https://user:pwd@example.com/source";

    let (stock_status, _stdout, stock_stderr) = command_output_with_env(
        stock_git_bin().to_str().expect("stock git utf8"),
        root.path(),
        &["clone", typed_url, "stock-clone"],
        &[
            ("HOME", home.to_str().expect("home utf8")),
            (
                "GIT_TRACE2",
                stock_trace.to_str().expect("stock trace utf8"),
            ),
            ("GIT_TRACE2_REDACT", "0"),
        ],
        "stock git clone trace2 unredacted",
    );
    assert_eq!(stock_status, 0, "stock clone stderr: {stock_stderr}");

    let (zmin_status, _stdout, zmin_stderr) = command_output_with_env(
        common::zmin_bin(),
        root.path(),
        &["clone", typed_url, "zmin-clone"],
        &[
            ("HOME", home.to_str().expect("home utf8")),
            ("GIT_TRACE2", zmin_trace.to_str().expect("zmin trace utf8")),
            ("GIT_TRACE2_REDACT", "0"),
        ],
        "zmin clone trace2 unredacted",
    );
    assert_eq!(zmin_status, 0, "zmin clone stderr: {zmin_stderr}");

    let zmin_trace = fs::read_to_string(&zmin_trace).expect("read zmin trace");
    assert!(
        zmin_trace.contains("start ")
            && zmin_trace.contains(" clone https://user:pwd@example.com/source zmin-clone"),
        "unexpected zmin start line: {zmin_trace}"
    );
    assert!(
        zmin_trace.contains("def_param remote.origin.url=https://user:pwd@example.com/source"),
        "missing unredacted remote.origin.url def_param: {zmin_trace}"
    );
}

#[test]
fn trace2_perf_http_fetch_emits_single_dashed_lifecycle_with_child_cmd_name() {
    let repo = git_init();
    let config_dir = tempfile::TempDir::new().expect("config dir");
    let trace_path = config_dir.path().join("http-fetch.perf");
    let config_path = config_dir.path().join("gitconfig");

    fs::write(
        &config_path,
        "[trace2]\n\tconfigParams = cfg.prop.*\n\tenvvars = ENV_PROP_FOO\n\tperfBrief = true\n[cfg \"prop\"]\n\tfoo = red\n",
    )
    .expect("write trace2 config");

    let output = Command::new(common::zmin_bin())
        .args(["http-fetch", "--stdin", "file:///"])
        .current_dir(repo.path())
        .env(
            "GIT_CONFIG_GLOBAL",
            config_path.to_str().expect("config path utf8"),
        )
        .env(
            "GIT_TRACE2_PERF",
            trace_path.to_str().expect("trace path utf8"),
        )
        .env("ENV_PROP_FOO", "blue")
        .output()
        .expect("run zmin http-fetch");
    let stderr = String::from_utf8(output.stderr)
        .expect("stderr utf8")
        .trim_end_matches('\n')
        .to_owned();
    assert_eq!(
        output.status.code(),
        Some(129),
        "unexpected exit status: {stderr}"
    );
    assert!(
        stderr.contains("usage: git http-fetch"),
        "expected usage stderr, got: {stderr}"
    );

    let trace = fs::read_to_string(&trace_path).expect("read http-fetch trace");
    let version_count = trace.matches("| version").count();
    let start_count = trace.matches("| start").count();
    assert_eq!(
        version_count, 1,
        "unexpected duplicate version lines: {trace}"
    );
    assert_eq!(start_count, 1, "unexpected duplicate start lines: {trace}");
    assert_eq!(
        trace.matches("_run_dashed_ (_run_dashed_)").count(),
        1,
        "unexpected duplicate root cmd_name lines: {trace}"
    );
    assert!(
        trace.contains("[ch0] class:dashed argv:[git-http-fetch]"),
        "missing dashed child_start line: {trace}"
    );
    assert!(
        trace.contains("http-fetch (_run_dashed_/http-fetch)"),
        "missing http-fetch cmd_name line: {trace}"
    );
    assert!(
        trace.contains("cfg.prop.foo:red") && trace.contains("ENV_PROP_FOO:blue"),
        "missing expected def_param lines: {trace}"
    );
}

#[test]
fn trace2_test_tool_return_uses_trace2_command_name() {
    let repo = git_init();
    let trace_dir = tempfile::TempDir::new().expect("trace dir");
    let trace_path = trace_dir.path().join("helper.trace");

    let (_status, stdout, stderr) = command_output_with_env(
        common::zmin_bin(),
        repo.path(),
        &["test-tool", "trace2", "001return", "0"],
        &[("GIT_TRACE2", trace_path.to_str().expect("trace path utf8"))],
        "zmin test-tool trace2 return",
    );

    assert!(stdout.is_empty(), "unexpected stdout: {stdout}");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");

    let trace = fs::read_to_string(&trace_path).expect("read helper trace");
    assert!(trace.contains("start "));
    assert!(
        trace.contains(" trace2 001return 0"),
        "unexpected start line: {trace}"
    );
    assert!(trace.contains("cmd_name trace2 (trace2)"));
    assert!(trace.contains("exit elapsed:"));
    assert!(trace.contains("atexit elapsed:"));
}

#[test]
fn trace2_test_tool_error_mode_emits_error_lines() {
    let repo = git_init();
    let trace_dir = tempfile::TempDir::new().expect("trace dir");
    let trace_path = trace_dir.path().join("helper-error.trace");

    let (_status, stdout, stderr) = command_output_with_env(
        common::zmin_bin(),
        repo.path(),
        &[
            "test-tool",
            "trace2",
            "003error",
            "hello world",
            "this is a test",
        ],
        &[("GIT_TRACE2", trace_path.to_str().expect("trace path utf8"))],
        "zmin test-tool trace2 error",
    );

    assert!(stdout.is_empty(), "unexpected stdout: {stdout}");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");

    let trace = fs::read_to_string(&trace_path).expect("read helper error trace");
    assert!(trace.contains("start "));
    assert!(
        trace.contains(" trace2 003error 'hello world' 'this is a test'"),
        "unexpected start line: {trace}"
    );
    assert!(trace.contains("error hello world"));
    assert!(trace.contains("error this is a test"));
}

#[test]
fn trace2_test_tool_uses_global_config_target_through_include() {
    let repo = git_init();
    let home = tempfile::TempDir::new().expect("home dir");
    let trace_path = home.path().join("helper-config.trace");
    let real_config = home.path().join("real.gitconfig");

    fs::write(
        &real_config,
        format!(
            "[trace2]\n\tnormalBrief = true\n\tnormalTarget = {}\n",
            trace_path.display()
        ),
    )
    .expect("write real config");
    fs::write(
        home.path().join(".gitconfig"),
        format!("[include]\n\tpath = {}\n", real_config.display()),
    )
    .expect("write include config");

    let (_status, stdout, stderr) = command_output_with_env(
        common::zmin_bin(),
        repo.path(),
        &["test-tool", "trace2", "001return", "0"],
        &[("HOME", home.path().to_str().expect("home utf8"))],
        "zmin test-tool trace2 config include",
    );

    assert!(stdout.is_empty(), "unexpected stdout: {stdout}");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");

    let trace = fs::read_to_string(&trace_path).expect("read helper config trace");
    assert!(trace.contains("cmd_name trace2 (trace2)"));
    assert!(trace.contains(" trace2 001return 0"));
}

#[test]
fn trace2_test_tool_perf_child_emits_nested_hierarchy() {
    let repo = git_init();
    let trace_dir = tempfile::TempDir::new().expect("trace dir");
    let trace_path = trace_dir.path().join("helper-child.perf");

    let (_status, stdout, stderr) = command_output_with_env(
        common::zmin_bin(),
        repo.path(),
        &[
            "test-tool",
            "trace2",
            "004child",
            "test-tool",
            "trace2",
            "001return",
            "0",
        ],
        &[(
            "GIT_TRACE2_PERF",
            trace_path.to_str().expect("trace path utf8"),
        )],
        "zmin test-tool trace2 child perf",
    );

    assert!(stdout.is_empty(), "unexpected stdout: {stdout}");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");

    let trace = fs::read_to_string(&trace_path).expect("read helper child perf trace");
    assert!(trace.contains("child_start"));
    assert!(
        trace.contains("argv:[test-tool trace2 001return 0]"),
        "{trace}"
    );
    assert!(trace.contains("d1 | main"), "{trace}");
    assert!(trace.contains("cmd_name     |     |"), "{trace}");
    assert!(trace.contains("trace2 (trace2/trace2)"), "{trace}");
    assert!(trace.contains("child_exit"), "{trace}");
}

#[test]
fn trace2_test_tool_perf_timer_modes_emit_summary_events() {
    let repo = git_init();
    let trace_dir = tempfile::TempDir::new().expect("trace dir");
    let trace_path = trace_dir.path().join("helper-timer.perf");

    let (_status, stdout, stderr) = command_output_with_env(
        common::zmin_bin(),
        repo.path(),
        &["test-tool", "trace2", "101timer", "5", "10", "3"],
        &[(
            "GIT_TRACE2_PERF",
            trace_path.to_str().expect("trace path utf8"),
        )],
        "zmin test-tool trace2 timer perf",
    );

    assert!(stdout.is_empty(), "unexpected stdout: {stdout}");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");

    let trace = fs::read_to_string(&trace_path).expect("read helper timer perf trace");
    assert!(trace.contains("th01:ut_101"), "{trace}");
    assert!(trace.contains("th_timer"), "{trace}");
    assert!(trace.contains("name:test2 intervals:5"), "{trace}");
    assert!(trace.contains("main"), "{trace}");
    assert!(trace.contains("name:test2 intervals:15"), "{trace}");
}

#[test]
fn trace2_test_tool_perf_counter_modes_emit_summary_events() {
    let repo = git_init();
    let trace_dir = tempfile::TempDir::new().expect("trace dir");
    let trace_path = trace_dir.path().join("helper-counter.perf");

    let (_status, stdout, stderr) = command_output_with_env(
        common::zmin_bin(),
        repo.path(),
        &["test-tool", "trace2", "201counter", "7", "13", "3"],
        &[(
            "GIT_TRACE2_PERF",
            trace_path.to_str().expect("trace path utf8"),
        )],
        "zmin test-tool trace2 counter perf",
    );

    assert!(stdout.is_empty(), "unexpected stdout: {stdout}");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");

    let trace = fs::read_to_string(&trace_path).expect("read helper counter perf trace");
    assert!(trace.contains("th01:ut_201"), "{trace}");
    assert!(trace.contains("th_counter"), "{trace}");
    assert!(trace.contains("name:test2 value:20"), "{trace}");
    assert!(trace.contains("counter"), "{trace}");
    assert!(trace.contains("name:test2 value:60"), "{trace}");
}

#[test]
fn trace2_query_command_uses_query_cmd_name() {
    let repo = git_init();
    let trace_dir = tempfile::TempDir::new().expect("trace dir");
    let trace_path = trace_dir.path().join("query.perf");

    let (_status, stdout, stderr) = command_output_with_env(
        common::zmin_bin(),
        repo.path(),
        &["--man-path"],
        &[
            (
                "GIT_TRACE2_PERF",
                trace_path.to_str().expect("trace path utf8"),
            ),
            ("GIT_TRACE2_PERF_BRIEF", "1"),
        ],
        "zmin query trace2 perf",
    );

    assert!(!stdout.is_empty(), "expected query output");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");

    let trace = fs::read_to_string(&trace_path).expect("read query perf trace");
    assert!(trace.contains("_query_ (_query_)"), "{trace}");
}
