mod common;

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use common::zmin_bin;

use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[cfg(unix)]
use std::os::unix::io::{FromRawFd, RawFd};

#[cfg(unix)]
use std::os::unix::fs::symlink;

#[cfg(unix)]
struct InteractiveFastImportOutput {
    status: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    responded_before_eof: bool,
}

#[cfg(unix)]
struct InteractiveFastImportFdOutput {
    status: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    response: Vec<u8>,
    responded_before_eof: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct FastImportProcessOutput {
    status: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

const FAST_IMPORT_TEST_DEADLINE: Duration = Duration::from_secs(10);

fn pinned_stock_git() -> PathBuf {
    static STOCK_GIT: OnceLock<PathBuf> = OnceLock::new();
    STOCK_GIT
        .get_or_init(|| {
            let path = std::env::var_os("ZMIN_STOCK_GIT")
                .map(PathBuf::from)
                .expect("set ZMIN_STOCK_GIT to the pinned Git comparator");
            assert!(
                path.is_absolute() && path.is_file(),
                "ZMIN_STOCK_GIT must be an absolute Git 2.55.0 file"
            );
            let cwd = std::env::current_dir().expect("current test directory");
            let output = run_command_with_watchdog(&path, &["--version"], &cwd, &[]);
            assert_eq!(output.status, 0, "pinned stock Git --version failed");
            assert_eq!(
                output.stdout, b"git version 2.55.0\n",
                "ZMIN_STOCK_GIT must be exactly Git 2.55.0"
            );
            path
        })
        .clone()
}

fn git_init() -> TempDir {
    git_init_with_program(&pinned_stock_git(), &[])
}

fn git_init_with_program(program: &Path, extra_args: &[&str]) -> TempDir {
    let repo = TempDir::new().expect("temp repo");
    let mut args = vec!["init"];
    args.extend_from_slice(extra_args);
    let output = run_command_with_watchdog(program, &args, repo.path(), &[]);
    assert_eq!(output.status, 0, "pinned stock Git init failed");
    repo
}

fn git<const N: usize>(cwd: &Path, args: [&str; N]) -> String {
    let output = run_command_with_watchdog(&pinned_stock_git(), &args, cwd, &[]);
    assert_eq!(output.status, 0, "pinned stock Git failed: {args:?}");
    String::from_utf8(output.stdout).expect("pinned stock Git stdout utf8")
}

fn git_status<const N: usize>(cwd: &Path, args: [&str; N]) -> i32 {
    run_command_with_watchdog(&pinned_stock_git(), &args, cwd, &[]).status
}

fn command_program(command: &str) -> PathBuf {
    if command == "git" {
        pinned_stock_git()
    } else {
        PathBuf::from(command)
    }
}

fn spawn_pipe_reader<R: Read + Send + 'static>(
    mut reader: R,
) -> mpsc::Receiver<std::io::Result<Vec<u8>>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = reader.read_to_end(&mut bytes).map(|_| bytes);
        let _ = sender.send(result);
    });
    receiver
}

fn spawn_pipe_writer(
    mut writer: std::process::ChildStdin,
    input: Vec<u8>,
) -> thread::JoinHandle<std::io::Result<()>> {
    thread::spawn(move || {
        let result = writer.write_all(&input);
        drop(writer);
        result
    })
}

struct JoinablePipeReader {
    result: mpsc::Receiver<io::Result<Vec<u8>>>,
    handle: thread::JoinHandle<()>,
}

fn spawn_joinable_pipe_reader<R: Read + Send + 'static>(mut reader: R) -> JoinablePipeReader {
    let (sender, result) = mpsc::channel();
    let handle = thread::spawn(move || {
        let mut bytes = Vec::new();
        let read_result = reader.read_to_end(&mut bytes).map(|_| bytes);
        let _ = sender.send(read_result);
    });
    JoinablePipeReader { result, handle }
}

fn finish_joinable_pipe_reader(reader: JoinablePipeReader) -> io::Result<Vec<u8>> {
    let result = reader
        .result
        .recv()
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "pipe reader result was dropped"))?;
    if reader.handle.join().is_err() {
        return Err(io::Error::other("pipe reader thread panicked"));
    }
    result
}

fn discard_joinable_pipe_reader(reader: JoinablePipeReader) {
    let _ = reader.result.recv();
    let _ = reader.handle.join();
}

fn terminate_fast_import_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let process_group = -(child.id() as libc::pid_t);
        // SAFETY: the child was started in its own process group by process_group(0).
        unsafe {
            libc::kill(process_group, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    let _ = child.wait();
}

fn remaining_until(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

fn receive_pipe(
    receiver: &mpsc::Receiver<std::io::Result<Vec<u8>>>,
    child: &mut std::process::Child,
    deadline: Instant,
    label: &str,
) -> Vec<u8> {
    match receiver.recv_timeout(remaining_until(deadline)) {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(error)) => {
            terminate_fast_import_group(child);
            panic!("read fast-import child {label}: {error}");
        }
        Err(error) => {
            terminate_fast_import_group(child);
            panic!("read fast-import child {label}: {error}");
        }
    }
}

fn run_command_with_watchdog(
    program: &Path,
    args: &[&str],
    cwd: &Path,
    input: &[u8],
) -> FastImportProcessOutput {
    let deadline = Instant::now() + FAST_IMPORT_TEST_DEADLINE;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("spawn {}: {error}", program.display()));
    let writer = spawn_pipe_writer(child.stdin.take().expect("child stdin"), input.to_vec());
    let stdout = spawn_pipe_reader(child.stdout.take().expect("child stdout"));
    let stderr = spawn_pipe_reader(child.stderr.take().expect("child stderr"));

    let status = loop {
        if let Some(status) = child.try_wait().expect("poll child status") {
            break status;
        }
        if remaining_until(deadline).is_zero() {
            terminate_fast_import_group(&mut child);
            let _ = writer.join();
            let _ = stdout.recv_timeout(Duration::from_secs(1));
            let _ = stderr.recv_timeout(Duration::from_secs(1));
            panic!("{} exceeded the 10 second deadline", program.display());
        }
        thread::sleep(Duration::from_millis(10));
    };

    while !writer.is_finished() && !remaining_until(deadline).is_zero() {
        thread::sleep(Duration::from_millis(10));
    }
    if !writer.is_finished() {
        terminate_fast_import_group(&mut child);
        let writer_result = writer.join();
        let _ = stdout.recv_timeout(Duration::from_secs(1));
        let _ = stderr.recv_timeout(Duration::from_secs(1));
        match writer_result {
            Ok(Ok(())) => panic!(
                "{} stdin transmission exceeded the 10 second deadline",
                program.display()
            ),
            Ok(Err(error)) => panic!(
                "{} stdin transmission exceeded the 10 second deadline: {error}",
                program.display()
            ),
            Err(_) => panic!(
                "{} stdin transmission exceeded the 10 second deadline: writer thread panicked",
                program.display()
            ),
        }
    }
    let _ = writer.join();
    let stdout = receive_pipe(&stdout, &mut child, deadline, "stdout");
    let stderr = receive_pipe(&stderr, &mut child, deadline, "stderr");
    FastImportProcessOutput {
        status: status.code().unwrap_or(1),
        stdout,
        stderr,
    }
}

fn command_with_stdin_output(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
) -> (i32, String, String) {
    let output = run_command_with_watchdog(&command_program(command), args, cwd, stdin.as_bytes());
    (
        output.status,
        String::from_utf8(output.stdout).expect("stdout utf8"),
        String::from_utf8(output.stderr).expect("stderr utf8"),
    )
}

fn normalize_fast_import_crash_stderr(stderr: &str, expected_fatal: &str) -> String {
    let mut lines = stderr.lines();
    let fatal = lines.next().expect("fatal line");
    let crash = lines.next().expect("crash report line");
    assert_eq!(fatal, expected_fatal);
    assert!(
        crash.starts_with("fast-import: dumping crash report to .git/fast_import_crash_"),
        "unexpected crash report line: {crash}"
    );
    assert_eq!(lines.next(), None);
    format!("{expected_fatal}\nfast-import: dumping crash report to .git/fast_import_crash_<pid>")
}

fn normalize_fast_import_crash_report(
    reports: &[String],
    expected_fatal: &str,
    expected_command: Option<&str>,
) -> String {
    assert_eq!(reports.len(), 1, "expected one crash report");
    let normalized = reports[0]
        .split_inclusive('\n')
        .map(normalize_fast_import_crash_line)
        .collect::<String>();
    assert!(
        normalized.contains(expected_fatal),
        "missing fatal report field: {normalized}"
    );
    if let Some(expected_command) = expected_command {
        assert!(
            normalized.contains(&format!("* {expected_command}")),
            "missing recent command field: {normalized}"
        );
    }
    normalized
}

fn normalize_fast_import_crash_line(line: &str) -> String {
    let (body, ending) = if let Some(body) = line.strip_suffix('\n') {
        if let Some(body) = body.strip_suffix('\r') {
            (body, "\r\n")
        } else {
            (body, "\n")
        }
    } else {
        (line, "")
    };
    if let Some(value) = body.strip_prefix("    fast-import process: ") {
        assert!(!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()));
        return format!("    fast-import process: <pid>{ending}");
    }
    if let Some(value) = body.strip_prefix("    parent process     : ") {
        assert!(!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()));
        return format!("    parent process     : <pid>{ending}");
    }
    if let Some(value) = body.strip_prefix("    at ") {
        let bytes = value.as_bytes();
        assert_eq!(bytes.len(), 25, "unexpected crash-report timestamp: {body}");
        assert!(bytes[0..4].iter().all(u8::is_ascii_digit));
        assert_eq!(bytes[4], b'-');
        assert!(bytes[5..7].iter().all(u8::is_ascii_digit));
        assert_eq!(bytes[7], b'-');
        assert!(bytes[8..10].iter().all(u8::is_ascii_digit));
        assert_eq!(bytes[10], b' ');
        assert!(bytes[11..13].iter().all(u8::is_ascii_digit));
        assert_eq!(bytes[13], b':');
        assert!(bytes[14..16].iter().all(u8::is_ascii_digit));
        assert_eq!(bytes[16], b':');
        assert!(bytes[17..19].iter().all(u8::is_ascii_digit));
        assert_eq!(bytes[19], b' ');
        assert!(
            bytes[20..25]
                .iter()
                .all(|byte| byte.is_ascii_digit() || *byte == b'+' || *byte == b'-')
        );
        return format!("    at <timestamp>{ending}");
    }
    format!("{body}{ending}")
}

fn normalize_fast_import_statistics_stderr(stderr: &str) -> String {
    assert!(
        stderr.contains("fast-import statistics:"),
        "stderr should include import statistics: {stderr}"
    );
    let mut normalized = stderr
        .lines()
        .filter(|line| {
            !line.starts_with("Alloc'd objects:")
                && !line.starts_with("Memory total:")
                && !line.starts_with("       pools:")
                && !line.starts_with("     objects:")
                && !line.starts_with("pack_report:")
        })
        .collect::<Vec<_>>()
        .join("\n");
    normalized.push('\n');
    normalized
}

fn normalize_fast_import_warning_and_statistics_stderr(stderr: &str, warning: &str) -> String {
    let rest = stderr
        .strip_prefix(warning)
        .expect("warning prefix")
        .trim_start_matches('\n');
    format!(
        "{warning}\n{}",
        normalize_fast_import_statistics_stderr(rest)
    )
}

struct FastImportPackEdge {
    path: PathBuf,
    ids: String,
}

fn parse_fast_import_pack_edges(edges: &str) -> Vec<FastImportPackEdge> {
    edges
        .lines()
        .map(|line| {
            let (path, ids) = line.rsplit_once(':').expect("pack edge separator");
            FastImportPackEdge {
                path: PathBuf::from(path),
                ids: ids.to_owned(),
            }
        })
        .collect()
}

fn assert_fast_import_pack_edges_match_stock(zmin: &str, stock: &str) {
    let zmin = parse_fast_import_pack_edges(zmin);
    let stock = parse_fast_import_pack_edges(stock);
    assert_eq!(zmin.len(), stock.len(), "pack edge line count");
    for (zmin, stock) in zmin.iter().zip(stock.iter()) {
        assert_eq!(
            zmin.path.parent(),
            stock.path.parent(),
            "pack edge directory"
        );
        assert_eq!(zmin.ids, stock.ids, "pack edge object ids");
        let zmin_name = zmin.path.file_name().expect("zmin pack edge filename");
        let stock_name = stock.path.file_name().expect("stock pack edge filename");
        assert!(
            zmin_name.to_string_lossy().starts_with("pack-")
                && zmin_name.to_string_lossy().ends_with(".pack")
        );
        assert!(
            stock_name.to_string_lossy().starts_with("pack-")
                && stock_name.to_string_lossy().ends_with(".pack")
        );
    }
}

fn init_with_pinned_git(program: &Path, repo: &Path) {
    let output = run_command_with_watchdog(program, &["init", "-q"], repo, &[]);
    assert!(
        output.status == 0,
        "pinned Git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_fast_import_child(
    program: &Path,
    args: &[&str],
    repo: &Path,
    input: &[u8],
) -> FastImportProcessOutput {
    run_command_with_watchdog(program, args, repo, input)
}

fn run_fast_import_batch(program: &Path, repo: &Path, input: &[u8]) -> FastImportProcessOutput {
    run_fast_import_child(program, &["fast-import", "--quiet"], repo, input)
}

fn run_fast_import_batch_with_option(
    program: &Path,
    repo: &Path,
    option: &str,
    input: &[u8],
) -> FastImportProcessOutput {
    let args = ["fast-import", "--quiet", option];
    run_fast_import_child(program, &args, repo, input)
}

fn run_pinned_mktree(program: &Path, repo: &Path) -> String {
    let output = run_fast_import_child(program, &["mktree"], repo, b"");
    assert!(
        output.status == 0,
        "pinned mktree failed: {:?}",
        output.stderr
    );
    String::from_utf8(output.stdout)
        .expect("pinned mktree stdout utf8")
        .trim()
        .to_owned()
}

fn wait_for_fast_import_child(
    child: &mut std::process::Child,
    deadline: Instant,
) -> std::io::Result<std::process::ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "interactive fast-import deadline exceeded",
            ));
        }
        thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

#[cfg(unix)]
fn write_pipe_with_deadline(
    stdin: &mut std::process::ChildStdin,
    input: &[u8],
    deadline: Instant,
) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;

    let fd = stdin.as_raw_fd();
    // SAFETY: fcntl operates on the live pipe descriptor owned by `stdin`.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the descriptor remains valid for the duration of this function.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let result = (|| {
        let mut written = 0;
        while written < input.len() {
            if remaining_until(deadline).is_zero() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "interactive fast-import stdin deadline exceeded",
                ));
            }
            // SAFETY: the source slice is valid for `input.len() - written` bytes.
            let count =
                unsafe { libc::write(fd, input[written..].as_ptr().cast(), input.len() - written) };
            if count > 0 {
                written += count as usize;
                continue;
            }
            if count == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "interactive fast-import stdin closed",
                ));
            }
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::WouldBlock {
                thread::sleep(remaining_until(deadline).min(Duration::from_millis(10)));
                continue;
            }
            return Err(error);
        }
        Ok(())
    })();

    // SAFETY: restore the descriptor flags before returning to the caller.
    let restore = unsafe { libc::fcntl(fd, libc::F_SETFL, flags) };
    if restore < 0 && result.is_ok() {
        return Err(std::io::Error::last_os_error());
    }
    result
}

#[cfg(unix)]
fn run_interactive_cat_blob(program: &Path, repo: &Path) -> InteractiveFastImportOutput {
    let deadline = Instant::now() + FAST_IMPORT_TEST_DEADLINE;
    let mut command = Command::new(program);
    command
        .args(["fast-import", "--quiet"])
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.process_group(0);
    let mut child = command.spawn().expect("spawn interactive fast-import");
    let mut stdin = child.stdin.take().expect("fast-import stdin");
    let stdout = child.stdout.take().expect("fast-import stdout");
    let mut stderr = child.stderr.take().expect("fast-import stderr");

    let (header_tx, header_rx) = mpsc::channel();
    let (rest_tx, rest_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut header = Vec::new();
        let header_result = reader.read_until(b'\n', &mut header).map(|_| header);
        let Ok(header) = header_result else {
            let _ = header_tx.send(header_result);
            return;
        };
        if header_tx.send(Ok(header)).is_err() {
            return;
        }
        let mut rest = Vec::new();
        let _ = rest_tx.send(reader.read_to_end(&mut rest).map(|_| rest));
    });
    let (stderr_tx, stderr_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr_tx.send(stderr.read_to_end(&mut bytes).map(|_| bytes));
    });

    let first = b"feature cat-blob\nblob\nmark :1\ndata 4\ntest\ncat-blob :1\n";
    if let Err(error) = write_pipe_with_deadline(&mut stdin, first, deadline) {
        terminate_fast_import_group(&mut child);
        panic!("write interactive prefix: {error}");
    }
    if let Err(error) = stdin.flush() {
        terminate_fast_import_group(&mut child);
        panic!("flush interactive prefix: {error}");
    }
    let header = match header_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(Ok(header)) => header,
        Ok(Err(error)) => {
            terminate_fast_import_group(&mut child);
            panic!("read interactive response: {error}");
        }
        Err(error) => {
            terminate_fast_import_group(&mut child);
            panic!("fast-import did not respond before deadline: {error}");
        }
    };

    if let Err(error) = write_pipe_with_deadline(
        &mut stdin,
        b"progress checkpoint\nget-mark :1\ndone\n",
        deadline,
    ) {
        terminate_fast_import_group(&mut child);
        panic!("write interactive suffix: {error}");
    }
    if let Err(error) = stdin.flush() {
        terminate_fast_import_group(&mut child);
        panic!("flush interactive suffix: {error}");
    }
    drop(stdin);
    let rest = rest_rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .unwrap_or_else(|error| {
            terminate_fast_import_group(&mut child);
            panic!("interactive fast-import response EOF: {error}");
        })
        .unwrap_or_else(|error| {
            terminate_fast_import_group(&mut child);
            panic!("read interactive response body: {error}");
        });
    let status = wait_for_fast_import_child(&mut child, deadline).unwrap_or_else(|error| {
        terminate_fast_import_group(&mut child);
        panic!("wait interactive fast-import: {error}");
    });
    let stderr = stderr_rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .unwrap_or_else(|error| {
            terminate_fast_import_group(&mut child);
            panic!("interactive fast-import stderr EOF: {error}");
        })
        .unwrap_or_else(|error| {
            terminate_fast_import_group(&mut child);
            panic!("read interactive fast-import stderr: {error}");
        });
    let mut stdout = header;
    stdout.extend_from_slice(&rest);
    InteractiveFastImportOutput {
        status: status.code().unwrap_or(1),
        stdout,
        stderr,
        responded_before_eof: true,
    }
}

#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
struct FastImportResetObservation {
    before_checkpoint: FastImportProcessOutput,
    after_checkpoint: FastImportProcessOutput,
    final_output: FastImportProcessOutput,
}

#[cfg(unix)]
fn run_interactive_reset_observation(program: &Path, repo: &Path) -> FastImportResetObservation {
    let deadline = Instant::now() + FAST_IMPORT_TEST_DEADLINE;
    let mut command = Command::new(program);
    command
        .args(["fast-import", "--quiet", "--force"])
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.process_group(0);
    let mut child = command
        .spawn()
        .expect("spawn reset observation fast-import");
    let mut stdin = child.stdin.take().expect("reset observation stdin");
    let stdout = child.stdout.take().expect("reset observation stdout");
    let stderr = spawn_joinable_pipe_reader(child.stderr.take().expect("reset observation stderr"));
    let (stdout_tx, stdout_rx) = mpsc::channel();
    let stdout_thread = thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = Vec::new();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => {
                    let _ = stdout_tx.send(Ok(None));
                    return;
                }
                Ok(_) => {
                    if stdout_tx.send(Ok(Some(line))).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = stdout_tx.send(Err(error));
                    return;
                }
            }
        }
    });

    let prefix = b"blob\nmark :1\ndata 3\nnew\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 new.txt\nreset refs/heads/main\nprogress before checkpoint\n";
    write_pipe_with_deadline(&mut stdin, prefix, deadline)
        .unwrap_or_else(|error| panic!("write reset observation prefix: {error}"));
    stdin.flush().expect("flush reset observation prefix");
    let before_progress = stdout_rx
        .recv_timeout(remaining_until(deadline))
        .expect("reset observation before progress timeout")
        .expect("reset observation before progress read")
        .expect("reset observation before progress EOF");
    assert_eq!(before_progress, b"progress before checkpoint\n");
    let before_checkpoint =
        fast_import_ref_snapshot_with_program_for(program, repo, "refs/heads/main");

    write_pipe_with_deadline(
        &mut stdin,
        b"checkpoint\nprogress after checkpoint\n",
        deadline,
    )
    .unwrap_or_else(|error| panic!("write reset observation checkpoint: {error}"));
    stdin.flush().expect("flush reset observation checkpoint");
    let after_progress = stdout_rx
        .recv_timeout(remaining_until(deadline))
        .expect("reset observation after progress timeout")
        .expect("reset observation after progress read")
        .expect("reset observation after progress EOF");
    assert_eq!(after_progress, b"progress after checkpoint\n");
    let after_checkpoint =
        fast_import_ref_snapshot_with_program_for(program, repo, "refs/heads/main");

    write_pipe_with_deadline(&mut stdin, b"done\n", deadline)
        .unwrap_or_else(|error| panic!("write reset observation done: {error}"));
    stdin.flush().expect("flush reset observation done");
    drop(stdin);
    loop {
        match stdout_rx
            .recv_timeout(remaining_until(deadline))
            .expect("reset observation final stdout timeout")
        {
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(error) => {
                terminate_fast_import_group(&mut child);
                panic!("read reset observation stdout: {error}");
            }
        }
    }
    let status = wait_for_fast_import_child(&mut child, deadline).unwrap_or_else(|error| {
        terminate_fast_import_group(&mut child);
        panic!("wait reset observation fast-import: {error}");
    });
    let stderr = finish_joinable_pipe_reader(stderr).unwrap_or_else(|error| {
        terminate_fast_import_group(&mut child);
        panic!("read reset observation stderr: {error}");
    });
    stdout_thread
        .join()
        .expect("reset observation stdout thread");
    FastImportResetObservation {
        before_checkpoint,
        after_checkpoint,
        final_output: FastImportProcessOutput {
            status: status.code().unwrap_or(1),
            stdout: Vec::new(),
            stderr,
        },
    }
}

#[cfg(unix)]
struct Fd3ResponseReader {
    first_response: mpsc::Receiver<io::Result<Vec<u8>>>,
    handle: thread::JoinHandle<io::Result<Vec<u8>>>,
}

#[cfg(unix)]
fn cleanup_fd3_process(
    child: &mut std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    stdout: JoinablePipeReader,
    stderr: JoinablePipeReader,
    response: Fd3ResponseReader,
) {
    drop(stdin);
    terminate_fast_import_group(child);
    let _ = response.handle.join();
    discard_joinable_pipe_reader(stdout);
    discard_joinable_pipe_reader(stderr);
}

