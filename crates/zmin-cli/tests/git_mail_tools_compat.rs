mod common;

use std::fs;
use std::io::{BufRead, Read, Write};
use std::path::Path;

use common::{
    configure_identity, git, git_args, git_init, git_with_env, git_with_stdin, git_with_stdin_args,
    read_named_files, run_zmin, run_zmin_args, run_zmin_with_stdin, run_zmin_with_stdin_args,
    write_file,
};
use tempfile::TempDir;

fn local_file_url(path: &Path) -> String {
    let path = path.display().to_string();
    if cfg!(windows) {
        format!("file:///{}", path.replace('\\', "/"))
    } else {
        format!("file://{path}")
    }
}

struct FakeImapServer {
    port: u16,
    messages: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct FakeSmtpServer {
    port: u16,
    messages: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl FakeSmtpServer {
    fn new(expected_connections: usize) -> Self {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind fake smtp");
        let port = listener.local_addr().expect("local addr").port();
        let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let thread_messages = messages.clone();
        let handle = std::thread::spawn(move || {
            for _ in 0..expected_connections {
                let (stream, _) = listener.accept().expect("accept fake smtp");
                serve_fake_smtp(stream, thread_messages.clone());
            }
        });
        Self {
            port,
            messages,
            handle: Some(handle),
        }
    }

    fn sent_messages(&self) -> Vec<Vec<u8>> {
        self.messages.lock().expect("messages lock").clone()
    }
}

impl Drop for FakeSmtpServer {
    fn drop(&mut self) {
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl FakeImapServer {
    fn new() -> Self {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind fake imap");
        let port = listener.local_addr().expect("local addr").port();
        let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let thread_messages = messages.clone();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept fake imap");
            serve_fake_imap(stream, thread_messages);
        });
        Self {
            port,
            messages,
            handle: Some(handle),
        }
    }

    fn appended_messages(&self) -> Vec<Vec<u8>> {
        self.messages.lock().expect("messages lock").clone()
    }
}

impl Drop for FakeImapServer {
    fn drop(&mut self) {
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn serve_fake_smtp(
    stream: std::net::TcpStream,
    messages: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
) {
    let mut reader = std::io::BufReader::new(stream.try_clone().expect("clone fake smtp"));
    let mut writer = stream;
    writer
        .write_all(b"220 fake smtp ready\r\n")
        .expect("smtp greeting");
    let mut in_data = false;
    let mut message = Vec::new();
    loop {
        let mut line = Vec::new();
        if reader.read_until(b'\n', &mut line).expect("read smtp") == 0 {
            return;
        }
        if in_data {
            if line == b".\r\n" || line == b".\n" {
                messages
                    .lock()
                    .expect("messages lock")
                    .push(message.clone());
                message.clear();
                in_data = false;
                writer.write_all(b"250 queued\r\n").expect("data ok");
            } else {
                if line.starts_with(b"..") {
                    message.extend_from_slice(&line[1..]);
                } else {
                    message.extend_from_slice(&line);
                }
            }
            continue;
        }
        let command = String::from_utf8_lossy(&line).to_ascii_uppercase();
        if command.starts_with("EHLO ") || command.starts_with("HELO ") {
            writer
                .write_all(b"250-fake\r\n250 OK\r\n")
                .expect("ehlo response");
        } else if command.starts_with("MAIL FROM:") || command.starts_with("RCPT TO:") {
            writer.write_all(b"250 OK\r\n").expect("address response");
        } else if command.starts_with("DATA") {
            in_data = true;
            writer
                .write_all(b"354 end with dot\r\n")
                .expect("data response");
        } else if command.starts_with("QUIT") {
            writer.write_all(b"221 bye\r\n").expect("quit response");
            return;
        } else {
            writer.write_all(b"250 OK\r\n").expect("generic response");
        }
    }
}

fn serve_fake_imap(
    stream: std::net::TcpStream,
    messages: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
) {
    let mut reader = std::io::BufReader::new(stream.try_clone().expect("clone fake imap"));
    let mut writer = stream;
    writer
        .write_all(b"* OK fake imap ready\r\n")
        .expect("greeting");
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).expect("read imap line") == 0 {
            return;
        }
        let line = line.trim_end_matches(['\r', '\n']).to_owned();
        let tag = line.split_whitespace().next().unwrap_or("A0000").to_owned();
        if line.contains(" LOGIN ") {
            writeln!(writer, "{tag} OK LOGIN completed\r").expect("login response");
        } else if line.contains(" APPEND ") {
            let size = line
                .rsplit_once('{')
                .and_then(|(_, rest)| rest.strip_suffix('}'))
                .and_then(|value| value.parse::<usize>().ok())
                .expect("append size");
            writer
                .write_all(b"+ ready for literal\r\n")
                .expect("continue");
            let mut message = vec![0_u8; size];
            reader.read_exact(&mut message).expect("read literal");
            let mut crlf = [0_u8; 2];
            reader.read_exact(&mut crlf).expect("read literal crlf");
            messages.lock().expect("messages lock").push(message);
            writeln!(writer, "{tag} OK APPEND completed\r").expect("append response");
        } else if line.contains(" LIST ") {
            writer
                .write_all(b"* LIST () \"/\" \"INBOX.Drafts\"\r\n")
                .expect("list row");
            writeln!(writer, "{tag} OK LIST completed\r").expect("list response");
        } else if line.contains(" LOGOUT") {
            writer.write_all(b"* BYE logging out\r\n").expect("bye");
            writeln!(writer, "{tag} OK LOGOUT completed\r").expect("logout response");
            return;
        } else {
            writeln!(writer, "{tag} BAD unsupported\r").expect("bad response");
        }
    }
}

#[test]
fn interpret_trailers_matches_stock_git_for_common_modes() {
    let repo = git_init();
    let fixture = "Subject\n\nBody\n\nAcked-by: B\nSigned-off-by: A\n";
    for args in [
        ["interpret-trailers"].as_slice(),
        ["interpret-trailers", "--only-trailers"].as_slice(),
        ["interpret-trailers", "--parse"].as_slice(),
        [
            "interpret-trailers",
            "--trailer",
            "Reviewed-by: C <c@example.com>",
        ]
        .as_slice(),
        [
            "interpret-trailers",
            "--where",
            "before",
            "--trailer",
            "Acked-by: C",
        ]
        .as_slice(),
        [
            "interpret-trailers",
            "--where",
            "after",
            "--trailer",
            "Acked-by: C",
        ]
        .as_slice(),
        [
            "interpret-trailers",
            "--if-exists",
            "addIfDifferent",
            "--trailer",
            "Acked-by: B",
        ]
        .as_slice(),
        [
            "interpret-trailers",
            "--if-exists",
            "add",
            "--trailer",
            "Acked-by: B",
        ]
        .as_slice(),
        [
            "interpret-trailers",
            "--if-exists",
            "replace",
            "--trailer",
            "Acked-by: C",
        ]
        .as_slice(),
        [
            "interpret-trailers",
            "--if-missing",
            "doNothing",
            "--trailer",
            "Reviewed-by: C",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_with_stdin_args(repo.path(), args, fixture),
            git_with_stdin_args(repo.path(), args, fixture),
            "args: {args:?}"
        );
    }

    let folded = "Subject\n\nKey: first\n second\nOther: value\n";
    for args in [
        ["interpret-trailers", "--only-trailers"].as_slice(),
        ["interpret-trailers", "--only-trailers", "--unfold"].as_slice(),
        ["interpret-trailers", "--parse"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_with_stdin_args(repo.path(), args, folded),
            git_with_stdin_args(repo.path(), args, folded),
            "args: {args:?}"
        );
    }

    let patch = "Subject\n\nBody\n---\nSigned-off-by: not-trailer\n";
    for args in [
        [
            "interpret-trailers",
            "--trailer",
            "Reviewed-by: C <c@example.com>",
        ]
        .as_slice(),
        [
            "interpret-trailers",
            "--no-divider",
            "--trailer",
            "Reviewed-by: C <c@example.com>",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_with_stdin_args(repo.path(), args, patch),
            git_with_stdin_args(repo.path(), args, patch),
            "args: {args:?}"
        );
    }

    let empty_trailer = "Subject\n\nBody\n\nAcked-by:\nSigned-off-by: A\n";
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["interpret-trailers", "--trim-empty"],
            empty_trailer
        ),
        git_with_stdin(
            repo.path(),
            ["interpret-trailers", "--trim-empty"],
            empty_trailer
        )
    );
}

