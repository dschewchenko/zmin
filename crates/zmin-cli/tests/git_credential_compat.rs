mod common;

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use common::{command_stdout_bytes, zmin_bin};
use tempfile::TempDir;

#[test]
fn credential_matches_stock_git_for_basic_protocol_flows() {
    let repo = common::git_init();
    let home = TempDir::new().expect("temp home");
    let complete = "protocol=https\nhost=example.com\nusername=u\npassword=p\n\n";
    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            home.path(),
            repo.path(),
            &["credential", "fill"],
            complete,
            "git credential fill complete",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            home.path(),
            repo.path(),
            &["credential", "fill"],
            complete,
            "zmin credential fill complete",
        )
    );
    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            home.path(),
            repo.path(),
            &["credential", "approve"],
            complete,
            "git credential approve complete",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            home.path(),
            repo.path(),
            &["credential", "approve"],
            complete,
            "zmin credential approve complete",
        )
    );
    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            home.path(),
            repo.path(),
            &["credential", "reject"],
            complete,
            "git credential reject complete",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            home.path(),
            repo.path(),
            &["credential", "reject"],
            complete,
            "zmin credential reject complete",
        )
    );
    let missing = "protocol=https\nhost=example.com\n\n";
    assert_eq!(
        command_status_with_home_cwd_stdin(
            "git",
            home.path(),
            repo.path(),
            &["credential", "fill"],
            missing,
            "git credential fill missing",
        ),
        command_status_with_home_cwd_stdin(
            zmin_bin(),
            home.path(),
            repo.path(),
            &["credential", "fill"],
            missing,
            "zmin credential fill missing",
        )
    );
}

#[cfg(unix)]
#[test]
fn credential_command_line_helper_works_outside_repository_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let home = dir.path().join("home");
    fs::create_dir_all(&home).expect("create home");
    let helper_dir = dir.path().join("helpers");
    fs::create_dir_all(&helper_dir).expect("create helper dir");
    write_test_credential_helpers(&helper_dir);
    let cwd = dir.path().join("cwd");
    fs::create_dir_all(&cwd).expect("create cwd");
    let stdin = "protocol=http\nhost=example.com\n\n";
    assert_eq!(
        command_status_with_home_cwd_stdin_and_path(
            "git",
            &home,
            &cwd,
            helper_dir.as_path(),
            &[
                "-c",
                "credential.helper=verbatim foo bar",
                "credential",
                "fill"
            ],
            stdin,
            "stock non-repo credential helper",
        ),
        command_status_with_home_cwd_stdin_and_path(
            zmin_bin(),
            &home,
            &cwd,
            helper_dir.as_path(),
            &[
                "-c",
                "credential.helper=verbatim foo bar",
                "credential",
                "fill"
            ],
            stdin,
            "zmin non-repo credential helper",
        )
    );
}

#[cfg(unix)]
#[test]
fn credential_url_scoped_helper_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let home = dir.path().join("home");
    fs::create_dir_all(&home).expect("create home");
    let helper_dir = dir.path().join("helpers");
    fs::create_dir_all(&helper_dir).expect("create helper dir");
    write_test_credential_helpers(&helper_dir);
    let repo = dir.path().join("repo");
    Command::new("git")
        .args(["init", "-q", repo.to_str().expect("repo path")])
        .status()
        .expect("init repo");
    Command::new("git")
        .current_dir(&repo)
        .args([
            "config",
            "credential.https://example.com.helper",
            "verbatim foo bar",
        ])
        .status()
        .expect("set url helper");
    let stdin = "url=https://example.com/repo.git\n\n";
    assert_eq!(
        command_status_with_home_cwd_stdin_and_path(
            "git",
            &home,
            &repo,
            helper_dir.as_path(),
            &["credential", "fill"],
            stdin,
            "stock url helper",
        ),
        command_status_with_home_cwd_stdin_and_path(
            zmin_bin(),
            &home,
            &repo,
            helper_dir.as_path(),
            &["credential", "fill"],
            stdin,
            "zmin url helper",
        )
    );
}

#[cfg(unix)]
#[test]
fn credential_uses_core_askpass_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let home = dir.path().join("home");
    fs::create_dir_all(&home).expect("create home");
    let repo = dir.path().join("repo");
    Command::new("git")
        .args(["init", "-q", repo.to_str().expect("repo path")])
        .status()
        .expect("init repo");
    let askpass = dir.path().join("askpass.sh");
    fs::write(
        &askpass,
        "#!/bin/sh\necho >&2 \"askpass invoked\"\necho alternate-value\n",
    )
    .expect("write askpass");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&askpass, fs::Permissions::from_mode(0o755)).expect("chmod askpass");
    Command::new("git")
        .current_dir(&repo)
        .args([
            "config",
            "core.askpass",
            askpass.to_str().expect("askpass path"),
        ])
        .status()
        .expect("set core.askpass");
    let stdin = "protocol=http\nhost=example.com\n\n";
    assert_eq!(
        command_status_with_home_cwd_stdin_and_path(
            "git",
            &home,
            &repo,
            dir.path(),
            &["credential", "fill"],
            stdin,
            "stock core.askpass",
        ),
        command_status_with_home_cwd_stdin_and_path(
            zmin_bin(),
            &home,
            &repo,
            dir.path(),
            &["credential", "fill"],
            stdin,
            "zmin core.askpass",
        )
    );
}

