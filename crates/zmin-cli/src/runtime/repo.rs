use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;

use super::{CliError, Result, parse_git_bool, read_config_file};
pub(crate) use zmin_cli_runtime::GitRepo;

static GLOBAL_REPO_OPTIONS: OnceLock<GlobalRepoOptions> = OnceLock::new();

#[derive(Debug, Clone, Default)]
pub(crate) struct GlobalRepoOptions {
    pub(crate) git_dir: Option<PathBuf>,
    pub(crate) git_dir_display: Option<String>,
    pub(crate) work_tree: Option<PathBuf>,
    pub(crate) bare: bool,
}

pub(crate) fn set_global_repo_options(options: GlobalRepoOptions) {
    let _ = GLOBAL_REPO_OPTIONS.set(options);
}

pub(crate) fn global_git_dir_display() -> Option<String> {
    GLOBAL_REPO_OPTIONS
        .get()
        .and_then(|options| options.git_dir_display.clone())
}

pub(crate) fn global_bare_option() -> bool {
    GLOBAL_REPO_OPTIONS
        .get()
        .is_some_and(|options| options.bare)
}

pub(crate) fn exact_repo_at(path: &std::path::Path) -> Option<GitRepo> {
    let repo = find_repo_at(path).ok()?;
    if canonical_or_absolute(repo.root.clone()) == canonical_or_absolute(path.to_path_buf()) {
        Some(repo)
    } else {
        None
    }
}

pub(crate) fn find_repo_at(path: &std::path::Path) -> Result<GitRepo> {
    let previous = std::env::current_dir()?;
    std::env::set_current_dir(path)?;
    let result = find_repo();
    std::env::set_current_dir(previous)?;
    result
}

pub(crate) fn repo_is_bare(repo: &GitRepo) -> bool {
    repo.root == repo.git_dir && is_bare_git_dir(&repo.git_dir)
}

pub(crate) fn repo_relative_path(
    root: &std::path::Path,
    path: &std::path::Path,
) -> Result<Vec<u8>> {
    let relative = match path.strip_prefix(root) {
        Ok(relative) => relative.to_path_buf(),
        Err(_) => {
            let canonical_root = fs::canonicalize(root);
            let canonical_path = fs::canonicalize(path);
            match (canonical_root, canonical_path) {
                (Ok(canonical_root), Ok(canonical_path)) => canonical_path
                    .strip_prefix(&canonical_root)
                    .map(|relative| relative.to_path_buf())
                    .map_err(|_| {
                        CliError::Message(format!(
                            "{} is outside repository {}",
                            git_path_output(path),
                            git_path_output(root)
                        ))
                    })?,
                _ => {
                    return Err(CliError::Message(format!(
                        "{} is outside repository {}",
                        git_path_output(path),
                        git_path_output(root)
                    )));
                }
            }
        }
    };
    Ok(relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
        .into_bytes())
}

pub(crate) fn absolute_path_from_arg(path: &std::path::Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

pub(crate) fn local_repository_path_from_location(location: &str) -> Result<Option<PathBuf>> {
    if let Some(path) = file_url_to_path(location)? {
        return Ok(Some(path));
    }
    if looks_like_remote_url(location) {
        return Ok(None);
    }
    absolute_path_from_arg(std::path::Path::new(location)).map(Some)
}

pub(crate) fn file_url_to_path(location: &str) -> Result<Option<PathBuf>> {
    let Some(rest) = location.strip_prefix("file://") else {
        return Ok(None);
    };
    let path = if rest.starts_with('/') {
        rest
    } else if cfg!(windows)
        && rest.as_bytes().get(1) == Some(&b':')
        && matches!(rest.as_bytes().get(2), Some(b'/') | Some(b'\\'))
    {
        rest
    } else {
        let Some((host, path)) = rest.split_once('/') else {
            return Ok(None);
        };
        if !host.is_empty() && host != "localhost" {
            return Ok(None);
        }
        path
    };
    let decoded = percent_decode_file_url_path(path)?;
    #[cfg(windows)]
    {
        let decoded = decoded
            .strip_prefix('/')
            .filter(|value| value.as_bytes().get(1) == Some(&b':'))
            .unwrap_or(&decoded)
            .to_owned();
        return Ok(Some(normalize_windows_input_path(PathBuf::from(decoded))));
    }
    #[cfg(not(windows))]
    Ok(Some(PathBuf::from(decoded)))
}

fn percent_decode_file_url_path(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx] == b'%' {
            let Some(hex) = bytes.get(idx + 1..idx + 3) else {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("invalid file URL escape in '{value}'"),
                });
            };
            let hex = std::str::from_utf8(hex).map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("invalid file URL escape in '{value}'"),
            })?;
            let byte = u8::from_str_radix(hex, 16).map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("invalid file URL escape in '{value}'"),
            })?;
            out.push(byte);
            idx += 3;
        } else {
            out.push(bytes[idx]);
            idx += 1;
        }
    }
    String::from_utf8(out).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("file URL path is not valid UTF-8: '{value}'"),
    })
}

