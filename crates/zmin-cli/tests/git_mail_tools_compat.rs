mod common;

use std::fs;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use common::{
    command_failure_output, command_output, command_output_with_env, configure_identity, git,
    git_args, git_init, git_with_env, git_with_stdin, git_with_stdin_args, read_named_files,
    run_zmin, run_zmin_args, run_zmin_with_stdin, run_zmin_with_stdin_args, write_file, zmin_bin,
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
    transcript: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    list_wire_transcript: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[derive(Debug, Eq, PartialEq)]
struct ProcessCapture {
    status: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

const PINNED_STOCK_GIT: &str = "/private/tmp/skron-git-w51-stock.vr2JAX/git-2.55.0/git";
const PINNED_STOCK_VERSION: &[u8] = b"git version 2.55.0\n";
const STOCK_IMAP_COMMAND_ERROR: &[u8] =
    b"git: 'imap-send' is not a git command. See 'git --help'.\n";

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
        let transcript = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let list_wire_transcript = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let thread_messages = messages.clone();
        let thread_transcript = transcript.clone();
        let thread_list_wire_transcript = list_wire_transcript.clone();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept fake imap");
            serve_fake_imap(
                stream,
                thread_messages,
                thread_transcript,
                thread_list_wire_transcript,
            );
        });
        Self {
            port,
            messages,
            transcript,
            list_wire_transcript,
            handle: Some(handle),
        }
    }

    fn appended_messages(&self) -> Vec<Vec<u8>> {
        self.messages.lock().expect("messages lock").clone()
    }

    fn transcript(&self) -> Vec<String> {
        self.transcript.lock().expect("transcript lock").clone()
    }

    fn list_wire_transcript(&self) -> Vec<Vec<u8>> {
        self.list_wire_transcript
            .lock()
            .expect("list wire transcript lock")
            .clone()
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
    transcript: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    list_wire_transcript: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
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
        transcript
            .lock()
            .expect("transcript lock")
            .push(line.clone());
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
            let mut wire = list_wire_transcript
                .lock()
                .expect("list wire transcript lock");
            wire.push(format!("{line}\r\n").into_bytes());
            let list_row = b"* LIST () \"/\" \"INBOX.Drafts\"\r\n";
            wire.push(list_row.to_vec());
            writer.write_all(list_row).expect("list row");
            let list_response = format!("{tag} OK LIST completed\r\n").into_bytes();
            wire.push(list_response.clone());
            writer.write_all(&list_response).expect("list response");
        } else if line.contains(" LOGOUT") {
            writer.write_all(b"* BYE logging out\r\n").expect("bye");
            writeln!(writer, "{tag} OK LOGOUT completed\r").expect("logout response");
            return;
        } else {
            writeln!(writer, "{tag} BAD unsupported\r").expect("bad response");
        }
    }
}

fn pinned_stock_git() -> PathBuf {
    let configured = std::env::var_os("ZMIN_STOCK_GIT")
        .expect("ZMIN_STOCK_GIT must point to the pinned Git v2.55.0 binary");
    let configured = PathBuf::from(configured);
    assert_eq!(configured, Path::new(PINNED_STOCK_GIT));
    let output = Command::new(&configured)
        .arg("--version")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .expect("run pinned stock Git");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, PINNED_STOCK_VERSION);
    assert!(output.stderr.is_empty());
    configured
}