#[test]
fn interpret_trailers_matches_stock_git_in_place() {
    let repo = git_init();
    fs::write(repo.path().join("zmin-msg.txt"), "Subject\n\nBody\n").expect("write zmin msg");
    fs::write(repo.path().join("git-msg.txt"), "Subject\n\nBody\n").expect("write git msg");

    run_zmin(
        repo.path(),
        [
            "interpret-trailers",
            "--in-place",
            "--trailer",
            "Reviewed-by: C",
            "zmin-msg.txt",
        ],
    );
    git(
        repo.path(),
        [
            "interpret-trailers",
            "--in-place",
            "--trailer",
            "Reviewed-by: C",
            "git-msg.txt",
        ],
    );

    assert_eq!(
        fs::read_to_string(repo.path().join("zmin-msg.txt")).expect("read zmin msg"),
        fs::read_to_string(repo.path().join("git-msg.txt")).expect("read git msg")
    );
}

#[test]
fn mailsplit_matches_stock_git_for_mbox_and_maildir() {
    let repo = git_init();
    fs::create_dir(repo.path().join("zmin-out")).expect("create zmin out");
    fs::create_dir(repo.path().join("git-out")).expect("create git out");
    fs::write(
        repo.path().join("mbox"),
        "From a@example.com Tue Jan 1 00:00:00 2024\nSubject: A\n\n>From escaped\nbody a\n\nFrom b@example.com Tue Jan 2 00:00:00 2024\nSubject: B\n\nbody b\n",
    )
    .expect("write mbox");

    assert_eq!(
        run_zmin(
            repo.path(),
            ["mailsplit", "-d4", "-f3", "-ozmin-out", "mbox"]
        ),
        git(
            repo.path(),
            ["mailsplit", "-d4", "-f3", "-ogit-out", "mbox"]
        )
    );
    assert_eq!(
        read_named_files(&repo.path().join("zmin-out")),
        read_named_files(&repo.path().join("git-out"))
    );

    fs::create_dir_all(repo.path().join("maildir/new")).expect("create maildir new");
    fs::create_dir_all(repo.path().join("maildir/cur")).expect("create maildir cur");
    fs::create_dir_all(repo.path().join("maildir/tmp")).expect("create maildir tmp");
    fs::write(repo.path().join("maildir/new/1"), "Subject: N\n\nnew\n").expect("write new");
    fs::write(repo.path().join("maildir/cur/2"), "Subject: C\n\ncur\n").expect("write cur");
    fs::create_dir(repo.path().join("zmin-maildir-out")).expect("create zmin maildir out");
    fs::create_dir(repo.path().join("git-maildir-out")).expect("create git maildir out");

    assert_eq!(
        run_zmin(repo.path(), ["mailsplit", "-ozmin-maildir-out", "maildir"]),
        git(repo.path(), ["mailsplit", "-ogit-maildir-out", "maildir"])
    );
    assert_eq!(
        read_named_files(&repo.path().join("zmin-maildir-out")),
        read_named_files(&repo.path().join("git-maildir-out"))
    );
}