#[test]
fn credential_store_matches_stock_git_for_store_get_and_erase() {
    let git_home = TempDir::new().expect("git home");
    let zmin_home = TempDir::new().expect("zmin home");
    let complete = "protocol=https\nhost=example.com\nusername=u\npassword=p\n\n";
    let query = "protocol=https\nhost=example.com\n\n";
    let erase = "protocol=https\nhost=example.com\nusername=u\n\n";

    assert_eq!(
        command_with_home_stdin(
            zmin_bin(),
            zmin_home.path(),
            &["credential-store", "store"],
            complete
        ),
        command_with_home_stdin(
            "git",
            git_home.path(),
            &["credential-store", "store"],
            complete
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_home.path().join(".git-credentials"))
            .expect("read zmin credentials"),
        fs::read_to_string(git_home.path().join(".git-credentials")).expect("read git credentials")
    );
    assert_eq!(
        command_with_home_stdin(
            zmin_bin(),
            zmin_home.path(),
            &["credential-store", "get"],
            query
        ),
        command_with_home_stdin("git", git_home.path(), &["credential-store", "get"], query)
    );
    assert_eq!(
        command_with_home_stdin(
            zmin_bin(),
            zmin_home.path(),
            &["credential-store", "erase"],
            erase
        ),
        command_with_home_stdin(
            "git",
            git_home.path(),
            &["credential-store", "erase"],
            erase
        )
    );
    assert_eq!(
        fs::read_to_string(zmin_home.path().join(".git-credentials"))
            .expect("read zmin credentials after erase"),
        fs::read_to_string(git_home.path().join(".git-credentials"))
            .expect("read git credentials after erase")
    );
}

#[test]
fn credential_fill_uses_configured_store_helpers_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_home = dir.path().join("git-home");
    let zmin_home = dir.path().join("zmin-home");
    fs::create_dir_all(&git_home).expect("create git home");
    fs::create_dir_all(&zmin_home).expect("create zmin home");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    Command::new("git")
        .args(["init", "-q", git_repo.to_str().expect("git repo path")])
        .status()
        .expect("init git repo");
    Command::new("git")
        .args(["init", "-q", zmin_repo.to_str().expect("zmin repo path")])
        .status()
        .expect("init zmin repo");
    let git_first = dir.path().join("git first credentials");
    let git_second = dir.path().join("git second credentials");
    let zmin_first = dir.path().join("zmin first credentials");
    let zmin_second = dir.path().join("zmin second credentials");
    fs::write(&git_first, "https://first:one@example.com\n").expect("write git first creds");
    fs::write(&git_second, "https://second:two@example.com\n").expect("write git second creds");
    fs::write(&zmin_first, "https://first:one@example.com\n").expect("write zmin first creds");
    fs::write(&zmin_second, "https://second:two@example.com\n").expect("write zmin second creds");
    configure_helper(
        &git_repo,
        &format!("store --file '{}'", git_first.display()),
    );
    configure_helper(
        &git_repo,
        &format!("store --file '{}'", git_second.display()),
    );
    configure_helper(
        &zmin_repo,
        &format!("store --file '{}'", zmin_first.display()),
    );
    configure_helper(
        &zmin_repo,
        &format!("store --file '{}'", zmin_second.display()),
    );
    let query = "protocol=https\nhost=example.com\n\n";
    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "fill"],
            query,
            "git credential fill",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "fill"],
            query,
            "zmin credential fill",
        )
    );
}

#[test]
fn credential_approve_and_reject_use_configured_store_helper_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_home = dir.path().join("git-home");
    let zmin_home = dir.path().join("zmin-home");
    fs::create_dir_all(&git_home).expect("create git home");
    fs::create_dir_all(&zmin_home).expect("create zmin home");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    Command::new("git")
        .args(["init", "-q", git_repo.to_str().expect("git repo path")])
        .status()
        .expect("init git repo");
    Command::new("git")
        .args(["init", "-q", zmin_repo.to_str().expect("zmin repo path")])
        .status()
        .expect("init zmin repo");
    let git_store = dir.path().join("git helper credentials");
    let zmin_store = dir.path().join("zmin helper credentials");
    configure_helper(
        &git_repo,
        &format!("store --file '{}'", git_store.display()),
    );
    configure_helper(
        &zmin_repo,
        &format!("store --file '{}'", zmin_store.display()),
    );
    let complete = "protocol=https\nhost=example.com\nusername=u\npassword=p\n\n";
    let query = "protocol=https\nhost=example.com\n\n";
    let erase = "protocol=https\nhost=example.com\nusername=u\n\n";

    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "approve"],
            complete,
            "git credential approve",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "approve"],
            complete,
            "zmin credential approve",
        )
    );
    assert_eq!(
        fs::read_to_string(&git_store).expect("read git helper store"),
        fs::read_to_string(&zmin_store).expect("read zmin helper store")
    );
    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "fill"],
            query,
            "git credential fill",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "fill"],
            query,
            "zmin credential fill",
        )
    );
    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "reject"],
            erase,
            "git credential reject",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "reject"],
            erase,
            "zmin credential reject",
        )
    );
    let git_contents = fs::read_to_string(&git_store).unwrap_or_default();
    let zmin_contents = fs::read_to_string(&zmin_store).unwrap_or_default();
    assert_eq!(git_contents, zmin_contents);
}

