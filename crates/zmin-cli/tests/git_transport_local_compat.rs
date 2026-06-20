mod common;

use std::fs;
use std::io::Write;
use std::process::Command;

use tempfile::TempDir;

use common::{
    command_any_output, command_output_with_env, configure_identity, git, git_failure_output,
    git_status, git_status_args, git_with_env, run_zmin, run_zmin_failure_output, run_zmin_status,
    run_zmin_with_env, zmin_bin,
};

fn write_sequence_editor_replace_pick(
    root: &std::path::Path,
    name: &str,
    action: &str,
    occurrence: usize,
) -> std::path::PathBuf {
    let editor = root.join(name);
    fs::write(
        &editor,
        format!(
            r#"#!/bin/sh
awk -v action='{}' -v occurrence='{}' '
  /^pick / {{
    seen += 1
    if (seen == occurrence) {{
      sub(/^pick /, action " ")
    }}
  }}
  {{ print }}
' "$1" > "$1.tmp" && mv "$1.tmp" "$1"
"#,
            action, occurrence
        ),
    )
    .expect("write sequence editor");
    editor
}

fn shell_command_path(path: &str) -> String {
    #[cfg(windows)]
    {
        path.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        path.to_owned()
    }
}

fn chmod_executable(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(path).expect("hook metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod hook");
    }
}

fn assert_matching_depth_fetch_state(
    zmin_client: &std::path::Path,
    git_client: &std::path::Path,
    missing_commit: &str,
) {
    assert_eq!(
        git(zmin_client, ["rev-parse", "--is-shallow-repository"]),
        git(git_client, ["rev-parse", "--is-shallow-repository"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/shallow")).expect("zmin shallow"),
        fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow")
    );
    assert_eq!(
        git_status_args(zmin_client, &["cat-file", "-e", missing_commit]),
        git_status_args(git_client, &["cat-file", "-e", missing_commit])
    );
}

#[test]
fn fetch_local_remote_updates_remote_refs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"main\n").expect("write main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    fs::write(source.join("README.md"), b"main update\n").expect("update main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main update"]);
    git(&source, ["tag", "v2"]);
    git(&source, ["switch", "-c", "feature"]);
    fs::write(source.join("feature.txt"), b"feature\n").expect("write feature");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "feature"]);
    git(&source, ["switch", "main"]);

    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(&git_client, ["fetch", "origin"]);
    run_zmin(&zmin_client, ["fetch", "origin"]);

    assert_eq!(
        run_zmin(&zmin_client, ["branch", "-r"]),
        git(&git_client, ["branch", "-r"])
    );
    assert_eq!(
        run_zmin(&zmin_client, ["show-ref", "--tags"]),
        git(&git_client, ["show-ref", "--tags"])
    );
    assert_eq!(
        git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
        git(&git_client, ["rev-parse", "refs/remotes/origin/main"])
    );
    assert_eq!(
        git(&zmin_client, ["rev-parse", "refs/remotes/origin/feature"]),
        git(&git_client, ["rev-parse", "refs/remotes/origin/feature"])
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", "refs/remotes/origin/main^{tree}"]
        ),
        git(
            &git_client,
            ["cat-file", "-p", "refs/remotes/origin/main^{tree}"]
        )
    );

    let git_file_client = dir.path().join("git-file-client");
    let zmin_file_client = dir.path().join("zmin-file-client");
    git(dir.path(), ["init", "-b", "main", "git-file-client"]);
    run_zmin(dir.path(), ["init", "-b", "main", "zmin-file-client"]);
    let source_file_url = format!("file://{}", source.display());
    git(
        &git_file_client,
        ["remote", "add", "origin", &source_file_url],
    );
    run_zmin(
        &zmin_file_client,
        ["remote", "add", "origin", &source_file_url],
    );
    git(&git_file_client, ["fetch", "--depth", "1", "origin"]);
    run_zmin(&zmin_file_client, ["fetch", "--depth", "1", "origin"]);
    assert_eq!(
        run_zmin(&zmin_file_client, ["branch", "-r"]),
        git(&git_file_client, ["branch", "-r"])
    );
    assert_eq!(
        run_zmin(&zmin_file_client, ["log", "--oneline", "--all"]),
        git(&git_file_client, ["log", "--oneline", "--all"])
    );
    assert_eq!(
        fs::read_to_string(zmin_file_client.join(".git/shallow")).expect("zmin shallow"),
        fs::read_to_string(git_file_client.join(".git/shallow")).expect("git shallow")
    );
}

#[test]
fn pull_local_path_ours_strategy_matches_stock_git_merge_commit() {
    let dir = TempDir::new().expect("temp dir");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    for repo in [&git_repo, &zmin_repo] {
        git(
            dir.path(),
            ["init", "-b", "main", repo.to_str().expect("repo path")],
        );
        configure_identity(repo);
        fs::write(repo.join("base.txt"), b"base\n").expect("write base");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "base"]);
        git(repo, ["switch", "-c", "side"]);
        fs::write(repo.join("side.txt"), b"side\n").expect("write side");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "side"]);
        git(repo, ["switch", "main"]);
        fs::write(repo.join("main.txt"), b"main\n").expect("write main");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "main"]);
    }

    git_with_env(
        &git_repo,
        ["pull", "-s", "ours", "--no-rebase", ".", "side"],
    );
    run_zmin_with_env(
        &zmin_repo,
        ["pull", "-s", "ours", "--no-rebase", ".", "side"],
    );

    assert_eq!(
        git(&zmin_repo, ["rev-parse", "HEAD^{tree}"]),
        git(&git_repo, ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&zmin_repo, ["rev-parse", "HEAD^{tree}"]),
        git(&zmin_repo, ["rev-parse", "HEAD^1^{tree}"])
    );
    assert_eq!(
        git(&zmin_repo, ["rev-list", "--parents", "-1", "HEAD"])
            .split_whitespace()
            .count(),
        git(&git_repo, ["rev-list", "--parents", "-1", "HEAD"])
            .split_whitespace()
            .count()
    );
}

#[test]
fn fetch_without_remote_uses_current_branch_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"original\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "original"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "one"],
    );
    let one = dir.path().join("one");
    configure_identity(&one);
    fs::write(one.join("file"), b"updated by one\n").expect("write one");
    git(&one, ["commit", "-am", "updated by one"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    for client in [&git_client, &zmin_client] {
        git(client, ["config", "branch.main.remote", "one"]);
        git(client, ["config", "remote.one.url", "../one/.git/"]);
        git(
            client,
            [
                "config",
                "remote.one.fetch",
                "refs/heads/main:refs/heads/one",
            ],
        );
    }

    git(&git_client, ["fetch"]);
    run_zmin(&zmin_client, ["fetch"]);

    assert_eq!(
        git(&zmin_client, ["rev-parse", "--verify", "refs/heads/one"]),
        git(&git_client, ["rev-parse", "--verify", "refs/heads/one"])
    );
}

#[test]
fn fetch_all_appends_fetch_head_for_all_remotes_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let one = dir.path().join("one");
    let two = dir.path().join("two");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    for (remote, file, branch) in [
        (&one, "one.txt", "one-branch"),
        (&two, "two.txt", "two-branch"),
    ] {
        git(
            dir.path(),
            ["init", "-b", "main", remote.to_str().expect("remote path")],
        );
        configure_identity(remote);
        fs::write(remote.join(file), format!("{file}\n")).expect("write remote file");
        git(remote, ["add", "-A"]);
        git_with_env(remote, ["commit", "-m", file]);
        git(remote, ["branch", branch]);
    }

    for client in [&git_client, &zmin_client] {
        git(
            dir.path(),
            ["init", "-b", "main", client.to_str().expect("client path")],
        );
        git(
            client,
            ["remote", "add", "one", one.to_str().expect("one path")],
        );
        git(
            client,
            ["remote", "add", "two", two.to_str().expect("two path")],
        );
    }

    git(&git_client, ["fetch", "--all"]);
    run_zmin(&zmin_client, ["fetch", "--all"]);

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
}

#[test]
fn fetch_all_with_upload_pack_local_transports_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let one = dir.path().join("one");
    let two = dir.path().join("two");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    let wrapper = dir.path().join("upload-pack-all.sh");
    let log = wrapper.with_extension("sh.log");

    fs::write(
        &wrapper,
        b"#!/bin/sh\nprintf 'invoked %s\\n' \"$*\" >> \"$0.log\"\nexec git-upload-pack \"$@\"\n",
    )
    .expect("write upload-pack wrapper");
    chmod_executable(&wrapper);

    for (remote, file, branch) in [
        (&one, "one.txt", "one-branch"),
        (&two, "two.txt", "two-branch"),
    ] {
        git(
            dir.path(),
            ["init", "-b", "main", remote.to_str().expect("remote path")],
        );
        configure_identity(remote);
        fs::write(remote.join(file), format!("{file}\n")).expect("write remote file");
        git(remote, ["add", "-A"]);
        git_with_env(remote, ["commit", "-m", file]);
        git(remote, ["branch", branch]);
    }

    for client in [&git_client, &zmin_client] {
        git(
            dir.path(),
            ["init", "-b", "main", client.to_str().expect("client path")],
        );
        git(
            client,
            ["remote", "add", "one", one.to_str().expect("one path")],
        );
        git(
            client,
            ["remote", "add", "two", &format!("file://{}", two.display())],
        );
    }

    let wrapper_command = shell_command_path(wrapper.to_str().expect("wrapper path"));
    let args = [
        "fetch",
        "--quiet",
        &format!("--upload-pack={wrapper_command}"),
        "--all",
    ];
    let git_output = command_any_output("git", &git_client, &args, "git");
    let zmin_output = command_any_output(zmin_bin(), &zmin_client, &args, "zmin");

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(zmin_output.2, git_output.2);
    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert!(
        !log.exists(),
        "stock Git does not invoke custom upload-pack for local/file --all"
    );
}

#[test]
fn fetch_multiple_remotes_appends_fetch_head_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let one = dir.path().join("one");
    let two = dir.path().join("two");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    for (remote, file, branch) in [
        (&one, "one.txt", "one-branch"),
        (&two, "two.txt", "two-branch"),
    ] {
        git(
            dir.path(),
            ["init", "-b", "main", remote.to_str().expect("remote path")],
        );
        configure_identity(remote);
        fs::write(remote.join(file), format!("{file}\n")).expect("write remote file");
        git(remote, ["add", "-A"]);
        git_with_env(remote, ["commit", "-m", file]);
        git(remote, ["branch", branch]);
    }

    for client in [&git_client, &zmin_client] {
        git(
            dir.path(),
            ["init", "-b", "main", client.to_str().expect("client path")],
        );
        git(
            client,
            ["remote", "add", "one", one.to_str().expect("one path")],
        );
        git(
            client,
            ["remote", "add", "two", two.to_str().expect("two path")],
        );
    }

    git(&git_client, ["fetch", "--multiple", "one", "two"]);
    run_zmin(&zmin_client, ["fetch", "--multiple", "one", "two"]);

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
}

#[test]
fn fetch_multiple_with_upload_pack_local_transports_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let one = dir.path().join("one");
    let two = dir.path().join("two");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    let wrapper = dir.path().join("upload-pack-multiple.sh");
    let log = wrapper.with_extension("sh.log");

    fs::write(
        &wrapper,
        b"#!/bin/sh\nprintf 'invoked %s\\n' \"$*\" >> \"$0.log\"\nexec git-upload-pack \"$@\"\n",
    )
    .expect("write upload-pack wrapper");
    chmod_executable(&wrapper);

    for (remote, file, branch) in [
        (&one, "one.txt", "one-branch"),
        (&two, "two.txt", "two-branch"),
    ] {
        git(
            dir.path(),
            ["init", "-b", "main", remote.to_str().expect("remote path")],
        );
        configure_identity(remote);
        fs::write(remote.join(file), format!("{file}\n")).expect("write remote file");
        git(remote, ["add", "-A"]);
        git_with_env(remote, ["commit", "-m", file]);
        git(remote, ["branch", branch]);
    }

    for client in [&git_client, &zmin_client] {
        git(
            dir.path(),
            ["init", "-b", "main", client.to_str().expect("client path")],
        );
        git(
            client,
            ["remote", "add", "one", one.to_str().expect("one path")],
        );
        git(
            client,
            ["remote", "add", "two", &format!("file://{}", two.display())],
        );
    }

    let wrapper_command = shell_command_path(wrapper.to_str().expect("wrapper path"));
    let args = [
        "fetch",
        "--quiet",
        &format!("--upload-pack={wrapper_command}"),
        "--multiple",
        "one",
        "two",
    ];
    let git_output = command_any_output("git", &git_client, &args, "git");
    let zmin_output = command_any_output(zmin_bin(), &zmin_client, &args, "zmin");

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(zmin_output.2, git_output.2);
    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert!(
        !log.exists(),
        "stock Git does not invoke custom upload-pack for local/file --multiple"
    );
}

#[test]
fn fetch_multiple_without_remotes_matches_stock_git_noop() {
    let dir = TempDir::new().expect("temp dir");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    for client in [&git_client, &zmin_client] {
        git(
            dir.path(),
            ["init", "-b", "main", client.to_str().expect("client path")],
        );
    }

    let git_output = command_any_output("git", &git_client, &["fetch", "--multiple"], "git");
    let zmin_output =
        command_any_output(zmin_bin(), &zmin_client, &["fetch", "--multiple"], "zmin");

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(zmin_output.2, git_output.2);
}

#[test]
fn fetch_prefetch_named_remote_writes_prefetch_namespace_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("main.txt"), b"main\n").expect("write main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["switch", "-c", "feature"]);
    fs::write(source.join("feature.txt"), b"feature\n").expect("write feature");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "feature"]);

    for client in [&git_client, &zmin_client] {
        git(
            dir.path(),
            ["init", "-b", "main", client.to_str().expect("client path")],
        );
        git(
            client,
            [
                "remote",
                "add",
                "origin",
                source.to_str().expect("source path"),
            ],
        );
    }

    git(&git_client, ["fetch", "--prefetch", "origin"]);
    run_zmin(&zmin_client, ["fetch", "--prefetch", "origin"]);

    assert_eq!(
        git(
            &zmin_client,
            [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/remotes",
                "refs/prefetch",
            ],
        ),
        git(
            &git_client,
            [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/remotes",
                "refs/prefetch",
            ],
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
}

#[test]
fn fetch_prefetch_explicit_branch_writes_prefetch_namespace_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("main.txt"), b"main\n").expect("write main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    for client in [&git_client, &zmin_client] {
        git(
            dir.path(),
            ["init", "-b", "main", client.to_str().expect("client path")],
        );
        git(
            client,
            [
                "remote",
                "add",
                "origin",
                source.to_str().expect("source path"),
            ],
        );
    }

    git(&git_client, ["fetch", "--prefetch", "origin", "main"]);
    run_zmin(&zmin_client, ["fetch", "--prefetch", "origin", "main"]);

    assert_eq!(
        git(
            &zmin_client,
            [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/remotes",
                "refs/prefetch",
            ],
        ),
        git(
            &git_client,
            [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/remotes",
                "refs/prefetch",
            ],
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
}

#[test]
fn fetch_creates_git_trace_packet_file_for_upstream_harness() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );
    let zmin_client = dir.path().join("zmin-client");
    let trace = dir.path().join("trace.out");
    let trace_value = trace.to_str().expect("trace path");
    let output = command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["fetch"],
        &[("GIT_TRACE_PACKET", trace_value)],
        "zmin",
    );

    assert_eq!(output.0, 0, "fetch failed: {}", output.2);
    let trace_contents = fs::read_to_string(trace).expect("trace file");
    assert!(!trace_contents.contains("ref-prefix HEAD"));
}

#[test]
fn fetch_negotiation_tip_writes_limited_have_trace_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("alpha"), b"1\n").expect("write alpha");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "alpha_1"]);
    git(&source, ["tag", "alpha_1"]);
    fs::write(source.join("alpha"), b"2\n").expect("write alpha 2");
    git(&source, ["commit", "-am", "alpha_2"]);
    git(&source, ["tag", "alpha_2"]);
    git(&source, ["checkout", "--orphan", "beta"]);
    fs::write(source.join("beta"), b"1\n").expect("write beta");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "beta_1"]);
    git(&source, ["tag", "beta_1"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );
    let zmin_client = dir.path().join("zmin-client");
    let alpha_1 = git(&zmin_client, ["rev-parse", "alpha_1"]);
    let beta_1 = git(&zmin_client, ["rev-parse", "beta_1"]);
    let alpha_2 = git(&zmin_client, ["rev-parse", "alpha_2"]);
    let trace = dir.path().join("negotiation.trace");
    let trace_value = trace.to_str().expect("trace path");

    let output = command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["fetch", "--negotiation-tip=*_1", "origin"],
        &[("GIT_TRACE_PACKET", trace_value)],
        "zmin",
    );

    assert_eq!(output.0, 0, "fetch failed: {}", output.2);
    let trace_contents = fs::read_to_string(trace).expect("trace file");
    assert!(trace_contents.contains(&format!("fetch> have {alpha_1}")));
    assert!(trace_contents.contains(&format!("fetch> have {beta_1}")));
    assert!(!trace_contents.contains(&format!("fetch> have {alpha_2}")));
}

#[test]
fn fetch_negotiation_tip_rejects_missing_object_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );
    let zmin_client = dir.path().join("zmin-client");
    let zero = "0000000000000000000000000000000000000000";
    let zero_tip = format!("--negotiation-tip={zero}");
    let (status, _, stderr) = run_zmin_failure_output(
        &zmin_client,
        &["fetch", "--negotiation-tip=HEAD", &zero_tip, "origin"],
    );

    assert_eq!(status, 128);
    assert!(stderr.contains(&format!("fatal: the object {zero} does not exist")));
}

#[test]
fn fetch_negotiate_only_outputs_common_tip_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("main.txt"), b"main\n").expect("write main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let args = [
        "fetch",
        "--negotiate-only",
        "--negotiation-tip=HEAD",
        "origin",
    ];
    let git_output = command_any_output("git", &git_client, &args, "git");
    let zmin_output = command_any_output(zmin_bin(), &zmin_client, &args, "zmin");

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(zmin_output.2, git_output.2);
    assert!(!zmin_client.join(".git/FETCH_HEAD").exists());
    assert!(!git_client.join(".git/FETCH_HEAD").exists());
}

#[test]
fn fetch_negotiate_only_without_tip_matches_stock_git_failure() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("main.txt"), b"main\n").expect("write main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let args = ["fetch", "--negotiate-only", "origin"];
    let git_output = command_any_output("git", &git_client, &args, "git");
    let zmin_output = command_any_output(zmin_bin(), &zmin_client, &args, "zmin");

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(zmin_output.2, git_output.2);
}

#[test]
fn fetch_follow_remote_head_never_does_not_recreate_remote_head() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    for client in [&git_client, &zmin_client] {
        git(
            client,
            ["update-ref", "--no-deref", "-d", "refs/remotes/origin/HEAD"],
        );
        git(
            client,
            ["config", "remote.origin.followRemoteHEAD", "never"],
        );
    }

    git(&git_client, ["fetch"]);
    run_zmin(&zmin_client, ["fetch"]);

    assert_eq!(
        run_zmin_status(
            &zmin_client,
            ["rev-parse", "--verify", "refs/remotes/origin/HEAD"]
        ),
        git_status(
            &git_client,
            ["rev-parse", "--verify", "refs/remotes/origin/HEAD"]
        )
    );
}

#[test]
fn fetch_default_follow_remote_head_preserves_existing_remote_head() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );
    let zmin_client = dir.path().join("zmin-client");
    configure_identity(&zmin_client);
    run_zmin(&zmin_client, ["switch", "-c", "other"]);
    fs::write(zmin_client.join("other"), b"other\n").expect("write other");
    run_zmin(&zmin_client, ["add", "-A"]);
    run_zmin_with_env(&zmin_client, ["commit", "-m", "other"]);
    run_zmin(&zmin_client, ["push", "-u", "origin", "other"]);
    run_zmin(&zmin_client, ["remote", "set-head", "origin", "other"]);

    run_zmin(&zmin_client, ["fetch"]);

    assert_eq!(
        git(&zmin_client, ["rev-parse", "refs/remotes/origin/HEAD"]),
        git(&zmin_client, ["rev-parse", "refs/remotes/origin/other"])
    );
}