#[test]
fn mailinfo_matches_stock_git_for_common_patch_mail() {
    let repo = git_init();
    let mail = "From: Alice <alice@example.com>\nDate: Tue, 1 Jan 2024 00:00:00 +0000\nSubject: [PATCH v2 1/2] [topic] add file\nMessage-ID: <m1@example.com>\n\nCommit message body.\n\n---\n a.txt | 1 +\n 1 file changed, 1 insertion(+)\n\ndiff --git a/a.txt b/a.txt\nnew file mode 100644\nindex 0000000..7898192\n--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1 @@\n+hello\n";

    for args in [
        ["mailinfo", "zmin-msg", "zmin-patch"].as_slice(),
        ["mailinfo", "-k", "zmin-msg", "zmin-patch"].as_slice(),
        ["mailinfo", "-b", "zmin-msg", "zmin-patch"].as_slice(),
        ["mailinfo", "-m", "zmin-msg", "zmin-patch"].as_slice(),
    ] {
        let git_args = args
            .iter()
            .map(|arg| match *arg {
                "zmin-msg" => "git-msg",
                "zmin-patch" => "git-patch",
                other => other,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            run_zmin_with_stdin_args(repo.path(), args, mail),
            git_with_stdin_args(repo.path(), &git_args, mail),
            "args: {args:?}"
        );
        assert_eq!(
            fs::read_to_string(repo.path().join("zmin-msg")).expect("read zmin msg"),
            fs::read_to_string(repo.path().join("git-msg")).expect("read git msg"),
            "msg args: {args:?}"
        );
        assert_eq!(
            fs::read_to_string(repo.path().join("zmin-patch")).expect("read zmin patch"),
            fs::read_to_string(repo.path().join("git-patch")).expect("read git patch"),
            "patch args: {args:?}"
        );
    }
}

#[test]
fn fmt_merge_msg_matches_stock_git_for_fetch_head_titles() {
    let origin = git_init();
    git(origin.path(), ["checkout", "-b", "main"]);
    configure_identity(origin.path());
    git_with_env(origin.path(), ["commit", "--allow-empty", "-m", "init"]);
    git(origin.path(), ["checkout", "-b", "feature"]);
    git_with_env(origin.path(), ["commit", "--allow-empty", "-m", "feature"]);
    git(origin.path(), ["checkout", "main"]);

    let work = TempDir::new().expect("temp work");
    git(
        work.path(),
        ["clone", origin.path().to_str().expect("origin path"), "."],
    );
    git(work.path(), ["fetch", "origin", "feature"]);
    let fetch_head =
        fs::read_to_string(work.path().join(".git/FETCH_HEAD")).expect("read FETCH_HEAD");
    fs::write(work.path().join("fetch-head-copy"), &fetch_head).expect("copy FETCH_HEAD");

    for args in [
        ["fmt-merge-msg"].as_slice(),
        ["fmt-merge-msg", "--into-name", "trunk"].as_slice(),
        ["fmt-merge-msg", "-m", "Custom merge"].as_slice(),
        ["fmt-merge-msg", "-F", "fetch-head-copy"].as_slice(),
    ] {
        let zmin_output = if args.contains(&"-F") {
            run_zmin_args(work.path(), args)
        } else {
            run_zmin_with_stdin_args(work.path(), args, &fetch_head)
        };
        let git_output = if args.contains(&"-F") {
            git_args(work.path(), args)
        } else {
            git_with_stdin_args(work.path(), args, &fetch_head)
        };
        assert_eq!(zmin_output, git_output, "args: {args:?}");
    }
}

#[test]
fn request_pull_matches_stock_git_for_local_pushed_branch() {
    let remote = TempDir::new().expect("temp remote");
    git(remote.path(), ["init", "--bare"]);
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    git(
        repo.path(),
        [
            "remote",
            "add",
            "origin",
            remote.path().to_str().expect("remote path"),
        ],
    );
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    let start = git(repo.path(), ["rev-parse", "HEAD"]);
    git(repo.path(), ["push", "-u", "origin", "main"]);
    write_file(repo.path(), "b.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    git(repo.path(), ["push", "origin", "main"]);
    let url = local_file_url(remote.path());

    assert_eq!(
        run_zmin_args(repo.path(), &["request-pull", &start, &url, "main"]),
        git_args(repo.path(), &["request-pull", &start, &url, "main"])
    );
}

#[test]
fn send_email_alias_modes_match_stock_git() {
    let repo = git_init();
    let aliases = repo.path().join("aliases");
    fs::write(
        &aliases,
        "alias dev Dev One <dev@example.test>\nalias ops Ops <ops@example.test>\n",
    )
    .expect("write aliases");
    let alias_path = aliases.to_str().expect("alias path");
    git(repo.path(), ["config", "sendemail.aliasesfile", alias_path]);
    git(repo.path(), ["config", "sendemail.aliasfiletype", "mutt"]);

    assert_eq!(
        run_zmin(repo.path(), ["send-email", "--dump-aliases"]),
        git(repo.path(), ["send-email", "--dump-aliases"])
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["send-email", "--translate-aliases"],
            "dev\nops\nunknown@example.test\n",
        ),
        git_with_stdin(
            repo.path(),
            ["send-email", "--translate-aliases"],
            "dev\nops\nunknown@example.test\n",
        )
    );
}

#[test]
fn send_email_alias_file_types_match_stock_git() {
    for (alias_type, content) in [
        ("mutt", "alias dev dev@example.test\n"),
        ("mailrc", "alias dev dev@example.test\n"),
        ("pine", "dev\tDev\tdev@example.test\n"),
        ("elm", "dev = Dev = dev@example.test\n"),
        ("sendmail", "dev: dev@example.test\n"),
        ("gnus", "(define-mail-alias \"dev\" \"dev@example.test\")\n"),
        ("unknown", "alias dev dev@example.test\n"),
    ] {
        let repo = git_init();
        let aliases = repo.path().join("aliases");
        fs::write(&aliases, content).expect("write aliases");
        let alias_path = aliases.to_str().expect("alias path");
        git(repo.path(), ["config", "sendemail.aliasesfile", alias_path]);
        git(
            repo.path(),
            ["config", "sendemail.aliasfiletype", alias_type],
        );

        assert_eq!(
            run_zmin(repo.path(), ["send-email", "--dump-aliases"]),
            git(repo.path(), ["send-email", "--dump-aliases"]),
            "dump aliases for {alias_type}"
        );
        assert_eq!(
            run_zmin_with_stdin(repo.path(), ["send-email", "--translate-aliases"], "dev\n"),
            git_with_stdin(repo.path(), ["send-email", "--translate-aliases"], "dev\n"),
            "translate aliases for {alias_type}"
        );
    }
}

#[test]
fn send_email_sends_patch_to_configured_smtp_server() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    let patch = run_zmin(repo.path(), ["format-patch", "-1"]);
    let patch = repo.path().join(patch.trim());
    let server = FakeSmtpServer::new(1);
    git(
        repo.path(),
        [
            "config",
            "sendemail.smtpserver",
            &format!("smtp://127.0.0.1:{}", server.port),
        ],
    );
    git(
        repo.path(),
        ["config", "sendemail.from", "sender@example.test"],
    );
    git(
        repo.path(),
        ["config", "sendemail.to", "receiver@example.test"],
    );

    run_zmin_args(
        repo.path(),
        &["send-email", patch.to_str().expect("patch path")],
    );

    let messages = server.sent_messages();
    assert_eq!(messages.len(), 1);
    let message = String::from_utf8_lossy(&messages[0]);
    assert!(message.contains("From:"));
    assert!(message.contains("To: receiver@example.test"));
    assert!(message.contains("Subject: [PATCH"));
    assert!(message.contains("diff --git"));
}