#[cfg(unix)]
#[test]
fn credential_external_helpers_and_capability_sensitive_fill_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let home = dir.path().join("home");
    fs::create_dir_all(&home).expect("create home");
    let helper_dir = dir.path().join("helpers");
    fs::create_dir_all(&helper_dir).expect("create helper dir");
    write_test_credential_helpers(&helper_dir);
    let repo = dir.path().join("repo");
    Command::new("git")
        .args(["init", "-q", repo.to_str().expect("repo path")])
        .status()
        .expect("init repo");

    let cases = [
        (
            vec!["verbatim foo bar"],
            "fill",
            "protocol=http\nhost=example.com\n\n",
        ),
        (
            vec!["verbatim-cred Bearer token"],
            "fill",
            "capability[]=authtype\nprotocol=http\nhost=example.com\n\n",
        ),
        (
            vec!["verbatim-cred Bearer token"],
            "fill",
            "capability[]=authtype\ncapability[]=state\nprotocol=http\nhost=example.com\n\n",
        ),
        (
            vec!["verbatim one \"\"", "verbatim two three"],
            "fill",
            "protocol=http\nhost=example.com\n\n",
        ),
        (
            vec!["verbatim-with-expiry one two 5", "verbatim three four"],
            "fill",
            "protocol=http\nhost=example.com\n\n",
        ),
        (
            vec!["useless", "verbatim foo bar"],
            "approve",
            "protocol=http\nhost=example.com\nusername=foo\npassword=bar\n\n",
        ),
        (
            vec!["definitely-missing-helper", "verbatim foo bar"],
            "fill",
            "protocol=http\nhost=example.com\n\n",
        ),
        (
            vec!["useless"],
            "approve",
            "protocol=http\nhost=example.com\nusername=foo\n\n",
        ),
        (
            vec!["useless"],
            "approve",
            "protocol=http\nhost=example.com\nusername=foo\npassword=bar\npassword_expiry_utc=5\n\n",
        ),
    ];

    for (index, (helpers, operation, stdin)) in cases.iter().enumerate() {
        reset_helpers(&repo);
        for helper in helpers {
            configure_helper(&repo, helper);
        }
        let label = format!("credential helper parity case {index}");
        assert_eq!(
            command_status_with_home_cwd_stdin_and_path(
                "git",
                &home,
                &repo,
                helper_dir.as_path(),
                &["credential", operation],
                stdin,
                &format!("stock {label}"),
            ),
            command_status_with_home_cwd_stdin_and_path(
                zmin_bin(),
                &home,
                &repo,
                helper_dir.as_path(),
                &["credential", operation],
                stdin,
                &format!("zmin {label}"),
            ),
            "{label}"
        );
    }
}

#[cfg(unix)]
#[test]
fn credential_cache_matches_stock_git_for_store_get_and_erase() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::set_permissions(
        dir.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .expect("tighten socket dir permissions");
    let git_socket = dir.path().join("git.sock");
    let zmin_socket = dir.path().join("zmin.sock");
    let git_socket_arg = format!("--socket={}", git_socket.display());
    let zmin_socket_arg = format!("--socket={}", zmin_socket.display());
    let complete = "protocol=https\nhost=example.com\nusername=u\npassword=p\n\n";
    let query = "protocol=https\nhost=example.com\n\n";
    let erase = "protocol=https\nhost=example.com\nusername=u\n\n";

    assert_eq!(
        command_with_stdin(
            zmin_bin(),
            dir.path(),
            &["credential-cache", zmin_socket_arg.as_str(), "store"],
            complete,
            "zmin credential-cache store",
        ),
        command_with_stdin(
            "git",
            dir.path(),
            &["credential-cache", git_socket_arg.as_str(), "store"],
            complete,
            "git credential-cache store",
        )
    );
    assert_eq!(
        command_with_stdin(
            zmin_bin(),
            dir.path(),
            &["credential-cache", zmin_socket_arg.as_str(), "get"],
            query,
            "zmin credential-cache get",
        ),
        command_with_stdin(
            "git",
            dir.path(),
            &["credential-cache", git_socket_arg.as_str(), "get"],
            query,
            "git credential-cache get",
        )
    );
    assert_eq!(
        command_with_stdin(
            zmin_bin(),
            dir.path(),
            &["credential-cache", zmin_socket_arg.as_str(), "erase"],
            erase,
            "zmin credential-cache erase",
        ),
        command_with_stdin(
            "git",
            dir.path(),
            &["credential-cache", git_socket_arg.as_str(), "erase"],
            erase,
            "git credential-cache erase",
        )
    );
    assert_eq!(
        command_with_stdin(
            zmin_bin(),
            dir.path(),
            &["credential-cache", zmin_socket_arg.as_str(), "get"],
            query,
            "zmin credential-cache get after erase",
        ),
        command_with_stdin(
            "git",
            dir.path(),
            &["credential-cache", git_socket_arg.as_str(), "get"],
            query,
            "git credential-cache get after erase",
        )
    );
    command_stdout_bytes(
        zmin_bin(),
        dir.path(),
        &["credential-cache", zmin_socket_arg.as_str(), "exit"],
    );
    command_stdout_bytes(
        "git",
        dir.path(),
        &["credential-cache", git_socket_arg.as_str(), "exit"],
    );
}