#[test]
fn fetch_explicit_refspec_does_not_update_remote_head_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );
    let zmin_client = dir.path().join("zmin-client");
    configure_identity(&zmin_client);
    run_zmin(&zmin_client, ["switch", "-c", "other"]);
    fs::write(zmin_client.join("other"), b"other\n").expect("write other");
    run_zmin(&zmin_client, ["add", "-A"]);
    run_zmin_with_env(&zmin_client, ["commit", "-m", "other"]);
    run_zmin(&zmin_client, ["push", "-u", "origin", "other"]);
    run_zmin(&zmin_client, ["remote", "set-head", "origin", "other"]);
    run_zmin(
        &zmin_client,
        ["config", "remote.origin.followRemoteHEAD", "always"],
    );

    run_zmin(
        &zmin_client,
        [
            "fetch",
            "origin",
            "refs/heads/main:refs/remotes/origin/main",
        ],
    );

    assert_eq!(
        git(&zmin_client, ["symbolic-ref", "refs/remotes/origin/HEAD"]),
        "refs/remotes/origin/other"
    );
}

#[test]
fn fetch_prune_removes_stale_remote_tracking_refs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    for client in [&git_client, &zmin_client] {
        git(
            client,
            ["update-ref", "refs/remotes/origin/extrabranch", "main"],
        );
    }

    git(&git_client, ["fetch", "--prune", "origin"]);
    run_zmin(&zmin_client, ["fetch", "--prune", "origin"]);

    assert_eq!(
        git_status(&zmin_client, ["rev-parse", "origin/extrabranch"]),
        git_status(&git_client, ["rev-parse", "origin/extrabranch"])
    );
}

#[test]
fn fetch_prune_with_branch_name_keeps_other_remote_tracking_refs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    for client in [&git_client, &zmin_client] {
        git(
            client,
            ["update-ref", "refs/remotes/origin/extrabranch", "main"],
        );
    }

    git(&git_client, ["fetch", "--prune", "origin", "main"]);
    run_zmin(&zmin_client, ["fetch", "--prune", "origin", "main"]);

    assert_eq!(
        git(&zmin_client, ["rev-parse", "origin/extrabranch"]),
        git(&git_client, ["rev-parse", "origin/extrabranch"])
    );
}

#[test]
fn fetch_prune_explicit_refspec_prunes_only_destination_namespace_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    for client in [&git_client, &zmin_client] {
        git(
            client,
            ["update-ref", "refs/remotes/origin/foo/otherbranch", "main"],
        );
        git(
            client,
            ["update-ref", "refs/remotes/origin/extrabranch", "main"],
        );
    }

    git(
        &git_client,
        [
            "fetch",
            "--prune",
            "origin",
            "refs/heads/foo/*:refs/remotes/origin/foo/*",
        ],
    );
    run_zmin(
        &zmin_client,
        [
            "fetch",
            "--prune",
            "origin",
            "refs/heads/foo/*:refs/remotes/origin/foo/*",
        ],
    );

    assert_eq!(
        git_status(
            &zmin_client,
            ["rev-parse", "refs/remotes/origin/foo/otherbranch"]
        ),
        git_status(
            &git_client,
            ["rev-parse", "refs/remotes/origin/foo/otherbranch"]
        )
    );
    assert_eq!(
        git(&zmin_client, ["rev-parse", "origin/extrabranch"]),
        git(&git_client, ["rev-parse", "origin/extrabranch"])
    );
}

#[test]
fn push_set_upstream_updates_remote_tracking_ref_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );
    let zmin_client = dir.path().join("zmin-client");
    configure_identity(&zmin_client);

    run_zmin(&zmin_client, ["switch", "-c", "other"]);
    fs::write(zmin_client.join("other"), b"other\n").expect("write other");
    run_zmin(&zmin_client, ["add", "-A"]);
    run_zmin_with_env(&zmin_client, ["commit", "-m", "other"]);
    run_zmin(&zmin_client, ["push", "-u", "origin", "other"]);

    assert_eq!(
        git(&zmin_client, ["rev-parse", "refs/remotes/origin/other"]),
        git(&source, ["rev-parse", "refs/heads/other"])
    );
}

#[test]
fn fetch_local_remote_does_not_copy_unreachable_objects() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"main\n").expect("write main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    let output = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(&source)
        .output()
        .expect("hash unreachable object");
    assert!(
        output.status.success(),
        "hash unreachable object failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let unreachable = String::from_utf8(output.stdout)
        .expect("unreachable oid utf8")
        .trim()
        .to_owned();

    run_zmin(dir.path(), ["init", "-b", "main", "client"]);
    run_zmin(
        &client,
        [
            "remote",
            "add",
            "origin",
            source.to_str().expect("source path"),
        ],
    );
    run_zmin(&client, ["fetch", "origin"]);

    assert_eq!(
        git_status_args(&client, &["cat-file", "-e", &unreachable]),
        1
    );
    assert_eq!(
        git(
            &client,
            ["cat-file", "-p", "refs/remotes/origin/main:README.md"]
        ),
        "main"
    );
}

#[test]
fn fetch_tags_from_direct_file_url_without_configured_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("foo"), b"foo\n").expect("write foo");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "foo"]);
    git(&source, ["tag", "foo"]);

    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            git_client.to_str().expect("git client path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            zmin_client.to_str().expect("zmin client path"),
        ],
    );

    let source_url = format!("file://{}", source.display());
    git(&git_client, ["fetch", "--tags", &source_url]);
    run_zmin(&zmin_client, ["fetch", "--tags", &source_url]);

    assert_eq!(run_zmin(&zmin_client, ["tag"]), git(&git_client, ["tag"]));
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert_eq!(
        git_status_args(
            &zmin_client,
            &["show-ref", "--verify", "refs/remotes/origin/main"]
        ),
        git_status_args(
            &git_client,
            &["show-ref", "--verify", "refs/remotes/origin/main"]
        )
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "refs/tags/foo:foo"]),
        git(&git_client, ["cat-file", "-p", "refs/tags/foo:foo"])
    );
}

#[test]
fn fetch_tags_rejects_clobber_but_fetches_non_conflicting_tags_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let base = dir.path().join("base");
    let repo = dir.path().join("repo");

    run_zmin(
        dir.path(),
        ["init", "-b", "main", base.to_str().expect("base path")],
    );
    configure_identity(&base);
    run_zmin_with_env(&base, ["commit", "--allow-empty", "-m", "empty"]);
    run_zmin(&base, ["tag", "tag-1"]);
    run_zmin(
        dir.path(),
        [
            "clone",
            "--bare",
            base.to_str().expect("base path"),
            repo.to_str().expect("repo path"),
        ],
    );

    run_zmin(&base, ["tag", "tag-2"]);
    run_zmin_with_env(&base, ["commit", "--allow-empty", "-m", "second"]);
    run_zmin(&base, ["tag", "-f", "tag-1"]);

    let (status, _, stderr) = run_zmin_failure_output(&repo, &["fetch", "--tags"]);

    assert_eq!(status, 1);
    assert!(stderr.contains("tag-1  (would clobber existing tag)"));
    assert!(run_zmin(&repo, ["for-each-ref"]).contains("refs/tags/tag-2"));
}

#[test]
fn fetch_direct_location_head_refspec_follows_tags_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("foo"), b"foo\n").expect("write foo");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "foo"]);
    git(&source, ["tag", "-a", "-m", "annotated", "anno", "HEAD"]);
    git(&source, ["tag", "light", "HEAD"]);

    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            git_client.to_str().expect("git client path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            zmin_client.to_str().expect("zmin client path"),
        ],
    );

    git(
        &git_client,
        ["fetch", source.to_str().expect("source path"), ":track"],
    );
    run_zmin(
        &zmin_client,
        ["fetch", source.to_str().expect("source path"), ":track"],
    );

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
}

#[test]
fn fetch_direct_location_branch_into_bare_writes_fetch_head_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_dest = dir.path().join("git-dest");
    let zmin_dest = dir.path().join("zmin-dest");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("base"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    git(&source, ["checkout", "--orphan", "onebranch"]);
    git(&source, ["rm", "--cached", "-r", "."]);
    fs::write(source.join("onebranch"), b"one\n").expect("write onebranch");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "onebranch"]);

    git(
        dir.path(),
        ["init", "--bare", git_dest.to_str().expect("git dest path")],
    );
    git(
        dir.path(),
        [
            "init",
            "--bare",
            zmin_dest.to_str().expect("zmin dest path"),
        ],
    );

    git(
        &git_dest,
        ["fetch", source.to_str().expect("source path"), "onebranch"],
    );
    run_zmin(
        &zmin_dest,
        ["fetch", source.to_str().expect("source path"), "onebranch"],
    );

    assert_eq!(
        git(&zmin_dest, ["rev-parse", "--verify", "FETCH_HEAD"]),
        git(&git_dest, ["rev-parse", "--verify", "FETCH_HEAD"])
    );
    assert_eq!(
        fs::read_to_string(zmin_dest.join("FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_dest.join("FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert_eq!(
        git_status_args(
            &zmin_dest,
            &["show-ref", "--verify", "refs/heads/onebranch"]
        ),
        git_status_args(&git_dest, &["show-ref", "--verify", "refs/heads/onebranch"])
    );
}

#[test]
fn fetch_direct_location_branch_unpack_limit_trumps_transfer_limit_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let packed_dest = source.join("packed-dest");
    let loose_dest = source.join("loose-dest");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("base"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    git(&source, ["checkout", "--orphan", "onebranch"]);
    git(&source, ["rm", "--cached", "-r", "."]);
    fs::write(source.join("onebranch"), b"one\n").expect("write onebranch");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "onebranch"]);

    for (dest, fetch_limit, transfer_limit, expected) in [
        (&packed_dest, "1", "10000", "count: 0"),
        (&loose_dest, "10000", "1", "packs: 0"),
    ] {
        run_zmin(
            &source,
            [
                "--bare",
                "init",
                dest.file_name().unwrap().to_str().unwrap(),
            ],
        );
        run_zmin(dest, ["config", "fetch.unpacklimit", fetch_limit]);
        run_zmin(dest, ["config", "transfer.unpacklimit", transfer_limit]);
        run_zmin(dest, ["fetch", "..", "onebranch"]);

        let count = run_zmin(dest, ["count-objects", "-v"]);
        assert!(
            count.lines().any(|line| line == expected),
            "expected {expected:?} in count-objects output:\n{count}"
        );
    }
}

#[test]
fn fetch_named_local_remote_in_bare_updates_remote_head_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_dest = dir.path().join("git-dest");
    let zmin_dest = dir.path().join("zmin-dest");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write file");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    git(
        dir.path(),
        ["init", "--bare", git_dest.to_str().expect("git dest path")],
    );
    git(
        dir.path(),
        [
            "init",
            "--bare",
            zmin_dest.to_str().expect("zmin dest path"),
        ],
    );

    git(
        &git_dest,
        [
            "remote",
            "add",
            "upstream",
            source.to_str().expect("source path"),
        ],
    );
    run_zmin(
        &zmin_dest,
        [
            "remote",
            "add",
            "upstream",
            source.to_str().expect("source path"),
        ],
    );
    git(&git_dest, ["fetch", "upstream"]);
    run_zmin(&zmin_dest, ["fetch", "upstream"]);

    assert_eq!(
        git(&zmin_dest, ["rev-parse", "refs/remotes/upstream/HEAD"]),
        git(&git_dest, ["rev-parse", "refs/remotes/upstream/HEAD"])
    );
    assert_eq!(
        run_zmin(
            &zmin_dest,
            ["rev-parse", "--verify", "refs/remotes/upstream/HEAD"]
        ),
        git(
            &git_dest,
            ["rev-parse", "--verify", "refs/remotes/upstream/HEAD"]
        )
    );
    assert_eq!(
        git(&zmin_dest, ["rev-parse", "refs/remotes/upstream/main"]),
        git(&git_dest, ["rev-parse", "refs/remotes/upstream/main"])
    );
}

#[test]
fn for_each_ref_in_bare_clone_reads_bare_refs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    git_with_env(&source, ["commit", "--allow-empty", "-m", "empty"]);

    git(
        dir.path(),
        [
            "clone",
            "--bare",
            source.to_str().expect("source path"),
            "git-bare",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--bare",
            source.to_str().expect("source path"),
            "zmin-bare",
        ],
    );

    assert_eq!(
        run_zmin(&dir.path().join("zmin-bare"), ["for-each-ref"]),
        git(&dir.path().join("git-bare"), ["for-each-ref"])
    );
}

#[test]
fn fetch_direct_location_resolves_short_remote_name_but_not_short_tag_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("foo"), b"foo\n").expect("write foo");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "foo"]);
    git(&source, ["tag", "-a", "-m", "annotated", "anno", "HEAD"]);
    git(&source, ["update-ref", "refs/remotes/six/HEAD", "HEAD"]);

    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            git_client.to_str().expect("git client path"),
        ],
    );
    run_zmin(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            zmin_client.to_str().expect("zmin client path"),
        ],
    );

    assert_ne!(
        run_zmin_status(
            &zmin_client,
            ["fetch", source.to_str().expect("source path"), "anno:five"],
        ),
        0
    );
    git(
        &git_client,
        ["fetch", source.to_str().expect("source path"), "six:six"],
    );
    run_zmin(
        &zmin_client,
        ["fetch", source.to_str().expect("source path"), "six:six"],
    );

    assert_eq!(
        git(&zmin_client, ["show-ref", "--heads"]),
        git(&git_client, ["show-ref", "--heads"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
}

#[test]
fn fetch_direct_location_refspec_rejects_current_branch_destination_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");

    git(
        dir.path(),
        ["init", "-b", "main", git_repo.to_str().expect("git path")],
    );
    configure_identity(&git_repo);
    fs::write(git_repo.join("file"), b"main\n").expect("write git file");
    git(&git_repo, ["add", "-A"]);
    git_with_env(&git_repo, ["commit", "-m", "main"]);
    git(&git_repo, ["branch", "side"]);

    run_zmin(
        dir.path(),
        ["init", "-b", "main", zmin_repo.to_str().expect("zmin path")],
    );
    configure_identity(&zmin_repo);
    fs::write(zmin_repo.join("file"), b"main\n").expect("write zmin file");
    run_zmin(&zmin_repo, ["add", "-A"]);
    run_zmin_with_env(&zmin_repo, ["commit", "-m", "main"]);
    run_zmin(&zmin_repo, ["branch", "side"]);

    let git_failure = git_failure_output(&git_repo, &["fetch", ".", "side:main"]);
    let zmin_failure = run_zmin_failure_output(&zmin_repo, &["fetch", ".", "side:main"]);

    assert_eq!(zmin_failure.0, git_failure.0);
    assert!(zmin_failure.2.contains("refusing to fetch into branch"));
    assert!(zmin_failure.2.contains("refs/heads/main"));
    assert_eq!(
        run_zmin_status(&zmin_repo, ["fetch", "--update-head-ok", ".", "side:main"]),
        git_status(&git_repo, ["fetch", "--update-head-ok", ".", "side:main"])
    );
}

#[test]
fn fetch_direct_location_dry_run_write_fetch_head_modes_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let zmin_repo = dir.path().join("zmin-repo");
    run_zmin(
        dir.path(),
        ["init", "-b", "main", zmin_repo.to_str().expect("zmin path")],
    );
    configure_identity(&zmin_repo);
    fs::write(zmin_repo.join("file"), b"main\n").expect("write zmin file");
    run_zmin(&zmin_repo, ["add", "-A"]);
    run_zmin_with_env(&zmin_repo, ["commit", "-m", "main"]);
    let fetch_head = zmin_repo.join(".git/FETCH_HEAD");

    let dry_run = Command::new(zmin_bin())
        .args(["fetch", "--dry-run", "."])
        .current_dir(&zmin_repo)
        .output()
        .expect("run zmin dry-run fetch");
    assert!(dry_run.status.success());
    assert!(!fetch_head.exists());
    assert!(String::from_utf8_lossy(&dry_run.stderr).contains("FETCH_HEAD"));

    let no_write = Command::new(zmin_bin())
        .args(["fetch", "--no-write-fetch-head", "."])
        .current_dir(&zmin_repo)
        .output()
        .expect("run zmin no-write-fetch-head fetch");
    assert!(no_write.status.success());
    assert!(!fetch_head.exists());
    assert!(!String::from_utf8_lossy(&no_write.stderr).contains("FETCH_HEAD"));

    let dry_run_write = Command::new(zmin_bin())
        .args(["fetch", "--dry-run", "--write-fetch-head", "."])
        .current_dir(&zmin_repo)
        .output()
        .expect("run zmin dry-run write-fetch-head fetch");
    assert!(dry_run_write.status.success());
    assert!(!fetch_head.exists());
}

#[test]
fn fetch_direct_location_lhs_refspec_disambiguation_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let server = dir.path().join("server");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", server.to_str().expect("server path")],
    );
    configure_identity(&server);
    fs::write(server.join("unwanted"), b"unwanted\n").expect("write unwanted");
    git(&server, ["add", "-A"]);
    git_with_env(&server, ["commit", "-m", "unwanted"]);
    let unwanted = git(&server, ["rev-parse", "HEAD"]);
    fs::write(server.join("wanted"), b"wanted\n").expect("write wanted");
    git(&server, ["add", "-A"]);
    git_with_env(&server, ["commit", "-m", "wanted"]);
    let wanted = git(&server, ["rev-parse", "HEAD"]);

    run_zmin(
        dir.path(),
        ["init", "-b", "main", client.to_str().expect("client path")],
    );

    git(&server, ["update-ref", "refs/heads/s", &wanted]);
    git(
        &server,
        ["update-ref", "refs/heads/refs/heads/s", &unwanted],
    );
    run_zmin(
        &client,
        [
            "fetch",
            server.to_str().expect("server path"),
            "+refs/heads/s:refs/heads/checkthis",
        ],
    );
    assert_eq!(git(&client, ["rev-parse", "checkthis"]), wanted);

    git(&server, ["update-ref", "refs/heads/q", &wanted]);
    git(
        &server,
        ["update-ref", "refs/heads/refs/heads/q", &unwanted],
    );
    run_zmin(
        &client,
        [
            "fetch",
            server.to_str().expect("server path"),
            "+refs/heads/q:refs/heads/checkthis",
        ],
    );
    assert_eq!(git(&client, ["rev-parse", "checkthis"]), wanted);

    git(&server, ["update-ref", "refs/tags/t", &wanted]);
    git(&server, ["update-ref", "refs/heads/t", &unwanted]);
    run_zmin(
        &client,
        [
            "fetch",
            server.to_str().expect("server path"),
            "+t:refs/heads/checkthis",
        ],
    );
    assert_eq!(git(&client, ["rev-parse", "checkthis"]), wanted);
}

#[test]
fn fetch_write_commit_graph_creates_split_chain_marker_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            client.to_str().expect("client path"),
        ],
    );
    fs::write(source.join("file"), b"next\n").expect("write next");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "next"]);

    run_zmin(&client, ["-c", "fetch.writeCommitGraph", "fetch", "origin"]);

    assert!(
        client
            .join(".git/objects/info/commit-graphs/commit-graph-chain")
            .is_file()
    );
    run_zmin(
        &client,
        [
            "-c",
            "fetch.writeCommitGraph=true",
            "fetch",
            "--recurse-submodules",
            "origin",
        ],
    );
}