#[cfg(unix)]
fn run_interactive_cat_blob_fd3(program: &Path, repo: &Path) -> InteractiveFastImportFdOutput {
    let deadline = Instant::now() + FAST_IMPORT_TEST_DEADLINE;
    let mut response_pipe = [0; 2];
    // SAFETY: response_pipe points to two valid slots for libc::pipe to fill.
    assert_eq!(unsafe { libc::pipe(response_pipe.as_mut_ptr()) }, 0);
    let read_fd = response_pipe[0];
    let write_fd = response_pipe[1];

    let mut command = Command::new(program);
    command
        .args(["fast-import", "--quiet", "--cat-blob-fd=3"])
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.process_group(0);
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(write_fd, 3) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if write_fd != 3 {
                libc::close(write_fd);
            }
            Ok(())
        });
    }
    let mut child = command.spawn().expect("spawn fd3 fast-import");
    // SAFETY: the parent owns this copy and the child has received fd 3.
    unsafe {
        libc::close(write_fd);
    }
    let mut stdin = child.stdin.take().expect("fast-import stdin");
    let stdout = spawn_joinable_pipe_reader(child.stdout.take().expect("fast-import stdout"));
    let stderr = spawn_joinable_pipe_reader(child.stderr.take().expect("fast-import stderr"));
    // SAFETY: read_fd is the live read end owned by this reader thread.
    let response_file = unsafe { fs::File::from_raw_fd(read_fd as RawFd) };
    let (response_tx, response_rx) = mpsc::channel::<std::io::Result<Vec<u8>>>();
    let response_handle = thread::spawn(move || {
        let mut reader = BufReader::new(response_file);
        let mut response = Vec::new();
        let mut header = Vec::new();
        reader.read_until(b'\n', &mut header)?;
        let header_text = String::from_utf8_lossy(&header);
        let size = header_text
            .split_whitespace()
            .nth(2)
            .and_then(|value| value.trim().parse::<usize>().ok())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid cat-blob header")
            })?;
        response.extend_from_slice(&header);
        let mut body = vec![0; size + 1];
        reader.read_exact(&mut body)?;
        response.extend_from_slice(&body);
        response_tx.send(Ok(response.clone())).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "response receiver closed")
        })?;
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest)?;
        response.extend_from_slice(&rest);
        Ok::<Vec<u8>, std::io::Error>(response)
    });
    let response_reader = Fd3ResponseReader {
        first_response: response_rx,
        handle: response_handle,
    };

    let first = b"feature cat-blob\noption cat-blob-fd=1\nblob\nmark :1\ndata 4\ntest\ncat-blob :1\ncommit refs/heads/main\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 4\nmsg\nM 100644 :1 file.txt\nls \"file.txt\"\n";
    if let Err(error) = write_pipe_with_deadline(&mut stdin, first, deadline) {
        cleanup_fd3_process(&mut child, Some(stdin), stdout, stderr, response_reader);
        panic!("write fd3 prefix: {error}");
    }
    if let Err(error) = stdin.flush() {
        cleanup_fd3_process(&mut child, Some(stdin), stdout, stderr, response_reader);
        panic!("flush fd3 prefix: {error}");
    }
    let first_response = match response_reader
        .first_response
        .recv_timeout(remaining_until(deadline))
    {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            cleanup_fd3_process(&mut child, Some(stdin), stdout, stderr, response_reader);
            panic!("read fd3 response: {error}");
        }
        Err(error) => {
            cleanup_fd3_process(&mut child, Some(stdin), stdout, stderr, response_reader);
            panic!("fd3 response before EOF: {error}");
        }
    };
    if let Err(error) = write_pipe_with_deadline(
        &mut stdin,
        b"progress checkpoint\nget-mark :1\ndone\n",
        deadline,
    ) {
        cleanup_fd3_process(&mut child, Some(stdin), stdout, stderr, response_reader);
        panic!("write fd3 suffix: {error}");
    }
    if let Err(error) = stdin.flush() {
        cleanup_fd3_process(&mut child, Some(stdin), stdout, stderr, response_reader);
        panic!("flush fd3 suffix: {error}");
    }
    drop(stdin);
    let status = match wait_for_fast_import_child(&mut child, deadline) {
        Ok(status) => status,
        Err(error) => {
            cleanup_fd3_process(&mut child, None, stdout, stderr, response_reader);
            panic!("wait fd3 fast-import: {error}");
        }
    };
    let response_result = response_reader
        .handle
        .join()
        .map_err(|_| io::Error::other("fd3 response reader thread panicked"))
        .and_then(|result| result);
    let stdout_result = finish_joinable_pipe_reader(stdout);
    let stderr_result = finish_joinable_pipe_reader(stderr);
    let response = response_result.expect("fd3 response reader");
    let stdout = stdout_result.expect("fd3 stdout reader");
    let stderr = stderr_result.expect("fd3 stderr reader");
    assert!(first_response.ends_with(b"\ntest\n"));
    InteractiveFastImportFdOutput {
        status: status.code().unwrap_or(1),
        stdout,
        stderr,
        response,
        responded_before_eof: true,
    }
}

#[cfg(unix)]
#[test]
fn fast_import_cat_blob_streams_default_response_before_eof() {
    let stock = pinned_stock_git();
    let git_repo = tempfile::TempDir::new().expect("stock fast-import repo");
    let zmin_repo = tempfile::TempDir::new().expect("zmin fast-import repo");
    init_with_pinned_git(&stock, git_repo.path());
    init_with_pinned_git(&stock, zmin_repo.path());

    let stock_output = run_interactive_cat_blob(&stock, git_repo.path());
    let zmin_output = run_interactive_cat_blob(Path::new(zmin_bin()), zmin_repo.path());
    assert!(stock_output.responded_before_eof);
    assert!(zmin_output.responded_before_eof);
    assert_eq!(stock_output.status, 0);
    assert_eq!(zmin_output.status, 0);
    assert!(stock_output.stderr.is_empty());
    assert!(zmin_output.stderr.is_empty());
    assert_eq!(stock_output.stdout, zmin_output.stdout);
    assert_eq!(
        stock_output.stdout,
        b"30d74d258442c7c65512eafab474568dd706c430 blob 4\ntest\nprogress checkpoint\n30d74d258442c7c65512eafab474568dd706c430\n"
    );
}

#[test]
fn fast_import_ls_active_named_and_missing_entries_match_pinned_git() {
    let stock = pinned_stock_git();
    let git_repo = git_init();
    let zmin_repo = git_init();
    let seed = b"blob\nmark :1\ndata 5\nhello\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 4\nmsg\nM 100644 :1 file.txt\nget-mark :2\ndone\n";
    let stock_seed = run_fast_import_batch(&stock, git_repo.path(), seed);
    let zmin_seed = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), seed);
    assert_eq!(stock_seed.status, 0);
    assert_eq!(zmin_seed.status, 0);
    assert_eq!(stock_seed.stdout, zmin_seed.stdout);
    assert_eq!(stock_seed.stderr, zmin_seed.stderr);
    let commit = String::from_utf8(stock_seed.stdout)
        .expect("commit mark response utf8")
        .trim()
        .to_owned();
    let tree = git(
        &git_repo.path(),
        ["rev-parse", &format!("{commit}^{{tree}}")],
    )
    .trim()
    .to_owned();
    let tag_input = format!(
        "object {commit}\ntype commit\ntag v1\ntagger A <a@example.test> 0 +0000\n\nmessage\n"
    );
    let stock_tag =
        run_command_with_watchdog(&stock, &["mktag"], git_repo.path(), tag_input.as_bytes());
    let zmin_tag =
        run_command_with_watchdog(&stock, &["mktag"], zmin_repo.path(), tag_input.as_bytes());
    assert_eq!(stock_tag.status, 0);
    assert_eq!(zmin_tag.status, 0);
    assert_eq!(stock_tag.stdout, zmin_tag.stdout);
    let tag = String::from_utf8(stock_tag.stdout)
        .expect("tag id utf8")
        .trim()
        .to_owned();
    let query = format!(
        "feature ls\ncommit refs/heads/query\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 5\nquery\nfrom {commit}\nls \"file.txt\"\nls \"\"\nls \"missing\"\n\nls {commit} file.txt\nls {tree} file.txt\nls {tag} file.txt\nls {commit} \nls {commit} missing\nls :3 file.txt\ndone\n"
    );
    let stock_output = run_fast_import_batch(&stock, git_repo.path(), query.as_bytes());
    let zmin_output =
        run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), query.as_bytes());
    assert_eq!(stock_output.status, 0);
    assert_eq!(zmin_output.status, 0);
    assert_eq!(stock_output.stdout, zmin_output.stdout);
    assert_eq!(stock_output.stderr, zmin_output.stderr);
    assert_eq!(
        stock_output
            .stdout
            .iter()
            .filter(|byte| **byte == b'\n')
            .count(),
        9
    );
}

#[test]
fn fast_import_ls_malformed_requests_match_pinned_git() {
    let stock = pinned_stock_git();
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stock_tree = run_command_with_watchdog(
        &stock,
        &["hash-object", "-t", "tree", "-w", "--stdin"],
        git_repo.path(),
        b"",
    );
    let zmin_tree = run_command_with_watchdog(
        &stock,
        &["hash-object", "-t", "tree", "-w", "--stdin"],
        zmin_repo.path(),
        b"",
    );
    assert_eq!(stock_tree.status, 0);
    assert_eq!(zmin_tree.status, 0);
    assert_eq!(stock_tree.stdout, zmin_tree.stdout);
    let tree = String::from_utf8(stock_tree.stdout)
        .expect("empty tree id utf8")
        .trim()
        .to_owned();
    let fatal_cases = [
        (
            "feature ls\nls \"foo bar\"\n".to_owned(),
            "fatal: not in a commit: ls \"foo bar\"".to_owned(),
        ),
        (
            format!("feature ls\nls {tree}\n"),
            format!("fatal: missing space after tree-ish: ls {tree}"),
        ),
        (
            "feature ls\nls 0000 path\n".to_owned(),
            "fatal: invalid dataref: ls 0000 path".to_owned(),
        ),
        (
            "feature ls\nls 0000000000000000000000000000000000000000 path\n".to_owned(),
            "fatal: object not found: 0000000000000000000000000000000000000000".to_owned(),
        ),
        (
            format!("feature ls\nls {tree} \"file\"tail\n"),
            format!("fatal: garbage after path: ls {tree} \"file\"tail"),
        ),
        (
            format!("feature ls\nls {tree} \"file\n"),
            format!("fatal: invalid path: ls {tree} \"file"),
        ),
        (
            format!("feature ls\nls {tree} \"file\\000\"\n"),
            format!("fatal: NUL in path: ls {tree} \"file\\000\""),
        ),
    ];
    for (input, fatal) in fatal_cases {
        let stock_output = run_fast_import_batch(&stock, git_repo.path(), input.as_bytes());
        let zmin_output =
            run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), input.as_bytes());
        assert_eq!(stock_output.status, 128);
        assert_eq!(zmin_output.status, 128);
        assert_eq!(stock_output.stdout, zmin_output.stdout);
        assert_eq!(
            normalize_fast_import_crash_stderr(
                std::str::from_utf8(&stock_output.stderr).expect("stock stderr utf8"),
                &fatal,
            ),
            normalize_fast_import_crash_stderr(
                std::str::from_utf8(&zmin_output.stderr).expect("zmin stderr utf8"),
                &fatal,
            )
        );
    }
    let absent_oid_command = "ls 0000000000000000000000000000000000000000 path";
    let absent_oid_input = format!("feature ls\n{absent_oid_command}\n");
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stock_output = run_fast_import_batch(&stock, git_repo.path(), absent_oid_input.as_bytes());
    let zmin_output = run_fast_import_batch(
        Path::new(zmin_bin()),
        zmin_repo.path(),
        absent_oid_input.as_bytes(),
    );
    assert_eq!(stock_output.status, 128);
    assert_eq!(zmin_output.status, 128);
    assert_eq!(stock_output.stdout, zmin_output.stdout);
    assert_eq!(
        normalize_fast_import_crash_stderr(
            std::str::from_utf8(&stock_output.stderr).expect("stock stderr utf8"),
            "fatal: object not found: 0000000000000000000000000000000000000000",
        ),
        normalize_fast_import_crash_stderr(
            std::str::from_utf8(&zmin_output.stderr).expect("zmin stderr utf8"),
            "fatal: object not found: 0000000000000000000000000000000000000000",
        )
    );
    assert_eq!(
        normalize_fast_import_crash_report(
            &fast_import_crash_reports(git_repo.path()),
            "fatal: object not found: 0000000000000000000000000000000000000000",
            Some(absent_oid_command),
        ),
        normalize_fast_import_crash_report(
            &fast_import_crash_reports(zmin_repo.path()),
            "fatal: object not found: 0000000000000000000000000000000000000000",
            Some(absent_oid_command),
        )
    );
    let trailing_slash = format!("feature ls\nls {tree} \"file/\"\ndone\n");
    let stock_output = run_fast_import_batch(&stock, git_repo.path(), trailing_slash.as_bytes());
    let zmin_output = run_fast_import_batch(
        Path::new(zmin_bin()),
        zmin_repo.path(),
        trailing_slash.as_bytes(),
    );
    assert_eq!(stock_output.status, 0);
    assert_eq!(zmin_output.status, 0);
    assert_eq!(stock_output.stdout, b"missing file/\n");
    assert_eq!(stock_output.stdout, zmin_output.stdout);
}

#[test]
fn fast_import_delete_and_raw_path_forms_match_pinned_git() {
    let stock = pinned_stock_git();
    let cases: [(&str, &[u8]); 8] = [
        ("file", b"root.txt"),
        ("directory", b"dir"),
        ("missing", b"missing"),
        ("parent-blob", b"root.txt/child"),
        ("root", b""),
        ("nul-truncated", b"dir\0ignored"),
        ("trailing-space", b"dir "),
        ("quoted", br#""dir""#),
    ];
    for (label, path) in cases {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let input = fast_import_delete_fixture(path);
        let stock_output = run_fast_import_batch(&stock, git_repo.path(), &input);
        let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), &input);
        assert_eq!(stock_output.status, 0, "stock {label}");
        assert_eq!(zmin_output.status, 0, "zmin {label}");
        assert_eq!(stock_output.stdout, zmin_output.stdout, "stdout {label}");
        assert_eq!(stock_output.stderr, zmin_output.stderr, "stderr {label}");
        assert_eq!(
            git(&git_repo.path(), ["rev-parse", "refs/heads/main"]),
            git(&zmin_repo.path(), ["rev-parse", "refs/heads/main"]),
            "ref {label}"
        );
        assert_eq!(
            git(
                &git_repo.path(),
                ["ls-tree", "-r", "--name-only", "refs/heads/main"]
            ),
            git(
                &zmin_repo.path(),
                ["ls-tree", "-r", "--name-only", "refs/heads/main"]
            ),
            "tree {label}"
        );
        assert_eq!(
            fast_import_index_snapshot(git_repo.path()),
            fast_import_index_snapshot(zmin_repo.path()),
            "index {label}"
        );
    }

    let malformed = fast_import_delete_fixture(br#""bad\q""#);
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stock_output = run_fast_import_batch(&stock, git_repo.path(), &malformed);
    let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), &malformed);
    assert_eq!(stock_output.status, 128);
    assert_eq!(zmin_output.status, 128);
    assert_eq!(stock_output.stdout, zmin_output.stdout);
    assert_eq!(
        normalize_fast_import_crash_stderr(
            std::str::from_utf8(&stock_output.stderr).expect("stock stderr utf8"),
            "fatal: invalid path: D \"bad\\q\"",
        ),
        normalize_fast_import_crash_stderr(
            std::str::from_utf8(&zmin_output.stderr).expect("zmin stderr utf8"),
            "fatal: invalid path: D \"bad\\q\"",
        )
    );
    assert_eq!(
        fast_import_ref_snapshot(git_repo.path()),
        fast_import_ref_snapshot(zmin_repo.path()),
        "malformed D must not publish a ref"
    );
}

#[test]
fn fast_import_copy_rename_modes_match_pinned_git() {
    let input = b"blob\nmark :1\ndata 3\nabc\nblob\nmark :2\ndata 4\nlink\ncommit refs/heads/objects\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\ncommit refs/heads/main\nmark :4\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 file\nM 100755 :1 executable\nM 120000 :2 symlink\nM 160000 :3 gitlink\nC file file-copy\nC executable executable-copy\nC symlink symlink-copy\nC gitlink gitlink-copy\nR executable executable-renamed\ndone\n";
    assert_fast_import_success_differential(input, "mode/OID preservation");
}

#[test]
fn fast_import_copy_rename_subtrees_and_overlaps_match_pinned_git() {
    let cases = [
        (
            "copy subtree and replacement",
            b"C dir dst\nC dir dst/child\n".as_slice(),
        ),
        (
            "rename subtree and replacement",
            b"R dir dst\nC dst dst/child\n".as_slice(),
        ),
        (
            "copy file into descendant",
            b"C dir/a dir/a/child\n".as_slice(),
        ),
        (
            "rename file into descendant",
            b"R dir/a dir/a/child\n".as_slice(),
        ),
        ("copy child over ancestor", b"C dir/a dir\n".as_slice()),
        ("rename child over ancestor", b"R dir/a dir\n".as_slice()),
    ];
    for (label, operations) in cases {
        let mut input = b"blob\nmark :1\ndata 1\na\nblob\nmark :2\ndata 1\nb\ncommit refs/heads/main\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 dir/a\nM 100644 :2 dir/b\nM 100644 :2 dst/old\n".to_vec();
        input.extend_from_slice(operations);
        input.extend_from_slice(b"done\n");
        assert_fast_import_success_differential(&input, label);
    }
}

#[test]
fn fast_import_copy_snapshot_is_independent_of_later_source_changes() {
    let input = b"blob\nmark :1\ndata 1\na\nblob\nmark :2\ndata 1\nb\nblob\nmark :3\ndata 1\nc\ncommit refs/heads/main\nmark :4\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 dir/a\nM 100644 :2 dir/b\ncommit refs/heads/main\nmark :5\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :4\nC dir copied\nD dir/a\nM 100644 :3 dir/b\ndone\n";
    assert_fast_import_success_differential(input, "copy snapshot independence");

    let stock = pinned_stock_git();
    let zmin = PathBuf::from(zmin_bin());
    let stock_repo = git_init();
    let zmin_repo = git_init();
    let stock_output = run_fast_import_batch(&stock, stock_repo.path(), input);
    let zmin_output = run_fast_import_batch(&zmin, zmin_repo.path(), input);
    assert_eq!(stock_output.status, 0);
    assert_eq!(zmin_output.status, 0);
    for (label, tree) in [
        ("copied/a", b"copied/a\0".as_slice()),
        ("copied/b", b"copied/b\0".as_slice()),
        ("dir/b", b"dir/b\0".as_slice()),
    ] {
        assert!(
            fast_import_tree_snapshot(&stock, stock_repo.path(), "refs/heads/main")
                .windows(tree.len())
                .any(|window| window == tree),
            "stock retains {label}"
        );
        assert!(
            fast_import_tree_snapshot(&zmin, zmin_repo.path(), "refs/heads/main")
                .windows(tree.len())
                .any(|window| window == tree),
            "zmin retains {label}"
        );
    }
    for (label, tree) in [("source dir/a", b"dir/a\0".as_slice())] {
        assert!(
            !fast_import_tree_snapshot(&stock, stock_repo.path(), "refs/heads/main")
                .windows(tree.len())
                .any(|window| window == tree),
            "stock source no longer contains {label}"
        );
        assert!(
            !fast_import_tree_snapshot(&zmin, zmin_repo.path(), "refs/heads/main")
                .windows(tree.len())
                .any(|window| window == tree),
            "zmin source no longer contains {label}"
        );
    }
}

#[test]
fn fast_import_copy_rename_root_empty_and_raw_paths_match_pinned_git() {
    let cases = [
        ("quoted empty source", b"C \"\" dst\n".as_slice()),
        ("unquoted empty source", b"C  dst\n".as_slice()),
        ("quoted empty root replacement", b"C \"\" \n".as_slice()),
        ("unquoted empty root replacement", b"C  \n".as_slice()),
        ("rename quoted empty source", b"R \"\" dst\n".as_slice()),
        ("rename unquoted empty source", b"R  dst\n".as_slice()),
    ];
    for (label, operation) in cases {
        let mut input = b"blob\nmark :1\ndata 1\na\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 file\n".to_vec();
        input.extend_from_slice(operation);
        input.extend_from_slice(b"done\n");
        assert_fast_import_success_differential(&input, label);
    }

    let mut input = b"blob\nmark :1\ndata 1\na\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 raw\x80\n".to_vec();
    input.extend_from_slice(b"C raw\x80 \"copy\\040raw\"\nR \"copy\\040raw\" renamed\x80\ndone\n");
    assert_fast_import_raw_tree_differential(&input, "raw non-UTF-8 source");

    let input = b"blob\nmark :1\ndata 1\na\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 src\nC src \"copied\"\0ignored\ndone\n";
    assert_fast_import_success_differential(input, "raw NUL destination terminator");
}

#[test]
fn fast_import_copy_rename_leading_slash_source_asymmetry_matches_pinned_git() {
    let cases = [
        (
            "copy leading file slash",
            b"C /foo dst\ndone\n".as_slice(),
            true,
            b"".as_slice(),
        ),
        (
            "copy leading root slash",
            b"C / dst\ndone\n".as_slice(),
            true,
            b"".as_slice(),
        ),
        (
            "rename leading file slash",
            b"R /foo dst\ndone\n".as_slice(),
            false,
            b"path /foo not in branch".as_slice(),
        ),
        (
            "rename leading root slash",
            b"R / dst\ndone\n".as_slice(),
            false,
            b"path / not in branch".as_slice(),
        ),
    ];
    for (label, operation, success, fatal) in cases {
        let mut input = b"blob\nmark :1\ndata 1\na\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 foo\n".to_vec();
        input.extend_from_slice(operation);
        if success {
            assert_fast_import_success_differential(&input, label);
        } else {
            assert_fast_import_failure_differential(
                &input,
                fatal,
                operation.strip_suffix(b"\ndone\n").unwrap(),
                label,
            );
        }
    }
}

#[test]
fn fast_import_copy_rename_directory_to_empty_root_match_pinned_git() {
    for (label, operation) in [("copy", "C dir \"\""), ("rename", "R dir \"\"")] {
        let mut input = b"blob\nmark :1\ndata 1\na\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 dir/file\ncommit refs/heads/main\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :2\n".to_vec();
        input.extend_from_slice(operation.as_bytes());
        input.extend_from_slice(b"\ndone\n");
        assert_fast_import_success_differential(&input, label);
    }
}

#[test]
fn fast_import_copy_rename_explicit_empty_trees_match_pinned_git() {
    let stock = pinned_stock_git();
    let zmin = PathBuf::from(zmin_bin());
    for init_args in [&[][..], &["--object-format=sha256"][..]] {
        for (label, operation) in [("copy", "C empty-a copied"), ("rename", "R empty-a moved")] {
            let stock_repo = git_init_with_program(&stock, init_args);
            let zmin_repo = git_init_with_program(&stock, init_args);
            let (stock_empty, stock_root, stock_base, stock_blob) =
                fast_import_seed_explicit_empty_tree(&stock, stock_repo.path());
            let (zmin_empty, zmin_root, zmin_base, zmin_blob) =
                fast_import_copy_seeded_tree_objects(
                    stock_repo.path(),
                    zmin_repo.path(),
                    &stock_empty,
                    &stock_root,
                    &stock_base,
                    &stock_blob,
                );
            assert_eq!(stock_empty, zmin_empty, "empty tree seed {label}");
            assert_eq!(stock_root, zmin_root, "root tree seed {label}");
            assert_eq!(stock_base, zmin_base, "base commit seed {label}");
            assert_eq!(stock_blob, zmin_blob, "root blob seed {label}");

            let make_input = |base: &str| {
                format!(
                    "commit refs/heads/main\nmark :1\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom {base}\n{operation}\ndone\n"
                )
            };
            let stock_output = run_fast_import_batch(
                &stock,
                stock_repo.path(),
                make_input(&stock_base).as_bytes(),
            );
            let zmin_output =
                run_fast_import_batch(&zmin, zmin_repo.path(), make_input(&zmin_base).as_bytes());
            assert_eq!(stock_output.status, 0, "stock {label}");
            assert_eq!(zmin_output.status, 0, "zmin {label}");
            assert_eq!(stock_output.stdout, zmin_output.stdout, "stdout {label}");
            assert_eq!(stock_output.stderr, zmin_output.stderr, "stderr {label}");
            assert_eq!(
                fast_import_ref_snapshot_with_program(&stock, stock_repo.path()),
                fast_import_ref_snapshot_with_program(&zmin, zmin_repo.path()),
                "refs {label}"
            );
            assert_eq!(
                fast_import_tree_state(&stock, stock_repo.path(), "refs/heads/main"),
                fast_import_tree_state(&zmin, zmin_repo.path(), "refs/heads/main"),
                "tree {label}"
            );
            assert_eq!(
                fast_import_commit_tree_object(&stock, stock_repo.path()),
                fast_import_commit_tree_object(&zmin, zmin_repo.path()),
                "raw tree object {label}"
            );
        }
    }
}