#[cfg(unix)]
#[test]
fn credential_fill_approve_and_reject_use_configured_cache_helper_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::set_permissions(
        dir.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .expect("tighten socket dir permissions");
    let git_home = dir.path().join("git-home");
    let zmin_home = dir.path().join("zmin-home");
    fs::create_dir_all(&git_home).expect("create git home");
    fs::create_dir_all(&zmin_home).expect("create zmin home");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    Command::new("git")
        .args(["init", "-q", git_repo.to_str().expect("git repo path")])
        .status()
        .expect("init git repo");
    Command::new("git")
        .args(["init", "-q", zmin_repo.to_str().expect("zmin repo path")])
        .status()
        .expect("init zmin repo");
    let git_socket = dir.path().join("git helper.sock");
    let zmin_socket = dir.path().join("zmin helper.sock");
    configure_helper(
        &git_repo,
        &format!("cache --socket='{}' --timeout=60", git_socket.display()),
    );
    configure_helper(
        &zmin_repo,
        &format!("cache --socket='{}' --timeout=60", zmin_socket.display()),
    );
    let complete = "protocol=https\nhost=example.com\nusername=u\npassword=p\n\n";
    let query = "protocol=https\nhost=example.com\n\n";
    let erase = "protocol=https\nhost=example.com\nusername=u\n\n";

    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "approve"],
            complete,
            "git credential approve cache",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "approve"],
            complete,
            "zmin credential approve cache",
        )
    );
    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "fill"],
            query,
            "git credential fill cache",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "fill"],
            query,
            "zmin credential fill cache",
        )
    );
    assert_eq!(
        command_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "reject"],
            erase,
            "git credential reject cache",
        ),
        command_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "reject"],
            erase,
            "zmin credential reject cache",
        )
    );
    assert_eq!(
        command_status_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["credential", "fill"],
            query,
            "git credential fill cache after reject",
        ),
        command_status_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["credential", "fill"],
            query,
            "zmin credential fill cache after reject",
        )
    );
    command_stdout_bytes(
        zmin_bin(),
        &zmin_repo,
        &[
            "credential-cache",
            &format!("--socket={}", zmin_socket.display()),
            "exit",
        ],
    );
    command_stdout_bytes(
        "git",
        &git_repo,
        &[
            "credential-cache",
            &format!("--socket={}", git_socket.display()),
            "exit",
        ],
    );
}

#[cfg(unix)]
#[test]
fn credential_cache_askpass_prompt_spacing_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_home = dir.path().join("git-home");
    let zmin_home = dir.path().join("zmin-home");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    fs::create_dir_all(&git_home).expect("create git home");
    fs::create_dir_all(&zmin_home).expect("create zmin home");
    Command::new("git")
        .args(["init", "-q", git_repo.to_str().expect("git repo path")])
        .status()
        .expect("init git repo");
    Command::new("git")
        .args(["init", "-q", zmin_repo.to_str().expect("zmin repo path")])
        .status()
        .expect("init zmin repo");
    let askpass = dir.path().join("askpass.sh");
    fs::write(
        &askpass,
        "#!/bin/sh\nprintf 'askpass: %s\\n' \"$1\" >&2\ncase \"$1\" in\n  Username*) echo askpass-username ;;\n  Password*) echo askpass-password ;;\n  *) echo unexpected ;;\nesac\n",
    )
    .expect("write askpass");
    std::fs::set_permissions(
        &askpass,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .expect("chmod askpass");
    let input = "protocol=https\nhost=example.com\n\n";

    let git = command_status_with_home_cwd_stdin_and_env(
        "git",
        &git_home,
        &git_repo,
        &[
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_ASKPASS", askpass.to_str().expect("askpass path")),
        ],
        &["-c", "credential.helper=cache", "credential", "fill"],
        input,
        "git credential cache askpass spacing",
    );
    let zmin = command_status_with_home_cwd_stdin_and_env(
        zmin_bin(),
        &zmin_home,
        &zmin_repo,
        &[
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_ASKPASS", askpass.to_str().expect("askpass path")),
        ],
        &["-c", "credential.helper=cache", "credential", "fill"],
        input,
        "zmin credential cache askpass spacing",
    );

    assert_eq!(git, zmin);
}