#[test]
fn fetch_recurse_submodules_no_submodule_modes_match_stock_git() {
    let modes = [
        vec!["fetch", "--quiet", "--recurse-submodules", "origin"],
        vec!["fetch", "--quiet", "--recurse-submodules=yes", "origin"],
        vec![
            "fetch",
            "--quiet",
            "--recurse-submodules=on-demand",
            "origin",
        ],
        vec!["fetch", "--quiet", "--recurse-submodules=no", "origin"],
        vec!["fetch", "--quiet", "--no-recurse-submodules", "origin"],
    ];

    for (idx, args) in modes.iter().enumerate() {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join(format!("source-{idx}"));
        let git_client = dir.path().join(format!("git-client-{idx}"));
        let zmin_client = dir.path().join(format!("zmin-client-{idx}"));

        git(
            dir.path(),
            ["init", "-b", "main", source.to_str().expect("source path")],
        );
        configure_identity(&source);
        fs::write(source.join("file"), b"main\n").expect("write source");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", "main"]);

        git(
            dir.path(),
            [
                "clone",
                source.to_str().expect("source path"),
                git_client.to_str().expect("git client path"),
            ],
        );
        run_zmin(
            dir.path(),
            [
                "clone",
                source.to_str().expect("source path"),
                zmin_client.to_str().expect("zmin client path"),
            ],
        );

        fs::write(source.join("file"), b"next\n").expect("write next");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", "next"]);

        let git_output = command_any_output("git", &git_client, args, "git");
        let zmin_output = command_any_output(zmin_bin(), &zmin_client, args, "zmin");

        assert_eq!(zmin_output.0, git_output.0, "{args:?}");
        assert_eq!(zmin_output.1, git_output.1, "{args:?}");
        assert_eq!(zmin_output.2, git_output.2, "{args:?}");
        assert_eq!(
            git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
            git(&git_client, ["rev-parse", "refs/remotes/origin/main"]),
            "{args:?}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
            "{args:?}"
        );
    }
}

#[test]
fn fetch_server_option_local_transports_match_stock_git_noop() {
    let cases = [
        (
            "local-path-equals",
            false,
            vec!["fetch", "--quiet", "--server-option=x", "origin"],
        ),
        (
            "local-path-separate",
            false,
            vec!["fetch", "--quiet", "--server-option", "x", "origin"],
        ),
        (
            "file-url-equals",
            true,
            vec!["fetch", "--quiet", "--server-option=x", "origin"],
        ),
        (
            "file-url-separate",
            true,
            vec!["fetch", "--quiet", "--server-option", "x", "origin"],
        ),
    ];

    for (label, file_url, args) in cases {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join(format!("source-{label}"));
        let git_client = dir.path().join(format!("git-client-{label}"));
        let zmin_client = dir.path().join(format!("zmin-client-{label}"));

        git(
            dir.path(),
            ["init", "-b", "main", source.to_str().expect("source path")],
        );
        configure_identity(&source);
        fs::write(source.join("file"), b"main\n").expect("write source");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", "main"]);

        git(
            dir.path(),
            [
                "clone",
                source.to_str().expect("source path"),
                git_client.to_str().expect("git client path"),
            ],
        );
        run_zmin(
            dir.path(),
            [
                "clone",
                source.to_str().expect("source path"),
                zmin_client.to_str().expect("zmin client path"),
            ],
        );
        if file_url {
            let url = format!("file://{}", source.display());
            git(&git_client, ["remote", "set-url", "origin", &url]);
            git(&zmin_client, ["remote", "set-url", "origin", &url]);
        }

        fs::write(source.join("file"), b"next\n").expect("write next");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", "next"]);

        let git_output = command_any_output("git", &git_client, &args, "git");
        let zmin_output = command_any_output(zmin_bin(), &zmin_client, &args, "zmin");

        assert_eq!(zmin_output.0, git_output.0, "{label}");
        assert_eq!(zmin_output.1, git_output.1, "{label}");
        assert_eq!(zmin_output.2, git_output.2, "{label}");
        assert_eq!(
            git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
            git(&git_client, ["rev-parse", "refs/remotes/origin/main"]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
            "{label}"
        );
    }
}

#[test]
fn fetch_upload_pack_local_transports_use_external_command_like_stock_git() {
    let cases = [
        ("local-path-equals", false, true, false),
        ("local-path-separate", false, false, false),
        ("local-path-branch-equals", false, true, true),
        ("local-path-branch-separate", false, false, true),
        ("file-url-equals", true, true, false),
        ("file-url-separate", true, false, false),
        ("file-url-branch-equals", true, true, true),
        ("file-url-branch-separate", true, false, true),
    ];

    for (label, file_url, equals_form, explicit_branch) in cases {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join(format!("source-{label}"));
        let git_client = dir.path().join(format!("git-client-{label}"));
        let zmin_client = dir.path().join(format!("zmin-client-{label}"));
        let wrapper = dir.path().join(format!("upload-pack-{label}.sh"));
        let log = wrapper.with_extension("sh.log");

        fs::write(
            &wrapper,
            b"#!/bin/sh\nprintf 'invoked %s\\n' \"$*\" >> \"$0.log\"\nexec git-upload-pack \"$@\"\n",
        )
        .expect("write upload-pack wrapper");
        chmod_executable(&wrapper);

        git(
            dir.path(),
            ["init", "-b", "main", source.to_str().expect("source path")],
        );
        configure_identity(&source);
        fs::write(source.join("file"), b"main\n").expect("write source");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", "main"]);

        git(
            dir.path(),
            [
                "clone",
                source.to_str().expect("source path"),
                git_client.to_str().expect("git client path"),
            ],
        );
        run_zmin(
            dir.path(),
            [
                "clone",
                source.to_str().expect("source path"),
                zmin_client.to_str().expect("zmin client path"),
            ],
        );
        if file_url {
            let url = format!("file://{}", source.display());
            git(&git_client, ["remote", "set-url", "origin", &url]);
            git(&zmin_client, ["remote", "set-url", "origin", &url]);
        }

        fs::write(source.join("file"), b"next\n").expect("write next");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", "next"]);

        let wrapper_command = shell_command_path(wrapper.to_str().expect("wrapper path"));
        let args = if equals_form {
            let mut args = vec![
                "fetch".to_owned(),
                "--quiet".to_owned(),
                format!("--upload-pack={wrapper_command}"),
                "origin".to_owned(),
            ];
            if explicit_branch {
                args.push("main".to_owned());
            }
            args
        } else {
            let mut args = vec![
                "fetch".to_owned(),
                "--quiet".to_owned(),
                "--upload-pack".to_owned(),
                wrapper_command,
                "origin".to_owned(),
            ];
            if explicit_branch {
                args.push("main".to_owned());
            }
            args
        };
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();

        let git_output = command_any_output("git", &git_client, &arg_refs, "git");
        let zmin_output = command_any_output(zmin_bin(), &zmin_client, &arg_refs, "zmin");

        assert_eq!(zmin_output.0, git_output.0, "{label}");
        assert_eq!(zmin_output.1, git_output.1, "{label}");
        assert_eq!(zmin_output.2, git_output.2, "{label}");
        assert_eq!(
            git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
            git(&git_client, ["rev-parse", "refs/remotes/origin/main"]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
            "{label}"
        );
        assert_eq!(
            git(&zmin_client, ["rev-parse", "FETCH_HEAD"]),
            git(&git_client, ["rev-parse", "FETCH_HEAD"]),
            "{label}"
        );
        assert_eq!(
            git(&zmin_client, ["cat-file", "-p", "FETCH_HEAD:file"]),
            git(&git_client, ["cat-file", "-p", "FETCH_HEAD:file"]),
            "{label}"
        );
        let invocations = fs::read_to_string(&log).expect("upload-pack log");
        assert_eq!(
            invocations.lines().count(),
            2,
            "expected stock Git and Zmin to invoke upload-pack for {label}: {invocations}"
        );
    }
}

#[test]
fn fetch_upload_pack_multiple_refspecs_local_transports_match_stock_git() {
    let cases = [
        ("local-path-equals", false, true),
        ("local-path-separate", false, false),
        ("file-url-equals", true, true),
        ("file-url-separate", true, false),
    ];

    for (label, file_url, equals_form) in cases {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join(format!("source-{label}"));
        let git_client = dir.path().join(format!("git-client-{label}"));
        let zmin_client = dir.path().join(format!("zmin-client-{label}"));
        let wrapper = dir.path().join(format!("upload-pack-multi-{label}.sh"));
        let log = wrapper.with_extension("sh.log");

        fs::write(
            &wrapper,
            b"#!/bin/sh\nprintf 'invoked %s\\n' \"$*\" >> \"$0.log\"\nexec git-upload-pack \"$@\"\n",
        )
        .expect("write upload-pack wrapper");
        chmod_executable(&wrapper);

        git(
            dir.path(),
            ["init", "-b", "main", source.to_str().expect("source path")],
        );
        configure_identity(&source);
        fs::write(source.join("file"), b"main\n").expect("write source");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", "main"]);

        git(
            dir.path(),
            [
                "clone",
                source.to_str().expect("source path"),
                git_client.to_str().expect("git client path"),
            ],
        );
        run_zmin(
            dir.path(),
            [
                "clone",
                source.to_str().expect("source path"),
                zmin_client.to_str().expect("zmin client path"),
            ],
        );
        if file_url {
            let url = format!("file://{}", source.display());
            git(&git_client, ["remote", "set-url", "origin", &url]);
            git(&zmin_client, ["remote", "set-url", "origin", &url]);
        }

        fs::write(source.join("file"), b"next\n").expect("write next");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", "next"]);
        git(&source, ["switch", "-c", "feature"]);
        fs::write(source.join("feature.txt"), b"feature\n").expect("write feature");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", "feature"]);
        git(&source, ["switch", "main"]);

        let wrapper_command = shell_command_path(wrapper.to_str().expect("wrapper path"));
        let mut args = if equals_form {
            vec![
                "fetch".to_owned(),
                "--quiet".to_owned(),
                format!("--upload-pack={wrapper_command}"),
                "origin".to_owned(),
            ]
        } else {
            vec![
                "fetch".to_owned(),
                "--quiet".to_owned(),
                "--upload-pack".to_owned(),
                wrapper_command,
                "origin".to_owned(),
            ]
        };
        args.push("refs/heads/main:refs/remotes/origin/main".to_owned());
        args.push("refs/heads/feature:refs/remotes/origin/feature".to_owned());
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();

        let git_output = command_any_output("git", &git_client, &arg_refs, "git");
        let zmin_output = command_any_output(zmin_bin(), &zmin_client, &arg_refs, "zmin");

        assert_eq!(zmin_output.0, git_output.0, "{label}");
        assert_eq!(zmin_output.1, git_output.1, "{label}");
        assert_eq!(zmin_output.2, git_output.2, "{label}");
        for ref_name in ["refs/remotes/origin/main", "refs/remotes/origin/feature"] {
            assert_eq!(
                git(&zmin_client, ["rev-parse", ref_name]),
                git(&git_client, ["rev-parse", ref_name]),
                "{label}: {ref_name}"
            );
        }
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
            "{label}"
        );
        assert_eq!(
            git(
                &zmin_client,
                ["cat-file", "-p", "refs/remotes/origin/main:file"]
            ),
            git(
                &git_client,
                ["cat-file", "-p", "refs/remotes/origin/main:file"]
            ),
            "{label}"
        );
        assert_eq!(
            git(
                &zmin_client,
                ["cat-file", "-p", "refs/remotes/origin/feature:feature.txt"]
            ),
            git(
                &git_client,
                ["cat-file", "-p", "refs/remotes/origin/feature:feature.txt"]
            ),
            "{label}"
        );
        let invocations = fs::read_to_string(&log).expect("upload-pack log");
        assert_eq!(
            invocations.lines().count(),
            2,
            "expected stock Git and Zmin to invoke upload-pack for {label}: {invocations}"
        );
    }
}

#[test]
fn fetch_upload_pack_depth_local_transports_match_stock_git() {
    let cases = [
        ("local-path-equals", false, true),
        ("local-path-separate", false, false),
        ("file-url-equals", true, true),
        ("file-url-separate", true, false),
    ];

    for (label, file_url, equals_form) in cases {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join(format!("source-{label}"));
        let git_client = dir.path().join(format!("git-client-{label}"));
        let zmin_client = dir.path().join(format!("zmin-client-{label}"));
        let wrapper = dir.path().join(format!("upload-pack-depth-{label}.sh"));
        let log = wrapper.with_extension("sh.log");

        fs::write(
            &wrapper,
            b"#!/bin/sh\nprintf 'invoked %s\\n' \"$*\" >> \"$0.log\"\nexec git-upload-pack \"$@\"\n",
        )
        .expect("write upload-pack wrapper");
        chmod_executable(&wrapper);

        git(
            dir.path(),
            ["init", "-b", "main", source.to_str().expect("source path")],
        );
        configure_identity(&source);
        for n in 1..=3 {
            fs::write(source.join("file"), format!("{n}\n")).expect("write source");
            git(&source, ["add", "-A"]);
            git_with_env(&source, ["commit", "-m", &format!("c{n}")]);
        }

        git(
            dir.path(),
            [
                "init",
                "-b",
                "main",
                git_client.to_str().expect("git client"),
            ],
        );
        run_zmin(
            dir.path(),
            [
                "init",
                "-b",
                "main",
                zmin_client.to_str().expect("zmin client"),
            ],
        );
        let remote_url = if file_url {
            format!("file://{}", source.display())
        } else {
            source.to_string_lossy().into_owned()
        };
        git(&git_client, ["remote", "add", "origin", &remote_url]);
        git(&zmin_client, ["remote", "add", "origin", &remote_url]);

        let wrapper_command = shell_command_path(wrapper.to_str().expect("wrapper path"));
        let args = if equals_form {
            vec![
                "fetch".to_owned(),
                "--quiet".to_owned(),
                "--depth=1".to_owned(),
                format!("--upload-pack={wrapper_command}"),
                "origin".to_owned(),
                "main".to_owned(),
            ]
        } else {
            vec![
                "fetch".to_owned(),
                "--quiet".to_owned(),
                "--depth=1".to_owned(),
                "--upload-pack".to_owned(),
                wrapper_command,
                "origin".to_owned(),
                "main".to_owned(),
            ]
        };
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();

        let git_output = command_any_output("git", &git_client, &arg_refs, "git");
        let zmin_output = command_any_output(zmin_bin(), &zmin_client, &arg_refs, "zmin");

        assert_eq!(zmin_output.0, git_output.0, "{label}");
        assert_eq!(zmin_output.1, git_output.1, "{label}");
        assert_eq!(zmin_output.2, git_output.2, "{label}");
        let remote_main = git(&git_client, ["rev-parse", "refs/remotes/origin/main"]);
        assert_eq!(
            git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
            remote_main,
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/shallow")).expect("zmin shallow"),
            fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow"),
            "{label}"
        );
        assert_eq!(
            git(&zmin_client, ["rev-list", "--count", "FETCH_HEAD"]),
            git(&git_client, ["rev-list", "--count", "FETCH_HEAD"]),
            "{label}"
        );
        assert_eq!(
            git_status_args(
                &zmin_client,
                &["cat-file", "-e", &format!("{remote_main}^")]
            ),
            git_status_args(&git_client, &["cat-file", "-e", &format!("{remote_main}^")]),
            "{label}"
        );
        let invocations = fs::read_to_string(&log).expect("upload-pack log");
        assert_eq!(
            invocations.lines().count(),
            2,
            "expected stock Git and Zmin to invoke upload-pack for {label}: {invocations}"
        );
    }
}

#[test]
fn fetch_upload_pack_deepen_local_transports_match_stock_git() {
    let cases = [
        ("local-path-equals", false, true),
        ("local-path-separate", false, false),
        ("file-url-equals", true, true),
        ("file-url-separate", true, false),
    ];

    for (label, file_url, equals_form) in cases {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join(format!("source-{label}"));
        let git_client = dir.path().join(format!("git-client-{label}"));
        let zmin_client = dir.path().join(format!("zmin-client-{label}"));
        let wrapper = dir.path().join(format!("upload-pack-deepen-{label}.sh"));
        let log = wrapper.with_extension("sh.log");

        fs::write(
            &wrapper,
            b"#!/bin/sh\nprintf 'invoked %s\\n' \"$*\" >> \"$0.log\"\nexec git-upload-pack \"$@\"\n",
        )
        .expect("write upload-pack wrapper");
        chmod_executable(&wrapper);

        git(
            dir.path(),
            ["init", "-b", "main", source.to_str().expect("source path")],
        );
        configure_identity(&source);
        for n in 1..=4 {
            fs::write(source.join("file"), format!("{n}\n")).expect("write source");
            git(&source, ["add", "-A"]);
            git_with_env(&source, ["commit", "-m", &format!("c{n}")]);
        }

        git(
            dir.path(),
            [
                "init",
                "-b",
                "main",
                git_client.to_str().expect("git client"),
            ],
        );
        run_zmin(
            dir.path(),
            [
                "init",
                "-b",
                "main",
                zmin_client.to_str().expect("zmin client"),
            ],
        );
        let remote_url = if file_url {
            format!("file://{}", source.display())
        } else {
            source.to_string_lossy().into_owned()
        };
        git(&git_client, ["remote", "add", "origin", &remote_url]);
        git(&zmin_client, ["remote", "add", "origin", &remote_url]);
        git(
            &git_client,
            ["fetch", "--quiet", "--depth=1", "origin", "main"],
        );
        run_zmin(
            &zmin_client,
            ["fetch", "--quiet", "--depth=1", "origin", "main"],
        );

        let wrapper_command = shell_command_path(wrapper.to_str().expect("wrapper path"));
        let args = if equals_form {
            vec![
                "fetch".to_owned(),
                "--quiet".to_owned(),
                "--deepen=1".to_owned(),
                format!("--upload-pack={wrapper_command}"),
                "origin".to_owned(),
                "main".to_owned(),
            ]
        } else {
            vec![
                "fetch".to_owned(),
                "--quiet".to_owned(),
                "--deepen=1".to_owned(),
                "--upload-pack".to_owned(),
                wrapper_command,
                "origin".to_owned(),
                "main".to_owned(),
            ]
        };
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();

        let git_output = command_any_output("git", &git_client, &arg_refs, "git");
        let zmin_output = command_any_output(zmin_bin(), &zmin_client, &arg_refs, "zmin");

        assert_eq!(zmin_output.0, git_output.0, "{label}");
        assert_eq!(zmin_output.1, git_output.1, "{label}");
        assert_eq!(zmin_output.2, git_output.2, "{label}");
        assert_eq!(
            git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
            git(&git_client, ["rev-parse", "refs/remotes/origin/main"]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/shallow")).expect("zmin shallow"),
            fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow"),
            "{label}"
        );
        assert_eq!(
            git(&zmin_client, ["rev-list", "--count", "FETCH_HEAD"]),
            git(&git_client, ["rev-list", "--count", "FETCH_HEAD"]),
            "{label}"
        );
        let invocations = fs::read_to_string(&log).expect("upload-pack log");
        assert_eq!(
            invocations.lines().count(),
            2,
            "expected stock Git and Zmin to invoke upload-pack for {label}: {invocations}"
        );
    }
}

#[test]
fn fetch_upload_pack_unshallow_local_transports_match_stock_git() {
    let cases = [
        ("local-path-equals-remote", false, true, false),
        ("local-path-separate-remote", false, false, false),
        ("file-url-equals-remote", true, true, false),
        ("file-url-separate-remote", true, false, false),
        ("local-path-equals-branch", false, true, true),
        ("local-path-separate-branch", false, false, true),
        ("file-url-equals-branch", true, true, true),
        ("file-url-separate-branch", true, false, true),
    ];

    for (label, file_url, equals_form, explicit_branch) in cases {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join(format!("source-{label}"));
        let git_client = dir.path().join(format!("git-client-{label}"));
        let zmin_client = dir.path().join(format!("zmin-client-{label}"));
        let wrapper = dir.path().join(format!("upload-pack-unshallow-{label}.sh"));
        let log = wrapper.with_extension("sh.log");

        fs::write(
            &wrapper,
            b"#!/bin/sh\nprintf 'invoked %s\\n' \"$*\" >> \"$0.log\"\nexec git-upload-pack \"$@\"\n",
        )
        .expect("write upload-pack wrapper");
        chmod_executable(&wrapper);

        git(
            dir.path(),
            ["init", "-b", "main", source.to_str().expect("source path")],
        );
        configure_identity(&source);
        for n in 1..=4 {
            fs::write(source.join("file"), format!("{n}\n")).expect("write source");
            git(&source, ["add", "-A"]);
            git_with_env(&source, ["commit", "-m", &format!("c{n}")]);
        }

        git(
            dir.path(),
            [
                "init",
                "-b",
                "main",
                git_client.to_str().expect("git client"),
            ],
        );
        run_zmin(
            dir.path(),
            [
                "init",
                "-b",
                "main",
                zmin_client.to_str().expect("zmin client"),
            ],
        );
        let remote_url = if file_url {
            format!("file://{}", source.display())
        } else {
            source.to_string_lossy().into_owned()
        };
        git(&git_client, ["remote", "add", "origin", &remote_url]);
        git(&zmin_client, ["remote", "add", "origin", &remote_url]);
        git(
            &git_client,
            ["fetch", "--quiet", "--depth=1", "origin", "main"],
        );
        run_zmin(
            &zmin_client,
            ["fetch", "--quiet", "--depth=1", "origin", "main"],
        );

        let wrapper_command = shell_command_path(wrapper.to_str().expect("wrapper path"));
        let mut args = if equals_form {
            vec![
                "fetch".to_owned(),
                "--quiet".to_owned(),
                "--unshallow".to_owned(),
                format!("--upload-pack={wrapper_command}"),
                "origin".to_owned(),
            ]
        } else {
            vec![
                "fetch".to_owned(),
                "--quiet".to_owned(),
                "--unshallow".to_owned(),
                "--upload-pack".to_owned(),
                wrapper_command,
                "origin".to_owned(),
            ]
        };
        if explicit_branch {
            args.push("main".to_owned());
        }
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();

        let git_output = command_any_output("git", &git_client, &arg_refs, "git");
        let zmin_output = command_any_output(zmin_bin(), &zmin_client, &arg_refs, "zmin");

        assert_eq!(zmin_output.0, git_output.0, "{label}");
        assert_eq!(zmin_output.1, git_output.1, "{label}");
        assert_eq!(zmin_output.2, git_output.2, "{label}");
        assert_eq!(
            git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
            git(&git_client, ["rev-parse", "refs/remotes/origin/main"]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
            "{label}"
        );
        assert_eq!(
            zmin_client.join(".git/shallow").exists(),
            git_client.join(".git/shallow").exists(),
            "{label}"
        );
        assert_eq!(
            git(&zmin_client, ["rev-list", "--count", "FETCH_HEAD"]),
            git(&git_client, ["rev-list", "--count", "FETCH_HEAD"]),
            "{label}"
        );
        let invocations = fs::read_to_string(&log).expect("upload-pack log");
        assert_eq!(
            invocations.lines().count(),
            2,
            "expected stock Git and Zmin to invoke upload-pack for {label}: {invocations}"
        );
    }
}

#[test]
fn fetch_upload_pack_shallow_since_local_transports_match_stock_git() {
    let cases = [
        ("local-path-upload-equals-since-equals", false, true, true),
        (
            "local-path-upload-separate-since-equals",
            false,
            false,
            true,
        ),
        ("file-url-upload-equals-since-equals", true, true, true),
        ("file-url-upload-separate-since-equals", true, false, true),
        (
            "local-path-upload-equals-since-separate",
            false,
            true,
            false,
        ),
        (
            "local-path-upload-separate-since-separate",
            false,
            false,
            false,
        ),
        ("file-url-upload-equals-since-separate", true, true, false),
        (
            "file-url-upload-separate-since-separate",
            true,
            false,
            false,
        ),
    ];

    for (label, file_url, upload_equals_form, since_equals_form) in cases {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join(format!("source-{label}"));
        let git_client = dir.path().join(format!("git-client-{label}"));
        let zmin_client = dir.path().join(format!("zmin-client-{label}"));
        let wrapper = dir
            .path()
            .join(format!("upload-pack-shallow-since-{label}.sh"));
        let log = wrapper.with_extension("sh.log");

        fs::write(
            &wrapper,
            b"#!/bin/sh\nprintf 'invoked %s\\n' \"$*\" >> \"$0.log\"\nexec git-upload-pack \"$@\"\n",
        )
        .expect("write upload-pack wrapper");
        chmod_executable(&wrapper);

        git(
            dir.path(),
            ["init", "-b", "main", source.to_str().expect("source path")],
        );
        configure_identity(&source);
        for idx in 1..=4 {
            fs::write(source.join("file"), format!("{idx}\n")).expect("write source");
            git(&source, ["add", "-A"]);
            let date = format!("2020-01-0{idx}T00:00:00 +0000");
            let env = [
                ("GIT_AUTHOR_DATE", date.as_str()),
                ("GIT_COMMITTER_DATE", date.as_str()),
            ];
            command_output_with_env(
                "git",
                &source,
                &["commit", "-m", &format!("c{idx}")],
                &env,
                "git",
            );
        }

        git(
            dir.path(),
            [
                "init",
                "-b",
                "main",
                git_client.to_str().expect("git client"),
            ],
        );
        run_zmin(
            dir.path(),
            [
                "init",
                "-b",
                "main",
                zmin_client.to_str().expect("zmin client"),
            ],
        );
        let remote_url = if file_url {
            format!("file://{}", source.display())
        } else {
            source.to_string_lossy().into_owned()
        };
        git(&git_client, ["remote", "add", "origin", &remote_url]);
        git(&zmin_client, ["remote", "add", "origin", &remote_url]);

        let cutoff = "2020-01-03T00:00:00 +0000";
        let wrapper_command = shell_command_path(wrapper.to_str().expect("wrapper path"));
        let mut args = vec!["fetch".to_owned(), "--quiet".to_owned()];
        if since_equals_form {
            args.push(format!("--shallow-since={cutoff}"));
        } else {
            args.push("--shallow-since".to_owned());
            args.push(cutoff.to_owned());
        }
        if upload_equals_form {
            args.push(format!("--upload-pack={wrapper_command}"));
        } else {
            args.push("--upload-pack".to_owned());
            args.push(wrapper_command);
        }
        args.push("origin".to_owned());
        args.push("main".to_owned());
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();

        let git_output = command_any_output("git", &git_client, &arg_refs, "git");
        let zmin_output = command_any_output(zmin_bin(), &zmin_client, &arg_refs, "zmin");

        assert_eq!(zmin_output.0, git_output.0, "{label}");
        assert_eq!(zmin_output.1, git_output.1, "{label}");
        assert_eq!(zmin_output.2, git_output.2, "{label}");
        assert_eq!(
            git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
            git(&git_client, ["rev-parse", "refs/remotes/origin/main"]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/shallow")).expect("zmin shallow"),
            fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow"),
            "{label}"
        );
        assert_eq!(
            git(&zmin_client, ["rev-list", "--count", "FETCH_HEAD"]),
            git(&git_client, ["rev-list", "--count", "FETCH_HEAD"]),
            "{label}"
        );
        let invocations = fs::read_to_string(&log).expect("upload-pack log");
        assert_eq!(
            invocations.lines().count(),
            2,
            "expected stock Git and Zmin to invoke upload-pack for {label}: {invocations}"
        );
    }
}

#[test]
fn fetch_upload_pack_shallow_exclude_local_transports_match_stock_git() {
    let cases = [
        ("local-path-upload-equals-exclude-equals", false, true, true),
        (
            "local-path-upload-separate-exclude-equals",
            false,
            false,
            true,
        ),
        ("file-url-upload-equals-exclude-equals", true, true, true),
        ("file-url-upload-separate-exclude-equals", true, false, true),
        (
            "local-path-upload-equals-exclude-separate",
            false,
            true,
            false,
        ),
        (
            "local-path-upload-separate-exclude-separate",
            false,
            false,
            false,
        ),
        ("file-url-upload-equals-exclude-separate", true, true, false),
        (
            "file-url-upload-separate-exclude-separate",
            true,
            false,
            false,
        ),
    ];

    for (label, file_url, upload_equals_form, exclude_equals_form) in cases {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join(format!("source-{label}"));
        let git_client = dir.path().join(format!("git-client-{label}"));
        let zmin_client = dir.path().join(format!("zmin-client-{label}"));
        let wrapper = dir
            .path()
            .join(format!("upload-pack-shallow-exclude-{label}.sh"));
        let log = wrapper.with_extension("sh.log");

        fs::write(
            &wrapper,
            b"#!/bin/sh\nprintf 'invoked %s\\n' \"$*\" >> \"$0.log\"\nexec git-upload-pack \"$@\"\n",
        )
        .expect("write upload-pack wrapper");
        chmod_executable(&wrapper);

        git(
            dir.path(),
            ["init", "-b", "main", source.to_str().expect("source path")],
        );
        configure_identity(&source);
        for name in ["base 1", "base 2"] {
            fs::write(source.join("file"), format!("{name}\n")).expect("write source");
            git(&source, ["add", "-A"]);
            git_with_env(&source, ["commit", "-m", name]);
        }
        git(&source, ["branch", "base"]);
        for name in ["main 3", "main 4"] {
            fs::write(source.join("file"), format!("{name}\n")).expect("write source");
            git(&source, ["add", "-A"]);
            git_with_env(&source, ["commit", "-m", name]);
        }

        git(
            dir.path(),
            [
                "init",
                "-b",
                "main",
                git_client.to_str().expect("git client"),
            ],
        );
        run_zmin(
            dir.path(),
            [
                "init",
                "-b",
                "main",
                zmin_client.to_str().expect("zmin client"),
            ],
        );
        let remote_url = if file_url {
            format!("file://{}", source.display())
        } else {
            source.to_string_lossy().into_owned()
        };
        git(&git_client, ["remote", "add", "origin", &remote_url]);
        git(&zmin_client, ["remote", "add", "origin", &remote_url]);

        let wrapper_command = shell_command_path(wrapper.to_str().expect("wrapper path"));
        let mut args = vec!["fetch".to_owned(), "--quiet".to_owned()];
        if exclude_equals_form {
            args.push("--shallow-exclude=refs/heads/base".to_owned());
        } else {
            args.push("--shallow-exclude".to_owned());
            args.push("refs/heads/base".to_owned());
        }
        if upload_equals_form {
            args.push(format!("--upload-pack={wrapper_command}"));
        } else {
            args.push("--upload-pack".to_owned());
            args.push(wrapper_command);
        }
        args.push("origin".to_owned());
        args.push("main".to_owned());
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();

        let git_output = command_any_output("git", &git_client, &arg_refs, "git");
        let zmin_output = command_any_output(zmin_bin(), &zmin_client, &arg_refs, "zmin");

        assert_eq!(zmin_output.0, git_output.0, "{label}");
        assert_eq!(zmin_output.1, git_output.1, "{label}");
        assert_eq!(zmin_output.2, git_output.2, "{label}");
        assert_eq!(
            git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
            git(&git_client, ["rev-parse", "refs/remotes/origin/main"]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/shallow")).expect("zmin shallow"),
            fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow"),
            "{label}"
        );
        assert_eq!(
            git(&zmin_client, ["rev-list", "--count", "FETCH_HEAD"]),
            git(&git_client, ["rev-list", "--count", "FETCH_HEAD"]),
            "{label}"
        );
        let invocations = fs::read_to_string(&log).expect("upload-pack log");
        assert_eq!(
            invocations.lines().count(),
            2,
            "expected stock Git and Zmin to invoke upload-pack for {label}: {invocations}"
        );
    }
}

#[test]
fn fetch_uses_first_configured_remote_url_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let url1 = dir.path().join("url1");
    let url2 = dir.path().join("url2");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", url1.to_str().expect("url1 path")],
    );
    run_zmin(
        dir.path(),
        ["init", "-b", "main", client.to_str().expect("client path")],
    );
    run_zmin(
        &client,
        [
            "remote",
            "add",
            "multipleurls",
            url1.to_str().expect("url1 path"),
        ],
    );
    run_zmin(
        &client,
        [
            "remote",
            "set-url",
            "--add",
            "multipleurls",
            url2.to_str().expect("url2 path"),
        ],
    );

    run_zmin(&client, ["fetch", "multipleurls"]);
}

#[test]
fn fetch_direct_file_url_wildcard_refspec_updates_refs_and_tags_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["branch", "-f", "newbranch"]);
    git(&source, ["tag", "-f", "newtag"]);

    run_zmin(
        dir.path(),
        ["init", "-b", "main", client.to_str().expect("client path")],
    );
    let source_url = format!("file://{}", source.display());
    run_zmin(
        &client,
        ["fetch", &source_url, "+refs/heads/*:refs/remotes/origin/*"],
    );

    assert_eq!(
        git(&client, ["rev-parse", "refs/remotes/origin/newbranch"]),
        git(&source, ["rev-parse", "refs/heads/newbranch"])
    );
    assert_eq!(
        git(&client, ["rev-parse", "refs/tags/newtag"]),
        git(&source, ["rev-parse", "refs/tags/newtag"])
    );
}

#[test]
fn fetch_no_prune_option_is_accepted_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            client.to_str().expect("client path"),
        ],
    );
    run_zmin(&client, ["fetch", "--no-prune", "origin"]);
    run_zmin(&client, ["fetch", "--prune-tags", "origin"]);
}