#[test]
fn fast_import_copy_rename_true_empty_root_match_pinned_git() {
    let stock = pinned_stock_git();
    let zmin = PathBuf::from(zmin_bin());
    for init_args in [&[][..], &["--object-format=sha256"][..]] {
        let stock_repo = git_init_with_program(&stock, init_args);
        let zmin_repo = git_init_with_program(&stock, init_args);
        let (stock_empty, stock_base) = fast_import_seed_true_empty_root(&stock, stock_repo.path());
        fast_import_copy_seeded_objects(
            stock_repo.path(),
            zmin_repo.path(),
            &[&stock_empty, &stock_base],
        );
        let expected_empty = if init_args.is_empty() {
            "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
        } else {
            "6ef19b41225c5369f1c104d45d8d85efa9b057b53b14b4b9b939dd74decc5321"
        };
        assert_eq!(stock_empty, expected_empty);
        for (label, operation) in [("copy", "C \"\" dst"), ("rename", "R \"\" dst")] {
            let input = format!(
                "commit refs/heads/main\nmark :1\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom {stock_base}\n{operation}\ndone\n"
            );
            let stock_output = run_fast_import_batch(&stock, stock_repo.path(), input.as_bytes());
            let zmin_output = run_fast_import_batch(&zmin, zmin_repo.path(), input.as_bytes());
            assert_eq!(stock_output.status, 0, "stock {label}");
            assert_eq!(zmin_output.status, 0, "zmin {label}");
            assert_eq!(stock_output.stdout, zmin_output.stdout, "stdout {label}");
            assert_eq!(stock_output.stderr, zmin_output.stderr, "stderr {label}");
            assert_eq!(
                fast_import_ref_snapshot_with_program(&stock, stock_repo.path()),
                fast_import_ref_snapshot_with_program(&zmin, zmin_repo.path()),
                "refs {label}"
            );
            let stock_tree = fast_import_commit_tree_object(&stock, stock_repo.path());
            let zmin_tree = fast_import_commit_tree_object(&zmin, zmin_repo.path());
            assert_eq!(stock_tree, zmin_tree, "root tree {label}");
            let mut expected_entry = b"40000 dst\0".to_vec();
            expected_entry.extend_from_slice(&fast_import_hex_decode(stock_empty.as_bytes()));
            assert!(
                stock_tree
                    .windows(expected_entry.len())
                    .any(|window| window == expected_entry),
                "empty root destination entry {label}"
            );
        }
    }
}

#[test]
fn fast_import_generated_empty_tree_survives_delete_and_copy_rename() {
    let stock = pinned_stock_git();
    let zmin = PathBuf::from(zmin_bin());
    for init_args in [&[][..], &["--object-format=sha256"][..]] {
        for (label, emptying, operation) in [
            ("deleteall-copy", "deleteall\n", "C \"\" dst\n"),
            ("deleteall-rename", "deleteall\n", "R \"\" dst\n"),
            ("delete-copy", "D file\n", "C \"\" dst\n"),
            ("delete-rename", "D file\n", "R \"\" dst\n"),
        ] {
            let stock_repo = git_init_with_program(&stock, init_args);
            let zmin_repo = git_init_with_program(&stock, init_args);
            let input = if emptying.starts_with('D') {
                format!(
                    "blob\nmark :1\ndata 1\na\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 file\ncommit refs/heads/main\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :2\n{emptying}{operation}commit refs/heads/after\nmark :4\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :3\nC dst copied\nreset refs/heads/check\nfrom :3\ndone\n"
                )
            } else {
                format!(
                    "commit refs/heads/main\nmark :1\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :1\n{emptying}{operation}commit refs/heads/after\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :2\nC dst copied\nreset refs/heads/check\nfrom :2\ndone\n"
                )
            };
            let stock_output = run_fast_import_batch(&stock, stock_repo.path(), input.as_bytes());
            let zmin_output = run_fast_import_batch(&zmin, zmin_repo.path(), input.as_bytes());
            assert_eq!(stock_output.status, 0, "stock {label}");
            assert_eq!(zmin_output.status, 0, "zmin {label}");
            assert_eq!(stock_output.stdout, zmin_output.stdout, "stdout {label}");
            assert_eq!(stock_output.stderr, zmin_output.stderr, "stderr {label}");
            assert_eq!(
                fast_import_named_refs(&stock, stock_repo.path()),
                fast_import_named_refs(&zmin, zmin_repo.path()),
                "refs {label}"
            );
            assert_eq!(
                fast_import_commit_tree_object(&stock, stock_repo.path()),
                fast_import_commit_tree_object(&zmin, zmin_repo.path()),
                "tree {label}"
            );
            let empty_id = if init_args.is_empty() {
                b"4b825dc642cb6eb9a060e54bf8d69288fbee4904".as_slice()
            } else {
                b"6ef19b41225c5369f1c104d45d8d85efa9b057b53b14b4b9b939dd74decc5321".as_slice()
            };
            assert!(
                fast_import_physical_object_exists(stock_repo.path(), empty_id),
                "stock empty tree must be physically stored {label}"
            );
            assert!(
                fast_import_physical_object_exists(zmin_repo.path(), empty_id),
                "zmin empty tree must be physically stored {label}"
            );
            let stock_empty =
                fast_import_zmin_batch_object_typed(&stock, stock_repo.path(), empty_id, b"tree");
            let zmin_empty =
                fast_import_zmin_batch_object_typed(&zmin, zmin_repo.path(), empty_id, b"tree");
            assert!(stock_empty.is_empty(), "stock empty tree body {label}");
            assert!(zmin_empty.is_empty(), "zmin empty tree body {label}");
        }
    }
}

#[test]
fn fast_import_reset_without_from_clears_empty_tree_state() {
    let stock = pinned_stock_git();
    let zmin = PathBuf::from(zmin_bin());
    for init_args in [&[][..], &["--object-format=sha256"][..]] {
        let stock_repo = git_init_with_program(&stock, init_args);
        let zmin_repo = git_init_with_program(&stock, init_args);
        let input = b"commit refs/heads/main\nmark :1\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nreset refs/heads/main\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nC \"\" dst\ndone\n";
        let stock_output = run_fast_import_batch(&stock, stock_repo.path(), input);
        let zmin_output = run_fast_import_batch(&zmin, zmin_repo.path(), input);
        assert_eq!(stock_output.status, 0, "stock reset no-from");
        assert_eq!(zmin_output.status, 0, "zmin reset no-from");
        assert_eq!(stock_output.stdout, zmin_output.stdout, "reset stdout");
        assert_eq!(stock_output.stderr, zmin_output.stderr, "reset stderr");
        assert_eq!(
            fast_import_named_refs(&stock, stock_repo.path()),
            fast_import_named_refs(&zmin, zmin_repo.path()),
            "reset refs"
        );
        assert_eq!(
            fast_import_commit_tree_object(&stock, stock_repo.path()),
            fast_import_commit_tree_object(&zmin, zmin_repo.path()),
            "reset tree"
        );
    }
}

fn fast_import_seed_ref(program: &Path, repo: &Path, reference: &str) -> String {
    let input = format!(
        "blob\nmark :1\ndata 3\nold\ncommit {reference}\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 old.txt\ndone\n"
    );
    let output = run_fast_import_batch(program, repo, input.as_bytes());
    assert_eq!(output.status, 0, "seed {reference}: {:?}", output.stderr);
    let reference = fast_import_ref_snapshot_with_program_for(program, repo, reference);
    assert_eq!(reference.status, 0, "seed ref {reference:?}");
    String::from_utf8(reference.stdout)
        .expect("seed ref id utf8")
        .trim()
        .to_owned()
}

fn assert_fast_import_reset_success_pair(
    stock: &Path,
    stock_repo: &Path,
    zmin_repo: &Path,
    input: &[u8],
) {
    let stock_output = run_fast_import_batch(stock, stock_repo, input);
    let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo, input);
    assert_eq!(
        stock_output.status,
        0,
        "stock reset lifecycle input={:?}: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(input),
        stock_output.stdout,
        stock_output.stderr
    );
    assert_eq!(
        zmin_output.status,
        0,
        "zmin reset lifecycle input={:?}: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(input),
        zmin_output.stdout,
        zmin_output.stderr
    );
    assert_eq!(
        stock_output.stdout, zmin_output.stdout,
        "reset lifecycle stdout"
    );
    assert_eq!(
        stock_output.stderr, zmin_output.stderr,
        "reset lifecycle stderr"
    );
}

fn assert_fast_import_reset_success_pair_with_option(
    stock: &Path,
    stock_repo: &Path,
    zmin_repo: &Path,
    option: &str,
    input: &[u8],
) {
    let stock_output = run_fast_import_batch_with_option(stock, stock_repo, option, input);
    let zmin_output =
        run_fast_import_batch_with_option(Path::new(zmin_bin()), zmin_repo, option, input);
    assert_eq!(
        stock_output.status, 0,
        "stock reset lifecycle with {option}: stdout={:?} stderr={:?}",
        stock_output.stdout, stock_output.stderr
    );
    assert_eq!(
        zmin_output.status, 0,
        "zmin reset lifecycle with {option}: stdout={:?} stderr={:?}",
        zmin_output.stdout, zmin_output.stderr
    );
    assert_eq!(
        stock_output.stdout, zmin_output.stdout,
        "reset option stdout"
    );
    assert_eq!(
        stock_output.stderr, zmin_output.stderr,
        "reset option stderr"
    );
}

#[test]
fn fast_import_reset_to_empty_tombstone_lifecycle_matches_stock_git() {
    let stock = pinned_stock_git();
    let zmin = PathBuf::from(zmin_bin());
    for init_args in [&[][..], &["--object-format=sha256"][..]] {
        // A pending update for the reset ref must be discarded at checkpoint,
        // while an update for another ref must still publish.
        let stock_repo = git_init_with_program(&stock, init_args);
        let zmin_repo = git_init_with_program(&stock, init_args);
        let pending_isolation = b"blob\nmark :1\ndata 1\nO\ncommit refs/heads/other\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 other.txt\ncommit refs/heads/main\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 main.txt\nreset refs/heads/main\nprogress before checkpoint\ncheckpoint\nprogress after checkpoint\ndone\n";
        assert_fast_import_reset_success_pair(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            pending_isolation,
        );
        let stock_main =
            fast_import_ref_snapshot_with_program_for(&stock, stock_repo.path(), "refs/heads/main");
        let zmin_main =
            fast_import_ref_snapshot_with_program_for(&zmin, zmin_repo.path(), "refs/heads/main");
        assert_eq!(stock_main, zmin_main, "reset target pending update");
        assert_ne!(stock_main.status, 0, "reset target must remain unborn");
        assert_eq!(
            fast_import_ref_snapshot_with_program_for(
                &stock,
                stock_repo.path(),
                "refs/heads/other"
            ),
            fast_import_ref_snapshot_with_program_for(&zmin, zmin_repo.path(), "refs/heads/other"),
            "other ref pending update must survive reset"
        );

        // A reset followed by EOF must not publish an earlier uncheckpointed
        // commit, and an unborn ref remains absent after checkpoint plus EOF.
        let stock_repo = git_init_with_program(&stock, init_args);
        let zmin_repo = git_init_with_program(&stock, init_args);
        let pending_eof = b"blob\nmark :1\ndata 1\nP\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 pending.txt\nreset refs/heads/main\n";
        assert_fast_import_reset_success_pair(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            pending_eof,
        );
        assert_eq!(
            fast_import_ref_snapshot_with_program_for(&stock, stock_repo.path(), "refs/heads/main"),
            fast_import_ref_snapshot_with_program_for(&zmin, zmin_repo.path(), "refs/heads/main"),
            "EOF reset target"
        );
        assert_ne!(
            fast_import_ref_snapshot_with_program_for(&stock, stock_repo.path(), "refs/heads/main")
                .status,
            0,
            "EOF reset target must remain unborn"
        );

        let stock_repo = git_init_with_program(&stock, init_args);
        let zmin_repo = git_init_with_program(&stock, init_args);
        let checkpoint_eof = b"blob\nmark :1\ndata 1\nQ\ncommit refs/heads/unborn\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 unborn.txt\nreset refs/heads/unborn\ncheckpoint\n";
        assert_fast_import_reset_success_pair(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            checkpoint_eof,
        );
        assert_eq!(
            fast_import_ref_snapshot_with_program_for(
                &stock,
                stock_repo.path(),
                "refs/heads/unborn"
            ),
            fast_import_ref_snapshot_with_program_for(&zmin, zmin_repo.path(), "refs/heads/unborn"),
            "checkpoint EOF unborn ref"
        );

        // Existing on-disk state must remain untouched by reset, but the next
        // commit must be parentless and must not inherit old paths.
        let stock_repo = git_init_with_program(&stock, init_args);
        let zmin_repo = git_init_with_program(&stock, init_args);
        let stock_old = fast_import_seed_ref(&stock, stock_repo.path(), "refs/heads/main");
        let zmin_old = fast_import_seed_ref(&zmin, zmin_repo.path(), "refs/heads/main");
        assert_eq!(stock_old, zmin_old, "seed old main");
        let parentless = b"blob\nmark :1\ndata 3\nnew\nreset refs/heads/main\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 1 +0000\ncommitter A <a@example.test> 1 +0000\ndata 0\nM 100644 :1 new.txt\ndone\n";
        assert_fast_import_reset_success_pair_with_option(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            "--force",
            parentless,
        );
        let stock_commit = fast_import_commit_object(&stock, stock_repo.path(), "refs/heads/main");
        let zmin_commit = fast_import_commit_object(&zmin, zmin_repo.path(), "refs/heads/main");
        assert_eq!(stock_commit, zmin_commit, "parentless commit object");
        assert!(
            !stock_commit
                .split(|byte| *byte == b'\n')
                .any(|line| line.starts_with(b"parent ")),
            "reset commit must have no parent"
        );
        assert_eq!(
            fast_import_commit_tree_object(&stock, stock_repo.path()),
            fast_import_commit_tree_object(&zmin, zmin_repo.path()),
            "parentless commit tree"
        );
        assert!(
            !fast_import_commit_tree_object(&stock, stock_repo.path())
                .windows(b"old.txt".len())
                .any(|window| window == b"old.txt"),
            "parentless commit must not retain old path"
        );

        // C and R must see the reset empty root rather than the on-disk tree.
        for operation in [b"C \"\" dst\n".as_slice(), b"R \"\" dst\n".as_slice()] {
            let stock_repo = git_init_with_program(&stock, init_args);
            let zmin_repo = git_init_with_program(&stock, init_args);
            fast_import_seed_ref(&stock, stock_repo.path(), "refs/heads/main");
            fast_import_seed_ref(&zmin, zmin_repo.path(), "refs/heads/main");
            let mut input = b"reset refs/heads/main\ncommit refs/heads/main\nmark :1\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\n".to_vec();
            input.extend_from_slice(operation);
            input.extend_from_slice(b"done\n");
            assert_fast_import_reset_success_pair_with_option(
                &stock,
                stock_repo.path(),
                zmin_repo.path(),
                "--force",
                &input,
            );
            assert_eq!(
                fast_import_commit_tree_object(&stock, stock_repo.path()),
                fast_import_commit_tree_object(&zmin, zmin_repo.path()),
                "reset empty-root {operation:?}"
            );
        }

        // A reset with `from` must retain normal target loading and clear the
        // tombstone only after the target has been staged.
        let stock_repo = git_init_with_program(&stock, init_args);
        let zmin_repo = git_init_with_program(&stock, init_args);
        let stock_base = fast_import_seed_ref(&stock, stock_repo.path(), "refs/heads/base");
        let zmin_base = fast_import_seed_ref(&zmin, zmin_repo.path(), "refs/heads/base");
        assert_eq!(stock_base, zmin_base, "seed reset-from base");
        let reset_from = b"reset refs/heads/main\nfrom refs/heads/base\ndone\n";
        assert_fast_import_reset_success_pair(
            &stock,
            stock_repo.path(),
            zmin_repo.path(),
            reset_from,
        );
        assert_eq!(
            fast_import_ref_snapshot_with_program_for(&stock, stock_repo.path(), "refs/heads/main"),
            fast_import_ref_snapshot_with_program_for(&zmin, zmin_repo.path(), "refs/heads/main"),
            "reset with from ref"
        );
        assert_eq!(
            fast_import_commit_tree_object(&stock, stock_repo.path()),
            fast_import_commit_tree_object(&zmin, zmin_repo.path()),
            "reset with from tree"
        );
    }
}

#[cfg(unix)]
#[test]
fn fast_import_reset_checkpoint_barriers_preserve_live_existing_ref() {
    let stock = pinned_stock_git();
    let zmin = PathBuf::from(zmin_bin());
    for init_args in [&[][..], &["--object-format=sha256"][..]] {
        let stock_repo = git_init_with_program(&stock, init_args);
        let zmin_repo = git_init_with_program(&stock, init_args);
        let stock_old = fast_import_seed_ref(&stock, stock_repo.path(), "refs/heads/main");
        let zmin_old = fast_import_seed_ref(&zmin, zmin_repo.path(), "refs/heads/main");
        assert_eq!(stock_old, zmin_old, "live reset seed");
        let stock_observation = run_interactive_reset_observation(&stock, stock_repo.path());
        let zmin_observation = run_interactive_reset_observation(&zmin, zmin_repo.path());
        assert_eq!(
            stock_observation, zmin_observation,
            "live reset observations"
        );
        assert_eq!(stock_observation.final_output.status, 0);
        assert_eq!(stock_observation.final_output.stdout, b"");
        assert_eq!(stock_observation.final_output.stderr, b"");
        assert_eq!(stock_observation.before_checkpoint.status, 0);
        assert_eq!(stock_observation.after_checkpoint.status, 0);
        assert_eq!(
            String::from_utf8_lossy(&stock_observation.before_checkpoint.stdout).trim(),
            stock_old
        );
        assert_eq!(
            String::from_utf8_lossy(&stock_observation.after_checkpoint.stdout).trim(),
            stock_old
        );
    }
}

#[test]
fn fast_import_copy_rename_failures_are_atomic_and_match_pinned_git() {
    let cases = [
        (
            "missing source",
            b"C missing dst\ndone\n".as_slice(),
            b"path missing not in branch".as_slice(),
            b"C missing dst".as_slice(),
        ),
        (
            "invalid destination",
            b"C src bad/../x\ndone\n".as_slice(),
            b"invalid path 'bad/../x'".as_slice(),
            b"C src bad/../x".as_slice(),
        ),
        (
            "malformed quoted source",
            b"C \"src\"\0dst\ndone\n".as_slice(),
            b"missing space after source: C \"src\"".as_slice(),
            b"C \"src\"".as_slice(),
        ),
        (
            "malformed destination",
            b"C src \"bad\\q\"\ndone\n".as_slice(),
            b"invalid dest: C src \"bad\\q\"".as_slice(),
            b"C src \"bad\\q\"".as_slice(),
        ),
        (
            "raw non-UTF-8 missing source",
            b"C missing\x80 dst\ndone\n".as_slice(),
            b"path missing\x80 not in branch".as_slice(),
            b"C missing\x80 dst".as_slice(),
        ),
        (
            "escaped NUL source",
            b"C \"src\\000\" dst\ndone\n".as_slice(),
            b"NUL in source: C \"src\\000\" dst".as_slice(),
            b"C \"src\\000\" dst".as_slice(),
        ),
        (
            "escaped NUL destination",
            b"C src \"dst\\000\"\ndone\n".as_slice(),
            b"NUL in dest: C src \"dst\\000\"".as_slice(),
            b"C src \"dst\\000\"".as_slice(),
        ),
        (
            "escaped NUL rename source",
            b"R \"src\\000\" dst\ndone\n".as_slice(),
            b"NUL in source: R \"src\\000\" dst".as_slice(),
            b"R \"src\\000\" dst".as_slice(),
        ),
        (
            "escaped NUL rename destination",
            b"R src \"dst\\000\"\ndone\n".as_slice(),
            b"NUL in dest: R src \"dst\\000\"".as_slice(),
            b"R src \"dst\\000\"".as_slice(),
        ),
    ];
    for (label, operation, fatal, command) in cases {
        let mut input = b"blob\nmark :1\ndata 1\na\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 src\n".to_vec();
        input.extend_from_slice(operation);
        assert_fast_import_failure_differential(&input, fatal, command, label);
    }
}

#[test]
fn fast_import_copy_rename_source_trailing_slashes_match_pinned_git() {
    let cases = [
        (
            b"M 100644 :1 file\n".as_slice(),
            b"C file/ dst\ndone\n".as_slice(),
            b"path file/ not in branch".as_slice(),
            b"C file/ dst".as_slice(),
            "copy file trailing slash",
        ),
        (
            b"M 100644 :1 file\n".as_slice(),
            b"R file/ dst\ndone\n".as_slice(),
            b"path file/ not in branch".as_slice(),
            b"R file/ dst".as_slice(),
            "rename file trailing slash",
        ),
        (
            b"M 100644 :1 dir/file\n".as_slice(),
            b"C dir/ dst\ndone\n".as_slice(),
            b"empty path component found in input".as_slice(),
            b"C dir/ dst".as_slice(),
            "copy directory trailing slash",
        ),
        (
            b"M 100644 :1 dir/file\n".as_slice(),
            b"R dir/ dst\ndone\n".as_slice(),
            b"path dir/ not in branch".as_slice(),
            b"R dir/ dst".as_slice(),
            "rename directory trailing slash",
        ),
        (
            b"M 100644 :1 dir/file\n".as_slice(),
            b"C dir dst/\ndone\n".as_slice(),
            b"invalid path 'dst/'".as_slice(),
            b"C dir dst/".as_slice(),
            "copy tree destination trailing slash",
        ),
        (
            b"M 100644 :1 dir/file\n".as_slice(),
            b"R dir dst/\ndone\n".as_slice(),
            b"invalid path 'dst/'".as_slice(),
            b"R dir dst/".as_slice(),
            "rename tree destination trailing slash",
        ),
        (
            b"M 100644 :1 src\n".as_slice(),
            b"C src \"\"\ndone\n".as_slice(),
            b"root cannot be a non-directory".as_slice(),
            b"C src \"\"".as_slice(),
            "copy file empty root",
        ),
        (
            b"M 100644 :1 src\n".as_slice(),
            b"R src \"\"\ndone\n".as_slice(),
            b"root cannot be a non-directory".as_slice(),
            b"R src \"\"".as_slice(),
            "rename file empty root",
        ),
    ];
    for (seed, operation, fatal, command, label) in cases {
        let input = fast_import_copy_rename_failure_input(seed, operation);
        assert_fast_import_failure_differential(&input, fatal, command, label);
    }
}

#[test]
fn fast_import_rename_failure_removes_source_in_crash_state_only() {
    let input = b"blob\nmark :1\ndata 1\na\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 src\ncommit refs/heads/main\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :2\nR src bad/../x\n";
    let report = assert_fast_import_failure_differential(
        input,
        b"invalid path 'bad/../x'",
        b"R src bad/../x",
        "rename invalid destination state",
    );
    let report = String::from_utf8(report).expect("ASCII rename crash report");
    assert!(
        report.contains("status      : active loaded dirty"),
        "rename failure must leave the active branch dirty: {report}"
    );
    assert!(
        report.contains("cur tree    : 0000000000000000000000000000000000000000"),
        "rename failure must clear the current tree: {report}"
    );
    assert!(
        !report.contains("old tree    : 0000000000000000000000000000000000000000"),
        "rename failure must retain the committed old tree: {report}"
    );
}