#[cfg(unix)]
#[test]
fn credential_cache_long_home_socket_path_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let long_suffix = "nested-home-path-for-unix-socket-length-check-credential-cache";
    let git_home = dir.path().join("git").join(long_suffix);
    let zmin_home = dir.path().join("zmin").join(long_suffix);
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    fs::create_dir_all(&git_home).expect("create git home");
    fs::create_dir_all(&zmin_home).expect("create zmin home");
    Command::new("git")
        .args(["init", "-q", git_repo.to_str().expect("git repo path")])
        .status()
        .expect("init git repo");
    Command::new("git")
        .args(["init", "-q", zmin_repo.to_str().expect("zmin repo path")])
        .status()
        .expect("init zmin repo");

    let store = "protocol=https\nhost=example.com\nusername=store-user\npassword=store-pass\n\n";
    let query = "protocol=https\nhost=example.com\n\n";

    assert_eq!(
        command_status_with_home_cwd_stdin(
            "git",
            &git_home,
            &git_repo,
            &["-c", "credential.helper=cache", "credential", "approve"],
            store,
            "git credential cache approve long home",
        ),
        command_status_with_home_cwd_stdin(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &["-c", "credential.helper=cache", "credential", "approve"],
            store,
            "zmin credential cache approve long home",
        )
    );
    assert_eq!(
        command_status_with_home_cwd_stdin_and_env(
            "git",
            &git_home,
            &git_repo,
            &[("GIT_TERMINAL_PROMPT", "0"), ("GIT_ASKPASS", "true")],
            &["-c", "credential.helper=cache", "credential", "fill"],
            query,
            "git credential cache fill long home",
        ),
        command_status_with_home_cwd_stdin_and_env(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &[("GIT_TERMINAL_PROMPT", "0"), ("GIT_ASKPASS", "true")],
            &["-c", "credential.helper=cache", "credential", "fill"],
            query,
            "zmin credential cache fill long home",
        )
    );
}

#[cfg(unix)]
#[test]
fn credential_cache_use_http_path_prompt_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_home = dir.path().join("git-home");
    let zmin_home = dir.path().join("zmin-home");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    fs::create_dir_all(&git_home).expect("create git home");
    fs::create_dir_all(&zmin_home).expect("create zmin home");
    Command::new("git")
        .args(["init", "-q", git_repo.to_str().expect("git repo path")])
        .status()
        .expect("init git repo");
    Command::new("git")
        .args(["init", "-q", zmin_repo.to_str().expect("zmin repo path")])
        .status()
        .expect("init zmin repo");
    Command::new("git")
        .current_dir(&git_repo)
        .args(["config", "credential.useHttpPath", "true"])
        .status()
        .expect("set git useHttpPath");
    Command::new("git")
        .current_dir(&zmin_repo)
        .args(["config", "credential.useHttpPath", "true"])
        .status()
        .expect("set zmin useHttpPath");
    let askpass = dir.path().join("askpass.sh");
    fs::write(
        &askpass,
        "#!/bin/sh\nprintf 'askpass: %s\\n' \"$1\" >&2\ncase \"$1\" in\n  Username*) echo askpass-username ;;\n  Password*) echo askpass-password ;;\n  *) echo unexpected ;;\nesac\n",
    )
    .expect("write askpass");
    std::fs::set_permissions(
        &askpass,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .expect("chmod askpass");
    let store = "protocol=http\nhost=path.tld\npath=foo.git\nusername=user\npassword=pass\n\n";
    let query = "protocol=http\nhost=path.tld\npath=bar.git\n\n";

    let _ = command_status_with_home_cwd_stdin(
        "git",
        &git_home,
        &git_repo,
        &["-c", "credential.helper=cache", "credential", "approve"],
        store,
        "git credential cache approve http path",
    );
    let _ = command_status_with_home_cwd_stdin(
        zmin_bin(),
        &zmin_home,
        &zmin_repo,
        &["-c", "credential.helper=cache", "credential", "approve"],
        store,
        "zmin credential cache approve http path",
    );

    assert_eq!(
        command_status_with_home_cwd_stdin_and_env(
            "git",
            &git_home,
            &git_repo,
            &[
                ("GIT_TERMINAL_PROMPT", "0"),
                ("GIT_ASKPASS", askpass.to_str().expect("askpass path")),
            ],
            &["-c", "credential.helper=cache", "credential", "fill"],
            query,
            "git credential cache fill http path",
        ),
        command_status_with_home_cwd_stdin_and_env(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &[
                ("GIT_TERMINAL_PROMPT", "0"),
                ("GIT_ASKPASS", askpass.to_str().expect("askpass path")),
            ],
            &["-c", "credential.helper=cache", "credential", "fill"],
            query,
            "zmin credential cache fill http path",
        )
    );
}