#[test]
fn fetch_no_tags_option_skips_auto_followed_tags_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["tag", "v1"]);

    git(
        dir.path(),
        ["init", "-b", "main", git_client.to_str().expect("git path")],
    );
    run_zmin(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            zmin_client.to_str().expect("zmin path"),
        ],
    );
    git(
        &git_client,
        [
            "remote",
            "add",
            "origin",
            source.to_str().expect("source path"),
        ],
    );
    run_zmin(
        &zmin_client,
        [
            "remote",
            "add",
            "origin",
            source.to_str().expect("source path"),
        ],
    );

    let refspec = "+refs/heads/*:refs/remotes/origin/*";
    git(&git_client, ["fetch", "origin", refspec, "--no-tags"]);
    run_zmin(&zmin_client, ["fetch", "origin", refspec, "--no-tags"]);

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(git_status_args(&git_client, &["show-ref", "--tags"]), 1);
    assert_eq!(git_status_args(&zmin_client, &["show-ref", "--tags"]), 1);
}

#[test]
fn fetch_prune_config_prunes_stale_remote_tracking_refs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["branch", "-f", "gone"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            client.to_str().expect("client path"),
        ],
    );
    git(&source, ["branch", "-D", "gone"]);
    run_zmin(&client, ["config", "fetch.prune", "true"]);
    run_zmin(&client, ["fetch", "origin"]);

    assert_eq!(
        git_status_args(
            &client,
            &["rev-parse", "--verify", "refs/remotes/origin/gone"]
        ),
        128
    );
}

#[test]
fn fetch_prune_only_prints_remote_url_header_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["branch", "-f", "goodbye"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            client.to_str().expect("client path"),
        ],
    );
    git(&source, ["branch", "-D", "goodbye"]);
    let output = Command::new(zmin_bin())
        .args(["fetch", "--prune", "origin"])
        .current_dir(&client)
        .output()
        .expect("zmin fetch --prune output");
    assert!(
        output.status.success(),
        "fetch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let first_line = String::from_utf8(output.stderr)
        .expect("stderr utf8")
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();

    assert_eq!(first_line, format!("From {}", source.display()));
}

#[test]
fn fetch_prune_resolves_remote_tracking_directory_file_conflict_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["branch", "dir/file"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            client.to_str().expect("client path"),
        ],
    );
    git(&source, ["branch", "-D", "dir/file"]);
    git(&source, ["branch", "dir"]);
    run_zmin(&client, ["fetch", "--prune"]);

    assert_eq!(
        git(&client, ["rev-parse", "refs/remotes/origin/dir"]),
        git(&source, ["rev-parse", "dir"])
    );
    assert_eq!(
        git_status_args(
            &client,
            &["rev-parse", "--verify", "refs/remotes/origin/dir/file"]
        ),
        128
    );
}

#[test]
fn fetch_rejects_remote_tracking_directory_file_conflict_with_prune_hint_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            client.to_str().expect("client path"),
        ],
    );
    git(&source, ["branch", "dir-conflict"]);
    run_zmin(
        &client,
        [
            "update-ref",
            "refs/remotes/origin/dir-conflict/file",
            "HEAD",
        ],
    );
    let loose = run_zmin_failure_output(&client, &["fetch"]);
    assert!(
        loose
            .2
            .contains("error: some local refs could not be updated; try running")
    );
    assert!(
        loose
            .2
            .contains("'git remote prune origin' to remove any old, conflicting branches")
    );

    run_zmin(&client, ["pack-refs", "--all"]);
    let packed = run_zmin_failure_output(&client, &["fetch"]);
    assert!(
        packed
            .2
            .contains("error: some local refs could not be updated; try running")
    );
    assert!(
        packed
            .2
            .contains("'git remote prune origin' to remove any old, conflicting branches")
    );
}

#[test]
fn fetch_verbose_prints_auto_gc_message_when_auto_pack_limit_is_enabled_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            client.to_str().expect("client path"),
        ],
    );
    fs::write(source.join("file"), b"next\n").expect("write next");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "next"]);
    run_zmin(&client, ["config", "gc.autoPackLimit", "1"]);
    let output = Command::new(zmin_bin())
        .args(["fetch", "--verbose"])
        .current_dir(&client)
        .output()
        .expect("zmin fetch --verbose");
    assert!(
        output.status.success(),
        "fetch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr utf8")
            .contains("Auto packing the repository")
    );
}

#[test]
fn fetch_hidden_refs_writes_rev_list_trace_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            client.to_str().expect("client path"),
        ],
    );
    let trace = dir.path().join("trace");
    let output = Command::new(zmin_bin())
        .args([
            "-c",
            "fetch.hideRefs=refs",
            "-c",
            "fetch.hideRefs=!refs/tags/",
            "fetch",
        ])
        .env("GIT_TRACE", &trace)
        .current_dir(&client)
        .output()
        .expect("zmin fetch with hidden refs");
    assert!(
        output.status.success(),
        "fetch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fs::read_to_string(trace)
            .expect("read trace")
            .contains("git rev-list --exclude-hidden=fetch")
    );
}

#[test]
fn fetch_prune_tags_with_prune_removes_stale_tags_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["branch", "-f", "gone"]);
    git(&source, ["tag", "-f", "gone-tag"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            client.to_str().expect("client path"),
        ],
    );
    git(&source, ["branch", "-D", "gone"]);
    git(&source, ["tag", "-d", "gone-tag"]);
    run_zmin(&client, ["fetch", "--prune", "--prune-tags", "origin"]);

    assert_eq!(
        git_status_args(
            &client,
            &["rev-parse", "--verify", "refs/remotes/origin/gone"]
        ),
        128
    );
    assert_eq!(
        git_status_args(&client, &["rev-parse", "--verify", "refs/tags/gone-tag"]),
        128
    );
}

#[test]
fn fetch_prune_tags_config_with_prune_config_removes_stale_tags_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["branch", "-f", "gone"]);
    git(&source, ["tag", "-f", "gone-tag"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            client.to_str().expect("client path"),
        ],
    );
    git(&source, ["branch", "-D", "gone"]);
    git(&source, ["tag", "-d", "gone-tag"]);
    run_zmin(&client, ["config", "fetch.prune", "true"]);
    run_zmin(&client, ["config", "fetch.pruneTags", "true"]);
    run_zmin(&client, ["fetch", "origin"]);

    assert_eq!(
        git_status_args(
            &client,
            &["rev-parse", "--verify", "refs/remotes/origin/gone"]
        ),
        128
    );
    assert_eq!(
        git_status_args(&client, &["rev-parse", "--verify", "refs/tags/gone-tag"]),
        128
    );
}