fn run_capture(command: &Path, cwd: &Path, args: &[&str], stdin: &[u8]) -> ProcessCapture {
    let mut child = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn captured command");
    child
        .stdin
        .as_mut()
        .expect("captured stdin")
        .write_all(stdin)
        .expect("write captured stdin");
    let output = child.wait_with_output().expect("wait for captured command");
    ProcessCapture {
        status: output.status.code().expect("captured exit code"),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

fn configure_imap(repo: &Path, port: u16) {
    git(
        repo,
        ["config", "imap.host", &format!("imap://127.0.0.1:{port}")],
    );
    git(repo, ["config", "imap.user", "user"]);
    git(repo, ["config", "imap.pass", "pass"]);
    git(repo, ["config", "imap.folder", "INBOX.Configured"]);
}

fn run_zmin_capture(cwd: &Path, args: &[&str], stdin: &[u8]) -> ProcessCapture {
    run_capture(Path::new(zmin_bin()), cwd, args, stdin)
}

fn normalize_send_email_patch_output(text: &str) -> String {
    text.replace("\r\n", "\n")
        .lines()
        .map(|line| {
            if line.starts_with("Date: ") {
                return "Date: <normalized-date>".to_owned();
            }
            if line.starts_with("Message-ID: ") {
                return "Message-ID: <normalized-message-id>".to_owned();
            }
            if line.starts_with("X-Mailer: ") {
                return "X-Mailer: <normalized-x-mailer>".to_owned();
            }
            line.to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_send_email_smtp_init_error(text: &str) -> String {
    text.replace("\r\n", "\n")
        .lines()
        .map(|line| {
            if let Some(prefix) = line
                .split(" hello=")
                .next()
                .filter(|prefix| prefix.starts_with("Unable to initialize SMTP properly."))
            {
                let port = line
                    .split(" port=")
                    .nth(1)
                    .and_then(|rest| rest.split_whitespace().next())
                    .map(|value| value.trim_end_matches('.'))
                    .unwrap_or("<port>");
                return format!("{prefix} hello=<normalized-hello> port={port}");
            }
            line.to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
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

    for (args, stdin) in [
        (
            [
                "interpret-trailers",
                "--where",
                "before",
                "--no-where",
                "--trailer",
                "Acked-by: C",
            ]
            .as_slice(),
            fixture,
        ),
        (
            [
                "interpret-trailers",
                "--if-exists",
                "add",
                "--no-if-exists",
                "--trailer",
                "Acked-by: B",
            ]
            .as_slice(),
            fixture,
        ),
        (
            [
                "interpret-trailers",
                "--if-missing",
                "doNothing",
                "--no-if-missing",
                "--trailer",
                "Reviewed-by: C",
            ]
            .as_slice(),
            fixture,
        ),
    ] {
        assert_eq!(
            run_zmin_with_stdin_args(repo.path(), args, stdin),
            git_with_stdin_args(repo.path(), args, stdin),
            "args: {args:?}"
        );
    }
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
        ["mailinfo", "--no-scissors", "zmin-msg", "zmin-patch"].as_slice(),
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
fn fmt_merge_msg_summary_synonyms_match_stock_git() {
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

    for args in [
        ["fmt-merge-msg", "--summary"].as_slice(),
        ["fmt-merge-msg", "--summary=3"].as_slice(),
        ["fmt-merge-msg", "--no-summary"].as_slice(),
        ["fmt-merge-msg", "--summary", "--no-log"].as_slice(),
        ["fmt-merge-msg", "--no-summary", "--summary"].as_slice(),
    ] {
        let zmin_output = run_zmin_with_stdin_args(work.path(), args, &fetch_head);
        let git_output = git_with_stdin_args(work.path(), args, &fetch_head);
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
fn send_email_override_option_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    let patch = git(repo.path(), ["format-patch", "-1"]);
    let patch = patch.trim().to_owned();
    let server = FakeSmtpServer::new(2);
    let port = server.port.to_string();
    git(
        repo.path(),
        ["config", "sendemail.smtpserver", "ignored.example.test"],
    );
    git(repo.path(), ["config", "sendemail.smtpserverport", "2525"]);
    git(
        repo.path(),
        ["config", "sendemail.from", "sender@example.test"],
    );
    git(
        repo.path(),
        ["config", "sendemail.to", "receiver@example.test"],
    );

    let smtp_port_arg = format!("--smtp-server-port={port}");
    let args = [
        "send-email",
        "--suppress-cc=author",
        "--from=bench@example.test",
        "--to=to1@example.test",
        "--cc=cc1@example.test",
        "--bcc=bcc1@example.test",
        "--reply-to=reply@example.test",
        "--subject=Custom subject",
        "--smtp-server=127.0.0.1",
        smtp_port_arg.as_str(),
        patch.as_str(),
    ];

    let stock = command_output("git", repo.path(), &args, "git send-email");
    let zmin = command_output(zmin_bin(), repo.path(), &args, "zmin send-email");

    assert_eq!(stock.0, zmin.0);
    assert_eq!(stock.2, zmin.2);
    assert_eq!(
        normalize_send_email_patch_output(&stock.1),
        normalize_send_email_patch_output(&zmin.1)
    );

    let messages = server.sent_messages();
    assert_eq!(messages.len(), 2);
    let stock_message = String::from_utf8_lossy(&messages[0]).to_string();
    let zmin_message = String::from_utf8_lossy(&messages[1]).to_string();
    assert_eq!(
        normalize_send_email_patch_output(&stock_message),
        normalize_send_email_patch_output(&zmin_message)
    );
}

#[test]
fn send_email_smtp_noop_option_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    let patch = git(repo.path(), ["format-patch", "-1"]);
    let patch = patch.trim().to_owned();
    let server = FakeSmtpServer::new(2);
    let port = server.port.to_string();
    git(repo.path(), ["config", "sendemail.smtpserver", "127.0.0.1"]);
    git(repo.path(), ["config", "sendemail.smtpserverport", &port]);
    git(
        repo.path(),
        ["config", "sendemail.from", "bench@example.test"],
    );
    git(repo.path(), ["config", "sendemail.to", "to1@example.test"]);

    let args = [
        "send-email",
        "--suppress-cc=author",
        "--no-smtp-auth",
        "--smtp-auth=none",
        "--smtp-pass=secret",
        "--smtp-debug=0",
        "--smtp-domain=example.test",
        "--validate",
        "--no-validate",
        "--force",
        "--format-patch",
        "--no-format-patch",
        "--xmailer",
        patch.as_str(),
    ];

    let stock = command_output("git", repo.path(), &args, "git send-email");
    let zmin = command_output(zmin_bin(), repo.path(), &args, "zmin send-email");

    assert_eq!(stock.0, zmin.0);
    assert_eq!(stock.2, zmin.2);
    assert_eq!(
        normalize_send_email_patch_output(&stock.1),
        normalize_send_email_patch_output(&zmin.1)
    );

    let messages = server.sent_messages();
    assert_eq!(messages.len(), 2);
    let stock_message = String::from_utf8_lossy(&messages[0]).to_string();
    let zmin_message = String::from_utf8_lossy(&messages[1]).to_string();
    assert_eq!(
        normalize_send_email_patch_output(&stock_message),
        normalize_send_email_patch_output(&zmin_message)
    );
}

#[test]
fn send_email_metadata_noop_option_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    let patch = git(repo.path(), ["format-patch", "-1"]);
    let patch = patch.trim().to_owned();
    let server = FakeSmtpServer::new(2);
    let port = server.port.to_string();
    git(repo.path(), ["config", "sendemail.smtpserver", "127.0.0.1"]);
    git(repo.path(), ["config", "sendemail.smtpserverport", &port]);
    git(
        repo.path(),
        ["config", "sendemail.from", "bench@example.test"],
    );
    git(repo.path(), ["config", "sendemail.to", "to1@example.test"]);
    git(repo.path(), ["config", "sendemail.identity", "default"]);
    git(
        repo.path(),
        ["config", "sendemail.test.smtpserver", "127.0.0.1"],
    );
    git(
        repo.path(),
        ["config", "sendemail.test.smtpserverport", &port],
    );
    git(
        repo.path(),
        ["config", "sendemail.test.from", "bench@example.test"],
    );
    git(
        repo.path(),
        ["config", "sendemail.test.to", "to1@example.test"],
    );

    let args = [
        "send-email",
        "--suppress-cc=author",
        "--no-bcc",
        "--no-cc",
        "--no-identity",
        "--no-mailmap",
        "--no-signed-off-by-cc",
        "--no-suppress-from",
        "--no-thread",
        "--no-to-cover",
        "--cc-cover",
        "--to-cover",
        "--thread",
        "--chain-reply-to",
        "--no-chain-reply-to",
        "--mailmap",
        "--identity=test",
        "--suppress-from",
        patch.as_str(),
    ];

    let stock = command_output("git", repo.path(), &args, "git send-email");
    let zmin = command_output(zmin_bin(), repo.path(), &args, "zmin send-email");

    assert_eq!(stock.0, zmin.0);
    assert_eq!(stock.2, zmin.2);
    assert_eq!(
        normalize_send_email_patch_output(&stock.1),
        normalize_send_email_patch_output(&zmin.1)
    );

    let messages = server.sent_messages();
    assert_eq!(messages.len(), 2);
    let stock_message = String::from_utf8_lossy(&messages[0]).to_string();
    let zmin_message = String::from_utf8_lossy(&messages[1]).to_string();
    assert_eq!(
        normalize_send_email_patch_output(&stock_message),
        normalize_send_email_patch_output(&zmin_message)
    );
}

#[test]
fn send_email_invalid_smtp_noop_option_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    let patch = git(repo.path(), ["format-patch", "-1"]);
    let patch = patch.trim().to_owned();
    git(repo.path(), ["config", "sendemail.smtpserver", "127.0.0.1"]);
    git(repo.path(), ["config", "sendemail.smtpserverport", "1"]);
    git(
        repo.path(),
        ["config", "sendemail.from", "bench@example.test"],
    );
    git(repo.path(), ["config", "sendemail.to", "to1@example.test"]);

    let args = [
        "send-email",
        "--8bit-encoding=UTF-8",
        "--batch-size=1",
        "--cc-cmd=true",
        "--compose-encoding=UTF-8",
        "--confirm=never",
        "--envelope-sender=auto",
        "--header-cmd=true",
        "--in-reply-to=<msg@example.test>",
        "--no-cc-cover",
        "--no-header-cmd",
        "--no-to",
        "--no-xmailer",
        "--relogin-delay=1",
        "--signed-off-by-cc",
        "--smtp-encryption=none",
        "--smtp-server-option=foo",
        "--smtp-ssl-cert-path=/tmp/cert.pem",
        "--smtp-user=test",
        "--to-cmd=true",
        "--transfer-encoding=8bit",
        "--suppress-cc=author",
        patch.as_str(),
    ];

    let stock = command_failure_output("git", repo.path(), &args, "git send-email");
    let zmin = command_failure_output(zmin_bin(), repo.path(), &args, "zmin send-email");

    assert_eq!(stock.0, zmin.0);
    assert_eq!(stock.1, zmin.1);
    assert_eq!(
        normalize_send_email_smtp_init_error(&stock.2),
        normalize_send_email_smtp_init_error(&zmin.2)
    );
}

#[test]
fn send_email_dry_run_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    let patch = git(repo.path(), ["format-patch", "-1"]);
    let patch = patch.trim().to_owned();
    git(repo.path(), ["config", "sendemail.smtpserver", "127.0.0.1"]);
    git(repo.path(), ["config", "sendemail.smtpserverport", "1"]);
    git(
        repo.path(),
        ["config", "sendemail.from", "bench@example.test"],
    );
    git(repo.path(), ["config", "sendemail.to", "to1@example.test"]);

    let args = [
        "send-email",
        "--dry-run",
        "--suppress-cc=author",
        patch.as_str(),
    ];

    let stock = command_output("git", repo.path(), &args, "git send-email");
    let zmin = command_output(zmin_bin(), repo.path(), &args, "zmin send-email");

    assert_eq!(stock.0, zmin.0);
    assert_eq!(stock.2, zmin.2);
    assert_eq!(
        normalize_send_email_patch_output(&stock.1),
        normalize_send_email_patch_output(&zmin.1)
    );
}

#[test]
fn send_email_helper_tail_option_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    let patch = git(repo.path(), ["format-patch", "-1"]);
    let patch = patch.trim().to_owned();
    git(repo.path(), ["config", "sendemail.smtpserver", "127.0.0.1"]);
    git(repo.path(), ["config", "sendemail.smtpserverport", "1"]);
    git(
        repo.path(),
        ["config", "sendemail.from", "bench@example.test"],
    );
    git(repo.path(), ["config", "sendemail.to", "to1@example.test"]);

    let cases = [
        vec![
            "send-email",
            "--quiet",
            "--dry-run",
            "--suppress-cc=author",
            patch.as_str(),
        ],
        vec![
            "send-email",
            "--smtp-ssl",
            "--dry-run",
            "--suppress-cc=author",
            patch.as_str(),
        ],
        vec![
            "send-email",
            "--annotate",
            "--dry-run",
            "--suppress-cc=author",
            patch.as_str(),
        ],
        vec![
            "send-email",
            "--compose",
            "--dry-run",
            "--confirm=never",
            "--suppress-cc=author",
            patch.as_str(),
        ],
        vec![
            "send-email",
            "--sendmail-cmd=true",
            "--dry-run",
            "--suppress-cc=author",
            patch.as_str(),
        ],
    ];

    for args in cases {
        let stock = command_output_with_env(
            "git",
            repo.path(),
            &args,
            &[("GIT_EDITOR", "true")],
            "git send-email helper tail family",
        );
        let zmin = command_output_with_env(
            zmin_bin(),
            repo.path(),
            &args,
            &[("GIT_EDITOR", "true")],
            "zmin send-email helper tail family",
        );

        assert_eq!(stock.0, zmin.0, "args: {args:?}");
        assert_eq!(stock.2, zmin.2, "args: {args:?}");
        assert_eq!(
            normalize_send_email_patch_output(&stock.1),
            normalize_send_email_patch_output(&zmin.1),
            "args: {args:?}"
        );
    }
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
fn imap_send_zmin_options_are_rejected_by_pinned_stock_git() {
    let stock = pinned_stock_git();
    let cwd = TempDir::new().expect("stock imap rejection cwd");
    let cases: &[&[&str]] = &[
        &["imap-send", "--folder", "INBOX.Custom"],
        &["imap-send", "-f", "INBOX.Short"],
        &["imap-send", "--list"],
    ];

    for args in cases {
        let capture = run_capture(&stock, cwd.path(), args, b"");
        assert_eq!(capture.status, 1, "args: {args:?}");
        assert!(capture.stdout.is_empty(), "args: {args:?}");
        assert_eq!(capture.stderr, STOCK_IMAP_COMMAND_ERROR, "args: {args:?}");
    }
}

fn run_zmin_imap_append_case(
    args: &[&str],
    mbox: &[u8],
) -> (ProcessCapture, Vec<Vec<u8>>, Vec<String>) {
    let repo = git_init();
    let server = FakeImapServer::new();
    configure_imap(repo.path(), server.port);
    let capture = run_zmin_capture(repo.path(), args, mbox);
    let messages = server.appended_messages();
    let transcript = server.transcript();
    (capture, messages, transcript)
}

#[test]
fn imap_send_folder_and_short_alias_have_exact_mailbox_state() {
    let mbox = b"From sender@example.test Mon Sep 17 00:00:00 2001\nFrom: Sender <sender@example.test>\nSubject: folder override\n\nbody\n";
    let (folder_capture, folder_messages, folder_transcript) =
        run_zmin_imap_append_case(&["imap-send", "--quiet", "--folder", "INBOX.Custom"], mbox);
    let (short_capture, short_messages, short_transcript) =
        run_zmin_imap_append_case(&["imap-send", "--quiet", "-f", "INBOX.Short"], mbox);

    for capture in [&folder_capture, &short_capture] {
        assert_eq!(capture.status, 0);
        assert!(capture.stdout.is_empty());
        assert!(capture.stderr.is_empty());
    }
    assert_eq!(folder_messages, vec![mbox.to_vec()]);
    assert_eq!(short_messages, vec![mbox.to_vec()]);
    assert_eq!(
        folder_transcript,
        vec![
            "A0001 LOGIN \"user\" \"pass\"".to_owned(),
            format!("A0002 APPEND \"INBOX.Custom\" {{{}}}", mbox.len()),
            "A0003 LOGOUT".to_owned(),
        ]
    );
    assert_eq!(
        short_transcript,
        vec![
            "A0001 LOGIN \"user\" \"pass\"".to_owned(),
            format!("A0002 APPEND \"INBOX.Short\" {{{}}}", mbox.len()),
            "A0003 LOGOUT".to_owned(),
        ]
    );
}

#[test]
fn imap_send_list_has_exact_output_and_transcript() {
    let repo = git_init();
    let server = FakeImapServer::new();
    configure_imap(repo.path(), server.port);
    let capture = run_zmin_capture(repo.path(), &["imap-send", "--list"], b"");
    let messages = server.appended_messages();
    let transcript = server.transcript();
    let list_wire_transcript = server.list_wire_transcript();

    assert_eq!(capture.status, 0);
    assert_eq!(capture.stdout, b"() \"/\" \"INBOX.Drafts\"\n");
    assert!(capture.stderr.is_empty());
    assert!(messages.is_empty());
    assert_eq!(
        transcript,
        vec![
            "A0001 LOGIN \"user\" \"pass\"".to_owned(),
            "A0002 LIST \"\" \"*\"".to_owned(),
            "A0003 LOGOUT".to_owned(),
        ]
    );
    assert_eq!(
        list_wire_transcript,
        vec![
            b"A0002 LIST \"\" \"*\"\r\n".to_vec(),
            b"* LIST () \"/\" \"INBOX.Drafts\"\r\n".to_vec(),
            b"A0002 OK LIST completed\r\n".to_vec(),
        ]
    );
}

#[test]
fn imap_send_folder_reports_missing_host_without_network_side_effects() {
    let repo = git_init();
    let capture = run_zmin_capture(
        repo.path(),
        &["imap-send", "--quiet", "--folder", "INBOX.Custom"],
        b"",
    );

    assert_eq!(capture.status, 1);
    assert!(capture.stdout.is_empty());
    assert_eq!(capture.stderr, b"fatal: no imap host specified\n");
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
        ["init", "-b", "main", zmin_repo.to_str().expect("zmin path")],
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
        git(&zmin_repo, ["log", "--format=%an <%ae>|%s|%b", "--reverse"]),
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