#[cfg(unix)]
#[test]
fn credential_cache_long_header_fill_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_home = dir.path().join("git-home");
    let zmin_home = dir.path().join("zmin-home");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    fs::create_dir_all(&git_home).expect("create git home");
    fs::create_dir_all(&zmin_home).expect("create zmin home");
    Command::new("git")
        .args(["init", "-q", git_repo.to_str().expect("git repo path")])
        .status()
        .expect("init git repo");
    Command::new("git")
        .args(["init", "-q", zmin_repo.to_str().expect("zmin repo path")])
        .status()
        .expect("init zmin repo");
    let askpass = dir.path().join("askpass.sh");
    fs::write(
        &askpass,
        "#!/bin/sh\nprintf 'askpass: %s\\n' \"$1\" >&2\ncase \"$1\" in\n  Username*) echo askpass-username ;;\n  Password*) echo askpass-password ;;\n  *) echo unexpected ;;\nesac\n",
    )
    .expect("write askpass");
    std::fs::set_permissions(
        &askpass,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .expect("chmod askpass");
    let store = "protocol=https\nhost=victim.example.com\nusername=user\npassword=to-be-stolen\n\n";
    let long_value = "a".repeat(1001);
    let query = format!(
        "protocol=https\nhost=badguy.example.com\nwwwauth[]=basic realm={long_value}host=victim.example.com\n\n"
    );

    let _ = command_status_with_home_cwd_stdin(
        "git",
        &git_home,
        &git_repo,
        &["-c", "credential.helper=cache", "credential", "approve"],
        store,
        "git credential cache approve long header",
    );
    let _ = command_status_with_home_cwd_stdin(
        zmin_bin(),
        &zmin_home,
        &zmin_repo,
        &["-c", "credential.helper=cache", "credential", "approve"],
        store,
        "zmin credential cache approve long header",
    );

    assert_eq!(
        command_status_with_home_cwd_stdin_and_env(
            "git",
            &git_home,
            &git_repo,
            &[
                ("GIT_TERMINAL_PROMPT", "0"),
                ("GIT_ASKPASS", askpass.to_str().expect("askpass path")),
            ],
            &["-c", "credential.helper=cache", "credential", "fill"],
            &query,
            "git credential cache fill long header",
        ),
        command_status_with_home_cwd_stdin_and_env(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &[
                ("GIT_TERMINAL_PROMPT", "0"),
                ("GIT_ASKPASS", askpass.to_str().expect("askpass path")),
            ],
            &["-c", "credential.helper=cache", "credential", "fill"],
            &query,
            "zmin credential cache fill long header",
        )
    );
}

#[cfg(unix)]
#[test]
fn credential_cache_authtype_username_mismatch_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_home = dir.path().join("git-home");
    let zmin_home = dir.path().join("zmin-home");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    fs::create_dir_all(&git_home).expect("create git home");
    fs::create_dir_all(&zmin_home).expect("create zmin home");
    Command::new("git")
        .args(["init", "-q", git_repo.to_str().expect("git repo path")])
        .status()
        .expect("init git repo");
    Command::new("git")
        .args(["init", "-q", zmin_repo.to_str().expect("zmin repo path")])
        .status()
        .expect("init zmin repo");
    let askpass = dir.path().join("askpass.sh");
    fs::write(
        &askpass,
        "#!/bin/sh\nprintf 'askpass: %s\\n' \"$1\" >&2\ncase \"$1\" in\n  Username*) echo askpass-username ;;\n  Password*) echo askpass-password ;;\n  *) echo unexpected ;;\nesac\n",
    )
    .expect("write askpass");
    std::fs::set_permissions(
        &askpass,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .expect("chmod askpass");
    let store = "capability[]=authtype\nauthtype=Bearer\ncredential=other-token\nprotocol=https\nhost=git.example.com\nusername=foobar\n\n";
    let query = "capability[]=authtype\nprotocol=https\nhost=git.example.com\nusername=barbaz\n\n";

    let _ = command_status_with_home_cwd_stdin(
        "git",
        &git_home,
        &git_repo,
        &["-c", "credential.helper=cache", "credential", "approve"],
        store,
        "git credential cache approve authtype mismatch",
    );
    let _ = command_status_with_home_cwd_stdin(
        zmin_bin(),
        &zmin_home,
        &zmin_repo,
        &["-c", "credential.helper=cache", "credential", "approve"],
        store,
        "zmin credential cache approve authtype mismatch",
    );

    assert_eq!(
        command_status_with_home_cwd_stdin_and_env(
            "git",
            &git_home,
            &git_repo,
            &[
                ("GIT_TERMINAL_PROMPT", "0"),
                ("GIT_ASKPASS", askpass.to_str().expect("askpass path")),
            ],
            &["-c", "credential.helper=cache", "credential", "fill"],
            query,
            "git credential cache fill authtype mismatch",
        ),
        command_status_with_home_cwd_stdin_and_env(
            zmin_bin(),
            &zmin_home,
            &zmin_repo,
            &[
                ("GIT_TERMINAL_PROMPT", "0"),
                ("GIT_ASKPASS", askpass.to_str().expect("askpass path")),
            ],
            &["-c", "credential.helper=cache", "credential", "fill"],
            query,
            "zmin credential cache fill authtype mismatch",
        )
    );
}