#[test]
fn imap_send_appends_mbox_messages_to_plain_imap_server() {
    let repo = git_init();
    let server = FakeImapServer::new();
    git(repo.path(), ["config", "imap.folder", "INBOX.Drafts"]);
    git(
        repo.path(),
        [
            "config",
            "imap.host",
            &format!("imap://127.0.0.1:{}", server.port),
        ],
    );
    git(repo.path(), ["config", "imap.user", "user"]);
    git(repo.path(), ["config", "imap.pass", "pass"]);
    let mbox = "From one@example.test Mon Sep 17 00:00:00 2001\nFrom: One <one@example.test>\nDate: Tue, 1 Jan 2030 00:00:00 +0000\nSubject: one\n\nbody one\nFrom two@example.test Mon Sep 17 00:00:00 2001\nFrom: Two <two@example.test>\nDate: Tue, 1 Jan 2030 00:00:00 +0000\nSubject: two\n\nbody two\n";

    run_zmin_with_stdin(repo.path(), ["imap-send", "--no-curl"], mbox);

    let appends = server.appended_messages();
    assert_eq!(appends.len(), 2);
    assert!(String::from_utf8_lossy(&appends[0]).contains("Subject: one"));
    assert!(String::from_utf8_lossy(&appends[1]).contains("Subject: two"));
}