#[test]
fn fast_import_modify_raw_and_quoted_paths_match_pinned_git() {
    let stock = pinned_stock_git();
    let input = b"blob\nmark :1\ndata 1\na\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 raw\x80\nM 100644 :1 \"quoted\\040name\"\nM 100644 :1 \"octal\\303\\251\"\ndone\n".to_vec();
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stock_output = run_fast_import_batch(&stock, git_repo.path(), &input);
    let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), &input);
    assert_eq!(stock_output.status, 0);
    assert_eq!(zmin_output.status, 0);
    assert_eq!(stock_output.stdout, zmin_output.stdout);
    assert_eq!(stock_output.stderr, zmin_output.stderr);
    let tree_args = ["ls-tree", "-r", "-z", "--name-only", "refs/heads/main"];
    let stock_tree = run_command_with_watchdog(&stock, &tree_args, git_repo.path(), &[]);
    let zmin_tree =
        run_command_with_watchdog(Path::new(zmin_bin()), &tree_args, zmin_repo.path(), &[]);
    assert_eq!(stock_tree.status, 0);
    assert_eq!(zmin_tree.status, 0);
    assert_eq!(stock_tree.stdout, zmin_tree.stdout);
    assert_eq!(stock_tree.stderr, zmin_tree.stderr);

    let mut input = fast_import_modify_base();
    input.extend_from_slice(b"M 100644 :1 \"bad\\q\"\ndone\n");
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stock_ref_before = fast_import_ref_snapshot(git_repo.path());
    let zmin_ref_before = fast_import_ref_snapshot(zmin_repo.path());
    let stock_tree_before = fast_import_tree_state(&stock, git_repo.path(), "refs/heads/main");
    let zmin_tree_before =
        fast_import_tree_state(Path::new(zmin_bin()), zmin_repo.path(), "refs/heads/main");
    let stock_output = run_fast_import_batch(&stock, git_repo.path(), &input);
    let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), &input);
    assert_eq!(stock_output.status, 128);
    assert_eq!(zmin_output.status, 128);
    assert_eq!(stock_output.stdout, zmin_output.stdout);
    assert_eq!(
        normalize_fast_import_crash_stderr(
            std::str::from_utf8(&stock_output.stderr).expect("stock stderr utf8"),
            "fatal: invalid path: M 100644 :1 \"bad\\q\"",
        ),
        normalize_fast_import_crash_stderr(
            std::str::from_utf8(&zmin_output.stderr).expect("zmin stderr utf8"),
            "fatal: invalid path: M 100644 :1 \"bad\\q\"",
        )
    );
    assert_eq!(
        stock_ref_before,
        fast_import_ref_snapshot(git_repo.path()),
        "stock malformed M must not publish a ref"
    );
    assert_eq!(
        zmin_ref_before,
        fast_import_ref_snapshot(zmin_repo.path()),
        "zmin malformed M must not publish a ref"
    );
    assert_eq!(
        stock_tree_before,
        fast_import_tree_state(&stock, git_repo.path(), "refs/heads/main"),
        "stock malformed M must not change the tree"
    );
    assert_eq!(
        zmin_tree_before,
        fast_import_tree_state(Path::new(zmin_bin()), zmin_repo.path(), "refs/heads/main"),
        "zmin malformed M must not change the tree"
    );
    assert_eq!(
        normalize_fast_import_crash_report(
            &fast_import_crash_reports(git_repo.path()),
            "fatal: invalid path: M 100644 :1 \"bad\\q\"",
            Some("M 100644 :1 \"bad\\q\""),
        ),
        normalize_fast_import_crash_report(
            &fast_import_crash_reports(zmin_repo.path()),
            "fatal: invalid path: M 100644 :1 \"bad\\q\"",
            Some("M 100644 :1 \"bad\\q\""),
        )
    );
}

#[test]
fn fast_import_modify_tree_root_and_empty_tree_match_pinned_git() {
    let stock = pinned_stock_git();
    let empty_tree = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
    let cases = [
        ("root-tree", format!("M 040000 {empty_tree} \n"), ""),
        (
            "nested-empty-tree",
            format!("M 040000 {empty_tree} dir\n"),
            "100644 blob",
        ),
    ];
    for (label, change, expected_marker) in cases {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let mut input = fast_import_modify_base();
        input.extend_from_slice(
            format!(
                "commit refs/heads/test\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :2\n{change}done\n"
            )
            .as_bytes(),
        );
        let stock_output = run_fast_import_batch(&stock, git_repo.path(), &input);
        let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), &input);
        assert_eq!(stock_output.status, 0, "stock {label}");
        assert_eq!(zmin_output.status, 0, "zmin {label}");
        assert_eq!(stock_output.stdout, zmin_output.stdout, "stdout {label}");
        assert_eq!(stock_output.stderr, zmin_output.stderr, "stderr {label}");
        let tree_args = ["ls-tree", "-r", "-z", "--full-name", "refs/heads/test"];
        let stock_tree = run_command_with_watchdog(&stock, &tree_args, git_repo.path(), &[]);
        let zmin_tree =
            run_command_with_watchdog(Path::new(zmin_bin()), &tree_args, zmin_repo.path(), &[]);
        assert_eq!(stock_tree.status, 0, "stock tree {label}");
        assert_eq!(zmin_tree.status, 0, "zmin tree {label}");
        assert_eq!(stock_tree.stdout, zmin_tree.stdout, "tree {label}");
        assert_eq!(stock_tree.stderr, zmin_tree.stderr, "tree stderr {label}");
        if expected_marker.is_empty() {
            assert!(stock_tree.stdout.is_empty(), "expected empty tree {label}");
        } else {
            assert!(
                stock_tree
                    .stdout
                    .windows(expected_marker.len())
                    .any(|window| { window == expected_marker.as_bytes() }),
                "expected retained file in tree {label}"
            );
        }
    }

    let git_repo = git_init();
    let zmin_repo = git_init();
    let mut input = fast_import_modify_base();
    input.extend_from_slice(
        b"commit refs/heads/test\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :2\nM 100644 :1 \ndone\n",
    );
    let stock_output = run_fast_import_batch(&stock, git_repo.path(), &input);
    let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), &input);
    assert_eq!(stock_output.status, 128);
    assert_eq!(zmin_output.status, 128);
    assert_eq!(stock_output.stdout, zmin_output.stdout);
    assert_eq!(
        normalize_fast_import_crash_stderr(
            std::str::from_utf8(&stock_output.stderr).expect("stock root error utf8"),
            "fatal: root cannot be a non-directory",
        ),
        normalize_fast_import_crash_stderr(
            std::str::from_utf8(&zmin_output.stderr).expect("zmin root error utf8"),
            "fatal: root cannot be a non-directory",
        )
    );
    assert_eq!(
        fast_import_ref_snapshot_for(git_repo.path(), "refs/heads/test"),
        fast_import_ref_snapshot_for(zmin_repo.path(), "refs/heads/test")
    );
    assert_eq!(
        fast_import_tree_snapshot(&stock, git_repo.path(), "refs/heads/test"),
        fast_import_tree_snapshot(Path::new(zmin_bin()), zmin_repo.path(), "refs/heads/test")
    );

    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_tree = fast_import_seed_nonempty_tree(&stock, git_repo.path());
    let zmin_tree = fast_import_seed_nonempty_tree(Path::new(zmin_bin()), zmin_repo.path());
    assert_eq!(git_tree, zmin_tree, "seeded non-empty tree must match");
    let mut input = fast_import_modify_base();
    input.extend_from_slice(
        format!(
            "commit refs/heads/test\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :2\nM 040000 {git_tree} dir/\ndone\n"
        )
        .as_bytes(),
    );
    let stock_ref_before = fast_import_ref_snapshot_for(git_repo.path(), "refs/heads/test");
    let zmin_ref_before = fast_import_ref_snapshot_for(zmin_repo.path(), "refs/heads/test");
    let stock_tree_before = fast_import_tree_state(&stock, git_repo.path(), "refs/heads/test");
    let zmin_tree_before =
        fast_import_tree_state(Path::new(zmin_bin()), zmin_repo.path(), "refs/heads/test");
    let stock_output = run_fast_import_batch(&stock, git_repo.path(), &input);
    let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), &input);
    assert_eq!(stock_output.status, 128);
    assert_eq!(zmin_output.status, 128);
    assert_eq!(stock_output.stdout, zmin_output.stdout);
    assert_eq!(
        normalize_fast_import_crash_stderr(
            std::str::from_utf8(&stock_output.stderr).expect("stock trailing slash utf8"),
            "fatal: invalid path 'dir/'",
        ),
        normalize_fast_import_crash_stderr(
            std::str::from_utf8(&zmin_output.stderr).expect("zmin trailing slash utf8"),
            "fatal: invalid path 'dir/'",
        )
    );
    assert_eq!(
        stock_ref_before,
        fast_import_ref_snapshot_for(git_repo.path(), "refs/heads/test")
    );
    assert_eq!(
        zmin_ref_before,
        fast_import_ref_snapshot_for(zmin_repo.path(), "refs/heads/test")
    );
    assert_eq!(
        stock_tree_before,
        fast_import_tree_state(&stock, git_repo.path(), "refs/heads/test")
    );
    assert_eq!(
        zmin_tree_before,
        fast_import_tree_state(Path::new(zmin_bin()), zmin_repo.path(), "refs/heads/test")
    );
    assert_eq!(
        normalize_fast_import_crash_report(
            &fast_import_crash_reports(git_repo.path()),
            "fatal: invalid path 'dir/'",
            Some(&format!("M 040000 {git_tree} dir/")),
        ),
        normalize_fast_import_crash_report(
            &fast_import_crash_reports(zmin_repo.path()),
            "fatal: invalid path 'dir/'",
            Some(&format!("M 040000 {git_tree} dir/")),
        )
    );
}

#[test]
fn fast_import_modify_empty_tree_trailing_slash_matches_pinned_git() {
    let stock = pinned_stock_git();
    let zmin = PathBuf::from(zmin_bin());
    for (label, existing_path) in [
        ("existing directory", "dir/"),
        ("existing file", "file/"),
        ("missing directory", "missing/"),
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let git_tree = fast_import_seed_empty_tree(&stock, git_repo.path());
        let zmin_tree = fast_import_seed_empty_tree(&zmin, zmin_repo.path());
        assert_eq!(git_tree, zmin_tree, "empty tree seed {label}");
        let mut input = fast_import_modify_base();
        input.extend_from_slice(
            format!(
                "commit refs/heads/test\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :2\nM 040000 {git_tree} {existing_path}\ndone\n"
            )
            .as_bytes(),
        );
        let stock_output = run_fast_import_batch(&stock, git_repo.path(), &input);
        let zmin_output = run_fast_import_batch(&zmin, zmin_repo.path(), &input);
        assert_eq!(stock_output.status, 0, "stock {label}");
        assert_eq!(zmin_output.status, 0, "zmin {label}");
        assert_eq!(stock_output.stdout, zmin_output.stdout, "stdout {label}");
        assert_eq!(stock_output.stderr, zmin_output.stderr, "stderr {label}");
        assert_eq!(
            fast_import_refs_state(&stock, git_repo.path()),
            fast_import_refs_state(&zmin, zmin_repo.path()),
            "refs {label}"
        );
        assert_eq!(
            fast_import_tree_state(&stock, git_repo.path(), "refs/heads/test"),
            fast_import_tree_state(&zmin, zmin_repo.path(), "refs/heads/test"),
            "tree {label}"
        );
    }
}

#[test]
fn fast_import_modify_raw_nul_and_invalid_bytes_match_pinned_git() {
    let stock = pinned_stock_git();
    for path in [b"\"quoted\"\0ignored".as_slice(), b"unquoted\0ignored"] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let mut input = fast_import_modify_base();
        input.extend_from_slice(
            b"commit refs/heads/test\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :2\nM 100644 :1 ",
        );
        input.extend_from_slice(path);
        input.extend_from_slice(b"\ndone\n");
        let stock_output = run_fast_import_batch(&stock, git_repo.path(), &input);
        let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), &input);
        assert_eq!(stock_output.status, 0);
        assert_eq!(zmin_output.status, 0);
        assert_eq!(stock_output.stdout, zmin_output.stdout);
        assert_eq!(stock_output.stderr, zmin_output.stderr);
        assert_eq!(
            fast_import_tree_snapshot(&stock, git_repo.path(), "refs/heads/test"),
            fast_import_tree_snapshot(Path::new(zmin_bin()), zmin_repo.path(), "refs/heads/test")
        );
    }

    let escaped = b"commit refs/heads/test\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :2\nM 100644 :1 \"escaped\\000\"\ndone\n";
    let git_repo = git_init();
    let zmin_repo = git_init();
    let mut stock_input = fast_import_modify_base();
    stock_input.extend_from_slice(escaped);
    let mut zmin_input = fast_import_modify_base();
    zmin_input.extend_from_slice(escaped);
    let stock_output = run_fast_import_batch(&stock, git_repo.path(), &stock_input);
    let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), &zmin_input);
    assert_eq!(stock_output.status, 128);
    assert_eq!(zmin_output.status, 128);
    assert_eq!(
        normalize_fast_import_crash_stderr(
            std::str::from_utf8(&stock_output.stderr).expect("stock escaped NUL utf8"),
            "fatal: NUL in path: M 100644 :1 \"escaped\\000\"",
        ),
        normalize_fast_import_crash_stderr(
            std::str::from_utf8(&zmin_output.stderr).expect("zmin escaped NUL utf8"),
            "fatal: NUL in path: M 100644 :1 \"escaped\\000\"",
        )
    );

    let invalid = b"commit refs/heads/test\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :2\nM 100644 :1 foo/../\x80\ndone\n";
    let mut stock_input = fast_import_modify_base();
    stock_input.extend_from_slice(invalid);
    let mut zmin_input = fast_import_modify_base();
    zmin_input.extend_from_slice(invalid);
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stock_ref_before = fast_import_ref_snapshot(git_repo.path());
    let zmin_ref_before = fast_import_ref_snapshot(zmin_repo.path());
    let stock_tree_before = fast_import_tree_state(&stock, git_repo.path(), "refs/heads/main");
    let zmin_tree_before =
        fast_import_tree_state(Path::new(zmin_bin()), zmin_repo.path(), "refs/heads/main");
    let stock_output = run_fast_import_batch(&stock, git_repo.path(), &stock_input);
    let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), &zmin_input);
    assert_eq!(stock_output.status, 128);
    assert_eq!(zmin_output.status, 128);
    let expected = b"fatal: invalid path 'foo/../\x80'";
    assert!(stock_output.stderr.starts_with(expected));
    assert!(zmin_output.stderr.starts_with(expected));
    assert_eq!(stock_output.stdout, zmin_output.stdout);
    assert_eq!(stock_ref_before, fast_import_ref_snapshot(git_repo.path()));
    assert_eq!(zmin_ref_before, fast_import_ref_snapshot(zmin_repo.path()));
    assert_eq!(
        stock_tree_before,
        fast_import_tree_state(&stock, git_repo.path(), "refs/heads/main")
    );
    assert_eq!(
        zmin_tree_before,
        fast_import_tree_state(Path::new(zmin_bin()), zmin_repo.path(), "refs/heads/main")
    );
    assert_eq!(stock_ref_before, zmin_ref_before);
    assert_eq!(stock_tree_before, zmin_tree_before);
    let expected_command = b"M 100644 :1 foo/../\x80";
    assert_eq!(
        normalize_fast_import_crash_report_bytes(
            &fast_import_crash_report_bytes(git_repo.path()),
            expected,
            expected_command,
        ),
        normalize_fast_import_crash_report_bytes(
            &fast_import_crash_report_bytes(zmin_repo.path()),
            expected,
            expected_command,
        )
    );
    assert_eq!(
        fast_import_ref_snapshot(git_repo.path()),
        fast_import_ref_snapshot(zmin_repo.path())
    );
}

fn fast_import_modify_base() -> Vec<u8> {
    b"blob\nmark :1\ndata 1\na\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 file\nM 100644 :1 dir/a\n"
        .to_vec()
}

fn fast_import_copy_rename_failure_input(seed: &[u8], operation: &[u8]) -> Vec<u8> {
    let mut input = b"blob\nmark :1\ndata 1\na\ncommit refs/heads/main\nmark :2\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\n".to_vec();
    input.extend_from_slice(seed);
    input.extend_from_slice(operation);
    input
}

fn fast_import_tree_snapshot(program: &Path, repo: &Path, reference: &str) -> Vec<u8> {
    fast_import_tree_state(program, repo, reference).stdout
}

fn fast_import_tree_state(program: &Path, repo: &Path, reference: &str) -> FastImportProcessOutput {
    run_command_with_watchdog(
        program,
        &["ls-tree", "-r", "-z", "--full-name", reference],
        repo,
        &[],
    )
}

fn fast_import_seed_nonempty_tree(program: &Path, repo: &Path) -> String {
    let blob = run_command_with_watchdog(program, &["hash-object", "-w", "--stdin"], repo, b"a");
    assert_eq!(blob.status, 0, "seed blob");
    let blob = String::from_utf8(blob.stdout)
        .expect("seed blob id utf8")
        .trim()
        .to_owned();
    let tree = run_command_with_watchdog(
        program,
        &["mktree"],
        repo,
        format!("100644 blob {blob}\tfile\n").as_bytes(),
    );
    assert_eq!(tree.status, 0, "seed tree");
    String::from_utf8(tree.stdout)
        .expect("seed tree id utf8")
        .trim()
        .to_owned()
}

fn fast_import_seed_empty_tree(program: &Path, repo: &Path) -> String {
    let tree = run_command_with_watchdog(
        program,
        &["hash-object", "-t", "tree", "-w", "--stdin"],
        repo,
        &[],
    );
    assert_eq!(tree.status, 0, "seed empty tree: {:?}", tree.stderr);
    String::from_utf8(tree.stdout)
        .expect("seed empty tree id utf8")
        .trim()
        .to_owned()
}

fn fast_import_seed_explicit_empty_tree(
    program: &Path,
    repo: &Path,
) -> (String, String, String, String) {
    let empty = fast_import_seed_empty_tree(program, repo);
    let blob =
        run_command_with_watchdog(program, &["hash-object", "-w", "--stdin"], repo, b"root\n");
    assert_eq!(blob.status, 0, "seed root blob: {:?}", blob.stderr);
    let blob = String::from_utf8(blob.stdout)
        .expect("seed root blob id utf8")
        .trim()
        .to_owned();
    let root_content = {
        let mut content = b"40000 empty-a\0".to_vec();
        content.extend_from_slice(&fast_import_hex_decode(empty.as_bytes()));
        content.extend_from_slice(b"40000 empty-b\0");
        content.extend_from_slice(&fast_import_hex_decode(empty.as_bytes()));
        content.extend_from_slice(b"100644 root\0");
        content.extend_from_slice(&fast_import_hex_decode(blob.as_bytes()));
        content
    };
    let root = run_command_with_watchdog(
        program,
        &["hash-object", "-t", "tree", "-w", "--stdin"],
        repo,
        &root_content,
    );
    assert_eq!(
        root.status, 0,
        "seed explicit empty-tree root: stdout={:?} stderr={:?}",
        root.stdout, root.stderr
    );
    let root = String::from_utf8(root.stdout)
        .expect("seed root tree id utf8")
        .trim()
        .to_owned();
    let commit = run_command_with_watchdog(
        program,
        &["hash-object", "-t", "commit", "-w", "--stdin"],
        repo,
        format!(
            "tree {root}\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\n\ninitial\n"
        )
        .as_bytes(),
    );
    assert_eq!(commit.status, 0, "seed base commit");
    let commit = String::from_utf8(commit.stdout)
        .expect("seed commit id utf8")
        .trim()
        .to_owned();
    let update = run_command_with_watchdog(
        program,
        &["update-ref", "refs/heads/main", &commit],
        repo,
        &[],
    );
    assert_eq!(update.status, 0, "seed base ref: {:?}", update.stderr);
    (empty, root, commit, blob)
}

fn fast_import_seed_true_empty_root(program: &Path, repo: &Path) -> (String, String) {
    let empty = fast_import_seed_empty_tree(program, repo);
    let commit = run_command_with_watchdog(
        program,
        &["hash-object", "-t", "commit", "-w", "--stdin"],
        repo,
        format!(
            "tree {empty}\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\n\ninitial\n"
        )
        .as_bytes(),
    );
    assert_eq!(
        commit.status, 0,
        "seed true-empty commit: {:?}",
        commit.stderr
    );
    let commit = String::from_utf8(commit.stdout)
        .expect("seed true-empty commit id utf8")
        .trim()
        .to_owned();
    let update = run_command_with_watchdog(
        program,
        &["update-ref", "refs/heads/main", &commit],
        repo,
        &[],
    );
    assert_eq!(update.status, 0, "seed true-empty ref: {:?}", update.stderr);
    (empty, commit)
}

fn fast_import_copy_seeded_objects(source_repo: &Path, destination_repo: &Path, objects: &[&str]) {
    for object in objects {
        let (prefix, suffix) = object.split_at(2);
        let source = source_repo.join(".git/objects").join(prefix).join(suffix);
        let destination_dir = destination_repo.join(".git/objects").join(prefix);
        fs::create_dir_all(&destination_dir).expect("create destination object directory");
        fs::copy(&source, destination_dir.join(suffix)).expect("copy seeded object");
    }
}

fn fast_import_copy_seeded_tree_objects(
    source_repo: &Path,
    destination_repo: &Path,
    empty: &str,
    root: &str,
    commit: &str,
    blob: &str,
) -> (String, String, String, String) {
    fast_import_copy_seeded_objects(source_repo, destination_repo, &[empty, root, commit, blob]);
    (
        empty.to_owned(),
        root.to_owned(),
        commit.to_owned(),
        blob.to_owned(),
    )
}

fn fast_import_delete_fixture(path: &[u8]) -> Vec<u8> {
    let mut input = b"blob\nmark :1\ndata 1\na\nblob\nmark :2\ndata 1\nb\ncommit refs/heads/main\nmark :3\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nM 100644 :1 root.txt\nM 100644 :1 dir/a\nM 100644 :2 dir/nested/b\nM 100644 :1 sibling\ncommit refs/heads/main\nmark :4\nauthor A <a@example.test> 0 +0000\ncommitter A <a@example.test> 0 +0000\ndata 0\nfrom :3\nD ".to_vec();
    input.extend_from_slice(path);
    input.extend_from_slice(b"\ndone\n");
    input
}

fn fast_import_index_snapshot(repo: &Path) -> Option<Vec<u8>> {
    let path = repo.join(".git/index");
    path.exists().then(|| fs::read(path).expect("read index"))
}

fn fast_import_ref_snapshot(repo: &Path) -> (i32, Vec<u8>, Vec<u8>) {
    fast_import_ref_snapshot_for(repo, "refs/heads/main")
}

fn fast_import_ref_snapshot_for(repo: &Path, reference: &str) -> (i32, Vec<u8>, Vec<u8>) {
    let output = run_command_with_watchdog(
        &pinned_stock_git(),
        &["rev-parse", "-q", "--verify", reference],
        repo,
        &[],
    );
    (output.status, output.stdout, output.stderr)
}

fn fast_import_refs_state(program: &Path, repo: &Path) -> FastImportProcessOutput {
    run_command_with_watchdog(program, &["show-ref", "--head"], repo, &[])
}

fn fast_import_named_refs(program: &Path, repo: &Path) -> Vec<FastImportProcessOutput> {
    ["refs/heads/main", "refs/heads/after", "refs/heads/check"]
        .into_iter()
        .map(|reference| {
            run_command_with_watchdog(
                program,
                &["rev-parse", "-q", "--verify", reference],
                repo,
                &[],
            )
        })
        .collect()
}

fn fast_import_physical_object_exists(repo: &Path, object_hex: &[u8]) -> bool {
    let objects = repo.join(".git/objects");
    let fanout = std::str::from_utf8(&object_hex[..2]).expect("object fanout utf8");
    let suffix = std::str::from_utf8(&object_hex[2..]).expect("object suffix utf8");
    if objects.join(fanout).join(suffix).is_file() {
        return true;
    }
    let needle = fast_import_hex_decode(object_hex);
    let Ok(entries) = fs::read_dir(objects.join("pack")) else {
        return false;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "idx"))
        .filter_map(|path| fs::read(path).ok())
        .any(|bytes| bytes.windows(needle.len()).any(|window| window == needle))
}

fn fast_import_ref_snapshot_with_program(program: &Path, repo: &Path) -> FastImportProcessOutput {
    fast_import_ref_snapshot_with_program_for(program, repo, "refs/heads/main")
}

fn fast_import_ref_snapshot_with_program_for(
    program: &Path,
    repo: &Path,
    reference: &str,
) -> FastImportProcessOutput {
    run_command_with_watchdog(
        program,
        &["rev-parse", "-q", "--verify", reference],
        repo,
        &[],
    )
}

fn assert_fast_import_success_differential(input: &[u8], label: &str) {
    let stock = pinned_stock_git();
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stock_output = run_fast_import_batch(&stock, git_repo.path(), input);
    let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), input);
    assert_eq!(stock_output.status, 0, "stock {label}");
    assert_eq!(zmin_output.status, 0, "zmin {label}");
    assert_eq!(stock_output.stdout, zmin_output.stdout, "stdout {label}");
    assert_eq!(stock_output.stderr, zmin_output.stderr, "stderr {label}");
    assert_eq!(
        fast_import_refs_state(&stock, git_repo.path()),
        fast_import_refs_state(Path::new(zmin_bin()), zmin_repo.path()),
        "refs {label}"
    );
    assert_eq!(
        fast_import_tree_state(&stock, git_repo.path(), "refs/heads/main"),
        fast_import_tree_state(Path::new(zmin_bin()), zmin_repo.path(), "refs/heads/main"),
        "tree {label}"
    );
}