#[cfg(unix)]
#[test]
fn credential_cache_ephemeral_entries_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_home = dir.path().join("git-home");
    let zmin_home = dir.path().join("zmin-home");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    fs::create_dir_all(&git_home).expect("create git home");
    fs::create_dir_all(&zmin_home).expect("create zmin home");
    Command::new("git")
        .args(["init", "-q", git_repo.to_str().expect("git repo path")])
        .status()
        .expect("init git repo");
    Command::new("git")
        .args(["init", "-q", zmin_repo.to_str().expect("zmin repo path")])
        .status()
        .expect("init zmin repo");
    let askpass = dir.path().join("askpass.sh");
    fs::write(
        &askpass,
        "#!/bin/sh\nprintf 'askpass: %s\\n' \"$1\" >&2\ncase \"$1\" in\n  Username*) echo askpass-username ;;\n  Password*) echo askpass-password ;;\n  *) echo unexpected ;;\nesac\n",
    )
    .expect("write askpass");
    std::fs::set_permissions(
        &askpass,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .expect("chmod askpass");
    let query = "capability[]=authtype\nprotocol=https\nhost=git2.example.com\n\n";

    for store in [
        "capability[]=authtype\nauthtype=Bearer\ncredential=git2-token\nprotocol=https\nhost=git2.example.com\nephemeral=1\n\n",
        "capability[]=authtype\nprotocol=https\nhost=git2.example.com\nuser=barbaz\npassword=secret\nephemeral=1\n\n",
    ] {
        assert_eq!(
            command_status_with_home_cwd_stdin(
                "git",
                &git_home,
                &git_repo,
                &["-c", "credential.helper=cache", "credential", "approve"],
                store,
                "git credential cache approve ephemeral",
            ),
            command_status_with_home_cwd_stdin(
                zmin_bin(),
                &zmin_home,
                &zmin_repo,
                &["-c", "credential.helper=cache", "credential", "approve"],
                store,
                "zmin credential cache approve ephemeral",
            )
        );
        assert_eq!(
            command_status_with_home_cwd_stdin_and_env(
                "git",
                &git_home,
                &git_repo,
                &[
                    ("GIT_TERMINAL_PROMPT", "0"),
                    ("GIT_ASKPASS", askpass.to_str().expect("askpass path")),
                ],
                &["-c", "credential.helper=cache", "credential", "fill"],
                query,
                "git credential cache fill ephemeral",
            ),
            command_status_with_home_cwd_stdin_and_env(
                zmin_bin(),
                &zmin_home,
                &zmin_repo,
                &[
                    ("GIT_TERMINAL_PROMPT", "0"),
                    ("GIT_ASKPASS", askpass.to_str().expect("askpass path")),
                ],
                &["-c", "credential.helper=cache", "credential", "fill"],
                query,
                "zmin credential cache fill ephemeral",
            )
        );
    }
}

#[cfg(unix)]
#[test]
fn credential_cache_reject_with_distinct_password_preserves_matching_user_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::set_permissions(
        dir.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .expect("tighten socket dir permissions");
    let git_socket = dir.path().join("git.sock");
    let zmin_socket = dir.path().join("zmin.sock");
    let git_socket_arg = format!("--socket={}", git_socket.display());
    let zmin_socket_arg = format!("--socket={}", zmin_socket.display());

    let store_user1 = "protocol=https\nhost=example.com\nusername=user1\npassword=pass1\n\n";
    let store_user2 = "protocol=https\nhost=example.com\nusername=user2\npassword=pass2\n\n";
    let reject_user1_wrong_password =
        "protocol=https\nhost=example.com\nusername=user1\npassword=pass2\n\n";
    let query_user1 = "protocol=https\nhost=example.com\nusername=user1\n\n";
    let query_user2 = "protocol=https\nhost=example.com\nusername=user2\n\n";

    for stdin in [store_user1, store_user2] {
        assert_eq!(
            command_with_stdin(
                zmin_bin(),
                dir.path(),
                &["credential-cache", zmin_socket_arg.as_str(), "store"],
                stdin,
                "zmin credential-cache store distinct password regression",
            ),
            command_with_stdin(
                "git",
                dir.path(),
                &["credential-cache", git_socket_arg.as_str(), "store"],
                stdin,
                "git credential-cache store distinct password regression",
            )
        );
    }

    assert_eq!(
        command_with_stdin(
            zmin_bin(),
            dir.path(),
            &["credential-cache", zmin_socket_arg.as_str(), "erase"],
            reject_user1_wrong_password,
            "zmin credential-cache erase distinct password regression",
        ),
        command_with_stdin(
            "git",
            dir.path(),
            &["credential-cache", git_socket_arg.as_str(), "erase"],
            reject_user1_wrong_password,
            "git credential-cache erase distinct password regression",
        )
    );
    assert_eq!(
        command_with_stdin(
            zmin_bin(),
            dir.path(),
            &["credential-cache", zmin_socket_arg.as_str(), "get"],
            query_user1,
            "zmin credential-cache get user1 distinct password regression",
        ),
        command_with_stdin(
            "git",
            dir.path(),
            &["credential-cache", git_socket_arg.as_str(), "get"],
            query_user1,
            "git credential-cache get user1 distinct password regression",
        )
    );
    assert_eq!(
        command_with_stdin(
            zmin_bin(),
            dir.path(),
            &["credential-cache", zmin_socket_arg.as_str(), "get"],
            query_user2,
            "zmin credential-cache get user2 distinct password regression",
        ),
        command_with_stdin(
            "git",
            dir.path(),
            &["credential-cache", git_socket_arg.as_str(), "get"],
            query_user2,
            "git credential-cache get user2 distinct password regression",
        )
    );

    command_stdout_bytes(
        zmin_bin(),
        dir.path(),
        &["credential-cache", zmin_socket_arg.as_str(), "exit"],
    );
    command_stdout_bytes(
        "git",
        dir.path(),
        &["credential-cache", git_socket_arg.as_str(), "exit"],
    );
}