#[test]
fn fetch_direct_file_url_prune_tags_prunes_tags_but_keeps_remote_tracking_refs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["branch", "-f", "gone"]);
    git(&source, ["tag", "-f", "gone-tag"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            client.to_str().expect("client path"),
        ],
    );
    let source_url = format!("file://{}", source.display());
    run_zmin(
        &client,
        ["fetch", &source_url, "+refs/heads/*:refs/remotes/origin/*"],
    );
    git(&source, ["branch", "-D", "gone"]);
    git(&source, ["tag", "-d", "gone-tag"]);
    run_zmin(&client, ["fetch", &source_url, "--prune", "--prune-tags"]);

    assert_eq!(
        git_status_args(
            &client,
            &["rev-parse", "--verify", "refs/remotes/origin/gone"]
        ),
        0
    );
    assert_eq!(
        git_status_args(&client, &["rev-parse", "--verify", "refs/tags/gone-tag"]),
        128
    );
}

#[test]
fn fetch_named_remote_accepts_multiple_explicit_refspecs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["branch", "-f", "newbranch"]);
    git(&source, ["tag", "-f", "newtag"]);

    run_zmin(
        dir.path(),
        ["init", "-b", "main", client.to_str().expect("client path")],
    );
    run_zmin(
        &client,
        [
            "remote",
            "add",
            "origin",
            source.to_str().expect("source path"),
        ],
    );
    run_zmin(
        &client,
        [
            "fetch",
            "--prune",
            "origin",
            "refs/tags/*:refs/tags/*",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    );

    assert_eq!(
        git(&client, ["rev-parse", "refs/remotes/origin/newbranch"]),
        git(&source, ["rev-parse", "refs/heads/newbranch"])
    );
    assert_eq!(
        git(&client, ["rev-parse", "refs/tags/newtag"]),
        git(&source, ["rev-parse", "refs/tags/newtag"])
    );
}

#[test]
fn fetch_stdin_refspecs_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("main.txt"), b"main\n").expect("write main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["branch", "feature"]);

    for client in [&git_client, &zmin_client] {
        git(
            dir.path(),
            ["init", "-b", "main", client.to_str().expect("client path")],
        );
        git(
            client,
            [
                "remote",
                "add",
                "origin",
                source.to_str().expect("source path"),
            ],
        );
    }

    let stdin_refspecs =
        b"refs/heads/main:refs/remotes/origin/main\nrefs/heads/feature:refs/remotes/origin/feature\n";
    let mut git_child = Command::new("git")
        .args(["fetch", "--stdin", "origin"])
        .current_dir(&git_client)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("spawn git fetch --stdin");
    git_child
        .stdin
        .as_mut()
        .expect("git stdin")
        .write_all(stdin_refspecs)
        .expect("write git stdin");
    let git_status = git_child.wait().expect("git fetch --stdin status");
    assert!(
        git_status.success(),
        "git fetch --stdin failed: {git_status}"
    );

    let mut zmin_child = Command::new(zmin_bin())
        .args(["fetch", "--stdin", "origin"])
        .current_dir(&zmin_client)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("spawn zmin fetch --stdin");
    zmin_child
        .stdin
        .as_mut()
        .expect("zmin stdin")
        .write_all(stdin_refspecs)
        .expect("write zmin stdin");
    let zmin_status = zmin_child.wait().expect("zmin fetch --stdin status");
    assert!(
        zmin_status.success(),
        "zmin fetch --stdin failed: {zmin_status}"
    );

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
}

#[test]
fn fetch_depth_named_remote_accepts_multiple_explicit_refspecs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("main.txt"), b"main base\n").expect("write main base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main base"]);
    let main_parent = git(&source, ["rev-parse", "HEAD"]);
    fs::write(source.join("main.txt"), b"main tip\n").expect("write main tip");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main tip"]);
    git(&source, ["switch", "-c", "feature", &main_parent]);
    fs::write(source.join("feature.txt"), b"feature base\n").expect("write feature base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "feature base"]);
    let feature_parent = git(&source, ["rev-parse", "HEAD"]);
    fs::write(source.join("feature.txt"), b"feature tip\n").expect("write feature tip");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "feature tip"]);
    git(&source, ["tag", "-f", "feature-tip"]);

    let source_url = format!("file://{}", source.display());
    for client in [&git_client, &zmin_client] {
        git(
            dir.path(),
            ["init", "-b", "main", client.to_str().expect("client path")],
        );
        git(client, ["remote", "add", "origin", &source_url]);
    }

    let args = [
        "fetch",
        "--depth=1",
        "origin",
        "refs/heads/main:refs/remotes/origin/main",
        "refs/heads/feature:refs/remotes/origin/feature",
    ];
    git(&git_client, args);
    run_zmin(&zmin_client, args);

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/shallow")).expect("zmin shallow"),
        fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow")
    );
    for parent in [main_parent, feature_parent] {
        assert_eq!(
            git_status_args(&zmin_client, &["cat-file", "-e", &parent]),
            git_status_args(&git_client, &["cat-file", "-e", &parent])
        );
    }
}

#[test]
fn fetch_depth_explicit_file_url_branch_writes_fetch_head_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file.txt"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    let parent = git(&source, ["rev-parse", "HEAD"]);
    fs::write(source.join("file.txt"), b"tip\n").expect("write tip");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "tip"]);

    git(dir.path(), ["init", "-b", "main", "git-client"]);
    run_zmin(dir.path(), ["init", "-b", "main", "zmin-client"]);

    let source_url = format!("file://{}", source.display());
    git(&git_client, ["fetch", "--depth=1", &source_url, "main"]);
    run_zmin(&zmin_client, ["fetch", "--depth=1", &source_url, "main"]);

    assert_eq!(
        git(&zmin_client, ["rev-parse", "FETCH_HEAD"]),
        git(&git_client, ["rev-parse", "FETCH_HEAD"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert_matching_depth_fetch_state(&zmin_client, &git_client, &parent);
}

#[test]
fn fetch_depth_explicit_file_url_refspec_updates_ref_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file.txt"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    let parent = git(&source, ["rev-parse", "HEAD"]);
    fs::write(source.join("file.txt"), b"tip\n").expect("write tip");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "tip"]);

    git(dir.path(), ["init", "-b", "main", "git-client"]);
    run_zmin(dir.path(), ["init", "-b", "main", "zmin-client"]);

    let source_url = format!("file://{}", source.display());
    let refspec = "refs/heads/main:refs/remotes/origin/main";
    git(&git_client, ["fetch", "--depth=1", &source_url, refspec]);
    run_zmin(&zmin_client, ["fetch", "--depth=1", &source_url, refspec]);

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "origin/main:file.txt"]),
        git(&git_client, ["cat-file", "-p", "origin/main:file.txt"])
    );
    assert_matching_depth_fetch_state(&zmin_client, &git_client, &parent);
}

#[test]
fn fetch_depth_explicit_file_url_multiple_refspecs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("main.txt"), b"main base\n").expect("write main base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main base"]);
    let main_parent = git(&source, ["rev-parse", "HEAD"]);
    fs::write(source.join("main.txt"), b"main tip\n").expect("write main tip");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main tip"]);
    git(&source, ["switch", "-c", "feature", &main_parent]);
    fs::write(source.join("feature.txt"), b"feature base\n").expect("write feature base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "feature base"]);
    let feature_parent = git(&source, ["rev-parse", "HEAD"]);
    fs::write(source.join("feature.txt"), b"feature tip\n").expect("write feature tip");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "feature tip"]);

    git(dir.path(), ["init", "-b", "main", "git-client"]);
    run_zmin(dir.path(), ["init", "-b", "main", "zmin-client"]);

    let source_url = format!("file://{}", source.display());
    let args = [
        "fetch",
        "--depth=1",
        &source_url,
        "refs/heads/main:refs/remotes/origin/main",
        "refs/heads/feature:refs/remotes/origin/feature",
    ];
    git(&git_client, args);
    run_zmin(&zmin_client, args);

    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/shallow")).expect("zmin shallow"),
        fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow")
    );
    for parent in [main_parent, feature_parent] {
        assert_eq!(
            git_status_args(&zmin_client, &["cat-file", "-e", &parent]),
            git_status_args(&git_client, &["cat-file", "-e", &parent])
        );
    }
}

#[test]
fn fetch_depth_bundle_multiple_explicit_refspecs_ignores_depth_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    let bundle = dir.path().join("repo.bundle");
    let bundle_path = bundle.to_str().expect("bundle path");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("main.txt"), b"main\n").expect("write main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["switch", "-c", "feature"]);
    fs::write(source.join("feature.txt"), b"feature\n").expect("write feature");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "feature"]);
    git(&source, ["switch", "main"]);
    git(&source, ["bundle", "create", bundle_path, "--all"]);

    git(dir.path(), ["init", "-b", "main", "git-client"]);
    run_zmin(dir.path(), ["init", "-b", "main", "zmin-client"]);

    let args = [
        "fetch",
        "--depth=1",
        bundle_path,
        "refs/heads/main:refs/remotes/origin/main",
        "refs/heads/feature:refs/remotes/origin/feature",
    ];
    let git_output = command_any_output("git", &git_client, &args, "git");
    let zmin_output = command_any_output(zmin_bin(), &zmin_client, &args, "zmin");

    assert_eq!(zmin_output.0, git_output.0);
    assert!(
        git_output
            .2
            .contains("warning: option \"depth\" is ignored"),
        "{}",
        git_output.2
    );
    assert!(
        zmin_output
            .2
            .contains("warning: option \"depth\" is ignored"),
        "{}",
        zmin_output.2
    );
    assert_eq!(
        git(&zmin_client, ["show-ref"]),
        git(&git_client, ["show-ref"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert!(!zmin_client.join(".git/shallow").exists());
    assert!(!git_client.join(".git/shallow").exists());
}

#[test]
fn fetch_explicit_head_to_branch_backfills_tags_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let target = dir.path().join("target");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    git_with_env(&source, ["commit", "--allow-empty", "-m", "common"]);
    run_zmin(
        dir.path(),
        [
            "clone",
            &format!("file://{}", source.display()),
            target.to_str().expect("target path"),
        ],
    );

    fs::write(source.join("history.t"), b"history\n").expect("write history");
    git(&source, ["add", "history.t"]);
    git_with_env(&source, ["commit", "-m", "history"]);
    git(&source, ["tag", "history"]);
    fs::write(source.join("fetch-me.t"), b"fetch-me\n").expect("write fetch-me");
    git(&source, ["add", "fetch-me.t"]);
    git_with_env(&source, ["commit", "-m", "fetch-me"]);
    git(&source, ["tag", "fetch-me"]);

    run_zmin(&target, ["fetch", "origin", "HEAD:branch"]);

    assert_eq!(git(&target, ["tag", "-l"]), "fetch-me\nhistory");
    assert_eq!(
        git(&target, ["rev-parse", "refs/heads/branch"]),
        git(&source, ["rev-parse", "HEAD"])
    );
}

#[test]
fn fetch_writes_fetch_head_even_when_ref_update_fails_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let base = dir.path().join("base");
    let repo = dir.path().join("repo");

    run_zmin(
        dir.path(),
        ["init", "-b", "main", base.to_str().expect("base path")],
    );
    configure_identity(&base);
    fs::write(base.join("updated.t"), b"updated\n").expect("write updated");
    run_zmin(&base, ["add", "updated.t"]);
    run_zmin(&base, ["commit", "-m", "updated"]);
    run_zmin(&base, ["update-ref", "refs/heads/foo", "@"]);
    run_zmin(&base, ["update-ref", "refs/heads/branch", "@"]);

    run_zmin(
        dir.path(),
        ["init", "--bare", repo.to_str().expect("repo path")],
    );
    run_zmin(
        &repo,
        ["remote", "add", "origin", base.to_str().expect("base path")],
    );
    fs::write(repo.join("refs/heads/foo.lock"), b"").expect("write lock");

    let (status, _stdout, stderr) = run_zmin_failure_output(
        &repo,
        ["fetch", "-f", "origin", "refs/heads/*:refs/heads/*"].as_slice(),
    );
    assert_eq!(status, 1);
    assert!(
        stderr.contains("error: cannot lock ref 'refs/heads/foo'")
            || stderr.contains("refs/heads/foo.lock': File exists."),
        "{stderr}"
    );
    let fetch_head = fs::read_to_string(repo.join("FETCH_HEAD")).expect("read FETCH_HEAD");
    assert!(fetch_head.contains("branch 'branch' of"));
    assert!(fetch_head.contains("branch 'foo' of"));
}

#[test]
fn fetch_set_upstream_records_branch_tracking_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let base = dir.path().join("base");
    let repo = dir.path().join("repo");

    run_zmin(
        dir.path(),
        ["init", "-b", "main", base.to_str().expect("base path")],
    );
    configure_identity(&base);
    fs::write(base.join("updated.t"), b"updated\n").expect("write updated");
    run_zmin(&base, ["add", "updated.t"]);
    run_zmin(&base, ["commit", "-m", "updated"]);
    run_zmin(
        dir.path(),
        [
            "init",
            "--bare",
            "-b",
            "main",
            repo.to_str().expect("repo path"),
        ],
    );
    run_zmin(
        &repo,
        ["remote", "add", "origin", base.to_str().expect("base path")],
    );

    run_zmin(&repo, ["fetch", "origin", "--set-upstream", "main"]);

    assert_eq!(run_zmin(&repo, ["config", "branch.main.remote"]), "origin");
    assert_eq!(
        run_zmin(&repo, ["config", "branch.main.merge"]),
        "refs/heads/main"
    );
}

#[test]
fn fetch_set_upstream_records_tracking_even_when_ref_update_fails_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let base = dir.path().join("base");
    let repo = dir.path().join("repo");

    run_zmin(
        dir.path(),
        ["init", "-b", "main", base.to_str().expect("base path")],
    );
    configure_identity(&base);
    fs::write(base.join("updated.t"), b"updated\n").expect("write updated");
    run_zmin(&base, ["add", "updated.t"]);
    run_zmin(&base, ["commit", "-m", "updated"]);
    run_zmin(
        dir.path(),
        [
            "init",
            "--bare",
            "-b",
            "main",
            repo.to_str().expect("repo path"),
        ],
    );
    run_zmin(
        &repo,
        ["remote", "add", "origin", base.to_str().expect("base path")],
    );
    fs::create_dir_all(repo.join("refs/remotes/origin")).expect("create remote refs");
    fs::write(repo.join("refs/remotes/origin/main.lock"), b"").expect("write lock");

    let (status, _stdout, _stderr) = run_zmin_failure_output(
        &repo,
        ["fetch", "origin", "--set-upstream", "main"].as_slice(),
    );

    assert_eq!(status, 1);
    assert_eq!(run_zmin(&repo, ["config", "branch.main.remote"]), "origin");
    assert_eq!(
        run_zmin(&repo, ["config", "branch.main.merge"]),
        "refs/heads/main"
    );
}

#[test]
fn fetch_updates_remote_head_even_when_ref_update_fails_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let base = dir.path().join("base");
    let repo = dir.path().join("repo");

    run_zmin(
        dir.path(),
        ["init", "-b", "main", base.to_str().expect("base path")],
    );
    configure_identity(&base);
    fs::write(base.join("updated.t"), b"updated\n").expect("write updated");
    run_zmin(&base, ["add", "updated.t"]);
    run_zmin(&base, ["commit", "-m", "updated"]);
    run_zmin(&base, ["update-ref", "refs/heads/foo", "@"]);
    run_zmin(&base, ["update-ref", "refs/heads/branch", "@"]);
    run_zmin(
        dir.path(),
        ["init", "--bare", repo.to_str().expect("repo path")],
    );
    run_zmin(
        &repo,
        ["remote", "add", "origin", base.to_str().expect("base path")],
    );
    fs::create_dir_all(repo.join("refs/remotes/origin")).expect("create remote refs");
    fs::write(repo.join("refs/remotes/origin/branch.lock"), b"").expect("write lock");

    let (status, _stdout, _stderr) = run_zmin_failure_output(&repo, ["fetch", "origin"].as_slice());

    assert_eq!(status, 1);
    assert!(repo.join("refs/remotes/origin/HEAD").is_file());
}

#[test]
fn fetch_direct_file_url_accepts_multiple_explicit_refspecs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"main\n").expect("write source");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["branch", "-f", "newbranch"]);
    git(&source, ["tag", "-f", "newtag"]);

    run_zmin(
        dir.path(),
        ["init", "-b", "main", client.to_str().expect("client path")],
    );
    let source_url = format!("file://{}", source.display());
    run_zmin(
        &client,
        [
            "fetch",
            "--prune",
            &source_url,
            "refs/tags/*:refs/tags/*",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    );

    assert_eq!(
        git(&client, ["rev-parse", "refs/remotes/origin/newbranch"]),
        git(&source, ["rev-parse", "refs/heads/newbranch"])
    );
    assert_eq!(
        git(&client, ["rev-parse", "refs/tags/newtag"]),
        git(&source, ["rev-parse", "refs/tags/newtag"])
    );
}

#[test]
fn fetch_atomic_updates_single_local_remote_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    git(&source, ["branch", "atomic-branch"]);
    let expected = git(&source, ["rev-parse", "atomic-branch"]);

    git(&git_client, ["fetch", "--atomic", "origin"]);
    run_zmin(&zmin_client, ["fetch", "--atomic", "origin"]);

    assert_eq!(
        git(&zmin_client, ["rev-parse", "origin/atomic-branch"]),
        expected
    );
    assert_eq!(
        run_zmin(&zmin_client, ["rev-parse", "--verify", "FETCH_HEAD"]),
        git(&git_client, ["rev-parse", "--verify", "FETCH_HEAD"])
    );
}

#[test]
fn fetch_atomic_append_branch_fetch_head_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    git(&source, ["branch", "atomic-fetch-head-1"]);
    git(
        &git_client,
        ["fetch", "--atomic", "origin", "atomic-fetch-head-1"],
    );
    run_zmin(
        &zmin_client,
        ["fetch", "--atomic", "origin", "atomic-fetch-head-1"],
    );

    git(&source, ["branch", "atomic-fetch-head-2"]);
    git(
        &git_client,
        [
            "fetch",
            "--atomic",
            "--append",
            "origin",
            "atomic-fetch-head-2",
        ],
    );
    run_zmin(
        &zmin_client,
        [
            "fetch",
            "--atomic",
            "--append",
            "origin",
            "atomic-fetch-head-2",
        ],
    );
    let expected_fetch_head =
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD");
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        expected_fetch_head
    );

    let hook = zmin_client.join(".git/hooks/reference-transaction");
    fs::write(&hook, b"#!/bin/sh\nexit 1\n").expect("write reference transaction hook");
    chmod_executable(&hook);
    git(&source, ["branch", "atomic-fetch-head-3"]);
    assert_ne!(
        run_zmin_status(
            &zmin_client,
            [
                "fetch",
                "--atomic",
                "--append",
                "origin",
                "atomic-fetch-head-3"
            ],
        ),
        0
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        expected_fetch_head
    );
}

#[test]
fn fetch_atomic_aborts_non_fast_forward_wildcard_update_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    fs::write(source.join("README.md"), b"next\n").expect("write next");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "next"]);
    git(&source, ["branch", "atomic-non-ff"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );
    let original_remote = git(
        &zmin_client,
        ["rev-parse", "refs/remotes/origin/atomic-non-ff"],
    );

    git(&source, ["branch", "atomic-new-branch"]);
    let parent = git(&source, ["rev-parse", "atomic-non-ff~"]);
    git(&source, ["update-ref", "refs/heads/atomic-non-ff", &parent]);

    assert_ne!(
        run_zmin_status(
            &zmin_client,
            [
                "fetch",
                "--atomic",
                "origin",
                "refs/heads/*:refs/remotes/origin/*",
            ],
        ),
        0
    );
    assert_eq!(
        git_status_args(
            &zmin_client,
            &["rev-parse", "refs/remotes/origin/atomic-new-branch"]
        ),
        128
    );
    assert_eq!(
        git(
            &zmin_client,
            ["rev-parse", "refs/remotes/origin/atomic-non-ff"]
        ),
        original_remote
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        ""
    );
}