fn assert_fast_import_raw_tree_differential(input: &[u8], label: &str) {
    let stock = pinned_stock_git();
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stock_output = run_fast_import_batch(&stock, git_repo.path(), input);
    let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), input);
    assert_eq!(stock_output.status, 0, "stock {label}");
    assert_eq!(zmin_output.status, 0, "zmin {label}");
    assert_eq!(stock_output.stdout, zmin_output.stdout, "stdout {label}");
    assert_eq!(stock_output.stderr, zmin_output.stderr, "stderr {label}");
    assert_eq!(
        fast_import_refs_state(&stock, git_repo.path()),
        fast_import_refs_state(Path::new(zmin_bin()), zmin_repo.path()),
        "refs {label}"
    );
    let stock_tree = fast_import_tree_state(&stock, git_repo.path(), "refs/heads/main");
    assert_eq!(stock_tree.status, 0, "stock tree {label}");
    assert_eq!(
        stock_tree.stdout,
        fast_import_zmin_raw_tree_state(Path::new(zmin_bin()), zmin_repo.path()),
        "raw tree {label}"
    );
}

fn fast_import_zmin_raw_tree_state(program: &Path, repo: &Path) -> Vec<u8> {
    let head = run_command_with_watchdog(program, &["rev-parse", "refs/heads/main"], repo, &[]);
    assert_eq!(head.status, 0, "zmin rev-parse main");
    let head = head.stdout.strip_suffix(b"\n").expect("head newline");
    let digest_len = head.len() / 2;
    let commit = fast_import_zmin_batch_object_typed(program, repo, head, b"commit");
    let tree_line = commit
        .split(|byte| *byte == b'\n')
        .find(|line| line.starts_with(b"tree "))
        .expect("commit tree line");
    let tree = fast_import_zmin_batch_object_typed(program, repo, &tree_line[5..], b"tree");
    let mut output = Vec::new();
    append_fast_import_raw_tree(program, repo, &tree, digest_len, &[], &mut output);
    output
}

fn fast_import_commit_tree_object(program: &Path, repo: &Path) -> Vec<u8> {
    let commit = fast_import_commit_object(program, repo, "refs/heads/main");
    let tree_line = commit
        .split(|byte| *byte == b'\n')
        .find(|line| line.starts_with(b"tree "))
        .expect("commit tree line");
    fast_import_zmin_batch_object_typed(program, repo, &tree_line[5..], b"tree")
}

fn fast_import_commit_object(program: &Path, repo: &Path, reference: &str) -> Vec<u8> {
    let head = run_command_with_watchdog(program, &["rev-parse", reference], repo, &[]);
    assert_eq!(head.status, 0, "rev-parse main");
    let head = head.stdout.strip_suffix(b"\n").expect("head newline");
    fast_import_zmin_batch_object_typed(program, repo, head, b"commit")
}

fn fast_import_zmin_batch_object_typed(
    program: &Path,
    repo: &Path,
    object: &[u8],
    expected_type: &[u8],
) -> Vec<u8> {
    let input = [object, b"\n"].concat();
    let output = run_command_with_watchdog(program, &["cat-file", "--batch"], repo, &input);
    assert_eq!(output.status, 0, "zmin cat-file --batch");
    let header_end = output
        .stdout
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("batch header newline");
    let header = &output.stdout[..header_end];
    let mut fields = header.split(|byte| *byte == b' ');
    assert_eq!(fields.next(), Some(object), "batch object id");
    let object_type = fields.next().expect("batch object type");
    if !expected_type.is_empty() {
        assert_eq!(object_type, expected_type, "batch object type");
    }
    let size_field = fields.next().expect("batch size");
    assert!(fields.next().is_none(), "batch header has extra fields");
    assert!(!size_field.is_empty() && size_field.iter().all(u8::is_ascii_digit));
    let size = size_field
        .iter()
        .try_fold(0_usize, |value, byte| {
            value.checked_mul(10)?.checked_add(usize::from(byte - b'0'))
        })
        .expect("batch size fits");
    let body_start = header_end + 1;
    let body_end = body_start + size;
    assert!(output.stdout.len() >= body_end + 1, "complete batch body");
    assert_eq!(output.stdout[body_end], b'\n', "batch body separator");
    assert_eq!(output.stdout.len(), body_end + 1, "single batch response");
    output.stdout[body_start..body_end].to_vec()
}

fn append_fast_import_raw_tree(
    program: &Path,
    repo: &Path,
    tree: &[u8],
    digest_len: usize,
    prefix: &[u8],
    output: &mut Vec<u8>,
) {
    let mut cursor = 0;
    while cursor < tree.len() {
        let mode_end = tree[cursor..]
            .iter()
            .position(|byte| *byte == b' ')
            .map(|offset| cursor + offset)
            .expect("tree mode");
        let mode = &tree[cursor..mode_end];
        cursor = mode_end + 1;
        let name_end = tree[cursor..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|offset| cursor + offset)
            .expect("tree name");
        let name = &tree[cursor..name_end];
        cursor = name_end + 1;
        let id_end = cursor + digest_len;
        assert!(id_end <= tree.len(), "tree object id");
        let id = &tree[cursor..id_end];
        cursor = id_end;
        let mut full_name = prefix.to_vec();
        if !full_name.is_empty() {
            full_name.push(b'/');
        }
        full_name.extend_from_slice(name);
        if mode == b"40000" || mode == b"040000" {
            let object =
                fast_import_zmin_batch_object_typed(program, repo, &fast_import_hex(id), b"tree");
            append_fast_import_raw_tree(program, repo, &object, digest_len, &full_name, output);
            continue;
        }
        output.extend_from_slice(mode);
        output.push(b' ');
        output.extend_from_slice(if mode == b"160000" {
            b"commit"
        } else {
            b"blob"
        });
        output.push(b' ');
        output.extend_from_slice(&fast_import_hex(id));
        output.push(b'\t');
        output.extend_from_slice(&full_name);
        output.push(0);
    }
}

fn fast_import_hex(bytes: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = Vec::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)]);
        output.push(HEX[usize::from(byte & 0x0f)]);
    }
    output
}

fn fast_import_hex_decode(bytes: &[u8]) -> Vec<u8> {
    assert!(bytes.len() % 2 == 0 && bytes.iter().all(u8::is_ascii_hexdigit));
    bytes
        .chunks_exact(2)
        .map(|pair| fast_import_hex_value(pair[0]) * 16 + fast_import_hex_value(pair[1]))
        .collect()
}

fn fast_import_hex_value(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => panic!("invalid hex byte"),
    }
}

fn assert_fast_import_failure_differential(
    input: &[u8],
    expected_fatal: &[u8],
    expected_command: &[u8],
    label: &str,
) -> Vec<u8> {
    let stock = pinned_stock_git();
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stock_refs_before = fast_import_refs_state(&stock, git_repo.path());
    let zmin_refs_before = fast_import_refs_state(Path::new(zmin_bin()), zmin_repo.path());
    let stock_tree_before = fast_import_tree_state(&stock, git_repo.path(), "refs/heads/main");
    let zmin_tree_before =
        fast_import_tree_state(Path::new(zmin_bin()), zmin_repo.path(), "refs/heads/main");
    let stock_output = run_fast_import_batch(&stock, git_repo.path(), input);
    let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), input);
    assert_eq!(stock_output.status, 128, "stock {label}");
    assert_eq!(zmin_output.status, 128, "zmin {label}");
    assert_eq!(stock_output.stdout, zmin_output.stdout, "stdout {label}");
    assert_eq!(
        normalize_fast_import_crash_stderr_bytes(&stock_output.stderr, expected_fatal),
        normalize_fast_import_crash_stderr_bytes(&zmin_output.stderr, expected_fatal),
        "stderr {label}"
    );
    assert_eq!(
        stock_refs_before,
        fast_import_refs_state(&stock, git_repo.path()),
        "stock refs {label}"
    );
    assert_eq!(
        zmin_refs_before,
        fast_import_refs_state(Path::new(zmin_bin()), zmin_repo.path()),
        "zmin refs {label}"
    );
    assert_eq!(
        stock_tree_before,
        fast_import_tree_state(&stock, git_repo.path(), "refs/heads/main"),
        "stock tree {label}"
    );
    assert_eq!(
        zmin_tree_before,
        fast_import_tree_state(Path::new(zmin_bin()), zmin_repo.path(), "refs/heads/main"),
        "zmin tree {label}"
    );
    let stock_report = normalize_fast_import_crash_report_bytes(
        &fast_import_crash_report_bytes(git_repo.path()),
        expected_fatal,
        expected_command,
    );
    let zmin_report = normalize_fast_import_crash_report_bytes(
        &fast_import_crash_report_bytes(zmin_repo.path()),
        expected_fatal,
        expected_command,
    );
    assert_eq!(stock_report, zmin_report, "crash report {label}");
    stock_report
}

#[cfg(unix)]
#[test]
fn fast_import_cat_blob_fd3_routes_responses_and_progress_like_pinned_git() {
    let stock = pinned_stock_git();
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stock_output = run_interactive_cat_blob_fd3(&stock, git_repo.path());
    let zmin_output = run_interactive_cat_blob_fd3(Path::new(zmin_bin()), zmin_repo.path());
    assert!(stock_output.responded_before_eof);
    assert!(zmin_output.responded_before_eof);
    assert_eq!(stock_output.status, 0);
    assert_eq!(zmin_output.status, 0);
    assert_eq!(stock_output.stdout, zmin_output.stdout);
    assert_eq!(stock_output.stderr, zmin_output.stderr);
    assert_eq!(stock_output.response, zmin_output.response);
    assert_eq!(stock_output.stdout, b"progress checkpoint\n");
    assert!(
        stock_output
            .response
            .windows(b"\tfile.txt\n".len())
            .any(|window| { window == b"\tfile.txt\n" })
    );
}

#[test]
fn fast_import_cat_blob_fd_invalid_values_match_pinned_git() {
    let stock = pinned_stock_git();
    let input = b"done\n";
    for (option, fatal) in [
        (
            "--cat-blob-fd=-1",
            "fatal: --cat-blob-fd: argument must be a non-negative integer",
        ),
        (
            "--cat-blob-fd=bad",
            "fatal: --cat-blob-fd: argument must be a non-negative integer",
        ),
        (
            "--cat-blob-fd=2147483648",
            "fatal: --cat-blob-fd cannot exceed 2147483647",
        ),
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let stock_output =
            run_fast_import_batch_with_option(&stock, git_repo.path(), option, input);
        let zmin_output = run_fast_import_batch_with_option(
            Path::new(zmin_bin()),
            zmin_repo.path(),
            option,
            input,
        );
        assert_eq!(stock_output.status, 128);
        assert_eq!(zmin_output.status, 128);
        assert_eq!(stock_output.stdout, zmin_output.stdout);
        assert_eq!(
            normalize_fast_import_crash_stderr(
                std::str::from_utf8(&stock_output.stderr).expect("stock stderr utf8"),
                fatal,
            ),
            normalize_fast_import_crash_stderr(
                std::str::from_utf8(&zmin_output.stderr).expect("zmin stderr utf8"),
                fatal,
            )
        );
    }
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stock_output =
        run_fast_import_batch_with_option(&stock, git_repo.path(), "--cat-blob-fd=1", input);
    let zmin_output = run_fast_import_batch_with_option(
        Path::new(zmin_bin()),
        zmin_repo.path(),
        "--cat-blob-fd=1",
        input,
    );
    assert_eq!(stock_output.status, 0);
    assert_eq!(zmin_output.status, 0);
    assert_eq!(stock_output.stdout, zmin_output.stdout);
    assert_eq!(stock_output.stderr, zmin_output.stderr);
}

#[cfg(unix)]
#[test]
fn fast_import_ls_preserves_non_utf8_path_bytes() {
    let stock = pinned_stock_git();
    let git_repo = git_init();
    let zmin_repo = git_init();
    let mut trees = Vec::new();
    for repo in [git_repo.path(), zmin_repo.path()] {
        let blob =
            run_command_with_watchdog(&stock, &["hash-object", "-w", "--stdin"], repo, b"hello\n");
        assert_eq!(blob.status, 0);
        let blob = String::from_utf8(blob.stdout)
            .expect("blob id utf8")
            .trim()
            .to_owned();
        let mut inner_input = format!("100644 blob {blob}\t").into_bytes();
        inner_input.extend_from_slice(b"\x80\n");
        inner_input.extend_from_slice(format!("100644 blob {blob}\t").as_bytes());
        inner_input.extend_from_slice(b"\xef\xbf\xbd\n");
        let inner = run_command_with_watchdog(&stock, &["mktree"], repo, &inner_input);
        assert_eq!(inner.status, 0);
        let inner = String::from_utf8(inner.stdout)
            .expect("inner tree id utf8")
            .trim()
            .to_owned();
        let root_input = format!("040000 tree {inner}\tsrc\n");
        let root = run_command_with_watchdog(&stock, &["mktree"], repo, root_input.as_bytes());
        assert_eq!(root.status, 0);
        trees.push(
            String::from_utf8(root.stdout)
                .expect("root tree id utf8")
                .trim()
                .to_owned(),
        );
    }
    assert_eq!(trees[0], trees[1]);
    let mut query = format!("feature ls\nls {} src/", trees[0]).into_bytes();
    query.push(0x80);
    query.extend_from_slice(b"\nls ");
    query.extend_from_slice(trees[0].as_bytes());
    query.extend_from_slice(b" src/");
    query.extend_from_slice(b"\xef\xbf\xbd\ndone\n");
    let stock_output = run_fast_import_batch(&stock, git_repo.path(), &query);
    let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), &query);
    assert_eq!(stock_output.status, 0);
    assert_eq!(zmin_output.status, 0);
    assert_eq!(stock_output.stdout, zmin_output.stdout);
    assert_eq!(stock_output.stderr, zmin_output.stderr);
    assert_eq!(
        stock_output
            .stdout
            .iter()
            .filter(|byte| **byte == b'\n')
            .count(),
        2
    );
    assert!(
        stock_output
            .stdout
            .windows(b"src/\\200".len())
            .any(|window| { window == b"src/\\200" })
    );
    assert!(
        stock_output
            .stdout
            .windows(b"src/\\357\\277\\275".len())
            .any(|window| { window == b"src/\\357\\277\\275" })
    );
    for repo in [git_repo.path(), zmin_repo.path()] {
        let config =
            run_command_with_watchdog(&stock, &["config", "core.quotePath", "false"], repo, &[]);
        assert_eq!(config.status, 0);
    }
    let stock_unquoted = run_fast_import_batch(&stock, git_repo.path(), &query);
    let zmin_unquoted = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), &query);
    assert_eq!(stock_unquoted.status, 0);
    assert_eq!(zmin_unquoted.status, 0);
    assert_eq!(stock_unquoted.stdout, zmin_unquoted.stdout);
    assert!(
        stock_unquoted
            .stdout
            .windows(b"src/\x80".len())
            .any(|window| { window == b"src/\x80" })
    );
    assert!(
        stock_unquoted
            .stdout
            .windows(b"src/\xef\xbf\xbd".len())
            .any(|window| window == b"src/\xef\xbf\xbd")
    );
}

#[cfg(unix)]
#[test]
fn fast_import_cat_blob_missing_cases_match_pinned_git() {
    let stock = pinned_stock_git();
    let cases = [
        (
            b"feature cat-blob\ncat-blob :9\n".as_slice(),
            128,
            b"".as_slice(),
            Some("fatal: mark :9 not declared"),
        ),
        (
            b"feature cat-blob\ncat-blob 0000000000000000000000000000000000000000\n".as_slice(),
            0,
            b"0000000000000000000000000000000000000000 missing\n".as_slice(),
            None,
        ),
        (
            b"cat-blob 0000000000000000000000000000000000000000\n".as_slice(),
            0,
            b"0000000000000000000000000000000000000000 missing\n".as_slice(),
            None,
        ),
    ];
    for (input, expected_status, expected_stdout, expected_fatal) in cases {
        let git_repo = tempfile::TempDir::new().expect("stock fast-import repo");
        let zmin_repo = tempfile::TempDir::new().expect("zmin fast-import repo");
        init_with_pinned_git(&stock, git_repo.path());
        init_with_pinned_git(&stock, zmin_repo.path());

        let stock_output = run_fast_import_batch(&stock, git_repo.path(), input);
        let zmin_output = run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), input);
        assert_eq!(stock_output.status, expected_status);
        assert_eq!(zmin_output.status, expected_status);
        assert_eq!(stock_output.stdout, expected_stdout);
        assert_eq!(zmin_output.stdout, expected_stdout);
        if let Some(expected_fatal) = expected_fatal {
            assert_eq!(
                normalize_fast_import_crash_stderr(
                    std::str::from_utf8(&stock_output.stderr).expect("stock stderr utf8"),
                    expected_fatal,
                ),
                normalize_fast_import_crash_stderr(
                    std::str::from_utf8(&zmin_output.stderr).expect("zmin stderr utf8"),
                    expected_fatal,
                )
            );
        } else {
            assert!(stock_output.stderr.is_empty());
            assert!(zmin_output.stderr.is_empty());
        }
    }
}

#[cfg(unix)]
#[test]
fn fast_import_cat_blob_type_and_syntax_errors_match_pinned_git() {
    let stock = pinned_stock_git();
    let git_repo = tempfile::TempDir::new().expect("stock fast-import repo");
    let zmin_repo = tempfile::TempDir::new().expect("zmin fast-import repo");
    init_with_pinned_git(&stock, git_repo.path());
    init_with_pinned_git(&stock, zmin_repo.path());
    let stock_tree = run_pinned_mktree(&stock, git_repo.path());
    let zmin_tree = run_pinned_mktree(&stock, zmin_repo.path());
    assert_eq!(stock_tree, zmin_tree);

    let nonblob_input = format!("feature cat-blob\ncat-blob {stock_tree}\n");
    assert_fast_import_crash_failure_case(
        &stock,
        git_repo,
        zmin_repo,
        &nonblob_input,
        &format!("fatal: object {stock_tree} is a tree but a blob was expected."),
        &format!("cat-blob {stock_tree}"),
    );

    let git_repo = tempfile::TempDir::new().expect("stock fast-import repo");
    let zmin_repo = tempfile::TempDir::new().expect("zmin fast-import repo");
    init_with_pinned_git(&stock, git_repo.path());
    init_with_pinned_git(&stock, zmin_repo.path());
    let stock_tree = run_pinned_mktree(&stock, git_repo.path());
    let zmin_tree = run_pinned_mktree(&stock, zmin_repo.path());
    assert_eq!(stock_tree, zmin_tree);
    let nonblob_without_feature = format!("cat-blob {stock_tree}\n");
    assert_fast_import_crash_failure_case(
        &stock,
        git_repo,
        zmin_repo,
        &nonblob_without_feature,
        &format!("fatal: object {stock_tree} is a tree but a blob was expected."),
        &format!("cat-blob {stock_tree}"),
    );

    for (input, expected_fatal, expected_command) in [
        (
            "feature cat-blob\ncat-blob :403x\n",
            "fatal: garbage after mark: cat-blob :403x",
            "cat-blob :403x",
        ),
        (
            "feature cat-blob\ncat-blob not-an-object-id\n",
            "fatal: invalid dataref: cat-blob not-an-object-id",
            "cat-blob not-an-object-id",
        ),
    ] {
        let git_repo = tempfile::TempDir::new().expect("stock fast-import repo");
        let zmin_repo = tempfile::TempDir::new().expect("zmin fast-import repo");
        init_with_pinned_git(&stock, git_repo.path());
        init_with_pinned_git(&stock, zmin_repo.path());
        assert_fast_import_crash_failure_case(
            &stock,
            git_repo,
            zmin_repo,
            input,
            expected_fatal,
            expected_command,
        );
    }
}

#[cfg(unix)]
#[test]
fn fast_import_crash_history_matches_pinned_git_after_blob_and_mark() {
    let stock = pinned_stock_git();
    for (input, expected_command) in [
        (
            "feature cat-blob\nblob\nmark :1\ndata 4\ntest\ncat-blob :9\n",
            "cat-blob :9",
        ),
        ("blob\nmark :1\ndata 4\ntest\nget-mark :9\n", "get-mark :9"),
    ] {
        let git_repo = tempfile::TempDir::new().expect("stock fast-import repo");
        let zmin_repo = tempfile::TempDir::new().expect("zmin fast-import repo");
        init_with_pinned_git(&stock, git_repo.path());
        init_with_pinned_git(&stock, zmin_repo.path());
        assert_fast_import_crash_failure_case(
            &stock,
            git_repo,
            zmin_repo,
            input,
            "fatal: mark :9 not declared",
            expected_command,
        );
    }
}

#[cfg(unix)]
fn assert_fast_import_crash_failure_case(
    stock: &Path,
    git_repo: tempfile::TempDir,
    zmin_repo: tempfile::TempDir,
    input: &str,
    expected_fatal: &str,
    expected_command: &str,
) {
    let stock_output = run_fast_import_batch(stock, git_repo.path(), input.as_bytes());
    let zmin_output =
        run_fast_import_batch(Path::new(zmin_bin()), zmin_repo.path(), input.as_bytes());
    assert_eq!(stock_output.status, 128);
    assert_eq!(zmin_output.status, 128);
    assert!(stock_output.stdout.is_empty());
    assert!(zmin_output.stdout.is_empty());
    assert_eq!(
        normalize_fast_import_crash_stderr(
            std::str::from_utf8(&stock_output.stderr).expect("stock stderr utf8"),
            expected_fatal,
        ),
        normalize_fast_import_crash_stderr(
            std::str::from_utf8(&zmin_output.stderr).expect("zmin stderr utf8"),
            expected_fatal,
        )
    );
    assert_eq!(
        normalize_fast_import_crash_report(
            &fast_import_crash_reports(git_repo.path()),
            expected_fatal,
            Some(expected_command),
        ),
        normalize_fast_import_crash_report(
            &fast_import_crash_reports(zmin_repo.path()),
            expected_fatal,
            Some(expected_command),
        )
    );
}

#[cfg(unix)]
#[test]
fn fast_import_get_mark_errors_match_pinned_git_crash_report() {
    let stock = pinned_stock_git();
    for (input, expected_fatal, expected_command) in [
        (
            "get-mark :9\n",
            "fatal: mark :9 not declared",
            "get-mark :9",
        ),
        (
            "get-mark :403x\n",
            "fatal: garbage after mark: get-mark :403x",
            "get-mark :403x",
        ),
        (
            "get-mark not-a-mark\n",
            "fatal: not a mark: not-a-mark",
            "get-mark not-a-mark",
        ),
    ] {
        let git_repo = tempfile::TempDir::new().expect("stock fast-import repo");
        let zmin_repo = tempfile::TempDir::new().expect("zmin fast-import repo");
        init_with_pinned_git(&stock, git_repo.path());
        init_with_pinned_git(&stock, zmin_repo.path());
        assert_fast_import_crash_failure_case(
            &stock,
            git_repo,
            zmin_repo,
            input,
            expected_fatal,
            expected_command,
        );
    }
}

#[cfg(unix)]
#[test]
fn fast_import_rejects_oversized_declared_data_before_reading() {
    let repo = tempfile::TempDir::new().expect("fast-import repo");
    let stock = pinned_stock_git();
    init_with_pinned_git(&stock, repo.path());
    let input = b"commit refs/heads/main\ncommitter A <a@example.test> 0 +0000\ndata 536870913\n";
    let output = run_fast_import_batch(Path::new(zmin_bin()), repo.path(), input);
    assert_ne!(output.status, 0);
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("fast-import data exceeds configured object size limit")
    );
    assert_eq!(loose_object_file_count(repo.path()), 0);
}

