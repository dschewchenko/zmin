use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command as ProcessCommand;

use skron_git_core::Signature;

use super::{
    CliError, GitRepo, Result, find_repo, parse_config_name, read_config_file, read_config_value,
    read_local_config_entries, system_config_paths, unique_temp_sibling,
};

pub(crate) fn edit_history_message(repo: &GitRepo, message: &[u8]) -> Result<Vec<u8>> {
    edit_temp_buffer(repo, "HISTORY_EDITMSG", message, true)
}

pub(crate) fn edit_temp_buffer(
    repo: &GitRepo,
    temp_name: &str,
    message: &[u8],
    append_newline: bool,
) -> Result<Vec<u8>> {
    let editor = git_editor(repo)?.ok_or_else(editor_required_message_error)?;
    let path = unique_temp_sibling(&repo.git_dir.join(temp_name));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(message)?;
    file.flush()?;
    drop(file);
    let status = run_editor_command_with_path(&editor, &path)?;
    if !status.success() {
        let _ = fs::remove_file(&path);
        return Err(CliError::Fatal {
            code: status.code().unwrap_or(1),
            message: "editor failed".into(),
        });
    }
    let mut edited = fs::read(&path)?;
    let _ = fs::remove_file(&path);
    if append_newline && !edited.is_empty() && !edited.ends_with(b"\n") {
        edited.push(b'\n');
    }
    Ok(edited)
}

pub(crate) fn read_multi_config_values(name: &str) -> Result<Vec<String>> {
    let (section, subsection, key) = parse_config_name(name)?;
    let mut entries = Vec::new();
    for path in system_config_paths() {
        entries.extend(read_config_file(&path)?);
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        entries.extend(read_config_file(&home.join(".gitconfig"))?);
        entries.extend(read_config_file(&home.join(".config/git/config"))?);
    }
    if let Ok(repo) = find_repo() {
        entries.extend(read_local_config_entries(&repo)?);
    }
    Ok(entries
        .into_iter()
        .filter(|entry| {
            entry.section == section && entry.subsection == subsection && entry.key == key
        })
        .map(|entry| entry.value)
        .collect())
}

pub(crate) fn git_editor(repo: &GitRepo) -> Result<Option<String>> {
    Ok(std::env::var("GIT_EDITOR")
        .ok()
        .or_else(|| read_config_value(repo, "core.editor").ok().flatten())
        .or_else(|| std::env::var("VISUAL").ok())
        .or_else(|| std::env::var("EDITOR").ok())
        .or_else(default_git_editor))
}

pub(crate) fn git_sequence_editor(repo: &GitRepo) -> Result<Option<String>> {
    Ok(std::env::var("GIT_SEQUENCE_EDITOR")
        .ok()
        .or_else(|| read_config_value(repo, "sequence.editor").ok().flatten())
        .or_else(|| git_editor(repo).ok().flatten()))
}

pub(crate) fn git_pager(repo: &GitRepo) -> Result<String> {
    Ok(std::env::var("GIT_PAGER")
        .ok()
        .or_else(|| read_config_value(repo, "core.pager").ok().flatten())
        .or_else(|| std::env::var("PAGER").ok())
        .unwrap_or_else(default_git_pager))
}

pub(crate) fn git_shell_path() -> &'static str {
    #[cfg(windows)]
    {
        "C:/Program Files/Git/usr/bin/sh.exe"
    }
    #[cfg(not(windows))]
    {
        "/bin/sh"
    }
}

fn default_git_editor() -> Option<String> {
    #[cfg(windows)]
    {
        Some("vim".to_owned())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn default_git_pager() -> String {
    #[cfg(windows)]
    {
        "less".to_owned()
    }
    #[cfg(not(windows))]
    {
        "cat".to_owned()
    }
}

pub(crate) fn git_shell_command_path() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(path) = find_executable_on_path("sh.exe") {
            return path;
        }
        for path in [
            r"C:\Program Files\Git\usr\bin\sh.exe",
            r"C:\Program Files\Git\bin\sh.exe",
        ] {
            let path = PathBuf::from(path);
            if path.is_file() {
                return path;
            }
        }
    }
    PathBuf::from(git_shell_path())
}

pub(crate) fn run_editor_command_with_path(
    command: &str,
    path: &std::path::Path,
) -> io::Result<std::process::ExitStatus> {
    #[cfg(windows)]
    {
        let command_path = std::path::Path::new(command);
        if command_path.extension().and_then(|ext| ext.to_str()) == Some("sh")
            && command_path.is_file()
        {
            return ProcessCommand::new(git_shell_command_path())
                .arg(command_path)
                .arg(path)
                .status();
        }
    }
    ProcessCommand::new(git_shell_command_path())
        .arg("-c")
        .arg(format!("{} \"$1\"", command))
        .arg("skron-editor")
        .arg(path)
        .status()
}

#[cfg(windows)]
fn find_executable_on_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|path| path.join(name))
        .find(|path| path.is_file())
}

pub(crate) fn git_attr_system_path() -> &'static str {
    #[cfg(windows)]
    {
        "C:/Program Files/Git/etc/gitattributes"
    }
    #[cfg(not(windows))]
    {
        "/etc/gitattributes"
    }
}

pub(crate) fn git_attr_global_path() -> Result<String> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or(CliError::Exit(1))?);
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    Ok(git_var_path_output(&xdg.join("git/attributes")))
}

pub(crate) fn git_config_global_paths() -> Result<Vec<PathBuf>> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or(CliError::Exit(1))?);
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    Ok(vec![xdg.join("git/config"), home.join(".gitconfig")])
}

pub(crate) fn git_var_path_output(path: &std::path::Path) -> String {
    let value = path.display().to_string();
    #[cfg(windows)]
    {
        value.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        value
    }
}

pub(crate) fn signature_line(signature: &Signature) -> String {
    format!(
        "{} <{}> {} {}",
        signature.name, signature.email, signature.timestamp, signature.timezone
    )
}

pub(crate) fn default_branch_name(repo: &GitRepo) -> Result<String> {
    Ok(read_config_value(repo, "init.defaultBranch")?.unwrap_or_else(|| "master".to_owned()))
}
pub(crate) fn editor_required_message_error() -> CliError {
    CliError::Stderr {
        code: 1,
        text: "error: Terminal is dumb, but EDITOR unset\n\
               Please supply the message using either -m or -F option.\n"
            .into(),
    }
}
