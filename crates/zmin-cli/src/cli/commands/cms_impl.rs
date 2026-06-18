use super::*;
use std::ffi::OsString;

struct CommandOutput {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

pub(crate) fn save(message: &str) -> Result<()> {
    if message.trim().is_empty() {
        return Err(CliError::Fatal {
            code: 1,
            message: "save message cannot be empty".into(),
        });
    }
    if status_lines()?.is_empty() {
        println!("Nothing to save.");
        return Ok(());
    }
    run_child_checked(&["add", "-A"])?;
    let output = run_child(&["commit", "-m", message])?;
    if output.code == 0 {
        let commit_id = child_stdout_string(&["rev-parse", "HEAD"])?;
        append_operation_log("save", &commit_id, message)?;
        println!("Saved: {message}");
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("nothing to commit") || stderr.contains("no changes added to commit") {
        println!("Nothing to save.");
        return Ok(());
    }
    write_child_output(&output)?;
    Err(CliError::Exit(output.code))
}

pub(crate) fn publish() -> Result<()> {
    ensure_clean_for_remote_operation("publish")?;
    run_child_checked(&["push"])?;
    println!("Published.");
    Ok(())
}

pub(crate) fn update() -> Result<()> {
    ensure_clean_for_remote_operation("update")?;
    run_child_checked(&["pull", "--ff-only"])?;
    println!("Updated.");
    Ok(())
}

pub(crate) fn undo() -> Result<()> {
    if !status_lines()?.is_empty() {
        return Err(CliError::Fatal {
            code: 1,
            message: "save or discard changes before undo".into(),
        });
    }
    let operation = last_operation()?.ok_or_else(|| CliError::Fatal {
        code: 1,
        message: "nothing to undo".into(),
    })?;
    if operation.kind != "save" {
        return Err(CliError::Fatal {
            code: 1,
            message: format!("cannot undo operation '{}'", operation.kind),
        });
    }
    let head = child_stdout_string(&["rev-parse", "HEAD"])?;
    if head != operation.commit_id {
        return Err(CliError::Fatal {
            code: 1,
            message: "last saved change is no longer current".into(),
        });
    }
    let subject = child_stdout_string(&["log", "--format=%s", "--max-count=1", "HEAD"])?;
    let parents = child_stdout_string(&["rev-list", "--parents", "--max-count=1", "HEAD"])?;
    let parent_count = parents.split_whitespace().count().saturating_sub(1);
    if parent_count == 0 {
        run_child_checked(&["update-ref", "-d", "HEAD"])?;
    } else {
        run_child_checked(&["reset", "--mixed", "HEAD^"])?;
    }
    pop_last_operation()?;
    println!("Undid save: {subject}");
    Ok(())
}

pub(crate) fn changes() -> Result<()> {
    let lines = status_lines()?;
    if lines.is_empty() {
        println!("No changes.");
        return Ok(());
    }
    println!("Changes:");
    for line in lines {
        println!("{}", render_status_line(&line));
    }
    Ok(())
}

pub(crate) fn timeline() -> Result<()> {
    let output = run_child(&["log", "--oneline", "--max-count=10"])?;
    if output.code != 0 {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("does not have any commits")
            || stderr.contains("bad default revision")
            || stderr.contains("ambiguous argument 'HEAD'")
            || stderr.contains("unknown revision")
        {
            println!("No history.");
            return Ok(());
        }
        write_child_output(&output)?;
        return Err(CliError::Exit(output.code));
    }
    let stdout = String::from_utf8(output.stdout).map_err(|error| CliError::Fatal {
        code: 1,
        message: format!("log output was not UTF-8: {error}"),
    })?;
    let entries = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if entries.is_empty() {
        println!("No history.");
        return Ok(());
    }
    println!("History:");
    for entry in entries {
        let mut fields = entry.splitn(2, ' ');
        let id = fields.next().unwrap_or("");
        let subject = fields.next().unwrap_or("");
        if subject.is_empty() {
            println!("{id}");
        } else {
            println!("{id}  {subject}");
        }
    }
    Ok(())
}

pub(crate) fn recover(paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Err(CliError::Fatal {
            code: 1,
            message: "recover needs at least one path".into(),
        });
    }
    let lines = status_lines()?;
    for path in paths {
        if path_has_staged_changes(&lines, path) {
            return Err(CliError::Fatal {
                code: 1,
                message: format!("refusing to recover staged changes in {}", path.display()),
            });
        }
    }
    let mut args = Vec::<OsString>::new();
    args.push(OsString::from("restore"));
    args.push(OsString::from("--worktree"));
    args.push(OsString::from("--"));
    args.extend(paths.iter().map(|path| path.as_os_str().to_owned()));
    run_child_os_checked(&args)?;
    for path in paths {
        println!("Recovered: {}", path.display());
    }
    Ok(())
}

fn ensure_clean_for_remote_operation(operation: &str) -> Result<()> {
    if status_lines()?.is_empty() {
        return Ok(());
    }
    Err(CliError::Fatal {
        code: 1,
        message: format!("save or discard changes before {operation}"),
    })
}

fn status_lines() -> Result<Vec<String>> {
    let output = run_child(&["status", "--porcelain=v1", "--branch"])?;
    if output.code != 0 {
        write_child_output(&output)?;
        return Err(CliError::Exit(output.code));
    }
    let stdout = String::from_utf8(output.stdout).map_err(|error| CliError::Fatal {
        code: 1,
        message: format!("status output was not UTF-8: {error}"),
    })?;
    Ok(stdout
        .lines()
        .filter(|line| !line.starts_with("##"))
        .map(str::to_owned)
        .collect())
}

fn path_has_staged_changes(lines: &[String], path: &Path) -> bool {
    let Some(path) = path.to_str() else {
        return false;
    };
    lines.iter().any(|line| {
        if line.len() < 3 {
            return false;
        }
        let index_status = line.as_bytes()[0] as char;
        index_status != ' ' && !line.starts_with("??") && status_line_path(line) == path
    })
}

fn status_line_path(line: &str) -> &str {
    line.get(3..).unwrap_or("").trim()
}

struct CmsOperation {
    kind: String,
    commit_id: String,
}

fn append_operation_log(kind: &str, commit_id: &str, message: &str) -> Result<()> {
    let repo = find_repo()?;
    let log_path = operation_log_path(&repo);
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let sanitized_message = message.replace(['\n', '\r', '\t'], " ");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    writeln!(file, "{kind}\t{commit_id}\t{sanitized_message}")?;
    Ok(())
}

fn last_operation() -> Result<Option<CmsOperation>> {
    let repo = find_repo()?;
    let log_path = operation_log_path(&repo);
    if !log_path.is_file() {
        return Ok(None);
    }
    let contents = fs::read_to_string(log_path)?;
    let Some(line) = contents.lines().rev().find(|line| !line.trim().is_empty()) else {
        return Ok(None);
    };
    let mut fields = line.splitn(3, '\t');
    let Some(kind) = fields.next() else {
        return Ok(None);
    };
    let Some(commit_id) = fields.next() else {
        return Ok(None);
    };
    Ok(Some(CmsOperation {
        kind: kind.to_owned(),
        commit_id: commit_id.to_owned(),
    }))
}

fn pop_last_operation() -> Result<()> {
    let repo = find_repo()?;
    let log_path = operation_log_path(&repo);
    let contents = fs::read_to_string(&log_path)?;
    let mut lines = contents.lines().collect::<Vec<_>>();
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        fs::write(log_path, "")?;
        return Ok(());
    }
    lines.pop();
    let mut updated = lines.join("\n");
    if !updated.is_empty() {
        updated.push('\n');
    }
    fs::write(log_path, updated)?;
    Ok(())
}