pub(crate) fn looks_like_remote_url(value: &str) -> bool {
    value.contains("://") || value.starts_with("git@") || value.contains('@') && value.contains(':')
}

pub(crate) fn canonical_or_absolute(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

#[cfg(windows)]
pub(crate) fn windows_msys_path(value: &str) -> Option<PathBuf> {
    let bytes = value.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b'/' {
        let drive = bytes[1] as char;
        return Some(PathBuf::from(format!("{drive}:\\{}", &value[3..])));
    }
    None
}

#[cfg(windows)]
pub(crate) fn normalize_windows_input_path(path: PathBuf) -> PathBuf {
    path.to_str().and_then(windows_msys_path).unwrap_or(path)
}

#[cfg(not(windows))]
pub(crate) fn normalize_windows_input_path(path: PathBuf) -> PathBuf {
    path
}

pub(crate) fn git_path_config_output(path: &std::path::Path) -> String {
    git_path_output_string(path.display().to_string())
}

fn git_path_output(path: &std::path::Path) -> String {
    git_path_output_string(path.display().to_string())
}

#[cfg(windows)]
fn git_path_output_string(value: String) -> String {
    let value = if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        value
    };
    value.replace('\\', "/")
}

#[cfg(not(windows))]
fn git_path_output_string(value: String) -> String {
    value
}

pub(crate) fn find_repo() -> Result<GitRepo> {
    if let Some(repo) = repo_from_global_options()? {
        return Ok(repo);
    }
    if let Some(repo) = repo_from_env_options()? {
        return Ok(repo);
    }
    let mut dir = std::env::current_dir()?;
    loop {
        let git_dir = dir.join(".git");
        if git_dir.is_dir() {
            return Ok(GitRepo {
                root: dir,
                index_path: git_dir.join("index"),
                objects_dir: git_dir.join("objects"),
                git_dir,
            });
        }
        if git_dir.is_file() {
            let actual_git_dir = read_gitdir_file(&git_dir)?;
            if !is_git_dir_or_linked_worktree_git_dir(&actual_git_dir) {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("not a git repository: {}", git_path_output(&actual_git_dir)),
                });
            }
            let common_dir = read_common_git_dir(&actual_git_dir)?;
            return Ok(GitRepo {
                root: dir,
                index_path: actual_git_dir.join("index"),
                objects_dir: common_dir.join("objects"),
                git_dir: actual_git_dir,
            });
        }
        if !dir.pop() {
            return Err(CliError::Fatal {
                code: 128,
                message: "not a git repository".into(),
            });
        }
    }
}

pub(crate) fn find_repo_with_parent_dir_error() -> Result<GitRepo> {
    parent_dir_not_repo_error(find_repo())
}

pub(crate) fn find_repo_or_bare_with_parent_dir_error() -> Result<GitRepo> {
    parent_dir_not_repo_error(find_repo_or_bare())
}

fn parent_dir_not_repo_error(result: Result<GitRepo>) -> Result<GitRepo> {
    result.map_err(|error| match error {
        CliError::Fatal { code: 128, message } if message == "not a git repository" => {
            CliError::Fatal {
                code: 128,
                message: "not a git repository (or any of the parent directories): .git".into(),
            }
        }
        other => other,
    })
}

pub(crate) fn repo_from_worktree_root(root: PathBuf) -> Result<GitRepo> {
    let git_dir_path = root.join(".git");
    if git_dir_path.is_dir() {
        return Ok(GitRepo {
            root,
            index_path: git_dir_path.join("index"),
            objects_dir: git_dir_path.join("objects"),
            git_dir: git_dir_path,
        });
    }
    if git_dir_path.is_file() {
        let actual_git_dir = read_gitdir_file(&git_dir_path)?;
        if !is_git_dir_or_linked_worktree_git_dir(&actual_git_dir) {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("not a git repository: {}", git_path_output(&actual_git_dir)),
            });
        }
        let common_dir = read_common_git_dir(&actual_git_dir)?;
        return Ok(GitRepo {
            root,
            index_path: actual_git_dir.join("index"),
            objects_dir: common_dir.join("objects"),
            git_dir: actual_git_dir,
        });
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!("not a git repository: '{}'", git_path_output(&root)),
    })
}

fn repo_from_global_options() -> Result<Option<GitRepo>> {
    let Some(options) = GLOBAL_REPO_OPTIONS.get() else {
        return Ok(None);
    };
    let Some(git_dir) = options.git_dir.as_ref() else {
        return Ok(None);
    };
    if !is_git_dir(git_dir) {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("not a git repository: '{}'", git_path_output(git_dir)),
        });
    }
    let common_dir = read_common_git_dir(git_dir)?;
    let root = match (options.work_tree.as_ref(), options.bare) {
        (Some(path), _) => path.clone(),
        (None, true) => git_dir.clone(),
        (None, false) => std::env::current_dir()?,
    };
    Ok(Some(GitRepo {
        root,
        index_path: git_dir.join("index"),
        objects_dir: common_dir.join("objects"),
        git_dir: git_dir.clone(),
    }))
}