#[test]
fn fast_import_rejects_oversized_control_line_without_echoing_payload() {
    let repo = git_init();
    let oversized = "x".repeat(64 * 1024 + 1);
    let input = format!("{oversized}\n");
    let output =
        command_with_stdin_output(zmin_bin(), repo.path(), &["fast-import", "--quiet"], &input);

    assert_eq!(output.0, 128);
    assert!(output.1.is_empty());
    assert!(
        output
            .2
            .contains("fatal: fast-import control line exceeds configured limit")
    );
    assert!(!output.2.contains(&oversized));
    let reports = fast_import_crash_reports(repo.path());
    assert_eq!(reports.len(), 1);
    assert!(reports[0].contains("fatal: fast-import control line exceeds configured limit"));
    assert!(!reports[0].contains(&oversized));
}

fn fast_import_crash_reports(repo: &Path) -> Vec<String> {
    let mut reports = fs::read_dir(repo.join(".git"))
        .expect("read .git")
        .filter_map(|entry| {
            let entry = entry.expect("dir entry");
            let name = entry.file_name();
            name.to_str()
                .filter(|name| name.starts_with("fast_import_crash_"))
                .map(|_| fs::read_to_string(entry.path()).expect("crash report text"))
        })
        .collect::<Vec<_>>();
    reports.sort();
    reports
}

fn fast_import_crash_report_bytes(repo: &Path) -> Vec<Vec<u8>> {
    let mut reports = fs::read_dir(repo.join(".git"))
        .expect("read .git")
        .filter_map(|entry| {
            let entry = entry.expect("dir entry");
            let name = entry.file_name();
            name.to_str()
                .filter(|name| name.starts_with("fast_import_crash_"))
                .map(|_| fs::read(entry.path()).expect("crash report bytes"))
        })
        .collect::<Vec<_>>();
    reports.sort();
    reports
}

fn normalize_fast_import_crash_report_bytes(
    reports: &[Vec<u8>],
    expected_fatal: &[u8],
    expected_command: &[u8],
) -> Vec<u8> {
    assert_eq!(reports.len(), 1, "expected one crash report");
    let normalized = reports[0]
        .split_inclusive(|byte| *byte == b'\n')
        .map(normalize_fast_import_crash_line_bytes)
        .collect::<Vec<_>>()
        .concat();
    assert!(
        normalized
            .windows(expected_fatal.len())
            .any(|window| window == expected_fatal),
        "missing fatal report bytes: {normalized:?}"
    );
    let command_marker = [b"* ".as_slice(), expected_command].concat();
    assert!(
        normalized
            .windows(command_marker.len())
            .any(|window| window == command_marker),
        "missing recent command bytes: {normalized:?}"
    );
    normalized
}

fn normalize_fast_import_crash_stderr_bytes(stderr: &[u8], expected_fatal: &[u8]) -> Vec<u8> {
    let mut lines = stderr.split(|byte| *byte == b'\n');
    assert_eq!(
        lines.next(),
        Some([b"fatal: ".as_slice(), expected_fatal].concat().as_slice())
    );
    let crash_line = lines.next().expect("crash report stderr line");
    assert!(
        crash_line.starts_with(b"fast-import: dumping crash report to .git/fast_import_crash_")
    );
    let pid = &crash_line[b"fast-import: dumping crash report to .git/fast_import_crash_".len()..];
    assert!(!pid.is_empty() && pid.iter().all(u8::is_ascii_digit));
    assert_eq!(lines.next(), Some(&[][..]));
    assert_eq!(lines.next(), None);
    [
        b"fatal: ".as_slice(),
        expected_fatal,
        b"\nfast-import: dumping crash report to .git/fast_import_crash_<pid>\n".as_slice(),
    ]
    .concat()
}

fn normalize_fast_import_crash_line_bytes(line: &[u8]) -> Vec<u8> {
    let (body, ending) = if let Some(body) = line.strip_suffix(b"\n") {
        if let Some(body) = body.strip_suffix(b"\r") {
            (body, b"\r\n".as_slice())
        } else {
            (body, b"\n".as_slice())
        }
    } else {
        (line, b"".as_slice())
    };
    if let Some(value) = body.strip_prefix(b"    fast-import process: ") {
        assert!(!value.is_empty() && value.iter().all(u8::is_ascii_digit));
        return [b"    fast-import process: <pid>".as_slice(), ending].concat();
    }
    if let Some(value) = body.strip_prefix(b"    parent process     : ") {
        assert!(!value.is_empty() && value.iter().all(u8::is_ascii_digit));
        return [b"    parent process     : <pid>".as_slice(), ending].concat();
    }
    if let Some(value) = body.strip_prefix(b"    at ") {
        assert_eq!(value.len(), 25, "unexpected crash-report timestamp");
        assert!(value[0..4].iter().all(u8::is_ascii_digit));
        assert_eq!(value[4], b'-');
        assert!(value[5..7].iter().all(u8::is_ascii_digit));
        assert_eq!(value[7], b'-');
        assert!(value[8..10].iter().all(u8::is_ascii_digit));
        assert_eq!(value[10], b' ');
        assert!(value[11..13].iter().all(u8::is_ascii_digit));
        assert_eq!(value[13], b':');
        assert!(value[14..16].iter().all(u8::is_ascii_digit));
        assert_eq!(value[16], b':');
        assert!(value[17..19].iter().all(u8::is_ascii_digit));
        assert_eq!(value[19], b' ');
        assert!(
            value[20..25]
                .iter()
                .all(|byte| byte.is_ascii_digit() || *byte == b'+' || *byte == b'-')
        );
        return [b"    at <timestamp>".as_slice(), ending].concat();
    }
    line.to_vec()
}

fn loose_object_file_count(repo: &Path) -> usize {
    fs::read_dir(repo.join(".git/objects"))
        .expect("read objects dir")
        .filter_map(|entry| {
            let entry = entry.expect("objects dir entry");
            let name = entry.file_name();
            let name = name.to_str()?;
            (name.len() == 2).then_some(entry.path())
        })
        .map(|dir| {
            fs::read_dir(dir)
                .expect("read object fanout dir")
                .filter(|entry| {
                    entry
                        .as_ref()
                        .ok()
                        .and_then(|entry| entry.file_name().to_str().map(str::len))
                        == Some(38)
                })
                .count()
        })
        .sum()
}

fn object_layout(repo: &Path) -> (usize, usize) {
    let count = git(repo, ["count-objects", "-v"]);
    let field = |name: &str| {
        count
            .lines()
            .find_map(|line| {
                line.strip_prefix(name)
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .expect("count-objects field")
    };
    (field("count: "), field("packs: "))
}

fn pack_artifacts(repo: &Path) -> Vec<(String, Vec<u8>)> {
    let mut artifacts = fs::read_dir(repo.join(".git/objects/pack"))
        .expect("read pack directory")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?.to_owned();
            name.starts_with("pack-")
                .then(|| (name, fs::read(path).expect("read pack artifact")))
        })
        .collect::<Vec<_>>();
    artifacts.sort_by(|left, right| left.0.cmp(&right.0));
    artifacts
}

fn keep_artifacts(repo: &Path) -> Vec<String> {
    let mut keeps = fs::read_dir(repo.join(".git/objects/pack"))
        .expect("read pack directory")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name();
            let name = name.to_str()?.to_owned();
            name.ends_with(".keep").then_some(name)
        })
        .collect::<Vec<_>>();
    keeps.sort();
    keeps
}

#[cfg(unix)]
fn pinned_hash_object(repo: &Path, content: &[u8]) -> String {
    let output = run_command_with_watchdog(
        &pinned_stock_git(),
        &["hash-object", "--stdin"],
        repo,
        content,
    );
    assert_eq!(output.status, 0, "pinned hash-object failed");
    String::from_utf8(output.stdout)
        .expect("hash-object output utf8")
        .trim()
        .to_owned()
}

#[cfg(unix)]
#[test]
fn fast_import_loose_collision_preserves_foreign_inode() {
    for hard_link in [false, true] {
        let repo = git_init();
        let oid = pinned_hash_object(repo.path(), b"hello");
        let object_dir = repo.path().join(".git/objects").join(&oid[..2]);
        fs::create_dir_all(&object_dir).expect("create object fanout");
        let foreign_dir = tempfile::TempDir::new().expect("foreign object directory");
        let foreign = foreign_dir.path().join("sentinel");
        fs::write(&foreign, b"foreign sentinel").expect("write sentinel");
        let final_path = object_dir.join(&oid[2..]);
        if hard_link {
            fs::hard_link(&foreign, &final_path).expect("create foreign hard link");
        } else {
            symlink(&foreign, &final_path).expect("create foreign symlink");
        }

        let output = command_with_stdin_output(
            zmin_bin(),
            repo.path(),
            &["fast-import", "--quiet"],
            "blob\ndata 5\nhello\ndone\n",
        );
        assert_ne!(output.0, 0, "foreign collision must fail closed");
        assert_eq!(
            fs::read(&foreign).expect("read sentinel"),
            b"foreign sentinel"
        );
        if hard_link {
            use std::os::unix::fs::MetadataExt;

            assert_eq!(
                fs::metadata(&final_path)
                    .expect("hard link metadata")
                    .nlink(),
                2
            );
        } else {
            assert!(
                fs::symlink_metadata(&final_path)
                    .expect("symlink metadata")
                    .file_type()
                    .is_symlink()
            );
        }
    }
}

#[test]
fn fast_import_malformed_after_checkpoint_preserves_stock_partial_import() {
    let stream = "\
blob
mark :1
data 5
hello
commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

checkpoint
bogus
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    git(git_repo.path(), ["config", "fastimport.unpackLimit", "0"]);
    git(zmin_repo.path(), ["config", "fastimport.unpackLimit", "0"]);
    let git_output =
        command_with_stdin_output("git", git_repo.path(), &["fast-import", "--quiet"], stream);
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--quiet"],
        stream,
    );
    assert_eq!(git_output.0, 128);
    assert_eq!(zmin_output.0, 128);
    assert_eq!(git_output.1, zmin_output.1);
    assert_eq!(
        object_layout(git_repo.path()),
        object_layout(zmin_repo.path())
    );
    assert_eq!(
        loose_object_file_count(git_repo.path()),
        loose_object_file_count(zmin_repo.path())
    );
    assert_eq!(
        pack_artifacts(git_repo.path()),
        pack_artifacts(zmin_repo.path())
    );
    assert_eq!(keep_artifacts(git_repo.path()), Vec::<String>::new());
    assert_eq!(keep_artifacts(zmin_repo.path()), Vec::<String>::new());
    assert_eq!(
        git_status(
            git_repo.path(),
            ["rev-parse", "--verify", "refs/heads/main"]
        ),
        git_status(
            zmin_repo.path(),
            ["rev-parse", "--verify", "refs/heads/main"]
        )
    );
    assert_eq!(
        git(
            git_repo.path(),
            ["rev-parse", "--verify", "refs/heads/main"]
        ),
        git(
            zmin_repo.path(),
            ["rev-parse", "--verify", "refs/heads/main"]
        )
    );
}

#[test]
fn fast_import_malformed_before_checkpoint_does_not_publish_staged_refs() {
    let stream = "\
blob
mark :1
data 5
hello
commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

bogus
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    git(git_repo.path(), ["config", "fastimport.unpackLimit", "0"]);
    git(zmin_repo.path(), ["config", "fastimport.unpackLimit", "0"]);
    let git_output =
        command_with_stdin_output("git", git_repo.path(), &["fast-import", "--quiet"], stream);
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--quiet"],
        stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        git_status(
            git_repo.path(),
            ["rev-parse", "--verify", "refs/heads/main"]
        ),
        git_status(
            zmin_repo.path(),
            ["rev-parse", "--verify", "refs/heads/main"]
        )
    );
    assert_eq!(
        object_layout(git_repo.path()),
        object_layout(zmin_repo.path())
    );
}

#[test]
fn fast_import_stages_reset_and_tag_refs_until_checkpoint_or_end() {
    let stream = "\
blob
mark :1
data 5
hello
commit refs/heads/main
mark :2
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

checkpoint
reset refs/heads/other
from :2

tag v1
from :2
tagger A <a@example.test> 0 +0000
data 4
tag

bogus
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output =
        command_with_stdin_output("git", git_repo.path(), &["fast-import", "--quiet"], stream);
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--quiet"],
        stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    for reference in ["refs/heads/main", "refs/heads/other", "refs/tags/v1"] {
        assert_eq!(
            git_status(git_repo.path(), ["rev-parse", "--verify", reference]),
            git_status(zmin_repo.path(), ["rev-parse", "--verify", reference]),
            "ref state for {reference}"
        );
    }
    assert_eq!(
        git(
            git_repo.path(),
            ["rev-parse", "--verify", "refs/heads/main"]
        ),
        git(
            zmin_repo.path(),
            ["rev-parse", "--verify", "refs/heads/main"]
        )
    );
}

#[test]
fn fast_import_failure_exports_marks_and_edges_like_stock_git() {
    let stream = "\
blob
mark :1
data 5
hello
commit refs/heads/main
mark :2
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

checkpoint
bogus
";
    let args = [
        "fast-import",
        "--quiet",
        "--export-marks=marks.txt",
        "--export-pack-edges=edges.txt",
    ];
    let git_repo = git_init();
    let zmin_repo = git_init();
    git(git_repo.path(), ["config", "fastimport.unpackLimit", "0"]);
    git(zmin_repo.path(), ["config", "fastimport.unpackLimit", "0"]);
    let git_output = command_with_stdin_output("git", git_repo.path(), &args, stream);
    let zmin_output = command_with_stdin_output(zmin_bin(), zmin_repo.path(), &args, stream);
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        fs::read(git_repo.path().join("marks.txt")).expect("stock failure marks"),
        fs::read(zmin_repo.path().join("marks.txt")).expect("zmin failure marks")
    );
    assert_eq!(
        fs::read(git_repo.path().join("edges.txt")).expect("stock failure edges"),
        fs::read(zmin_repo.path().join("edges.txt")).expect("zmin failure edges")
    );
}

#[test]
fn fast_import_rfc2822_date_format_matches_stock_git_statistics() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stream = "\
commit refs/heads/main
author A U Thor <author@example.test> Thu, 01 Jan 1970 00:00:00 +0000
committer C O Mitter <committer@example.test> Thu, 01 Jan 1970 00:00:01 +0000
data <<EOF
rfc date
EOF
M 100644 inline a.txt
data <<EOF
contents
EOF
";

    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["fast-import", "--date-format=rfc2822"],
        stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--date-format=rfc2822"],
        stream,
    );

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["log", "--format=%an <%ae>|%cn <%ce>|%at %ct|%s", "main"]
        ),
        git(
            git_repo.path(),
            ["log", "--format=%an <%ae>|%cn <%ce>|%at %ct|%s", "main"]
        )
    );
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "main:a.txt"]),
        git(git_repo.path(), ["cat-file", "-p", "main:a.txt"])
    );
}

#[test]
fn fast_import_checkpoint_matches_stock_git_statistics() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stream = "\
blob
mark :1
data 6
hello

checkpoint
commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt
";

    let git_output = command_with_stdin_output("git", git_repo.path(), &["fast-import"], stream);
    let zmin_output =
        command_with_stdin_output(zmin_bin(), zmin_repo.path(), &["fast-import"], stream);

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["cat-file", "-p", "refs/heads/main:a.txt"]
        ),
        git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );
}

#[test]
fn fast_import_progress_matches_stock_git_statistics() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stream = "\
progress hello from importer
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

";

    let git_output = command_with_stdin_output("git", git_repo.path(), &["fast-import"], stream);
    let zmin_output =
        command_with_stdin_output(zmin_bin(), zmin_repo.path(), &["fast-import"], stream);

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["cat-file", "-p", "refs/heads/main:a.txt"]
        ),
        git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );
}

#[test]
fn fast_import_done_matches_stock_git_statistics() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stream = "\
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

";

    let git_output = command_with_stdin_output("git", git_repo.path(), &["fast-import"], stream);
    let zmin_output =
        command_with_stdin_output(zmin_bin(), zmin_repo.path(), &["fast-import"], stream);

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["cat-file", "-p", "refs/heads/main:a.txt"]
        ),
        git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );
}

#[test]
fn fast_import_documented_option_surface_matches_stock_git() {
    let stream = "\
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";

    for args in [
        &["fast-import", "--stats"][..],
        &["fast-import", "--force"][..],
        &["fast-import", "--done"][..],
        &["fast-import", "--allow-unsafe-features"][..],
        &["fast-import", "--active-branches=1"][..],
        &["fast-import", "--big-file-threshold=1"][..],
        &["fast-import", "--cat-blob-fd=9"][..],
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let git_output = command_with_stdin_output("git", git_repo.path(), args, stream);
        let zmin_output = command_with_stdin_output(zmin_bin(), zmin_repo.path(), args, stream);
        assert_eq!(zmin_output.0, git_output.0, "exit for {args:?}");
        assert_eq!(zmin_output.1, git_output.1, "stdout for {args:?}");
        assert_eq!(
            normalize_fast_import_statistics_stderr(&zmin_output.2),
            normalize_fast_import_statistics_stderr(&git_output.2),
            "stderr for {args:?}"
        );
        assert_eq!(
            git(
                zmin_repo.path(),
                ["cat-file", "-p", "refs/heads/main:a.txt"]
            ),
            git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"]),
            "imported content for {args:?}"
        );
    }

    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output =
        command_with_stdin_output("git", git_repo.path(), &["fast-import", "--quiet"], stream);
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--quiet"],
        stream,
    );
    assert_eq!(zmin_output, git_output);
    assert!(zmin_output.2.is_empty());
}

#[test]
fn fast_import_schema_gap_option_surface_matches_stock_git() {
    let stream = "\
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";

    for args in [
        &["fast-import", "--depth=1"][..],
        &["fast-import", "--no-relative-marks"][..],
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let git_output = command_with_stdin_output("git", git_repo.path(), args, stream);
        let zmin_output = command_with_stdin_output(zmin_bin(), zmin_repo.path(), args, stream);
        assert_eq!(zmin_output.0, git_output.0, "exit for {args:?}");
        assert_eq!(zmin_output.1, git_output.1, "stdout for {args:?}");
        assert_eq!(
            normalize_fast_import_statistics_stderr(&zmin_output.2),
            normalize_fast_import_statistics_stderr(&git_output.2),
            "stderr for {args:?}"
        );
        assert_eq!(
            git(
                zmin_repo.path(),
                ["cat-file", "-p", "refs/heads/main:a.txt"]
            ),
            git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"]),
            "imported content for {args:?}"
        );
    }

    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_edges = git_repo.path().join("edges.txt");
    let zmin_edges = zmin_repo.path().join("edges.txt");
    git(git_repo.path(), ["config", "fastimport.unpackLimit", "0"]);
    git(zmin_repo.path(), ["config", "fastimport.unpackLimit", "0"]);
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &[
            "fast-import",
            &format!("--export-pack-edges={}", git_edges.display()),
        ],
        stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "fast-import",
            &format!("--export-pack-edges={}", zmin_edges.display()),
        ],
        stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    let git_edges_contents = fs::read_to_string(&git_edges).expect("read git export-pack-edges");
    let zmin_edges_contents = fs::read_to_string(&zmin_edges).expect("read zmin export-pack-edges");
    assert!(!git_edges_contents.is_empty());
    assert!(!zmin_edges_contents.is_empty());
    assert_fast_import_pack_edges_match_stock(&zmin_edges_contents, &git_edges_contents);
}

#[test]
fn fast_import_max_pack_size_is_explicitly_unsupported_pending_b2b() {
    let repo = git_init();
    let output = command_with_stdin_output(
        zmin_bin(),
        repo.path(),
        &["fast-import", "--max-pack-size=1"],
        "done\n",
    );
    assert_eq!(output.0, 128);
    assert!(output.1.is_empty());
    assert_eq!(
        normalize_fast_import_crash_stderr(
            &output.2,
            "fatal: --max-pack-size is unsupported pending B2b",
        ),
        "fatal: --max-pack-size is unsupported pending B2b\nfast-import: dumping crash report to .git/fast_import_crash_<pid>"
    );
    assert_eq!(git_status(repo.path(), ["count-objects", "-v"]), 0);
    assert!(
        git(repo.path(), ["count-objects", "-v"])
            .lines()
            .any(|line| line == "count: 0"),
        "unsupported max-pack-size must fail before writing objects"
    );
}

#[test]
fn fast_import_b2a_emits_valid_pack_idx_and_edges() {
    let repo = git_init();
    git(repo.path(), ["config", "fastimport.unpackLimit", "0"]);
    let edges = repo.path().join("edges.txt");
    let stream = "\
blob
mark :1
data 5
hello
commit refs/heads/main
mark :2
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

tag v1
mark :3
from :2
tagger A <a@example.test> 0 +0000
data 4
tag

done
";
    let output = command_with_stdin_output(
        zmin_bin(),
        repo.path(),
        &[
            "fast-import",
            "--quiet",
            &format!("--export-pack-edges={}", edges.display()),
        ],
        stream,
    );
    assert_eq!(output.0, 0, "fast-import failed: {}", output.2);
    assert!(output.1.is_empty());
    let count = git(repo.path(), ["count-objects", "-v"]);
    assert!(count.lines().any(|line| line == "in-pack: 4"));
    assert!(count.lines().any(|line| line == "packs: 1"));
    assert_eq!(
        git(repo.path(), ["cat-file", "-t", "refs/tags/v1"]),
        "tag\n"
    );
    let idx = fs::read_dir(repo.path().join(".git/objects/pack"))
        .expect("pack directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "idx"))
        .expect("v2 pack index");
    let idx = idx.to_str().expect("temporary path utf8");
    assert_eq!(git_status(repo.path(), ["verify-pack", "-v", idx]), 0);
    let edge_text = fs::read_to_string(edges).expect("pack edge file");
    assert_eq!(edge_text.lines().count(), 1);
    assert!(edge_text.contains(&git(repo.path(), ["rev-parse", "refs/tags/v1"]).trim()));
}

#[test]
fn fast_import_b2a_uses_repository_sha256_algorithm() {
    let git_repo = TempDir::new().expect("stock sha256 repository");
    let zmin_repo = TempDir::new().expect("zmin sha256 repository");
    for repo in [git_repo.path(), zmin_repo.path()] {
        let output = run_command_with_watchdog(
            &pinned_stock_git(),
            &["init", "-q", "--object-format=sha256"],
            repo,
            &[],
        );
        assert_eq!(output.status, 0, "pinned sha256 init failed");
        git(repo, ["config", "fastimport.unpackLimit", "0"]);
    }
    let stream = "\
blob
mark :1
data 5
hello
commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    let git_output =
        command_with_stdin_output("git", git_repo.path(), &["fast-import", "--quiet"], stream);
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--quiet"],
        stream,
    );
    assert_eq!(zmin_output, git_output, "SHA-256 fast-import differential");
    for repo in [git_repo.path(), zmin_repo.path()] {
        let commit = git(repo, ["rev-parse", "refs/heads/main"]);
        assert_eq!(commit.trim().len(), 64);
        let count = git(repo, ["count-objects", "-v"]);
        assert!(count.lines().any(|line| line == "in-pack: 3"));
    }

    let find_artifact = |repo: &Path, extension: &str| {
        fs::read_dir(repo.join(".git/objects/pack"))
            .expect("read SHA-256 pack directory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|value| value == extension))
            .expect("SHA-256 pack artifact")
    };
    let git_pack = find_artifact(git_repo.path(), "pack");
    let zmin_pack = find_artifact(zmin_repo.path(), "pack");
    let git_idx = find_artifact(git_repo.path(), "idx");
    let zmin_idx = find_artifact(zmin_repo.path(), "idx");
    assert_eq!(
        &fs::read(&git_pack).expect("stock SHA-256 pack")[..32],
        &fs::read(&zmin_pack).expect("zmin SHA-256 pack")[..32],
        "pack headers are SHA-256/v2 equivalent"
    );
    let git_pack_bytes = fs::read(&git_pack).expect("stock SHA-256 pack bytes");
    let zmin_pack_bytes = fs::read(&zmin_pack).expect("zmin SHA-256 pack bytes");
    assert_eq!(
        &git_pack_bytes[git_pack_bytes.len() - 32..],
        &zmin_pack_bytes[zmin_pack_bytes.len() - 32..],
        "pack trailers are byte-equivalent"
    );
    assert_eq!(
        git_status(
            git_repo.path(),
            ["verify-pack", "-v", git_idx.to_str().unwrap()]
        ),
        0
    );
    assert_eq!(
        git_status(
            zmin_repo.path(),
            ["verify-pack", "-v", zmin_idx.to_str().unwrap()]
        ),
        0
    );
    let verify_objects = |repo: &Path, idx: &Path| {
        git(
            repo,
            ["verify-pack", "-v", idx.to_str().expect("UTF-8 index path")],
        )
        .lines()
        .filter(|line| {
            line.split_whitespace()
                .nth(1)
                .is_some_and(|kind| matches!(kind, "blob" | "tree" | "commit"))
        })
        .map(str::to_owned)
        .collect::<Vec<_>>()
    };
    assert_eq!(
        verify_objects(git_repo.path(), &git_idx),
        verify_objects(zmin_repo.path(), &zmin_idx),
        "verified SHA-256 object/index records"
    );
}

#[test]
fn fast_import_unpack_limit_precedence_and_boundary_match_stock_git() {
    let stream = "\
blob
mark :1
data 5
hello
commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    let cases = [
        (None, None, (3, 0)),
        (Some("3"), Some("0"), (3, 0)),
        (Some("2"), None, (0, 1)),
        (None, Some("2"), (0, 1)),
        (Some("-1"), Some("100"), (3, 0)),
    ];
    for (fastimport_limit, transfer_limit, expected) in cases {
        let git_repo = git_init();
        let zmin_repo = git_init();
        for repo in [git_repo.path(), zmin_repo.path()] {
            if let Some(value) = fastimport_limit {
                git(repo, ["config", "fastimport.unpackLimit", value]);
            }
            if let Some(value) = transfer_limit {
                git(repo, ["config", "transfer.unpackLimit", value]);
            }
        }
        let git_output =
            command_with_stdin_output("git", git_repo.path(), &["fast-import", "--quiet"], stream);
        let zmin_output = command_with_stdin_output(
            zmin_bin(),
            zmin_repo.path(),
            &["fast-import", "--quiet"],
            stream,
        );
        assert_eq!(zmin_output, git_output);
        assert_eq!(
            object_layout(zmin_repo.path()),
            expected,
            "zmin fastimport={fastimport_limit:?} transfer={transfer_limit:?}"
        );
        assert_eq!(
            object_layout(git_repo.path()),
            expected,
            "stock fastimport={fastimport_limit:?} transfer={transfer_limit:?}"
        );
    }
}

#[test]
fn fast_import_checkpoint_rotates_unpack_limit_like_stock_git() {
    let stream = "\
blob
mark :1
data 6
hello

blob
mark :2
data 6
world

blob
mark :3
data 6
third

checkpoint
blob
mark :4
data 6
fourth
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    git(git_repo.path(), ["config", "fastimport.unpackLimit", "2"]);
    git(zmin_repo.path(), ["config", "fastimport.unpackLimit", "2"]);
    let git_output =
        command_with_stdin_output("git", git_repo.path(), &["fast-import", "--quiet"], stream);
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--quiet"],
        stream,
    );
    assert_eq!(zmin_output, git_output);
    assert_eq!(object_layout(zmin_repo.path()), (1, 1));
    assert_eq!(object_layout(git_repo.path()), (1, 1));
}