fn configure_helper(repo: &std::path::Path, helper: &str) {
    Command::new("git")
        .current_dir(repo)
        .args(["config", "--add", "credential.helper", helper])
        .status()
        .expect("configure helper");
}

fn reset_helpers(repo: &std::path::Path) {
    let _ = Command::new("git")
        .current_dir(repo)
        .args(["config", "--unset-all", "credential.helper"])
        .status();
}

#[cfg(unix)]
fn write_test_credential_helpers(dir: &std::path::Path) {
    write_helper_script(
        dir,
        "git-credential-useless",
        r#"#!/bin/sh
name=$(basename "$0")
name=${name#git-credential-}
echo >&2 "$name: $*"
while IFS= read -r line; do
    test -z "$line" && break
    echo >&2 "$name: $line"
done
"#,
    );
    write_helper_script(
        dir,
        "git-credential-verbatim",
        r#"#!/bin/sh
user=$1
pass=$2
name=$(basename "$0")
name=${name#git-credential-}
echo >&2 "$name: $*"
while IFS= read -r line; do
    test -z "$line" && break
    echo >&2 "$name: $line"
done
test -z "$user" || echo "username=$user"
test -z "$pass" || echo "password=$pass"
"#,
    );
    write_helper_script(
        dir,
        "git-credential-verbatim-cred",
        r#"#!/bin/sh
authtype=$1
credential=$2
name=$(basename "$0")
name=${name#git-credential-}
caps=""
echo >&2 "$name: $*"
while IFS= read -r line; do
    test -z "$line" && break
    echo >&2 "$name: $line"
    case "$line" in
        capability[]=*) caps="$caps ${line#capability[]=}" ;;
    esac
done
echo "capability[]=authtype"
echo "capability[]=state"
case " $caps " in
    *" authtype "*) ;;
    *) exit 0 ;;
esac
test -z "$authtype" || echo "authtype=$authtype"
test -z "$credential" || echo "credential=$credential"
case " $caps " in
    *" state "*) echo "state[]=verbatim-cred:foo" ;;
esac
"#,
    );
    write_helper_script(
        dir,
        "git-credential-verbatim-with-expiry",
        r#"#!/bin/sh
user=$1
pass=$2
expiry=$3
name=$(basename "$0")
name=${name#git-credential-}
echo >&2 "$name: $*"
while IFS= read -r line; do
    test -z "$line" && break
    echo >&2 "$name: $line"
done
test -z "$user" || echo "username=$user"
test -z "$pass" || echo "password=$pass"
test -z "$expiry" || echo "password_expiry_utc=$expiry"
"#,
    );
}

#[cfg(unix)]
fn write_helper_script(dir: &std::path::Path, name: &str, content: &str) {
    let path = dir.join(name);
    fs::write(&path, content).expect("write helper script");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod helper script");
}

fn command_with_home_stdin(
    command: &str,
    home: &std::path::Path,
    args: &[&str],
    stdin: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {command} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let (code, stdout, stderr) =
        command_status_with_home_cwd_stdin(command, home, cwd, args, stdin, label);
    assert_eq!(code, 0, "{label} failed: {stderr}");
    stdout
}

fn command_status_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> (i32, String, String) {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout)
            .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .unwrap_or_else(|err| panic!("{label} stderr utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
    )
}

fn command_status_with_home_cwd_stdin_and_path(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    helper_dir: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> (i32, String, String) {
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let path_with_helpers = std::env::join_paths(
        std::iter::once(helper_dir.to_path_buf()).chain(std::env::split_paths(&inherited_path)),
    )
    .expect("join helper path");
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .env("PATH", path_with_helpers)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout)
            .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .unwrap_or_else(|err| panic!("{label} stderr utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
    )
}

fn command_status_with_home_cwd_stdin_and_env(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    envs: &[(&str, &str)],
    args: &[&str],
    stdin: &str,
    label: &str,
) -> (i32, String, String) {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .envs(envs.iter().copied())
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout)
            .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .unwrap_or_else(|err| panic!("{label} stderr utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
    )
}