fn repo_from_env_options() -> Result<Option<GitRepo>> {
    let Some(git_dir_raw) = std::env::var_os("GIT_DIR") else {
        return Ok(None);
    };
    let cwd = std::env::current_dir()?;
    let git_dir = {
        let path = normalize_windows_input_path(PathBuf::from(git_dir_raw));
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    };
    if !is_git_dir(&git_dir) {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("not a git repository: '{}'", git_path_output(&git_dir)),
        });
    }
    let common_dir = read_common_git_dir(&git_dir)?;
    let root = if let Some(work_tree_raw) = std::env::var_os("GIT_WORK_TREE") {
        let path = normalize_windows_input_path(PathBuf::from(work_tree_raw));
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    } else if is_bare_git_dir(&git_dir) {
        git_dir.clone()
    } else {
        cwd.clone()
    };
    let index_path = if let Some(index_raw) = std::env::var_os("GIT_INDEX_FILE") {
        let path = normalize_windows_input_path(PathBuf::from(index_raw));
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    } else {
        git_dir.join("index")
    };
    Ok(Some(GitRepo {
        root,
        index_path,
        objects_dir: common_dir.join("objects"),
        git_dir,
    }))
}

pub(crate) fn is_git_dir(path: &std::path::Path) -> bool {
    path.join("HEAD").is_file() && path.join("objects").is_dir() && path.join("refs").is_dir()
}

pub(crate) fn is_git_dir_or_linked_worktree_git_dir(path: &std::path::Path) -> bool {
    is_git_dir(path) || path.join("HEAD").is_file() && path.join("commondir").is_file()
}

pub(crate) fn is_bare_git_dir(path: &std::path::Path) -> bool {
    if !is_git_dir(path) {
        return false;
    }
    read_config_file(&path.join("config"))
        .map(|entries| {
            entries.into_iter().rev().any(|entry| {
                entry.section == "core"
                    && entry.subsection.is_empty()
                    && entry.key == "bare"
                    && parse_git_bool(&entry.value) == Some(true)
            })
        })
        .unwrap_or(false)
}

pub(crate) fn read_gitdir_file(path: &std::path::Path) -> Result<PathBuf> {
    let raw = fs::read_to_string(path)?;
    let value = raw
        .trim()
        .strip_prefix("gitdir:")
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("invalid gitfile format: {}", git_path_output(path)),
        })?
        .trim();
    let git_dir = normalize_windows_input_path(PathBuf::from(value));
    let git_dir = if git_dir.is_absolute() {
        git_dir
    } else {
        path.parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(git_dir)
    };
    Ok(fs::canonicalize(&git_dir).unwrap_or(git_dir))
}

pub(crate) fn read_common_git_dir(git_dir: &std::path::Path) -> Result<PathBuf> {
    match fs::read_to_string(git_dir.join("commondir")) {
        Ok(raw) => {
            let value = normalize_windows_input_path(PathBuf::from(raw.trim()));
            if value.is_absolute() {
                Ok(value)
            } else {
                Ok(git_dir.join(value))
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(git_dir.to_path_buf()),
        Err(error) => Err(CliError::Io(error)),
    }
}

pub(crate) fn find_repo_or_bare() -> Result<GitRepo> {
    if let Some(repo) = repo_from_global_options()? {
        return Ok(repo);
    }
    if let Some(repo) = repo_from_env_options()? {
        return Ok(repo);
    }
    let cwd = std::env::current_dir()?;
    if is_git_dir(&cwd) {
        let common_dir = read_common_git_dir(&cwd)?;
        let root = git_dir_worktree(&cwd)?.unwrap_or_else(|| cwd.clone());
        return Ok(GitRepo {
            root,
            index_path: cwd.join("index"),
            objects_dir: common_dir.join("objects"),
            git_dir: cwd,
        });
    }
    find_repo()
}

fn git_dir_worktree(git_dir: &std::path::Path) -> Result<Option<PathBuf>> {
    let Some(value) = read_config_file(&git_dir.join("config"))?
        .into_iter()
        .rev()
        .find(|entry| {
            entry.section == "core"
                && entry.subsection.is_empty()
                && entry.key == "worktree"
                && !entry.value.is_empty()
        })
        .map(|entry| entry.value)
    else {
        return Ok(None);
    };
    let path = normalize_windows_input_path(PathBuf::from(value));
    if path.is_absolute() {
        Ok(Some(path))
    } else {
        Ok(Some(git_dir.join(path)))
    }
}

#[cfg(test)]
mod tests {
    use super::file_url_to_path;

    #[cfg(windows)]
    #[test]
    fn file_url_to_path_accepts_windows_display_paths() {
        let path = file_url_to_path(r"file://D:\a\repo")
            .expect("parse file url")
            .expect("file url path");

        assert_eq!(path.to_string_lossy(), r"D:\a\repo");
    }

    #[cfg(windows)]
    #[test]
    fn file_url_to_path_accepts_git_for_windows_msys_paths() {
        let path = file_url_to_path("file:///c/Users/zmin/repo")
            .expect("parse file url")
            .expect("file url path");

        assert_eq!(path.to_string_lossy(), r"c:\Users\zmin\repo");
    }
}