#[test]
fn fast_import_duplicate_and_streamed_objects_are_packed_once_like_stock_git() {
    let duplicate_stream = "\
blob
mark :1
data 6
same!
blob
mark :2
data 6
same!
";
    let large_content = "x".repeat(70_000);
    let large_stream = format!(
        "blob\nmark :1\ndata {}\n{}",
        large_content.len(),
        large_content
    );
    for stream in [duplicate_stream.to_owned(), large_stream] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        git(git_repo.path(), ["config", "fastimport.unpackLimit", "0"]);
        git(zmin_repo.path(), ["config", "fastimport.unpackLimit", "0"]);
        let git_output =
            command_with_stdin_output("git", git_repo.path(), &["fast-import", "--quiet"], &stream);
        let zmin_output = command_with_stdin_output(
            zmin_bin(),
            zmin_repo.path(),
            &["fast-import", "--quiet"],
            &stream,
        );
        assert_eq!(zmin_output, git_output);
        assert_eq!(object_layout(zmin_repo.path()), (0, 1));
        assert_eq!(object_layout(git_repo.path()), (0, 1));
    }
}

#[cfg(unix)]
#[test]
fn fast_import_rejects_pack_edge_symlink_and_hardlink_redirection() {
    for use_hard_link in [false, true] {
        let repo = git_init();
        git(repo.path(), ["config", "fastimport.unpackLimit", "0"]);
        let sentinel = repo.path().join("pack-edges-sentinel");
        fs::write(&sentinel, b"do not modify\n").expect("sentinel");
        let edges = repo.path().join("edges.txt");
        if use_hard_link {
            fs::hard_link(&sentinel, &edges).expect("pack edge hard link");
        } else {
            symlink(&sentinel, &edges).expect("pack edge symlink");
        }
        let output = command_with_stdin_output(
            zmin_bin(),
            repo.path(),
            &[
                "fast-import",
                "--quiet",
                &format!("--export-pack-edges={}", edges.display()),
            ],
            "blob\nmark :1\ndata 5\nhello\n\ndone\n",
        );
        assert_ne!(output.0, 0, "redirection must fail closed");
        assert_eq!(
            fs::read(&sentinel).expect("sentinel remains"),
            b"do not modify\n"
        );
    }
}

#[cfg(unix)]
#[test]
fn fast_import_rejects_pack_final_symlink_without_touching_target() {
    let repo = git_init();
    git(repo.path(), ["config", "fastimport.unpackLimit", "0"]);
    let stream = "blob\nmark :1\ndata 5\nhello\n\ndone\n";
    let first =
        command_with_stdin_output(zmin_bin(), repo.path(), &["fast-import", "--quiet"], stream);
    assert_eq!(first.0, 0, "initial pack import failed: {}", first.2);
    let pack_dir = repo.path().join(".git/objects/pack");
    let pack = fs::read_dir(&pack_dir)
        .expect("pack directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "pack")
        })
        .expect("initial pack");
    let index = pack.with_extension("idx");
    let pack_name = pack.file_name().expect("pack filename").to_owned();
    let sentinel = repo.path().join("pack-final-sentinel");
    fs::write(&sentinel, b"do not modify\n").expect("sentinel");
    fs::remove_file(&pack).expect("remove initial pack");
    fs::remove_file(index).expect("remove initial index");
    symlink(&sentinel, pack_dir.join(pack_name)).expect("pack final symlink");

    let second =
        command_with_stdin_output(zmin_bin(), repo.path(), &["fast-import", "--quiet"], stream);
    assert_ne!(second.0, 0, "pack final symlink must fail closed");
    assert_eq!(
        fs::read(&sentinel).expect("sentinel remains"),
        b"do not modify\n"
    );
}

#[cfg(unix)]
#[test]
fn fast_import_rejects_pack_final_hardlink_without_touching_target() {
    for replaced_extension in ["pack", "idx"] {
        let repo = git_init();
        git(repo.path(), ["config", "fastimport.unpackLimit", "0"]);
        let stream = "blob\nmark :1\ndata 5\nhello\n\ndone\n";
        let first =
            command_with_stdin_output(zmin_bin(), repo.path(), &["fast-import", "--quiet"], stream);
        assert_eq!(first.0, 0, "initial pack import failed: {}", first.2);
        let pack_dir = repo.path().join(".git/objects/pack");
        let pack = fs::read_dir(&pack_dir)
            .expect("pack directory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "pack")
            })
            .expect("initial pack");
        let index = pack.with_extension("idx");
        let replaced = if replaced_extension == "pack" {
            pack.clone()
        } else {
            index.clone()
        };
        let replaced_name = replaced.file_name().expect("replaced filename").to_owned();
        let sentinel = repo
            .path()
            .join(format!("pack-final-hardlink-{replaced_extension}-sentinel"));
        fs::write(&sentinel, b"do not modify\n").expect("sentinel");
        fs::remove_file(pack_dir.join(&replaced_name)).expect("remove final artifact");
        if replaced_extension == "pack" {
            fs::remove_file(index).expect("remove initial index");
        } else {
            fs::remove_file(pack).expect("remove initial pack");
        }
        fs::hard_link(&sentinel, pack_dir.join(replaced_name)).expect("hard-link final artifact");

        let second =
            command_with_stdin_output(zmin_bin(), repo.path(), &["fast-import", "--quiet"], stream);
        assert_ne!(
            second.0, 0,
            "pack final {replaced_extension} hardlink must fail closed"
        );
        assert_eq!(
            fs::read(&sentinel).expect("sentinel remains"),
            b"do not modify\n"
        );
    }
}

#[test]
fn fast_import_marks_options_match_stock_git() {
    let export_stream = "\
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";

    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_marks = git_repo.path().join("marks.txt");
    let zmin_marks = zmin_repo.path().join("marks.txt");
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &[
            "fast-import",
            &format!("--export-marks={}", git_marks.display()),
        ],
        export_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "fast-import",
            &format!("--export-marks={}", zmin_marks.display()),
        ],
        export_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        fs::read_to_string(&zmin_marks).expect("read zmin marks"),
        fs::read_to_string(&git_marks).expect("read git marks")
    );

    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["fast-import", "--import-marks-if-exists=missing.marks"],
        export_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--import-marks-if-exists=missing.marks"],
        export_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );

    let import_stream = "\
commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_blob = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    let zmin_blob = command_with_stdin_output(
        "git",
        zmin_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    assert_eq!(git_blob, zmin_blob);
    let git_marks = git_repo.path().join("import.marks");
    let zmin_marks = zmin_repo.path().join("import.marks");
    fs::write(&git_marks, format!(":1 {git_blob}\n")).expect("write git import marks");
    fs::write(&zmin_marks, format!(":1 {zmin_blob}\n")).expect("write zmin import marks");
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &[
            "fast-import",
            &format!("--import-marks={}", git_marks.display()),
        ],
        import_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "fast-import",
            &format!("--import-marks={}", zmin_marks.display()),
        ],
        import_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["cat-file", "-p", "refs/heads/main:a.txt"]
        ),
        git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );
}

#[test]
fn fast_import_repeated_marks_and_order_families_match_stock_git() {
    let export_stream = "\
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";

    for args in [
        &[
            "fast-import",
            "--export-marks=marks.txt",
            "--export-marks=marks.txt",
        ][..],
        &[
            "fast-import",
            "--import-marks-if-exists=missing1.marks",
            "--import-marks-if-exists=missing2.marks",
        ],
        &[
            "fast-import",
            "--no-relative-marks",
            "--export-marks=marks.txt",
        ],
        &[
            "fast-import",
            "--export-marks=marks.txt",
            "--no-relative-marks",
        ],
        &[
            "fast-import",
            "--import-marks-if-exists=missing.marks",
            "--export-marks=out.marks",
        ],
        &[
            "fast-import",
            "--export-marks=out.marks",
            "--import-marks-if-exists=missing.marks",
        ],
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let git_output = command_with_stdin_output("git", git_repo.path(), args, export_stream);
        let zmin_output =
            command_with_stdin_output(zmin_bin(), zmin_repo.path(), args, export_stream);
        assert_eq!(zmin_output.0, git_output.0, "exit for {args:?}");
        assert_eq!(zmin_output.1, git_output.1, "stdout for {args:?}");
        assert_eq!(
            normalize_fast_import_statistics_stderr(&zmin_output.2),
            normalize_fast_import_statistics_stderr(&git_output.2),
            "stderr for {args:?}"
        );
        for path in ["marks.txt", "out.marks"] {
            let git_path = git_repo.path().join(path);
            let zmin_path = zmin_repo.path().join(path);
            assert_eq!(
                zmin_path.exists(),
                git_path.exists(),
                "existence for {args:?} {path}"
            );
            if git_path.exists() {
                assert_eq!(
                    fs::read_to_string(&zmin_path).expect("read zmin marks"),
                    fs::read_to_string(&git_path).expect("read git marks"),
                    "marks for {args:?} {path}"
                );
            }
        }
    }

    for args in [&[
        "fast-import",
        "--export-marks=one.marks",
        "--export-marks=two.marks",
    ][..]]
    {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let git_output = command_with_stdin_output("git", git_repo.path(), args, export_stream);
        let zmin_output =
            command_with_stdin_output(zmin_bin(), zmin_repo.path(), args, export_stream);
        assert_eq!(zmin_output.0, git_output.0, "exit for {args:?}");
        assert_eq!(zmin_output.1, git_output.1, "stdout for {args:?}");
        assert_eq!(
            normalize_fast_import_statistics_stderr(&zmin_output.2),
            normalize_fast_import_statistics_stderr(&git_output.2),
            "stderr for {args:?}"
        );
        for path in ["one.marks", "two.marks"] {
            let git_path = git_repo.path().join(path);
            let zmin_path = zmin_repo.path().join(path);
            assert_eq!(
                zmin_path.exists(),
                git_path.exists(),
                "existence for {args:?} {path}"
            );
            if git_path.exists() {
                assert_eq!(
                    fs::read_to_string(&zmin_path).expect("read zmin marks"),
                    fs::read_to_string(&git_path).expect("read git marks"),
                    "marks for {args:?} {path}"
                );
            }
        }
    }

    let import_stream = "\
commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    for args in [
        &[
            "fast-import",
            "--import-marks=one.marks",
            "--import-marks=two.marks",
        ][..],
        &[
            "fast-import",
            "--import-marks=two.marks",
            "--import-marks=one.marks",
        ],
        &[
            "fast-import",
            "--import-marks-if-exists=one.marks",
            "--import-marks-if-exists=two.marks",
        ],
        &[
            "fast-import",
            "--import-marks-if-exists=two.marks",
            "--import-marks-if-exists=one.marks",
        ],
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let git_blob_one = command_with_stdin_output(
            "git",
            git_repo.path(),
            &["hash-object", "-w", "--stdin"],
            "hello\n",
        )
        .1
        .trim()
        .to_owned();
        let git_blob_two = command_with_stdin_output(
            "git",
            git_repo.path(),
            &["hash-object", "-w", "--stdin"],
            "world\n",
        )
        .1
        .trim()
        .to_owned();
        let zmin_blob_one = command_with_stdin_output(
            "git",
            zmin_repo.path(),
            &["hash-object", "-w", "--stdin"],
            "hello\n",
        )
        .1
        .trim()
        .to_owned();
        let zmin_blob_two = command_with_stdin_output(
            "git",
            zmin_repo.path(),
            &["hash-object", "-w", "--stdin"],
            "world\n",
        )
        .1
        .trim()
        .to_owned();
        assert_eq!(git_blob_one, zmin_blob_one, "blob one for {args:?}");
        assert_eq!(git_blob_two, zmin_blob_two, "blob two for {args:?}");
        fs::write(
            git_repo.path().join("one.marks"),
            format!(":1 {git_blob_one}\n"),
        )
        .expect("write git one.marks");
        fs::write(
            git_repo.path().join("two.marks"),
            format!(":1 {git_blob_two}\n"),
        )
        .expect("write git two.marks");
        fs::write(
            zmin_repo.path().join("one.marks"),
            format!(":1 {zmin_blob_one}\n"),
        )
        .expect("write zmin one.marks");
        fs::write(
            zmin_repo.path().join("two.marks"),
            format!(":1 {zmin_blob_two}\n"),
        )
        .expect("write zmin two.marks");

        let git_output = command_with_stdin_output("git", git_repo.path(), args, import_stream);
        let zmin_output =
            command_with_stdin_output(zmin_bin(), zmin_repo.path(), args, import_stream);
        assert_eq!(zmin_output.0, git_output.0, "exit for {args:?}");
        assert_eq!(zmin_output.1, git_output.1, "stdout for {args:?}");
        assert_eq!(
            normalize_fast_import_statistics_stderr(&zmin_output.2),
            normalize_fast_import_statistics_stderr(&git_output.2),
            "stderr for {args:?}"
        );
        assert_eq!(
            git(
                zmin_repo.path(),
                ["cat-file", "-p", "refs/heads/main:a.txt"]
            ),
            git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"]),
            "imported content for {args:?}"
        );
    }
}

#[test]
fn fast_import_stream_feature_marks_match_stock_git() {
    let export_stream = "\
feature export-marks=marks.txt
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        export_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        export_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("marks.txt")).expect("read zmin marks"),
        fs::read_to_string(git_repo.path().join("marks.txt")).expect("read git marks")
    );

    let import_stream = "\
feature import-marks=one.marks
commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_blob = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    let zmin_blob = command_with_stdin_output(
        "git",
        zmin_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    fs::write(
        git_repo.path().join("one.marks"),
        format!(":1 {git_blob}\n"),
    )
    .expect("write git marks");
    fs::write(
        zmin_repo.path().join("one.marks"),
        format!(":1 {zmin_blob}\n"),
    )
    .expect("write zmin marks");
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        import_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        import_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["cat-file", "-p", "refs/heads/main:a.txt"]
        ),
        git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );

    let import_if_exists_stream = "\
feature import-marks-if-exists=one.marks
commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_blob = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    let zmin_blob = command_with_stdin_output(
        "git",
        zmin_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    fs::write(
        git_repo.path().join("one.marks"),
        format!(":1 {git_blob}\n"),
    )
    .expect("write git marks");
    fs::write(
        zmin_repo.path().join("one.marks"),
        format!(":1 {zmin_blob}\n"),
    )
    .expect("write zmin marks");
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        import_if_exists_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        import_if_exists_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["cat-file", "-p", "refs/heads/main:a.txt"]
        ),
        git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );

    let no_relative_export_stream = "\
feature no-relative-marks
feature export-marks=marks.txt
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        no_relative_export_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        no_relative_export_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("marks.txt")).expect("read zmin marks"),
        fs::read_to_string(git_repo.path().join("marks.txt")).expect("read git marks")
    );

    let cli_override_stream = "\
feature import-marks=two.marks
commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_blob_one = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    let git_blob_two = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "world\n",
    )
    .1
    .trim()
    .to_owned();
    let zmin_blob_one = command_with_stdin_output(
        "git",
        zmin_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    let zmin_blob_two = command_with_stdin_output(
        "git",
        zmin_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "world\n",
    )
    .1
    .trim()
    .to_owned();
    fs::write(
        git_repo.path().join("one.marks"),
        format!(":1 {git_blob_one}\n"),
    )
    .expect("write git one");
    fs::write(
        git_repo.path().join("two.marks"),
        format!(":1 {git_blob_two}\n"),
    )
    .expect("write git two");
    fs::write(
        zmin_repo.path().join("one.marks"),
        format!(":1 {zmin_blob_one}\n"),
    )
    .expect("write zmin one");
    fs::write(
        zmin_repo.path().join("two.marks"),
        format!(":1 {zmin_blob_two}\n"),
    )
    .expect("write zmin two");
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &[
            "fast-import",
            "--allow-unsafe-features",
            "--import-marks=one.marks",
        ],
        cli_override_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "fast-import",
            "--allow-unsafe-features",
            "--import-marks=one.marks",
        ],
        cli_override_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["cat-file", "-p", "refs/heads/main:a.txt"]
        ),
        git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );
}

#[test]
fn fast_import_stream_feature_marks_fail_like_stock_git() {
    for (args, stream, fatal) in [
        (
            &["fast-import"][..],
            "feature export-marks=marks.txt\n",
            "fatal: feature 'export-marks=marks.txt' forbidden in input without --allow-unsafe-features",
        ),
        (
            &["fast-import"],
            "feature import-marks=one.marks\n",
            "fatal: feature 'import-marks' forbidden in input without --allow-unsafe-features",
        ),
        (
            &["fast-import"],
            "feature import-marks-if-exists=one.marks\n",
            "fatal: feature 'import-marks-if-exists' forbidden in input without --allow-unsafe-features",
        ),
        (
            &["fast-import", "--allow-unsafe-features"],
            "feature import-marks=one.marks\nfeature import-marks=two.marks\n",
            "fatal: only one import-marks command allowed per stream",
        ),
        (
            &["fast-import", "--allow-unsafe-features"],
            "feature import-marks=one.marks\nfeature import-marks-if-exists=two.marks\n",
            "fatal: only one import-marks command allowed per stream",
        ),
        (
            &["fast-import", "--allow-unsafe-features"],
            "feature import-marks-if-exists=one.marks\nfeature import-marks-if-exists=two.marks\n",
            "fatal: only one import-marks command allowed per stream",
        ),
        (
            &[
                "fast-import",
                "--allow-unsafe-features",
                "--import-marks=one.marks",
            ],
            "feature import-marks=one.marks\nfeature import-marks=two.marks\n",
            "fatal: only one import-marks command allowed per stream",
        ),
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        fs::write(git_repo.path().join("one.marks"), "").expect("write git one");
        fs::write(git_repo.path().join("two.marks"), "").expect("write git two");
        fs::write(zmin_repo.path().join("one.marks"), "").expect("write zmin one");
        fs::write(zmin_repo.path().join("two.marks"), "").expect("write zmin two");
        let git_output = command_with_stdin_output("git", git_repo.path(), args, stream);
        let zmin_output = command_with_stdin_output(zmin_bin(), zmin_repo.path(), args, stream);
        assert_eq!(
            zmin_output.0, git_output.0,
            "exit for {args:?} / {stream:?}"
        );
        assert_eq!(
            zmin_output.1, git_output.1,
            "stdout for {args:?} / {stream:?}"
        );
        assert_eq!(
            normalize_fast_import_crash_stderr(&zmin_output.2, fatal),
            normalize_fast_import_crash_stderr(&git_output.2, fatal),
            "stderr for {args:?} / {stream:?}"
        );
    }
}

#[test]
fn fast_import_relative_marks_path_resolution_matches_stock_git() {
    let import_stream = "\
commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    let inline_stream = "\
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";

    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &[
            "fast-import",
            "--relative-marks",
            "--export-marks=marks.txt",
        ],
        inline_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "fast-import",
            "--relative-marks",
            "--export-marks=marks.txt",
        ],
        inline_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/info/fast-import/marks.txt"))
            .expect("read zmin relative marks"),
        fs::read_to_string(git_repo.path().join(".git/info/fast-import/marks.txt"))
            .expect("read git relative marks")
    );

    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_blob = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    let zmin_blob = command_with_stdin_output(
        "git",
        zmin_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    fs::write(
        git_repo.path().join("plain.marks"),
        format!(":1 {git_blob}\n"),
    )
    .expect("write git plain");
    fs::write(
        zmin_repo.path().join("plain.marks"),
        format!(":1 {zmin_blob}\n"),
    )
    .expect("write zmin plain");
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &[
            "fast-import",
            "--import-marks=plain.marks",
            "--relative-marks",
        ],
        import_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "fast-import",
            "--import-marks=plain.marks",
            "--relative-marks",
        ],
        import_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["cat-file", "-p", "refs/heads/main:a.txt"]
        ),
        git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );

    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_blob = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    let zmin_blob = command_with_stdin_output(
        "git",
        zmin_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    fs::create_dir_all(git_repo.path().join(".git/info/fast-import/rel")).expect("mkdir git rel");
    fs::create_dir_all(zmin_repo.path().join(".git/info/fast-import/rel")).expect("mkdir zmin rel");
    fs::write(
        git_repo
            .path()
            .join(".git/info/fast-import/rel/child.marks"),
        format!(":1 {git_blob}\n"),
    )
    .expect("write git child");
    fs::write(
        zmin_repo
            .path()
            .join(".git/info/fast-import/rel/child.marks"),
        format!(":1 {zmin_blob}\n"),
    )
    .expect("write zmin child");
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &[
            "fast-import",
            "--relative-marks",
            "--import-marks=rel/child.marks",
        ],
        import_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "fast-import",
            "--relative-marks",
            "--import-marks=rel/child.marks",
        ],
        import_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["cat-file", "-p", "refs/heads/main:a.txt"]
        ),
        git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );

    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &[
            "fast-import",
            "--relative-marks",
            "--import-marks-if-exists=missing.marks",
        ],
        inline_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "fast-import",
            "--relative-marks",
            "--import-marks-if-exists=missing.marks",
        ],
        inline_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["cat-file", "-p", "refs/heads/main:a.txt"]
        ),
        git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );

    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &[
            "fast-import",
            "--relative-marks",
            "--export-marks=out.marks",
            "--no-relative-marks",
            "--export-marks=tail.marks",
        ],
        inline_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "fast-import",
            "--relative-marks",
            "--export-marks=out.marks",
            "--no-relative-marks",
            "--export-marks=tail.marks",
        ],
        inline_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("tail.marks")).expect("read zmin tail"),
        fs::read_to_string(git_repo.path().join("tail.marks")).expect("read git tail")
    );
}