#[test]
fn quiltimport_applies_series_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let git_repo = dir.path().join("git-repo");
    let zmin_repo = dir.path().join("zmin-repo");
    git(
        dir.path(),
        ["init", "-b", "main", git_repo.to_str().expect("git path")],
    );
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            zmin_repo.to_str().expect("zmin path"),
        ],
    );
    for repo in [&git_repo, &zmin_repo] {
        configure_identity(repo);
        write_file(repo, "file.txt", "base\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "base"]);
        write_quilt_fixture(repo);
    }

    let args = [
        "quiltimport",
        "--author",
        "Patch Author <patch@example.test>",
        "--patches",
        "patches",
    ];
    assert_eq!(run_zmin(&zmin_repo, args), git(&git_repo, args));
    assert_eq!(
        git(
            &zmin_repo,
            ["log", "--format=%an <%ae>|%s|%b", "--reverse"]
        ),
        git(&git_repo, ["log", "--format=%an <%ae>|%s|%b", "--reverse"])
    );
    assert_eq!(
        git(&zmin_repo, ["rev-parse", "HEAD^{tree}"]),
        git(&git_repo, ["rev-parse", "HEAD^{tree}"])
    );

    let dry_run_repo = dir.path().join("dry-run-repo");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            dry_run_repo.to_str().expect("dry-run path"),
        ],
    );
    configure_identity(&dry_run_repo);
    write_file(&dry_run_repo, "file.txt", "base\n");
    git(&dry_run_repo, ["add", "-A"]);
    git_with_env(&dry_run_repo, ["commit", "-m", "base"]);
    write_quilt_fixture(&dry_run_repo);
    let before = git(&dry_run_repo, ["rev-parse", "HEAD"]);
    assert_eq!(
        run_zmin(
            &dry_run_repo,
            [
                "quiltimport",
                "-n",
                "--author",
                "Patch Author <patch@example.test>",
                "--patches",
                "patches",
            ],
        ),
        "change-one.patch\nadd-second.patch"
    );
    assert_eq!(git(&dry_run_repo, ["rev-parse", "HEAD"]), before);
}

fn write_quilt_fixture(repo: &std::path::Path) {
    fs::create_dir_all(repo.join("patches")).expect("create patches");
    fs::write(
        repo.join("patches/series"),
        "change-one.patch\nadd-second.patch\n",
    )
    .expect("write series");
    fs::write(
        repo.join("patches/change-one.patch"),
        "Change first file\n\nMore body.\n---\ndiff --git a/file.txt b/file.txt\nindex df967b9..ce01362 100644\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-base\n+changed\n",
    )
    .expect("write first patch");
    fs::write(
        repo.join("patches/add-second.patch"),
        "Add second file\n---\ndiff --git a/second.txt b/second.txt\nnew file mode 100644\nindex 0000000..e019be0\n--- /dev/null\n+++ b/second.txt\n@@ -0,0 +1 @@\n+second\n",
    )
    .expect("write second patch");
}