#[test]
fn fetch_empty_refmap_with_explicit_refspec_ignores_configured_refspec_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    fs::write(source.join("README.md"), b"next\n").expect("write next");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "next"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let previous = git(&source, ["rev-parse", "main~1"]);
    git(
        &git_client,
        ["update-ref", "refs/remotes/origin/main", &previous],
    );
    run_zmin(
        &zmin_client,
        ["update-ref", "refs/remotes/origin/main", &previous],
    );

    git(
        &git_client,
        [
            "fetch",
            "--refmap=",
            "origin",
            "+refs/heads/*:refs/hidden/origin/*",
        ],
    );
    run_zmin(
        &zmin_client,
        [
            "fetch",
            "--refmap=",
            "origin",
            "+refs/heads/*:refs/hidden/origin/*",
        ],
    );
    assert_eq!(
        git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
        git(&git_client, ["rev-parse", "refs/remotes/origin/main"])
    );
    assert_eq!(
        git(&zmin_client, ["rev-parse", "refs/hidden/origin/main"]),
        git(&git_client, ["rev-parse", "refs/hidden/origin/main"])
    );

    git(&git_client, ["fetch", "origin"]);
    run_zmin(&zmin_client, ["fetch", "origin"]);
    assert_eq!(
        git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
        git(&git_client, ["rev-parse", "refs/remotes/origin/main"])
    );
}

#[test]
fn fetch_empty_refmap_with_branch_disables_configured_refspec_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    let previous = git(&source, ["rev-parse", "HEAD"]);
    fs::write(source.join("README.md"), b"next\n").expect("write next");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "next"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );
    git(&source, ["branch", "-f", "side"]);
    let fetched = git(&source, ["rev-parse", "main"]);

    git(
        &git_client,
        ["update-ref", "refs/remotes/origin/main", &previous],
    );
    run_zmin(
        &zmin_client,
        ["update-ref", "refs/remotes/origin/main", &previous],
    );

    git(&git_client, ["fetch", "--refmap=", "origin", "main"]);
    run_zmin(&zmin_client, ["fetch", "--refmap=", "origin", "main"]);

    assert_eq!(
        git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
        git(&git_client, ["rev-parse", "refs/remotes/origin/main"])
    );
    assert_eq!(
        git_status_args(&zmin_client, &["rev-parse", "refs/remotes/origin/side"]),
        git_status_args(&git_client, &["rev-parse", "refs/remotes/origin/side"])
    );
    assert_eq!(
        git_status_args(&zmin_client, &["cat-file", "-e", &fetched]),
        0
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
}

#[test]
fn fetch_configured_refspec_describes_new_refs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let descriptive = dir.path().join("descriptive");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    run_zmin(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            descriptive.to_str().expect("descriptive path"),
        ],
    );
    git(
        &descriptive,
        [
            "config",
            "remote.o.url",
            source.to_str().expect("source path"),
        ],
    );
    git(
        &descriptive,
        ["config", "remote.o.fetch", "refs/heads/*:refs/crazyheads/*"],
    );
    git(
        &descriptive,
        [
            "config",
            "--add",
            "remote.o.fetch",
            "refs/others/*:refs/heads/*",
        ],
    );
    run_zmin(&descriptive, ["fetch", "o"]);

    git(
        &source,
        ["tag", "-a", "-m", "descriptive", "descriptive-tag"],
    );
    git(&source, ["branch", "descriptive-branch"]);
    git(&source, ["update-ref", "refs/others/crazy", "HEAD"]);

    let output = Command::new(zmin_bin())
        .args(["fetch", "o"])
        .current_dir(&descriptive)
        .output()
        .expect("zmin fetch output");
    assert!(
        output.status.success(),
        "fetch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(stderr.contains("[new branch]"));
    assert!(stderr.contains("-> refs/crazyheads/descriptive-branch"));
    assert!(stderr.contains("[new tag]"));
    assert!(stderr.contains("-> descriptive-tag"));
    assert!(stderr.contains("[new ref]"));
    assert!(stderr.contains("-> crazy"));
}

#[test]
fn fetch_deepen_existing_shallow_branch_matches_stock_git() {
    for (label, deepen_args) in [
        ("equals", vec!["--deepen=1"]),
        ("separate", vec!["--deepen", "1"]),
    ] {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join("source");
        let git_client = dir.path().join("git-client");
        let zmin_client = dir.path().join("zmin-client");

        git(
            dir.path(),
            ["init", "-b", "main", source.to_str().expect("source path")],
        );
        configure_identity(&source);
        for idx in 1..=4 {
            fs::write(source.join("file.txt"), format!("commit {idx}\n"))
                .expect("write source file");
            git(&source, ["add", "-A"]);
            git_with_env(&source, ["commit", "-m", &format!("commit {idx}")]);
        }

        let source_url = format!("file://{}", source.display());
        git(
            dir.path(),
            ["clone", "--depth=1", &source_url, "git-client"],
        );
        git(
            dir.path(),
            ["clone", "--depth=1", &source_url, "zmin-client"],
        );

        let mut args = vec!["fetch", "--quiet"];
        args.extend(deepen_args);
        args.extend(["origin", "main"]);
        let git_output = command_any_output("git", &git_client, &args, "git");
        let zmin_output = command_any_output(zmin_bin(), &zmin_client, &args, "zmin");

        assert_eq!(zmin_output.0, git_output.0, "{label}");
        assert_eq!(zmin_output.1, git_output.1, "{label}");
        assert_eq!(zmin_output.2, git_output.2, "{label}");
        assert_eq!(
            git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
            git(&git_client, ["rev-parse", "refs/remotes/origin/main"]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/shallow")).expect("zmin shallow"),
            fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow"),
            "{label}"
        );
        assert_eq!(
            git(&zmin_client, ["rev-list", "--count", "origin/main"]),
            git(&git_client, ["rev-list", "--count", "origin/main"]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
            "{label}"
        );
    }
}

#[test]
fn fetch_update_shallow_from_shallow_remote_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let shallow_remote = dir.path().join("shallow");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    for idx in 1..=4 {
        fs::write(source.join("file.txt"), format!("commit {idx}\n")).expect("write source file");
        git(&source, ["add", "-A"]);
        git_with_env(&source, ["commit", "-m", &format!("commit {idx}")]);
    }

    let source_url = format!("file://{}", source.display());
    git(
        dir.path(),
        [
            "clone",
            "--bare",
            "--depth=2",
            &source_url,
            shallow_remote.to_str().expect("shallow remote path"),
        ],
    );

    let file_url = format!("file://{}", shallow_remote.display());
    for (label, remote_url) in [
        (
            "local-path",
            shallow_remote.to_str().expect("shallow remote path"),
        ),
        ("file-url", file_url.as_str()),
    ] {
        let git_client = dir.path().join(format!("git-client-{label}"));
        let zmin_client = dir.path().join(format!("zmin-client-{label}"));
        for client in [&git_client, &zmin_client] {
            git(
                dir.path(),
                ["init", "-b", "main", client.to_str().expect("client path")],
            );
            git(client, ["remote", "add", "origin", remote_url]);
        }

        let args = ["fetch", "--quiet", "--update-shallow", "origin", "main"];
        let git_output = command_any_output("git", &git_client, &args, "git");
        let zmin_output = command_any_output(zmin_bin(), &zmin_client, &args, "zmin");

        assert_eq!(zmin_output.0, git_output.0, "{label}");
        assert_eq!(zmin_output.1, git_output.1, "{label}");
        assert_eq!(zmin_output.2, git_output.2, "{label}");
        assert_eq!(
            git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
            git(&git_client, ["rev-parse", "refs/remotes/origin/main"]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/shallow")).expect("zmin shallow"),
            fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow"),
            "{label}"
        );
        assert_eq!(
            git(&zmin_client, ["rev-list", "--count", "origin/main"]),
            git(&git_client, ["rev-list", "--count", "origin/main"]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
            "{label}"
        );
    }
}

#[test]
fn fetch_unshallow_local_remote_matches_stock_git() {
    for (label, args) in [
        (
            "named-remote",
            vec!["fetch", "--quiet", "--unshallow", "origin"],
        ),
        (
            "named-remote-branch",
            vec!["fetch", "--quiet", "--unshallow", "origin", "main"],
        ),
    ] {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join("source");
        let git_client = dir.path().join("git-client");
        let zmin_client = dir.path().join("zmin-client");

        git(
            dir.path(),
            ["init", "-b", "main", source.to_str().expect("source path")],
        );
        configure_identity(&source);
        for idx in 1..=4 {
            fs::write(source.join("file.txt"), format!("commit {idx}\n"))
                .expect("write source file");
            git(&source, ["add", "-A"]);
            git_with_env(&source, ["commit", "-m", &format!("commit {idx}")]);
        }

        let source_url = format!("file://{}", source.display());
        git(
            dir.path(),
            ["clone", "--depth=1", &source_url, "git-client"],
        );
        git(
            dir.path(),
            ["clone", "--depth=1", &source_url, "zmin-client"],
        );

        let git_output = command_any_output("git", &git_client, &args, "git");
        let zmin_output = command_any_output(zmin_bin(), &zmin_client, &args, "zmin");

        assert_eq!(zmin_output.0, git_output.0, "{label}");
        assert_eq!(zmin_output.1, git_output.1, "{label}");
        assert_eq!(zmin_output.2, git_output.2, "{label}");
        assert_eq!(
            git(&zmin_client, ["rev-parse", "--is-shallow-repository"]),
            git(&git_client, ["rev-parse", "--is-shallow-repository"]),
            "{label}"
        );
        assert!(!zmin_client.join(".git/shallow").exists(), "{label}");
        assert_eq!(
            git(&zmin_client, ["rev-list", "--count", "origin/main"]),
            git(&git_client, ["rev-list", "--count", "origin/main"]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
            "{label}"
        );
    }
}

#[test]
fn fetch_unshallow_complete_repo_matches_stock_git_failure() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file.txt"), b"base\n").expect("write source file");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    let source_url = format!("file://{}", source.display());
    git(dir.path(), ["clone", &source_url, "git-client"]);
    git(dir.path(), ["clone", &source_url, "zmin-client"]);

    let args = ["fetch", "--quiet", "--unshallow", "origin"];
    let git_output = command_any_output("git", &git_client, &args, "git");
    let zmin_output = command_any_output(zmin_bin(), &zmin_client, &args, "zmin");

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(zmin_output.2, git_output.2);
}

#[test]
fn fetch_shallow_since_local_branch_matches_stock_git() {
    for (label, shallow_since_args) in [
        ("equals", vec!["--shallow-since=2020-01-03T00:00:00 +0000"]),
        (
            "separate",
            vec!["--shallow-since", "2020-01-03T00:00:00 +0000"],
        ),
    ] {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join("source");
        let git_client = dir.path().join("git-client");
        let zmin_client = dir.path().join("zmin-client");

        git(
            dir.path(),
            ["init", "-b", "main", source.to_str().expect("source path")],
        );
        configure_identity(&source);
        for idx in 1..=4 {
            fs::write(source.join("file.txt"), format!("commit {idx}\n"))
                .expect("write source file");
            git(&source, ["add", "-A"]);
            let date = format!("2020-01-0{idx}T00:00:00 +0000");
            let env = [
                ("GIT_AUTHOR_DATE", date.as_str()),
                ("GIT_COMMITTER_DATE", date.as_str()),
            ];
            command_output_with_env(
                "git",
                &source,
                &["commit", "-m", &format!("commit {idx}")],
                &env,
                "git",
            );
        }

        let source_url = format!("file://{}", source.display());
        for client in [&git_client, &zmin_client] {
            git(
                dir.path(),
                ["init", "-b", "main", client.to_str().expect("client path")],
            );
            git(client, ["remote", "add", "origin", &source_url]);
        }

        let mut args = vec!["fetch", "--quiet"];
        args.extend(shallow_since_args);
        args.extend(["origin", "main"]);
        let git_output = command_any_output("git", &git_client, &args, "git");
        let zmin_output = command_any_output(zmin_bin(), &zmin_client, &args, "zmin");

        assert_eq!(zmin_output.0, git_output.0, "{label}");
        assert_eq!(zmin_output.1, git_output.1, "{label}");
        assert_eq!(zmin_output.2, git_output.2, "{label}");
        assert_eq!(
            git(&zmin_client, ["rev-parse", "--is-shallow-repository"]),
            git(&git_client, ["rev-parse", "--is-shallow-repository"]),
            "{label}"
        );
        assert_eq!(
            git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
            git(&git_client, ["rev-parse", "refs/remotes/origin/main"]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/shallow")).expect("zmin shallow"),
            fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow"),
            "{label}"
        );
        assert_eq!(
            git(&zmin_client, ["rev-list", "--count", "origin/main"]),
            git(&git_client, ["rev-list", "--count", "origin/main"]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
            "{label}"
        );
    }
}

#[test]
fn fetch_shallow_exclude_local_branch_matches_stock_git() {
    for (label, shallow_exclude_args) in [
        ("equals", vec!["--shallow-exclude=refs/heads/base"]),
        ("separate", vec!["--shallow-exclude", "refs/heads/base"]),
    ] {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join("source");
        let git_client = dir.path().join("git-client");
        let zmin_client = dir.path().join("zmin-client");

        git(
            dir.path(),
            ["init", "-b", "main", source.to_str().expect("source path")],
        );
        configure_identity(&source);
        for name in ["base 1", "base 2"] {
            fs::write(source.join("file.txt"), format!("{name}\n")).expect("write source file");
            git(&source, ["add", "-A"]);
            git_with_env(&source, ["commit", "-m", name]);
        }
        git(&source, ["branch", "base"]);
        for name in ["main 1", "main 2"] {
            fs::write(source.join("file.txt"), format!("{name}\n")).expect("write source file");
            git(&source, ["add", "-A"]);
            git_with_env(&source, ["commit", "-m", name]);
        }
        let base_tip = git(&source, ["rev-parse", "base"]);

        let source_url = format!("file://{}", source.display());
        for client in [&git_client, &zmin_client] {
            git(
                dir.path(),
                ["init", "-b", "main", client.to_str().expect("client path")],
            );
            git(client, ["remote", "add", "origin", &source_url]);
        }

        let mut args = vec!["fetch", "--quiet"];
        args.extend(shallow_exclude_args);
        args.extend(["origin", "main"]);
        let git_output = command_any_output("git", &git_client, &args, "git");
        let zmin_output = command_any_output(zmin_bin(), &zmin_client, &args, "zmin");

        assert_eq!(zmin_output.0, git_output.0, "{label}");
        assert_eq!(zmin_output.1, git_output.1, "{label}");
        assert_eq!(zmin_output.2, git_output.2, "{label}");
        assert_eq!(
            git(&zmin_client, ["rev-parse", "--is-shallow-repository"]),
            git(&git_client, ["rev-parse", "--is-shallow-repository"]),
            "{label}"
        );
        assert_eq!(
            git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
            git(&git_client, ["rev-parse", "refs/remotes/origin/main"]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/shallow")).expect("zmin shallow"),
            fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow"),
            "{label}"
        );
        assert_eq!(
            git(&zmin_client, ["rev-list", "--count", "origin/main"]),
            git(&git_client, ["rev-list", "--count", "origin/main"]),
            "{label}"
        );
        assert_eq!(
            git_status_args(&zmin_client, &["cat-file", "-e", &base_tip]),
            git_status_args(&git_client, &["cat-file", "-e", &base_tip]),
            "{label}"
        );
        assert_eq!(
            fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
            fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
            "{label}"
        );
    }
}

#[test]
fn fetch_with_depth_like_stock_git_for_local_remote() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let remote = dir.path().join("remote.git");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file.txt"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    fs::write(source.join("file.txt"), b"next\n").expect("write next");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "next"]);

    git(
        &source,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&source, ["push", "-q", "origin", "main"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let source_remote_url = format!("file://{}", remote.display());
    git(
        &git_client,
        ["remote", "set-url", "origin", &source_remote_url],
    );
    run_zmin(
        &zmin_client,
        ["remote", "set-url", "origin", &source_remote_url],
    );

    fs::write(source.join("file.txt"), b"next2\n").expect("write next2");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "next2"]);

    git(&source, ["push", "-q", "origin", "main"]);

    git(&git_client, ["fetch", "--depth=1", "origin", "main"]);
    run_zmin(&zmin_client, ["fetch", "--depth=1", "origin", "main"]);

    let remote_main = git(&git_client, ["rev-parse", "refs/remotes/origin/main"]);
    assert_eq!(
        git(&zmin_client, ["rev-parse", "refs/remotes/origin/main"]),
        remote_main
    );
    assert_eq!(
        git_status_args(
            &zmin_client,
            &["cat-file", "-e", &format!("{remote_main}^")]
        ),
        128
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/shallow")).expect("zmin shallow"),
        fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow")
    );
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", &format!("{remote_main}:file.txt")],
        ),
        git(
            &git_client,
            ["cat-file", "-p", &format!("{remote_main}:file.txt")],
        )
    );
    assert_eq!(
        run_zmin(&zmin_client, ["log", "--oneline", "--all"]),
        git(&git_client, ["log", "--oneline", "--all"])
    );
}

#[test]
fn fetch_depth_all_branches_uses_loose_ref_over_packed_ref_for_local_remote() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let zmin_client = dir.path().join("zmin-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file.txt"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    git(&source, ["branch", "feature"]);
    git(&source, ["pack-refs", "--all", "--prune"]);
    let packed_feature = git(&source, ["rev-parse", "refs/heads/feature"]);
    git(&source, ["switch", "feature"]);
    fs::write(source.join("feature.txt"), b"feature\n").expect("write feature");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "feature"]);
    let loose_feature = git(&source, ["rev-parse", "refs/heads/feature"]);
    assert_ne!(packed_feature, loose_feature);
    git(&source, ["switch", "main"]);

    run_zmin(dir.path(), ["init", "-b", "main", "zmin-client"]);
    run_zmin(
        &zmin_client,
        [
            "remote",
            "add",
            "origin",
            source.to_str().expect("source path"),
        ],
    );
    run_zmin(&zmin_client, ["fetch", "--depth", "1", "origin"]);

    assert_eq!(
        git(&zmin_client, ["rev-parse", "refs/remotes/origin/feature"]),
        loose_feature
    );
}

#[test]
fn fetch_with_depth_includes_nested_annotated_tags_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let remote = dir.path().join("remote.git");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");

    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    fs::write(source.join("b.txt"), b"next\n").expect("write next");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "next"]);

    git(
        &source,
        [
            "-c",
            "tag.gpgSign=false",
            "tag",
            "-a",
            "v1",
            "-m",
            "v1 tag",
            "HEAD",
        ],
    );
    git(
        &source,
        [
            "-c",
            "tag.gpgSign=false",
            "tag",
            "-a",
            "v1-nested",
            "-m",
            "nested v1 tag",
            "refs/tags/v1",
        ],
    );

    git(
        &source,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&source, ["push", "-q", "origin", "main", "--tags"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let remote_url = format!("file://{}", remote.display());
    git(&git_client, ["remote", "set-url", "origin", &remote_url]);
    run_zmin(&zmin_client, ["remote", "set-url", "origin", &remote_url]);

    git(&git_client, ["fetch", "--depth=1", "origin", "main"]);
    run_zmin(&zmin_client, ["fetch", "--depth=1", "origin", "main"]);

    let nested = git(&source, ["rev-parse", "refs/tags/v1-nested"]);
    let direct = git(&source, ["rev-parse", "refs/tags/v1"]);
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", &nested]),
        git(&git_client, ["cat-file", "-p", &nested])
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", &direct]),
        git(&git_client, ["cat-file", "-p", &direct])
    );
}