#[test]
fn fast_import_relative_marks_stream_features_match_stock_git() {
    let relative_export_stream = "\
feature relative-marks
feature export-marks=marks.txt
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        relative_export_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        relative_export_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/info/fast-import/marks.txt"))
            .expect("read zmin relative feature marks"),
        fs::read_to_string(git_repo.path().join(".git/info/fast-import/marks.txt"))
            .expect("read git relative feature marks")
    );

    let relative_import_stream = "\
feature relative-marks
feature import-marks=rel/child.marks
commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_blob = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    let zmin_blob = command_with_stdin_output(
        "git",
        zmin_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    fs::create_dir_all(git_repo.path().join(".git/info/fast-import/rel"))
        .expect("mkdir git feature rel");
    fs::create_dir_all(zmin_repo.path().join(".git/info/fast-import/rel"))
        .expect("mkdir zmin feature rel");
    fs::write(
        git_repo
            .path()
            .join(".git/info/fast-import/rel/child.marks"),
        format!(":1 {git_blob}\n"),
    )
    .expect("write git feature child");
    fs::write(
        zmin_repo
            .path()
            .join(".git/info/fast-import/rel/child.marks"),
        format!(":1 {zmin_blob}\n"),
    )
    .expect("write zmin feature child");
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        relative_import_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        relative_import_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["cat-file", "-p", "refs/heads/main:a.txt"]
        ),
        git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );

    let non_retroactive_stream = "\
feature import-marks=plain.marks
feature relative-marks
commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_blob = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    let zmin_blob = command_with_stdin_output(
        "git",
        zmin_repo.path(),
        &["hash-object", "-w", "--stdin"],
        "hello\n",
    )
    .1
    .trim()
    .to_owned();
    fs::write(
        git_repo.path().join("plain.marks"),
        format!(":1 {git_blob}\n"),
    )
    .expect("write git feature plain");
    fs::write(
        zmin_repo.path().join("plain.marks"),
        format!(":1 {zmin_blob}\n"),
    )
    .expect("write zmin feature plain");
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        non_retroactive_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        non_retroactive_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["cat-file", "-p", "refs/heads/main:a.txt"]
        ),
        git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );

    let missing_if_exists_stream = "\
feature relative-marks
feature import-marks-if-exists=missing.marks
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        missing_if_exists_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        missing_if_exists_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["cat-file", "-p", "refs/heads/main:a.txt"]
        ),
        git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );

    let toggle_export_stream = "\
feature relative-marks
feature export-marks=one.marks
feature no-relative-marks
feature export-marks=two.marks
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        toggle_export_stream,
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--allow-unsafe-features"],
        toggle_export_stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_statistics_stderr(&zmin_output.2),
        normalize_fast_import_statistics_stderr(&git_output.2)
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("two.marks")).expect("read zmin two.marks"),
        fs::read_to_string(git_repo.path().join("two.marks")).expect("read git two.marks")
    );
}

#[test]
fn fast_import_done_flag_requires_done_terminator_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stream = "\
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

";
    let git_output =
        command_with_stdin_output("git", git_repo.path(), &["fast-import", "--done"], stream);
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--done"],
        stream,
    );
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_crash_stderr(&zmin_output.2, "fatal: stream ends early"),
        normalize_fast_import_crash_stderr(&git_output.2, "fatal: stream ends early")
    );
}

#[test]
fn fast_import_relative_marks_and_rewrite_submodules_fail_like_stock_git() {
    let stream = "\
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";

    for (args, fatal) in [
        (
            &["fast-import", "--relative-marks=rel"][..],
            "fatal: unknown option --relative-marks=rel",
        ),
        (
            &["fast-import", "--rewrite-submodules-from=a:b"][..],
            "fatal: cannot read 'b': No such file or directory",
        ),
        (
            &["fast-import", "--rewrite-submodules-to=a:b"][..],
            "fatal: cannot read 'b': No such file or directory",
        ),
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let git_output = command_with_stdin_output("git", git_repo.path(), args, stream);
        let zmin_output = command_with_stdin_output(zmin_bin(), zmin_repo.path(), args, stream);
        assert_eq!(zmin_output.0, git_output.0, "exit for {args:?}");
        assert_eq!(zmin_output.1, git_output.1, "stdout for {args:?}");
        assert_eq!(
            normalize_fast_import_crash_stderr(&zmin_output.2, fatal),
            normalize_fast_import_crash_stderr(&git_output.2, fatal),
            "stderr for {args:?}"
        );
    }
}

#[test]
fn fast_import_invalid_date_format_matches_stock_git_crash_shape() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let git_output = command_with_stdin_output(
        "git",
        git_repo.path(),
        &["fast-import", "--date-format=bogus"],
        "",
    );
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--date-format=bogus"],
        "",
    );

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_crash_stderr(
            &zmin_output.2,
            "fatal: unknown --date-format argument bogus"
        ),
        normalize_fast_import_crash_stderr(
            &git_output.2,
            "fatal: unknown --date-format argument bogus"
        )
    );

    let git_reports = fast_import_crash_reports(git_repo.path());
    let zmin_reports = fast_import_crash_reports(zmin_repo.path());
    assert_eq!(git_reports.len(), 1);
    assert_eq!(zmin_reports.len(), 1);
    assert_eq!(
        normalize_fast_import_crash_report(
            &git_reports,
            "fatal: unknown --date-format argument bogus",
            None,
        ),
        normalize_fast_import_crash_report(
            &zmin_reports,
            "fatal: unknown --date-format argument bogus",
            None,
        )
    );
    for report in [git_reports[0].as_str(), zmin_reports[0].as_str()] {
        assert!(report.contains("fast-import crash report:"));
        assert!(report.contains("fatal: unknown --date-format argument bogus"));
        assert!(report.contains("Most Recent Commands Before Crash"));
        assert!(report.contains("END OF CRASH REPORT"));
    }
}

#[test]
fn fast_import_unknown_top_level_command_matches_stock_git_crash_shape() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    let git_output = command_with_stdin_output("git", git_repo.path(), &["fast-import"], "bogus\n");
    let zmin_output =
        command_with_stdin_output(zmin_bin(), zmin_repo.path(), &["fast-import"], "bogus\n");

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_crash_stderr(&zmin_output.2, "fatal: unsupported command: bogus"),
        normalize_fast_import_crash_stderr(&git_output.2, "fatal: unsupported command: bogus")
    );

    let git_reports = fast_import_crash_reports(git_repo.path());
    let zmin_reports = fast_import_crash_reports(zmin_repo.path());
    assert_eq!(git_reports.len(), 1);
    assert_eq!(zmin_reports.len(), 1);
    assert_eq!(
        normalize_fast_import_crash_report(
            &git_reports,
            "fatal: unsupported command: bogus",
            Some("bogus"),
        ),
        normalize_fast_import_crash_report(
            &zmin_reports,
            "fatal: unsupported command: bogus",
            Some("bogus"),
        )
    );
    for report in [git_reports[0].as_str(), zmin_reports[0].as_str()] {
        assert!(report.contains("fast-import crash report:"));
        assert!(report.contains("fatal: unsupported command: bogus"));
        assert!(report.contains("* bogus"));
        assert!(report.contains("END OF CRASH REPORT"));
    }
}

fn assert_fast_import_crash_stream_matches_stock(
    stream: &str,
    expected_fatal: &str,
    expected_command: &str,
) {
    assert_fast_import_crash_stream_with_unpack_limit(
        stream,
        expected_fatal,
        expected_command,
        None,
    );
}

fn assert_fast_import_crash_stream_with_unpack_limit(
    stream: &str,
    expected_fatal: &str,
    expected_command: &str,
    unpack_limit: Option<&str>,
) {
    let git_repo = git_init();
    let zmin_repo = git_init();
    if let Some(unpack_limit) = unpack_limit {
        git(
            git_repo.path(),
            ["config", "fastimport.unpackLimit", unpack_limit],
        );
        git(
            zmin_repo.path(),
            ["config", "fastimport.unpackLimit", unpack_limit],
        );
    }
    let git_output =
        command_with_stdin_output("git", git_repo.path(), &["fast-import", "--quiet"], stream);
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--quiet"],
        stream,
    );

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_crash_stderr(&zmin_output.2, expected_fatal),
        normalize_fast_import_crash_stderr(&git_output.2, expected_fatal)
    );
    let git_reports = fast_import_crash_reports(git_repo.path());
    let zmin_reports = fast_import_crash_reports(zmin_repo.path());
    assert_eq!(
        normalize_fast_import_crash_report(&zmin_reports, expected_fatal, Some(expected_command)),
        normalize_fast_import_crash_report(&git_reports, expected_fatal, Some(expected_command))
    );
}

#[test]
fn fast_import_new_branch_crash_state_matches_stock_git() {
    assert_fast_import_crash_stream_matches_stock(
        "commit refs/heads/topic\n\
committer A <a@example.test> 0 +0000\n\
data 0\n\
bogus\n",
        "fatal: unsupported command: bogus",
        "bogus",
    );
}

#[test]
fn fast_import_reset_created_branch_crash_state_matches_stock_git() {
    assert_fast_import_crash_stream_matches_stock(
        "reset refs/heads/new\n\
bogus\n",
        "fatal: unsupported command: bogus",
        "bogus",
    );
}

#[test]
fn fast_import_reset_existing_branch_crash_state_matches_stock_git() {
    let prefix = "blob\n\
mark :1\n\
data 4\n\
foo\n\
\n\
commit refs/heads/main\n\
mark :2\n\
committer A <a@example.test> 0 +0000\n\
data 0\n\
M 100644 :1 a\n\
";
    for suffix in [
        "reset refs/heads/main\nbogus\n",
        "reset refs/heads/main\nfrom :2\nbogus\n",
        "reset refs/heads/new\nfrom :2\nbogus\n",
    ] {
        assert_fast_import_crash_stream_matches_stock(
            &format!("{prefix}{suffix}"),
            "fatal: unsupported command: bogus",
            "bogus",
        );
    }
}

#[test]
fn fast_import_branch_lru_and_global_clock_match_stock_git() {
    assert_fast_import_crash_stream_matches_stock(
        "commit refs/heads/main\n\
committer A <a@example.test> 0 +0000\n\
data 0\n\
\n\
commit refs/heads/topic\n\
committer A <a@example.test> 1 +0000\n\
data 0\n\
\n\
commit refs/heads/main\n\
committer A <a@example.test> 2 +0000\n\
data 0\n\
bogus\n",
        "fatal: unsupported command: bogus",
        "bogus",
    );
}

#[test]
fn fast_import_branch_lru_bound_matches_stock_git() {
    let mut stream = String::new();
    for branch in 0..6 {
        stream.push_str(&format!(
            "commit refs/heads/b{branch}\ncommitter A <a@example.test> {branch} +0000\ndata 0\n\n"
        ));
    }
    stream.push_str(
        "commit refs/heads/b5\n\
committer A <a@example.test> 6 +0000\n\
data 0\n\
bogus\n",
    );
    assert_fast_import_crash_stream_matches_stock(
        &stream,
        "fatal: unsupported command: bogus",
        "bogus",
    );
}

#[test]
fn fast_import_duplicate_commit_clock_matches_stock_git() {
    assert_fast_import_crash_stream_matches_stock(
        "commit refs/heads/main\n\
committer A <a@example.test> 0 +0000\n\
data 0\n\
\n\
commit refs/heads/topic\n\
committer A <a@example.test> 0 +0000\n\
data 0\n\
bogus\n",
        "fatal: unsupported command: bogus",
        "bogus",
    );
}

#[test]
fn fast_import_checkpoint_crash_pack_generation_matches_stock_git() {
    assert_fast_import_crash_stream_matches_stock(
        "commit refs/heads/main\n\
committer A <a@example.test> 0 +0000\n\
data 0\n\
\n\
checkpoint\n\
commit refs/heads/topic\n\
committer A <a@example.test> 1 +0000\n\
data 0\n\
bogus\n",
        "fatal: unsupported command: bogus",
        "bogus",
    );
}

#[test]
fn fast_import_checkpoint_crash_published_pack_id_matches_stock_git() {
    assert_fast_import_crash_stream_with_unpack_limit(
        "commit refs/heads/main\n\
committer A <a@example.test> 0 +0000\n\
data 0\n\
\n\
checkpoint\n\
commit refs/heads/topic\n\
committer A <a@example.test> 1 +0000\n\
data 0\n\
bogus\n",
        "fatal: unsupported command: bogus",
        "bogus",
        Some("0"),
    );
}

#[test]
fn fast_import_malformed_commit_mark_prefix_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stream = "commit refs/heads/main\n\
mark :403x\n\
committer A <a@example.test> 0 +0000\n\
data 0\n\
\n\
done\n";
    let git_output =
        command_with_stdin_output("git", git_repo.path(), &["fast-import", "--quiet"], stream);
    let zmin_output = command_with_stdin_output(
        zmin_bin(),
        zmin_repo.path(),
        &["fast-import", "--quiet"],
        stream,
    );
    assert_eq!(zmin_output, git_output);
    assert_eq!(
        git(git_repo.path(), ["rev-parse", "refs/heads/main"]),
        git(zmin_repo.path(), ["rev-parse", "refs/heads/main"])
    );
}

#[test]
fn fast_import_early_commit_failures_include_registered_branch_state() {
    for (stream, expected_fatal, expected_command) in [
        (
            "commit refs/heads/main\n\
committer malformed\n",
            "fatal: missing < in ident string: malformed",
            "committer malformed",
        ),
        (
            "commit refs/heads/main\n\
committer A <a@example.test> 0 +0000\n",
            "fatal: expected 'data n' command, found: ",
            "committer A <a@example.test> 0 +0000",
        ),
        (
            "commit refs/heads/main\n\
committer A <a@example.test> 0 +0000\n\
data 0\n\
from :99\n",
            "fatal: mark :99 not declared",
            "from :99",
        ),
        (
            "commit refs/heads/main\n\
committer A <a@example.test> 0 +0000\n\
data 0\n\
from refs/heads/no-such\n",
            "fatal: invalid ref name or SHA1 expression: refs/heads/no-such",
            "from refs/heads/no-such",
        ),
        (
            "blob\n\
mark :1\n\
data 4\n\
foo\n\
\n\
commit refs/heads/main\n\
committer A <a@example.test> 0 +0000\n\
data 0\n\
M 100644 :1 a\n\
M malformed\n",
            "fatal: corrupt mode: M malformed",
            "M malformed",
        ),
        (
            "blob\n\
mark :1\n\
data 4\n\
foo\n\
\n\
commit refs/heads/main\n\
committer A <a@example.test> 0 +0000\n\
data 0\n\
M 100644 :1 a\n\
D\n",
            "fatal: unsupported command: D",
            "D",
        ),
    ] {
        assert_fast_import_crash_stream_matches_stock(stream, expected_fatal, expected_command);
    }
}

#[test]
fn fast_import_unknown_commit_command_matches_stock_git_crash_shape() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let stream = "\
commit refs/heads/main
committer A <a@example.test> 0 +0000
data 0
bogus
";

    let git_output = command_with_stdin_output("git", git_repo.path(), &["fast-import"], stream);
    let zmin_output =
        command_with_stdin_output(zmin_bin(), zmin_repo.path(), &["fast-import"], stream);

    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(
        normalize_fast_import_crash_stderr(&zmin_output.2, "fatal: unsupported command: bogus"),
        normalize_fast_import_crash_stderr(&git_output.2, "fatal: unsupported command: bogus")
    );

    let git_reports = fast_import_crash_reports(git_repo.path());
    let zmin_reports = fast_import_crash_reports(zmin_repo.path());
    assert_eq!(git_reports.len(), 1);
    assert_eq!(zmin_reports.len(), 1);
    assert_eq!(
        normalize_fast_import_crash_report(
            &git_reports,
            "fatal: unsupported command: bogus",
            Some("bogus"),
        ),
        normalize_fast_import_crash_report(
            &zmin_reports,
            "fatal: unsupported command: bogus",
            Some("bogus"),
        )
    );
    for report in [git_reports[0].as_str(), zmin_reports[0].as_str()] {
        assert!(report.contains("fast-import crash report:"));
        assert!(report.contains("fatal: unsupported command: bogus"));
        assert!(report.contains("* bogus"));
        assert!(report.contains("END OF CRASH REPORT"));
    }
    assert_eq!(
        git_status(
            git_repo.path(),
            ["rev-parse", "--verify", "refs/heads/main"]
        ),
        128
    );
    assert_eq!(
        git_status(
            zmin_repo.path(),
            ["rev-parse", "--verify", "refs/heads/main"]
        ),
        128
    );
    assert_eq!(
        loose_object_file_count(zmin_repo.path()),
        loose_object_file_count(git_repo.path())
    );
}

#[test]
fn fast_import_additional_value_and_order_families_match_stock_git() {
    let raw_date_stream = "\
commit refs/heads/main
author A U Thor <author@example.test> 0 +0000
committer C O Mitter <committer@example.test> 1 +0000
data <<EOF
raw date
EOF
M 100644 inline a.txt
data <<EOF
contents
EOF
";
    for args in [
        &["fast-import", "--date-format=raw"][..],
        &["fast-import", "--date-format=raw-permissive"],
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let git_output = command_with_stdin_output("git", git_repo.path(), args, raw_date_stream);
        let zmin_output =
            command_with_stdin_output(zmin_bin(), zmin_repo.path(), args, raw_date_stream);
        assert_eq!(zmin_output.0, git_output.0, "exit for {args:?}");
        assert_eq!(zmin_output.1, git_output.1, "stdout for {args:?}");
        assert_eq!(
            normalize_fast_import_statistics_stderr(&zmin_output.2),
            normalize_fast_import_statistics_stderr(&git_output.2),
            "stderr for {args:?}"
        );
        assert_eq!(
            git(
                zmin_repo.path(),
                ["cat-file", "-p", "refs/heads/main:a.txt"]
            ),
            git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"]),
            "imported content for {args:?}"
        );
    }

    let stream = "\
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";

    for args in [
        &["fast-import", "--active-branches=0"][..],
        &["fast-import", "--active-branches=2"],
        &["fast-import", "--depth=2"],
        &["fast-import", "--big-file-threshold=2"],
        &["fast-import", "--cat-blob-fd=0"],
        &["fast-import", "--cat-blob-fd=1"],
        &["fast-import", "--quiet", "--stats"],
        &["fast-import", "--done", "--stats"],
        &["fast-import", "--done", "--quiet"],
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let git_output = command_with_stdin_output("git", git_repo.path(), args, stream);
        let zmin_output = command_with_stdin_output(zmin_bin(), zmin_repo.path(), args, stream);
        assert_eq!(zmin_output.0, git_output.0, "exit for {args:?}");
        assert_eq!(zmin_output.1, git_output.1, "stdout for {args:?}");
        if args == ["fast-import", "--done", "--quiet"] {
            assert_eq!(zmin_output.2, git_output.2, "stderr for {args:?}");
            assert!(
                zmin_output.2.is_empty(),
                "quiet should suppress stats for {args:?}"
            );
        } else {
            assert_eq!(
                normalize_fast_import_statistics_stderr(&zmin_output.2),
                normalize_fast_import_statistics_stderr(&git_output.2),
                "stderr for {args:?}"
            );
        }
        assert_eq!(
            git(
                zmin_repo.path(),
                ["cat-file", "-p", "refs/heads/main:a.txt"]
            ),
            git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"]),
            "imported content for {args:?}"
        );
    }

    for args in [&["fast-import", "--stats", "--quiet"][..]] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let git_output = command_with_stdin_output("git", git_repo.path(), args, stream);
        let zmin_output = command_with_stdin_output(zmin_bin(), zmin_repo.path(), args, stream);
        assert_eq!(zmin_output.0, git_output.0, "exit for {args:?}");
        assert_eq!(zmin_output.1, git_output.1, "stdout for {args:?}");
        assert_eq!(zmin_output.2, git_output.2, "stderr for {args:?}");
        assert!(
            zmin_output.2.is_empty(),
            "quiet should suppress stats for {args:?}"
        );
        assert_eq!(
            git(
                zmin_repo.path(),
                ["cat-file", "-p", "refs/heads/main:a.txt"]
            ),
            git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"]),
            "imported content for {args:?}"
        );
    }
}

#[test]
fn fast_import_repeated_option_families_match_stock_git() {
    let raw_date_stream = "\
commit refs/heads/main
author A U Thor <author@example.test> 0 +0000
committer C O Mitter <committer@example.test> 1 +0000
data <<EOF
raw date
EOF
M 100644 inline a.txt
data <<EOF
contents
EOF
";
    for args in [
        &[
            "fast-import",
            "--date-format=raw",
            "--date-format=raw-permissive",
        ][..],
        &[
            "fast-import",
            "--date-format=raw-permissive",
            "--date-format=raw",
        ],
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let git_output = command_with_stdin_output("git", git_repo.path(), args, raw_date_stream);
        let zmin_output =
            command_with_stdin_output(zmin_bin(), zmin_repo.path(), args, raw_date_stream);
        assert_eq!(zmin_output.0, git_output.0, "exit for {args:?}");
        assert_eq!(zmin_output.1, git_output.1, "stdout for {args:?}");
        assert_eq!(
            normalize_fast_import_statistics_stderr(&zmin_output.2),
            normalize_fast_import_statistics_stderr(&git_output.2),
            "stderr for {args:?}"
        );
        assert_eq!(
            git(
                zmin_repo.path(),
                ["cat-file", "-p", "refs/heads/main:a.txt"]
            ),
            git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"]),
            "imported content for {args:?}"
        );
    }

    let stream = "\
blob
mark :1
data 6
hello

commit refs/heads/main
committer A <a@example.test> 0 +0000
data 8
initial
M 100644 :1 a.txt

done
";

    for args in [
        &["fast-import", "--done", "--done"][..],
        &["fast-import", "--force", "--force"],
        &[
            "fast-import",
            "--allow-unsafe-features",
            "--allow-unsafe-features",
        ],
        &["fast-import", "--active-branches=0", "--active-branches=2"],
        &["fast-import", "--active-branches=2", "--active-branches=0"],
        &["fast-import", "--depth=1", "--depth=2"],
        &["fast-import", "--depth=2", "--depth=1"],
        &[
            "fast-import",
            "--big-file-threshold=1",
            "--big-file-threshold=2",
        ],
        &["fast-import", "--cat-blob-fd=9", "--cat-blob-fd=0"],
        &["fast-import", "--cat-blob-fd=0", "--cat-blob-fd=9"],
        &["fast-import", "--stats", "--quiet", "--stats"],
    ] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let git_output = command_with_stdin_output("git", git_repo.path(), args, stream);
        let zmin_output = command_with_stdin_output(zmin_bin(), zmin_repo.path(), args, stream);
        assert_eq!(zmin_output.0, git_output.0, "exit for {args:?}");
        assert_eq!(zmin_output.1, git_output.1, "stdout for {args:?}");
        assert_eq!(
            normalize_fast_import_statistics_stderr(&zmin_output.2),
            normalize_fast_import_statistics_stderr(&git_output.2),
            "stderr for {args:?}"
        );
        assert_eq!(
            git(
                zmin_repo.path(),
                ["cat-file", "-p", "refs/heads/main:a.txt"]
            ),
            git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"]),
            "imported content for {args:?}"
        );
    }

    for args in [&["fast-import", "--quiet", "--stats", "--quiet"][..]] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let git_output = command_with_stdin_output("git", git_repo.path(), args, stream);
        let zmin_output = command_with_stdin_output(zmin_bin(), zmin_repo.path(), args, stream);
        assert_eq!(zmin_output.0, git_output.0, "exit for {args:?}");
        assert_eq!(zmin_output.1, git_output.1, "stdout for {args:?}");
        assert_eq!(zmin_output.2, git_output.2, "stderr for {args:?}");
        assert!(
            zmin_output.2.is_empty(),
            "quiet should suppress stats for {args:?}"
        );
        assert_eq!(
            git(
                zmin_repo.path(),
                ["cat-file", "-p", "refs/heads/main:a.txt"]
            ),
            git(git_repo.path(), ["cat-file", "-p", "refs/heads/main:a.txt"]),
            "imported content for {args:?}"
        );
    }
}