fn operation_log_path(repo: &GitRepo) -> PathBuf {
    repo.git_dir.join("zmin").join("operations.log")
}

fn render_status_line(line: &str) -> String {
    if line.len() < 3 {
        return format!("changed: {line}");
    }
    let code = &line[..2];
    let path = line[3..].trim();
    let label = if code.contains('?') {
        "new"
    } else if code.contains('D') {
        "deleted"
    } else if code.contains('R') {
        "renamed"
    } else if code.contains('A') {
        "added"
    } else if code.contains('M') {
        "modified"
    } else {
        "changed"
    };
    format!("{label}: {path}")
}

fn run_child_checked(args: &[&str]) -> Result<()> {
    let output = run_child(args)?;
    if output.code == 0 {
        return Ok(());
    }
    write_child_output(&output)?;
    Err(CliError::Exit(output.code))
}

fn child_stdout_string(args: &[&str]) -> Result<String> {
    let output = run_child(args)?;
    if output.code != 0 {
        write_child_output(&output)?;
        return Err(CliError::Exit(output.code));
    }
    let stdout = String::from_utf8(output.stdout).map_err(|error| CliError::Fatal {
        code: 1,
        message: format!("command output was not UTF-8: {error}"),
    })?;
    Ok(stdout.trim_end_matches(['\n', '\r']).to_owned())
}

fn run_child_os_checked(args: &[OsString]) -> Result<()> {
    let output = run_child_os(args)?;
    if output.code == 0 {
        return Ok(());
    }
    write_child_output(&output)?;
    Err(CliError::Exit(output.code))
}

fn run_child(args: &[&str]) -> Result<CommandOutput> {
    let output = ProcessCommand::new(std::env::current_exe()?)
        .args(args)
        .output()?;
    Ok(command_output_from_process(output))
}

fn run_child_os(args: &[OsString]) -> Result<CommandOutput> {
    let output = ProcessCommand::new(std::env::current_exe()?)
        .args(args)
        .output()?;
    Ok(command_output_from_process(output))
}

fn command_output_from_process(output: std::process::Output) -> CommandOutput {
    CommandOutput {
        code: output.status.code().unwrap_or(1),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

fn write_child_output(output: &CommandOutput) -> Result<()> {
    io::stdout().write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;
    Ok(())
}