#[test]
fn pull_local_remote_fast_forwards_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"main\n").expect("write main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    fs::write(source.join("README.md"), b"main update\n").expect("update main");
    fs::write(source.join("next.txt"), b"next\n").expect("write next");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main update"]);

    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(&git_client, ["pull", "--ff-only"]);
    run_zmin(&zmin_client, ["pull", "--ff-only"]);

    assert_eq!(
        git(&zmin_client, ["rev-parse", "HEAD"]),
        git(&git_client, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        run_zmin(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_local_remote_replays_local_commit_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    fs::write(git_client.join("local.txt"), b"local\n").expect("write git local");
    fs::write(zmin_client.join("local.txt"), b"local\n").expect("write zmin local");
    git(&git_client, ["add", "-A"]);
    git(&zmin_client, ["add", "-A"]);
    git_with_env(&git_client, ["commit", "-m", "local"]);
    git_with_env(&zmin_client, ["commit", "-m", "local"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    git_with_env(&git_client, ["pull", "--rebase"]);
    run_zmin_with_env(&zmin_client, ["pull", "--rebase"]);

    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&zmin_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        run_zmin(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_merges_local_remote_replays_linear_local_commit_like_stock_git() {
    pull_rebase_mode_local_remote_replays_linear_local_commit_like_stock_git(
        "--rebase=merges",
        &[],
    );
}

#[test]
fn pull_rebase_interactive_local_remote_replays_linear_local_commit_like_stock_git() {
    pull_rebase_mode_local_remote_replays_linear_local_commit_like_stock_git(
        "--rebase=interactive",
        &[("GIT_SEQUENCE_EDITOR", ":")],
    );
}

#[test]
fn pull_rebase_interactive_applies_sequence_editor_drop_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    fs::write(git_client.join("local.txt"), b"local\n").expect("write git local");
    fs::write(zmin_client.join("local.txt"), b"local\n").expect("write zmin local");
    git(&git_client, ["add", "-A"]);
    git(&zmin_client, ["add", "-A"]);
    git_with_env(&git_client, ["commit", "-m", "local"]);
    git_with_env(&zmin_client, ["commit", "-m", "local"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    let editor = write_sequence_editor_replace_pick(dir.path(), "drop-first-todo.sh", "drop", 1);
    let editor_command = format!("sh {}", editor.display());
    let env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ("GIT_SEQUENCE_EDITOR", editor_command.as_str()),
    ];
    command_output_with_env(
        "git",
        &git_client,
        &["pull", "--rebase=interactive"],
        &env,
        "git",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["pull", "--rebase=interactive"],
        &env,
        "zmin",
    );

    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&zmin_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert!(!zmin_client.join("local.txt").exists());
    assert_eq!(
        run_zmin(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_interactive_reword_uses_commit_editor_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    fs::write(git_client.join("local.txt"), b"local\n").expect("write git local");
    fs::write(zmin_client.join("local.txt"), b"local\n").expect("write zmin local");
    git(&git_client, ["add", "-A"]);
    git(&zmin_client, ["add", "-A"]);
    git_with_env(&git_client, ["commit", "-m", "local"]);
    git_with_env(&zmin_client, ["commit", "-m", "local"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    let sequence_editor =
        write_sequence_editor_replace_pick(dir.path(), "reword-first-todo.sh", "reword", 1);
    let commit_editor = dir.path().join("rewrite-message.sh");
    fs::write(
        &commit_editor,
        r#"#!/bin/sh
printf 'renamed local\n' > "$1"
"#,
    )
    .expect("write commit editor");
    let sequence_editor_command = format!("sh {}", sequence_editor.display());
    let commit_editor_command = format!("sh {}", commit_editor.display());
    let env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ("GIT_SEQUENCE_EDITOR", sequence_editor_command.as_str()),
        ("GIT_EDITOR", commit_editor_command.as_str()),
    ];
    command_output_with_env(
        "git",
        &git_client,
        &["pull", "--rebase=interactive"],
        &env,
        "git",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["pull", "--rebase=interactive"],
        &env,
        "zmin",
    );

    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&zmin_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        run_zmin(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_interactive_squash_uses_commit_editor_like_stock_git() {
    pull_rebase_interactive_melds_second_commit_like_stock_git("squash", Some("combined local"));
}

#[test]
fn pull_rebase_interactive_fixup_keeps_previous_message_like_stock_git() {
    pull_rebase_interactive_melds_second_commit_like_stock_git("fixup", None);
}

#[test]
fn pull_rebase_interactive_edit_stops_and_continue_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    for client in [&git_client, &zmin_client] {
        fs::write(client.join("local-a.txt"), b"local a\n").expect("write local a");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "local a"]);
        fs::write(client.join("local-b.txt"), b"local b\n").expect("write local b");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "local b"]);
    }

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    let sequence_editor =
        write_sequence_editor_replace_pick(dir.path(), "edit-first-todo.sh", "edit", 1);
    let sequence_editor_command = format!("sh {}", sequence_editor.display());
    let env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ("GIT_SEQUENCE_EDITOR", sequence_editor_command.as_str()),
    ];
    command_output_with_env(
        "git",
        &git_client,
        &["pull", "--rebase=interactive"],
        &env,
        "git",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["pull", "--rebase=interactive"],
        &env,
        "zmin",
    );

    assert!(
        git_client
            .join(".git/rebase-merge/git-rebase-todo")
            .exists()
    );
    assert!(
        zmin_client
            .join(".git/rebase-merge/git-rebase-todo")
            .exists()
    );
    assert_eq!(
        git(&zmin_client, ["log", "--format=%s", "--max-count=2"]),
        git(&git_client, ["log", "--format=%s", "--max-count=2"])
    );

    command_output_with_env("git", &git_client, &["rebase", "--continue"], &env, "git");
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["rebase", "--continue"],
        &env,
        "zmin",
    );

    assert!(!zmin_client.join(".git/rebase-merge").exists());
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&zmin_client, ["log", "--format=%s", "--max-count=4"]),
        git(&git_client, ["log", "--format=%s", "--max-count=4"])
    );
    assert_eq!(
        run_zmin(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_interactive_edit_abort_restores_original_head_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    for client in [&git_client, &zmin_client] {
        fs::write(client.join("local-a.txt"), b"local a\n").expect("write local a");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "local a"]);
        fs::write(client.join("local-b.txt"), b"local b\n").expect("write local b");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "local b"]);
    }

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    let sequence_editor =
        write_sequence_editor_replace_pick(dir.path(), "edit-first-abort-todo.sh", "edit", 1);
    let sequence_editor_command = format!("sh {}", sequence_editor.display());
    let env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ("GIT_SEQUENCE_EDITOR", sequence_editor_command.as_str()),
    ];
    let git_original = git(&git_client, ["rev-parse", "HEAD"]);
    let zmin_original = git(&zmin_client, ["rev-parse", "HEAD"]);
    command_output_with_env(
        "git",
        &git_client,
        &["pull", "--rebase=interactive"],
        &env,
        "git",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["pull", "--rebase=interactive"],
        &env,
        "zmin",
    );
    command_output_with_env("git", &git_client, &["rebase", "--abort"], &env, "git");
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["rebase", "--abort"],
        &env,
        "zmin",
    );

    assert_eq!(git(&git_client, ["rev-parse", "HEAD"]), git_original);
    assert_eq!(git(&zmin_client, ["rev-parse", "HEAD"]), zmin_original);
    assert!(!zmin_client.join(".git/rebase-merge").exists());
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        run_zmin(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

fn pull_rebase_interactive_melds_second_commit_like_stock_git(
    meld_command: &str,
    editor_message: Option<&str>,
) {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    for client in [&git_client, &zmin_client] {
        fs::write(client.join("local-a.txt"), b"local a\n").expect("write local a");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "local a"]);
        fs::write(client.join("local-b.txt"), b"local b\n").expect("write local b");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "local b"]);
    }

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    let sequence_editor = write_sequence_editor_replace_pick(
        dir.path(),
        &format!("{meld_command}-second-todo.sh"),
        meld_command,
        2,
    );
    let sequence_editor_command = format!("sh {}", sequence_editor.display());
    let commit_editor_command = if let Some(message) = editor_message {
        let commit_editor = dir.path().join("meld-message.sh");
        fs::write(
            &commit_editor,
            format!(
                r#"#!/bin/sh
printf '{}\n' > "$1"
"#,
                message
            ),
        )
        .expect("write commit editor");
        Some(format!("sh {}", commit_editor.display()))
    } else {
        None
    };
    let mut env = vec![
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
        ("GIT_SEQUENCE_EDITOR", sequence_editor_command.as_str()),
    ];
    if let Some(editor) = commit_editor_command.as_deref() {
        env.push(("GIT_EDITOR", editor));
    }
    command_output_with_env(
        "git",
        &git_client,
        &["pull", "--rebase=interactive"],
        &env,
        "git",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["pull", "--rebase=interactive"],
        &env,
        "zmin",
    );

    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&zmin_client, ["rev-list", "--count", "origin/main..HEAD"]),
        git(&git_client, ["rev-list", "--count", "origin/main..HEAD"])
    );
    assert_eq!(
        git(&zmin_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        run_zmin(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_merges_local_remote_preserves_merge_topology_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    for client in [&git_client, &zmin_client] {
        git(client, ["switch", "-c", "side"]);
        fs::write(client.join("side.txt"), b"side\n").expect("write side");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "side"]);
        git(client, ["switch", "main"]);
        fs::write(client.join("local.txt"), b"local\n").expect("write local");
        git(client, ["add", "-A"]);
        git_with_env(client, ["commit", "-m", "local"]);
        git_with_env(client, ["merge", "side", "-m", "Merge side"]);
    }

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    let env = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
    ];
    command_output_with_env(
        "git",
        &git_client,
        &["pull", "--rebase=merges"],
        &env,
        "git",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_client,
        &["pull", "--rebase=merges"],
        &env,
        "zmin",
    );

    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&zmin_client, ["rev-list", "--parents", "-n", "1", "HEAD"])
            .split_whitespace()
            .count(),
        git(&git_client, ["rev-list", "--parents", "-n", "1", "HEAD"])
            .split_whitespace()
            .count()
    );
    assert_eq!(
        git(&zmin_client, ["log", "--format=%s", "--max-count=1"]),
        git(&git_client, ["log", "--format=%s", "--max-count=1"])
    );
    assert_eq!(
        run_zmin(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

fn pull_rebase_mode_local_remote_replays_linear_local_commit_like_stock_git(
    mode: &str,
    extra_env: &[(&str, &str)],
) {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    fs::write(git_client.join("local.txt"), b"local\n").expect("write git local");
    fs::write(zmin_client.join("local.txt"), b"local\n").expect("write zmin local");
    git(&git_client, ["add", "-A"]);
    git(&zmin_client, ["add", "-A"]);
    git_with_env(&git_client, ["commit", "-m", "local"]);
    git_with_env(&zmin_client, ["commit", "-m", "local"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    let mut env = vec![
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
    ];
    env.extend_from_slice(extra_env);
    command_output_with_env("git", &git_client, &["pull", mode], &env, "git");
    command_output_with_env(zmin_bin(), &zmin_client, &["pull", mode], &env, "zmin");

    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&zmin_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        run_zmin(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_config_replays_local_commit_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    git(&git_client, ["config", "pull.rebase", "true"]);
    run_zmin(&zmin_client, ["config", "pull.rebase", "true"]);
    fs::write(git_client.join("local.txt"), b"local\n").expect("write git local");
    fs::write(zmin_client.join("local.txt"), b"local\n").expect("write zmin local");
    git(&git_client, ["add", "-A"]);
    git(&zmin_client, ["add", "-A"]);
    git_with_env(&git_client, ["commit", "-m", "local"]);
    git_with_env(&zmin_client, ["commit", "-m", "local"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    git_with_env(&git_client, ["pull"]);
    run_zmin_with_env(&zmin_client, ["pull"]);

    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&zmin_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        run_zmin(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_branch_rebase_config_replays_local_commit_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    git(&git_client, ["config", "branch.main.rebase", "true"]);
    run_zmin(&zmin_client, ["config", "branch.main.rebase", "true"]);
    fs::write(git_client.join("local.txt"), b"local\n").expect("write git local");
    fs::write(zmin_client.join("local.txt"), b"local\n").expect("write zmin local");
    git(&git_client, ["add", "-A"]);
    git(&zmin_client, ["add", "-A"]);
    git_with_env(&git_client, ["commit", "-m", "local"]);
    git_with_env(&zmin_client, ["commit", "-m", "local"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    git_with_env(&git_client, ["pull"]);
    run_zmin_with_env(&zmin_client, ["pull"]);

    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        git(&zmin_client, ["log", "--format=%s", "--max-count=3"]),
        git(&git_client, ["log", "--format=%s", "--max-count=3"])
    );
    assert_eq!(
        run_zmin(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn pull_rebase_false_overrides_config_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-client"],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-client",
        ],
    );

    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    configure_identity(&git_client);
    configure_identity(&zmin_client);
    git(&git_client, ["config", "pull.rebase", "true"]);
    run_zmin(&zmin_client, ["config", "pull.rebase", "true"]);

    fs::write(source.join("remote.txt"), b"remote\n").expect("write remote");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "remote"]);

    git_with_env(&git_client, ["pull", "--rebase=false"]);
    run_zmin_with_env(&zmin_client, ["pull", "--rebase=false"]);

    assert_eq!(
        git(&zmin_client, ["rev-parse", "HEAD"]),
        git(&git_client, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        run_zmin(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn fetch_pack_copies_local_ref_objects_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "zmin-client"]);

    let remote_path = remote.to_str().expect("remote path");
    let expected = git(&git_client, ["fetch-pack", remote_path, "refs/heads/main"]);
    assert_eq!(
        run_zmin(&zmin_client, ["fetch-pack", remote_path, "refs/heads/main"]),
        expected
    );
    let fetched = expected
        .split_whitespace()
        .next()
        .expect("fetched object id");
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", &format!("{fetched}:a.txt")]
        ),
        "hello"
    );
}

#[test]
fn fetch_pack_accepts_thin_and_no_progress_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "zmin-client"]);

    let remote_path = remote.to_str().expect("remote path");
    let expected = git(
        &git_client,
        [
            "fetch-pack",
            "--thin",
            "--no-progress",
            remote_path,
            "refs/heads/main",
        ],
    );
    assert_eq!(
        run_zmin(
            &zmin_client,
            [
                "fetch-pack",
                "--thin",
                "--no-progress",
                remote_path,
                "refs/heads/main",
            ],
        ),
        expected
    );
    let fetched = expected
        .split_whitespace()
        .next()
        .expect("fetched object id");
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", &format!("{fetched}:a.txt")]
        ),
        "hello"
    );
}

#[test]
fn fetch_pack_include_tag_copies_annotated_tag_objects_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "-c",
            "tag.gpgSign=false",
            "tag",
            "-a",
            "v1",
            "-m",
            "tag message",
        ],
    );
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "--tags"]);
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "zmin-client"]);

    let remote_path = remote.to_str().expect("remote path");
    let expected = git(
        &git_client,
        [
            "fetch-pack",
            "--include-tag",
            remote_path,
            "refs/heads/main",
        ],
    );
    assert_eq!(
        run_zmin(
            &zmin_client,
            [
                "fetch-pack",
                "--include-tag",
                remote_path,
                "refs/heads/main"
            ],
        ),
        expected
    );

    let tag_id = git(&work, ["rev-parse", "refs/tags/v1"]);
    assert_eq!(git(&git_client, ["cat-file", "-t", &tag_id]), "tag");
    assert_eq!(git(&zmin_client, ["cat-file", "-t", &tag_id]), "tag");
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", &tag_id]),
        git(&git_client, ["cat-file", "-p", &tag_id])
    );
}

#[test]
fn fetch_pack_include_tag_with_depth_limited_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    fs::write(work.join("b.txt"), b"next\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "next"]);
    git(
        &work,
        [
            "-c",
            "tag.gpgSign=false",
            "tag",
            "-a",
            "v1",
            "-m",
            "tag message",
        ],
    );
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "--tags"]);
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "zmin-client"]);

    let remote_path = remote.to_str().expect("remote path");
    let expected = git(
        &git_client,
        [
            "fetch-pack",
            "--depth=1",
            "--include-tag",
            remote_path,
            "refs/heads/main",
        ],
    );
    assert_eq!(
        run_zmin(
            &zmin_client,
            [
                "fetch-pack",
                "--depth=1",
                "--include-tag",
                remote_path,
                "refs/heads/main",
            ],
        ),
        expected
    );

    let fetched = expected
        .split_whitespace()
        .next()
        .expect("fetched object id");
    let tag_id = git(&work, ["rev-parse", "refs/tags/v1"]);
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", &tag_id]),
        git(&git_client, ["cat-file", "-p", &tag_id])
    );
    assert_eq!(
        git_status_args(&zmin_client, &["cat-file", "-e", &format!("{fetched}^")]),
        128
    );
}

#[test]
fn fetch_pack_include_tag_depth_includes_nested_annotated_tags_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"base\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "base"]);
    fs::write(work.join("b.txt"), b"next\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "next"]);
    git(
        &work,
        [
            "-c",
            "tag.gpgSign=false",
            "tag",
            "-a",
            "v1",
            "-m",
            "v1 tag",
            "HEAD",
        ],
    );
    git(
        &work,
        [
            "-c",
            "tag.gpgSign=false",
            "tag",
            "-a",
            "v1-nested",
            "-m",
            "nested v1 tag",
            "refs/tags/v1",
        ],
    );
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "--tags"]);
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "zmin-client"]);

    let remote_path = remote.to_str().expect("remote path");
    let expected = git(
        &git_client,
        [
            "fetch-pack",
            "--depth=1",
            "--include-tag",
            remote_path,
            "refs/heads/main",
        ],
    );
    assert_eq!(
        run_zmin(
            &zmin_client,
            [
                "fetch-pack",
                "--depth=1",
                "--include-tag",
                remote_path,
                "refs/heads/main",
            ],
        ),
        expected
    );

    let nested = git(&work, ["rev-parse", "refs/tags/v1-nested"]);
    let direct = git(&work, ["rev-parse", "refs/tags/v1"]);
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", &nested]),
        git(&git_client, ["cat-file", "-p", &nested])
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-p", &direct]),
        git(&git_client, ["cat-file", "-p", &direct])
    );
}

#[test]
fn fetch_pack_depth_one_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"base\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "base"]);
    fs::write(work.join("b.txt"), b"next\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "next"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    git(dir.path(), ["init", "git-client"]);
    git(dir.path(), ["init", "zmin-client"]);

    let remote_path = remote.to_str().expect("remote path");
    let expected = git(
        &git_client,
        ["fetch-pack", "--depth=1", remote_path, "refs/heads/main"],
    );
    assert_eq!(
        run_zmin(
            &zmin_client,
            ["fetch-pack", "--depth=1", remote_path, "refs/heads/main"],
        ),
        expected
    );
    let fetched = expected
        .split_whitespace()
        .next()
        .expect("fetched object id");
    assert_eq!(
        git(
            &zmin_client,
            ["cat-file", "-p", &format!("{fetched}:b.txt")]
        ),
        "next"
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join(".git/shallow")).expect("zmin shallow"),
        fs::read_to_string(git_client.join(".git/shallow")).expect("git shallow")
    );
    assert_eq!(
        git_status_args(&zmin_client, &["cat-file", "-e", &format!("{fetched}^")]),
        128
    );
}

#[test]
fn ls_remote_matches_stock_git_for_local_remotes() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "tag message"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);

    assert_eq!(
        run_zmin(&work, ["ls-remote", "origin"]),
        git(&work, ["ls-remote", "origin"])
    );
    assert_eq!(
        run_zmin(&work, ["ls-remote", "--heads", "origin"]),
        git(&work, ["ls-remote", "--heads", "origin"])
    );
    assert_eq!(
        run_zmin(&work, ["ls-remote", "--tags", "origin"]),
        git(&work, ["ls-remote", "--tags", "origin"])
    );
    assert_eq!(
        run_zmin(&work, ["ls-remote", "--refs", "origin"]),
        git(&work, ["ls-remote", "--refs", "origin"])
    );
    assert_eq!(
        run_zmin(&work, ["ls-remote", "origin", "main", "v*"]),
        git(&work, ["ls-remote", "origin", "main", "v*"])
    );
    assert_eq!(
        run_zmin(&work, ["ls-remote", remote.to_str().expect("remote path")]),
        git(&work, ["ls-remote", remote.to_str().expect("remote path")])
    );
    let remote_file_url = format!("file://{}", remote.display());
    assert_eq!(
        run_zmin(&work, ["ls-remote", &remote_file_url]),
        git(&work, ["ls-remote", &remote_file_url])
    );
}

#[test]
fn ls_remote_reads_local_gitfile_repository_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote");
    git(dir.path(), ["init", "-b", "main", "remote"]);
    configure_identity(&remote);
    fs::write(remote.join("a.txt"), b"hello\n").expect("write fixture");
    git(&remote, ["add", "-A"]);
    git_with_env(&remote, ["commit", "-m", "initial"]);
    git(&remote, ["tag", "lightweight"]);
    let real_git = remote.join(".realgit");
    fs::rename(remote.join(".git"), &real_git).expect("move git dir");
    fs::write(
        remote.join(".git"),
        format!("gitdir: {}\n", real_git.display()),
    )
    .expect("write gitfile");

    assert_eq!(
        run_zmin(
            dir.path(),
            ["ls-remote", remote.to_str().expect("remote path")]
        ),
        git(
            dir.path(),
            ["ls-remote", remote.to_str().expect("remote path")]
        )
    );
}

#[test]
fn ls_remote_unsupported_remote_helper_failure_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    git(dir.path(), ["init"]);

    assert_eq!(
        run_zmin_failure_output(dir.path(), &["ls-remote", "zminproto://example/repo"]),
        git_failure_output(dir.path(), &["ls-remote", "zminproto://example/repo"])
    );
}

#[test]
fn push_local_remote_updates_bare_refs_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_remote = dir.path().join("git-remote.git");
    let zmin_remote = dir.path().join("zmin-remote.git");
    let git_work = dir.path().join("git-work");
    let zmin_work = dir.path().join("zmin-work");

    git(
        dir.path(),
        ["init", "--bare", git_remote.to_str().expect("git remote")],
    );
    git(
        dir.path(),
        ["init", "--bare", zmin_remote.to_str().expect("zmin remote")],
    );
    git(
        dir.path(),
        ["init", "-b", "main", git_work.to_str().expect("git work")],
    );
    run_zmin(
        dir.path(),
        ["init", "-b", "main", zmin_work.to_str().expect("zmin work")],
    );
    configure_identity(&git_work);
    configure_identity(&zmin_work);
    git(
        &git_work,
        [
            "remote",
            "add",
            "origin",
            git_remote.to_str().expect("git remote"),
        ],
    );
    run_zmin(
        &zmin_work,
        [
            "remote",
            "add",
            "origin",
            zmin_remote.to_str().expect("zmin remote"),
        ],
    );

    fs::write(git_work.join("README.md"), b"main\n").expect("write git main");
    fs::write(zmin_work.join("README.md"), b"main\n").expect("write zmin main");
    git(&git_work, ["add", "-A"]);
    run_zmin(&zmin_work, ["add", "-A"]);
    git_with_env(&git_work, ["commit", "-m", "main"]);
    run_zmin_with_env(&zmin_work, ["commit", "-m", "main"]);
    git(&git_work, ["push", "-u", "origin", "HEAD"]);
    run_zmin(&zmin_work, ["push", "-u", "origin", "HEAD"]);

    assert_eq!(
        git(&zmin_remote, ["rev-parse", "refs/heads/main"]),
        git(&git_remote, ["rev-parse", "refs/heads/main"])
    );
    assert_eq!(
        git(&zmin_remote, ["cat-file", "-p", "refs/heads/main^{tree}"]),
        git(&git_remote, ["cat-file", "-p", "refs/heads/main^{tree}"])
    );
    assert_eq!(
        run_zmin(&zmin_work, ["config", "branch.main.remote"]),
        git(&git_work, ["config", "branch.main.remote"])
    );
    assert_eq!(
        run_zmin(&zmin_work, ["config", "branch.main.merge"]),
        git(&git_work, ["config", "branch.main.merge"])
    );

    let git_remote_file_url = format!("file://{}", git_remote.display());
    let zmin_remote_file_url = format!("file://{}", zmin_remote.display());
    git(
        &git_work,
        ["remote", "set-url", "origin", &git_remote_file_url],
    );
    run_zmin(
        &zmin_work,
        ["remote", "set-url", "origin", &zmin_remote_file_url],
    );

    fs::write(git_work.join("next.txt"), b"next\n").expect("write git next");
    fs::write(zmin_work.join("next.txt"), b"next\n").expect("write zmin next");
    git(&git_work, ["add", "-A"]);
    run_zmin(&zmin_work, ["add", "-A"]);
    git_with_env(&git_work, ["commit", "-m", "next"]);
    run_zmin_with_env(&zmin_work, ["commit", "-m", "next"]);
    git(&git_work, ["push"]);
    run_zmin(&zmin_work, ["push"]);

    assert_eq!(
        git(&zmin_remote, ["rev-parse", "refs/heads/main"]),
        git(&git_remote, ["rev-parse", "refs/heads/main"])
    );
    assert_eq!(
        git(&zmin_remote, ["cat-file", "-p", "refs/heads/main^{tree}"]),
        git(&git_remote, ["cat-file", "-p", "refs/heads/main^{tree}"])
    );

    git(&git_work, ["checkout", "-b", "feature"]);
    run_zmin(&zmin_work, ["checkout", "-b", "feature"]);
    fs::write(git_work.join("feature.txt"), b"feature\n").expect("write git feature");
    fs::write(zmin_work.join("feature.txt"), b"feature\n").expect("write zmin feature");
    git(&git_work, ["add", "-A"]);
    run_zmin(&zmin_work, ["add", "-A"]);
    git_with_env(&git_work, ["commit", "-m", "feature"]);
    run_zmin_with_env(&zmin_work, ["commit", "-m", "feature"]);
    git(&git_work, ["push", "origin", "feature"]);
    run_zmin(&zmin_work, ["push", "origin", "feature"]);
    assert_eq!(
        git(&zmin_remote, ["rev-parse", "refs/heads/feature"]),
        git(&git_remote, ["rev-parse", "refs/heads/feature"])
    );
    git(&git_work, ["push", "origin", ":feature"]);
    run_zmin(&zmin_work, ["push", "origin", ":feature"]);
    assert_eq!(
        git_status(
            &zmin_remote,
            ["rev-parse", "--verify", "refs/heads/feature"]
        ),
        git_status(&git_remote, ["rev-parse", "--verify", "refs/heads/feature"])
    );
}

#[test]
fn push_local_remote_does_not_copy_unreachable_objects() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");

    git(
        dir.path(),
        ["init", "--bare", remote.to_str().expect("remote path")],
    );
    run_zmin(
        dir.path(),
        ["init", "-b", "main", work.to_str().expect("work path")],
    );
    configure_identity(&work);
    run_zmin(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    fs::write(work.join("README.md"), b"main\n").expect("write main");
    run_zmin(&work, ["add", "-A"]);
    run_zmin_with_env(&work, ["commit", "-m", "main"]);
    let output = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(&work)
        .output()
        .expect("hash unreachable object");
    assert!(
        output.status.success(),
        "hash unreachable object failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let unreachable = String::from_utf8(output.stdout)
        .expect("unreachable oid utf8")
        .trim()
        .to_owned();

    run_zmin(&work, ["push", "origin", "HEAD:main"]);

    assert_eq!(
        git_status_args(&remote, &["cat-file", "-e", &unreachable]),
        1
    );
    assert_eq!(
        git(&remote, ["cat-file", "-p", "refs/heads/main:README.md"]),
        "main"
    );
}

#[test]
fn fetch_and_push_unsupported_remote_helper_failures_match_stock_git() {
    let git_repo = TempDir::new().expect("git repo");
    let zmin_repo = TempDir::new().expect("zmin repo");
    git(git_repo.path(), ["init", "-b", "main"]);
    git(zmin_repo.path(), ["init", "-b", "main"]);
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(
            repo,
            ["remote", "add", "origin", "zminproto://example/repo"],
        );
    }

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["fetch", "origin", "main"]),
        git_failure_output(git_repo.path(), &["fetch", "origin", "main"])
    );
    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["push", "origin", "main"]),
        git_failure_output(git_repo.path(), &["push", "origin", "main"])
    );
}

#[test]
fn send_pack_updates_local_bare_ref_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_remote = dir.path().join("git-remote.git");
    let zmin_remote = dir.path().join("zmin-remote.git");
    let git_work = dir.path().join("git-work");
    let zmin_work = dir.path().join("zmin-work");
    git(dir.path(), ["init", "--bare", "git-remote.git"]);
    git(dir.path(), ["init", "--bare", "zmin-remote.git"]);
    git(dir.path(), ["init", "-b", "main", "git-work"]);
    git(dir.path(), ["init", "-b", "main", "zmin-work"]);
    configure_identity(&git_work);
    configure_identity(&zmin_work);
    fs::write(git_work.join("a.txt"), b"hello\n").expect("write git");
    fs::write(zmin_work.join("a.txt"), b"hello\n").expect("write zmin");
    git(&git_work, ["add", "-A"]);
    git(&zmin_work, ["add", "-A"]);
    git_with_env(&git_work, ["commit", "-m", "initial"]);
    run_zmin_with_env(&zmin_work, ["commit", "-m", "initial"]);

    let git_remote_path = git_remote.to_str().expect("git remote");
    let zmin_remote_path = zmin_remote.to_str().expect("zmin remote");
    assert_eq!(
        run_zmin(
            &zmin_work,
            ["send-pack", zmin_remote_path, "refs/heads/main"]
        ),
        git(&git_work, ["send-pack", git_remote_path, "refs/heads/main"])
    );
    assert_eq!(
        git(&zmin_remote, ["rev-parse", "refs/heads/main"]),
        git(&zmin_work, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&zmin_remote, ["cat-file", "-p", "refs/heads/main:a.txt"]),
        "hello"
    );
}

#[test]
fn send_pack_thin_updates_local_bare_ref_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_remote = dir.path().join("git-remote.git");
    let zmin_remote = dir.path().join("zmin-remote.git");
    let git_work = dir.path().join("git-work");
    let zmin_work = dir.path().join("zmin-work");
    git(dir.path(), ["init", "--bare", "git-remote.git"]);
    git(dir.path(), ["init", "--bare", "zmin-remote.git"]);
    git(dir.path(), ["init", "-b", "main", "git-work"]);
    git(dir.path(), ["init", "-b", "main", "zmin-work"]);
    configure_identity(&git_work);
    configure_identity(&zmin_work);
    fs::write(git_work.join("a.txt"), b"hello\n").expect("write git");
    fs::write(zmin_work.join("a.txt"), b"hello\n").expect("write zmin");
    git(&git_work, ["add", "-A"]);
    git(&zmin_work, ["add", "-A"]);
    git_with_env(&git_work, ["commit", "-m", "initial"]);
    run_zmin_with_env(&zmin_work, ["commit", "-m", "initial"]);

    let git_remote_path = git_remote.to_str().expect("git remote");
    let zmin_remote_path = zmin_remote.to_str().expect("zmin remote");
    assert_eq!(
        run_zmin(
            &zmin_work,
            ["send-pack", "--thin", zmin_remote_path, "refs/heads/main"]
        ),
        git(
            &git_work,
            ["send-pack", "--thin", git_remote_path, "refs/heads/main"]
        )
    );
    assert_eq!(
        git(&zmin_remote, ["rev-parse", "refs/heads/main"]),
        git(&zmin_work, ["rev-parse", "HEAD"])
    );
}

#[test]
fn send_pack_mirror_syncs_heads_tags_and_deletions_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_remote = dir.path().join("git-remote.git");
    let zmin_remote = dir.path().join("zmin-remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "git-remote.git"]);
    git(dir.path(), ["init", "--bare", "zmin-remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("main.txt"), b"main\n").expect("write main");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "main"]);
    git(&work, ["branch", "feature"]);
    git(&work, ["tag", "v1"]);

    let git_remote_path = git_remote.to_str().expect("git remote");
    let zmin_remote_path = zmin_remote.to_str().expect("zmin remote");
    git(&work, ["push", git_remote_path, "HEAD:refs/heads/stale"]);
    git(&work, ["push", zmin_remote_path, "HEAD:refs/heads/stale"]);

    assert_eq!(
        run_zmin(&work, ["send-pack", "--mirror", zmin_remote_path]),
        git(&work, ["send-pack", "--mirror", git_remote_path])
    );
    assert_eq!(
        git(
            &zmin_remote,
            ["for-each-ref", "--format=%(refname) %(objectname)", "refs"]
        ),
        git(
            &git_remote,
            ["for-each-ref", "--format=%(refname) %(objectname)", "refs"]
        )
    );
    assert_ne!(
        git_status(&zmin_remote, ["rev-parse", "--verify", "refs/heads/stale"]),
        0
    );
}

#[test]
fn send_pack_atomic_rejects_all_updates_when_one_ref_fails() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("main.txt"), b"main\n").expect("write main");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "main"]);
    git(&work, ["branch", "feature"]);
    let remote_path = remote.to_str().expect("remote path");
    git(&work, ["push", remote_path, "HEAD:refs/heads/main"]);

    fs::write(work.join("main.txt"), b"updated\n").expect("update main");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "updated"]);
    let updated = git(&work, ["rev-parse", "HEAD"]);
    fs::write(dir.path().join("foreign.txt"), b"foreign\n").expect("write foreign");
    git(dir.path(), ["init", "-b", "main", "foreign"]);
    let foreign = dir.path().join("foreign");
    configure_identity(&foreign);
    fs::write(foreign.join("foreign.txt"), b"foreign\n").expect("write foreign repo");
    git(&foreign, ["add", "-A"]);
    git_with_env(&foreign, ["commit", "-m", "foreign"]);
    let foreign_id = git(&foreign, ["rev-parse", "HEAD"]);
    git(
        &foreign,
        ["push", "--force", remote_path, "HEAD:refs/heads/main"],
    );

    assert_ne!(
        git_status(
            &work,
            [
                "send-pack",
                "--atomic",
                remote_path,
                "refs/heads/feature",
                "refs/heads/main",
            ],
        ),
        0
    );
    assert_ne!(
        run_zmin_status(
            &work,
            [
                "send-pack",
                "--atomic",
                remote_path,
                "refs/heads/feature",
                "refs/heads/main",
            ]
        ),
        0
    );
    assert_ne!(
        git_status(&remote, ["rev-parse", "--verify", "refs/heads/feature"]),
        0
    );
    assert_eq!(git(&remote, ["rev-parse", "refs/heads/main"]), foreign_id);
    assert_ne!(git(&remote, ["rev-parse", "refs/heads/main"]), updated);
}

#[test]
fn receive_pack_accepts_stock_git_send_pack() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);

    let receive_pack = format!("{} receive-pack", shell_command_path(zmin_bin()));
    let output = Command::new("git")
        .args([
            "send-pack",
            "--receive-pack",
            &receive_pack,
            remote.to_str().expect("remote path"),
            "refs/heads/main",
        ])
        .current_dir(&work)
        .output()
        .expect("git send-pack via zmin receive-pack");
    assert!(
        output.status.success(),
        "git send-pack failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        git(&remote, ["rev-parse", "refs/heads/main"]),
        git(&work, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&remote, ["cat-file", "-p", "refs/heads/main:a.txt"]),
        "hello"
    );

    git(&work, ["checkout", "-b", "feature"]);
    fs::write(work.join("feature.txt"), b"feature\n").expect("write feature");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "feature"]);
    let output = Command::new("git")
        .args([
            "send-pack",
            "--receive-pack",
            &receive_pack,
            remote.to_str().expect("remote path"),
            "refs/heads/feature",
        ])
        .current_dir(&work)
        .output()
        .expect("git send-pack feature via zmin receive-pack");
    assert!(
        output.status.success(),
        "git send-pack feature failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        git(&remote, ["rev-parse", "refs/heads/feature"]),
        git(&work, ["rev-parse", "feature"])
    );

    let output = Command::new("git")
        .args([
            "send-pack",
            "--receive-pack",
            &receive_pack,
            remote.to_str().expect("remote path"),
            ":refs/heads/feature",
        ])
        .current_dir(&work)
        .output()
        .expect("git send-pack delete via zmin receive-pack");
    assert!(
        output.status.success(),
        "git send-pack delete failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_ne!(
        git_status(&remote, ["rev-parse", "--verify", "refs/heads/feature"]),
        0
    );
}

#[test]
fn upload_pack_advertisement_serves_stock_git_ls_remote() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(&work, ["branch", "feature"]);
    git_with_env(&work, ["tag", "-a", "v1", "-m", "tag message"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main", "feature", "--tags"]);

    let upload_pack = format!("{} upload-pack", shell_command_path(zmin_bin()));
    let output = Command::new("git")
        .args([
            "ls-remote",
            "--upload-pack",
            &upload_pack,
            remote.to_str().expect("remote path"),
        ])
        .output()
        .expect("git ls-remote via zmin upload-pack");
    assert!(
        output.status.success(),
        "git ls-remote failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).expect("stdout utf8");
    assert_eq!(
        actual.trim_end_matches('\n'),
        git(&work, ["ls-remote", remote.to_str().expect("remote path")])
    );
}

#[test]
fn upload_pack_serves_stock_git_clone_protocol_v1() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let clone = dir.path().join("clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write a");
    fs::create_dir_all(work.join("dir")).expect("create dir");
    fs::write(work.join("dir/b.txt"), b"world\n").expect("write b");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    git(&remote, ["symbolic-ref", "HEAD", "refs/heads/main"]);

    let upload_pack = format!("{} upload-pack", shell_command_path(zmin_bin()));
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.version=0",
            "clone",
            "--no-local",
            "--upload-pack",
            &upload_pack,
            remote.to_str().expect("remote path"),
            clone.to_str().expect("clone path"),
        ])
        .output()
        .expect("git clone via zmin upload-pack");
    assert!(
        output.status.success(),
        "git clone failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(clone.join("a.txt")).expect("read a"),
        "hello\n"
    );
    assert_eq!(
        fs::read_to_string(clone.join("dir/b.txt")).expect("read b"),
        "world\n"
    );
    assert_eq!(
        git(&clone, ["rev-parse", "HEAD"]),
        git(&work, ["rev-parse", "HEAD"])
    );
}

#[test]
fn shell_dispatches_stock_git_upload_pack_command() {
    let dir = TempDir::new().expect("temp dir");
    let remote = dir.path().join("remote.git");
    let work = dir.path().join("work");
    let clone = dir.path().join("clone");
    git(dir.path(), ["init", "--bare", "remote.git"]);
    git(dir.path(), ["init", "-b", "main", "work"]);
    configure_identity(&work);
    fs::write(work.join("a.txt"), b"hello\n").expect("write fixture");
    git(&work, ["add", "-A"]);
    git_with_env(&work, ["commit", "-m", "initial"]);
    git(
        &work,
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(&work, ["push", "-q", "origin", "main"]);
    git(&remote, ["symbolic-ref", "HEAD", "refs/heads/main"]);

    let shell_upload_pack = format!(
        "{} shell -c git-upload-pack",
        shell_command_path(zmin_bin())
    );
    let output = Command::new("git")
        .args([
            "-c",
            "protocol.version=0",
            "clone",
            "--no-local",
            "--upload-pack",
            shell_upload_pack.as_str(),
            remote.to_str().expect("remote path"),
            clone.to_str().expect("clone path"),
        ])
        .output()
        .expect("git clone via zmin shell upload-pack");
    assert!(
        output.status.success(),
        "git clone via shell failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(clone.join("a.txt")).expect("read clone file"),
        "hello\n"
    );
    assert_eq!(
        git(&clone, ["rev-parse", "HEAD"]),
        git(&work, ["rev-parse", "HEAD"])
    );
}
