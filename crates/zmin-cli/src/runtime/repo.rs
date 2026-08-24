use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{
    CliError, Result, parse_git_bool, read_config_file, read_protected_config_entries,
    wildcard_match_pathspec,
};
pub(crate) use zmin_cli_runtime::GitRepo;

static GLOBAL_REPO_OPTIONS: OnceLock<GlobalRepoOptions> = OnceLock::new();
static TRACE2_PERF_PREPARED: AtomicBool = AtomicBool::new(false);

pub(crate) fn mark_trace2_perf_target_prepared() {
    TRACE2_PERF_PREPARED.store(true, Ordering::SeqCst);
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GlobalRepoOptions {
    pub(crate) git_dir: Option<PathBuf>,
    pub(crate) git_dir_display: Option<String>,
    pub(crate) work_tree: Option<PathBuf>,
    pub(crate) bare: bool,
    pub(crate) attr_source: Option<String>,
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

pub(crate) fn global_work_tree_option() -> Option<PathBuf> {
    GLOBAL_REPO_OPTIONS
        .get()
        .and_then(|options| options.work_tree.clone())
}

pub(crate) fn global_attr_source_option() -> Option<String> {
    GLOBAL_REPO_OPTIONS
        .get()
        .and_then(|options| options.attr_source.clone())
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
    let cwd = std::env::current_dir()?;
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    find_repo_from_base(&path)
}

pub(crate) fn repo_is_bare(repo: &GitRepo) -> bool {
    repo.root == repo.git_dir
        && std::env::var_os("GIT_WORK_TREE").is_none()
        && GLOBAL_REPO_OPTIONS
            .get()
            .is_none_or(|options| options.work_tree.is_none())
        && git_dir_worktree(&repo.git_dir).ok().flatten().is_none()
}

pub(crate) fn repo_relative_path(
    root: &std::path::Path,
    path: &std::path::Path,
) -> Result<Vec<u8>> {
    repo_relative_path_with_options(root, path, false, true)
}

pub(crate) fn repo_relative_path_lexical(
    root: &std::path::Path,
    path: &std::path::Path,
) -> Result<Vec<u8>> {
    repo_relative_path_with_options(root, path, false, false)
}

pub(crate) fn repo_relative_path_preserve_final_component(
    root: &std::path::Path,
    path: &std::path::Path,
) -> Result<Vec<u8>> {
    repo_relative_path_with_options(root, path, true, true)
}

fn repo_relative_path_with_options(
    root: &std::path::Path,
    path: &std::path::Path,
    preserve_final_component: bool,
    resolve_existing_components: bool,
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
    let relative = normalize_repo_relative_path(&relative)?;
    let relative = if resolve_existing_components {
        resolve_existing_worktree_relative_path(root, &relative, preserve_final_component)
    } else {
        relative
    };
    Ok(relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
        .into_bytes())
}

fn normalize_repo_relative_path(relative: &std::path::Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return Err(CliError::Message("path is outside repository".into()));
                }
            }
            std::path::Component::Normal(part) => normalized.push(part),
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                return Err(CliError::Message("path is outside repository".into()));
            }
        }
    }
    Ok(normalized)
}

fn resolve_existing_worktree_relative_path(
    root: &std::path::Path,
    relative: &std::path::Path,
    preserve_final_component: bool,
) -> PathBuf {
    let mut resolved = PathBuf::new();
    let mut current = root.to_path_buf();
    let components = relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_os_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let last_index = components.len().saturating_sub(1);
    for (index, component) in components.into_iter().enumerate() {
        let preserve = preserve_final_component && index == last_index;
        let actual = if preserve {
            component.clone()
        } else {
            resolve_existing_child_name(&current, &component).unwrap_or_else(|| component.clone())
        };
        resolved.push(&actual);
        current.push(actual);
    }
    resolved
}

fn resolve_existing_child_name(parent: &std::path::Path, requested: &OsStr) -> Option<OsString> {
    let candidate = parent.join(requested);
    fs::symlink_metadata(&candidate).ok()?;

    let mut canonical_match = None;
    let entries = fs::read_dir(parent).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name == requested {
            return Some(name);
        }
        if existing_paths_equivalent(&entry.path(), &candidate) {
            if canonical_match.is_some() {
                return Some(requested.to_os_string());
            }
            canonical_match = Some(name);
        }
    }
    canonical_match.or_else(|| Some(requested.to_os_string()))
}

fn existing_paths_equivalent(left: &std::path::Path, right: &std::path::Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

pub(crate) fn absolute_path_from_arg(path: &std::path::Path) -> Result<PathBuf> {
    absolute_path_from_base(&std::env::current_dir()?, path)
}

pub(crate) fn absolute_path_from_base(
    base: &std::path::Path,
    path: &std::path::Path,
) -> Result<PathBuf> {
    let path = normalize_windows_input_path(path.to_path_buf());
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(base.join(path))
    }
}

pub(crate) fn local_repository_path_from_location(location: &str) -> Result<Option<PathBuf>> {
    local_repository_path_from_base(&std::env::current_dir()?, location)
}

pub(crate) fn local_repository_path_from_base(
    base: &std::path::Path,
    location: &str,
) -> Result<Option<PathBuf>> {
    if let Some(path) = file_url_to_path(location)? {
        return Ok(Some(path));
    }
    if looks_like_remote_url(location) {
        return Ok(None);
    }
    absolute_path_from_base(base, std::path::Path::new(location)).map(Some)
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
    find_repo_from_base(&std::env::current_dir()?)
}

fn find_repo_from_base(base: &std::path::Path) -> Result<GitRepo> {
    prepare_trace2_perf_target()?;
    if let Some(repo) = repo_from_global_options(base)? {
        return Ok(repo);
    }
    if let Some(repo) = repo_from_env_options(base)? {
        return Ok(repo);
    }
    find_discovered_repo(base, false)
}

fn find_discovered_repo(base: &std::path::Path, enforce_bare_safety: bool) -> Result<GitRepo> {
    let ceiling_dirs = repo_search_ceiling_dirs_at(base)?;
    let mut dir = base.to_path_buf();
    loop {
        if is_git_dir_or_linked_worktree_git_dir(&dir) {
            let repo = repo_from_git_dir(&dir, &dir, base, None, false)?;
            if enforce_bare_safety {
                enforce_safe_bare_repository_access(&repo, base)?;
            }
            return Ok(repo);
        }
        let git_dir = dir.join(".git");
        match inspect_dot_git_entry(&git_dir)? {
            DotGitEntry::ValidDir => {
                return repo_from_git_dir(&git_dir, &dir, base, None, false);
            }
            DotGitEntry::GitFile => {
                let actual_git_dir = read_gitdir_file(&git_dir)?;
                if !is_git_dir_or_linked_worktree_git_dir(&actual_git_dir) {
                    return Err(CliError::Fatal {
                        code: 128,
                        message: format!(
                            "not a git repository: {}",
                            git_path_output(&actual_git_dir)
                        ),
                    });
                }
                return repo_from_git_dir(&actual_git_dir, &dir, base, None, false);
            }
            DotGitEntry::InvalidSpecialFile => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "invalid gitfile format: {} not a regular file",
                        git_path_output(&git_dir)
                    ),
                });
            }
            DotGitEntry::MissingOrIgnoredDir => {}
        }
        if repo_search_stops_before_parent(&dir, &ceiling_dirs) {
            return Err(CliError::Fatal {
                code: 128,
                message: "not a git repository".into(),
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

fn repo_from_git_dir(
    git_dir: &std::path::Path,
    discovered_root: &std::path::Path,
    base: &std::path::Path,
    explicit_work_tree: Option<&std::path::Path>,
    explicit_bare: bool,
) -> Result<GitRepo> {
    if !is_git_dir_or_linked_worktree_git_dir(git_dir) {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("not a git repository: '{}'", git_path_output(git_dir)),
        });
    }
    let common_dir = read_common_git_dir_at(git_dir, base)?;
    let root = effective_work_tree(
        git_dir,
        discovered_root,
        base,
        explicit_work_tree,
        explicit_bare,
    )?;
    let repo = GitRepo {
        root,
        index_path: git_dir.join("index"),
        objects_dir: common_dir.join("objects"),
        git_dir: git_dir.to_path_buf(),
    };
    write_trace_setup(&repo, base)?;
    trace_implicit_git_directory_access(&repo, base)?;
    enforce_safe_directory_access(&repo, base)?;
    repo_with_env_index_path_at(repo, base)
}

fn write_trace_setup(repo: &GitRepo, base: &std::path::Path) -> Result<()> {
    let Some(target) = std::env::var_os("GIT_TRACE_SETUP") else {
        return Ok(());
    };
    if target.is_empty() || target == "0" || target.eq_ignore_ascii_case("false") {
        return Ok(());
    }
    let Ok(payload) = trace_setup_payload(repo, base) else {
        return Ok(());
    };
    if target == "1" || target == "2" || target.eq_ignore_ascii_case("true") {
        let _ = io::stderr().write_all(&payload);
        return Ok(());
    }
    let target_path = PathBuf::from(target);
    if !target_path.is_absolute() {
        warn_trace_setup_value(&target_path);
        return Ok(());
    }
    let mut file = match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&target_path)
    {
        Ok(file) => file,
        Err(error) => {
            warn_trace_setup_open(&target_path, &error);
            return Ok(());
        }
    };
    if let Err(error) = file.write_all(&payload) {
        warn_trace_setup_write(&error);
    }
    Ok(())
}

fn trace_setup_payload(repo: &GitRepo, base: &std::path::Path) -> Result<Vec<u8>> {
    let git_dir = trace_git_dir_display(repo, base)?;
    let common_dir = read_common_git_dir_at(&repo.git_dir, base)?;
    let common_dir = if canonical_or_absolute(common_dir.clone())
        == canonical_or_absolute(repo.git_dir.clone())
    {
        git_dir.clone()
    } else {
        trace_path_bytes(&canonical_or_absolute(common_dir))?
    };
    let worktree = if repo_is_bare(repo) {
        b"(null)".to_vec()
    } else {
        trace_path_bytes(&canonical_or_absolute(repo.root.clone()))?
    };
    let cwd = trace_path_bytes(&trace_cwd(repo, base))?;
    let prefix = trace_prefix(repo, base)?;
    let mut payload = Vec::new();
    append_trace_setup_line(&mut payload, b"setup: git_dir: ", &git_dir);
    append_trace_setup_line(&mut payload, b"setup: git_common_dir: ", &common_dir);
    append_trace_setup_line(&mut payload, b"setup: worktree: ", &worktree);
    append_trace_setup_line(&mut payload, b"setup: cwd: ", &cwd);
    append_trace_setup_line(&mut payload, b"setup: prefix: ", &prefix);
    Ok(payload)
}

fn append_trace_setup_line(payload: &mut Vec<u8>, label: &[u8], value: &[u8]) {
    payload.extend_from_slice(label);
    payload.extend_from_slice(value);
    payload.push(b'\n');
}

fn warn_trace_setup_value(target: &std::path::Path) {
    let mut message = b"warning: unknown trace value for 'GIT_TRACE_SETUP': ".to_vec();
    message.extend(trace_path_bytes(target).unwrap_or_else(|_| b"<invalid path>".to_vec()));
    message.extend_from_slice(b"\n");
    let _ = io::stderr().write_all(&message);
}

fn warn_trace_setup_open(target: &std::path::Path, error: &io::Error) {
    let mut message = b"warning: could not open '".to_vec();
    message.extend(trace_path_bytes(target).unwrap_or_else(|_| b"<invalid path>".to_vec()));
    message.extend_from_slice(b"' for tracing: ");
    let error_text = error.to_string();
    let error_text = error_text
        .split_once(" (os error ")
        .map_or(error_text.as_str(), |(message, _)| message);
    message.extend_from_slice(error_text.as_bytes());
    message.push(b'\n');
    let _ = io::stderr().write_all(&message);
}

fn warn_trace_setup_write(error: &io::Error) {
    let mut message = b"warning: unable to write trace for GIT_TRACE_SETUP: ".to_vec();
    let error_text = error.to_string();
    let error_text = error_text
        .split_once(" (os error ")
        .map_or(error_text.as_str(), |(message, _)| message);
    message.extend_from_slice(error_text.as_bytes());
    message.push(b'\n');
    let _ = io::stderr().write_all(&message);
}

fn trace_git_dir_display(repo: &GitRepo, base: &std::path::Path) -> Result<Vec<u8>> {
    if let Some(raw) = std::env::var_os("GIT_DIR") {
        let raw_path = normalize_windows_input_path(PathBuf::from(raw));
        let absolute = absolute_path_from_base(base, &raw_path)?;
        if absolute.is_file() || raw_path.is_absolute() {
            return trace_path_bytes(&canonical_or_absolute(repo.git_dir.clone()));
        }
        let inside_worktree = trace_path_is_inside(repo, base);
        let base_is_below_worktree =
            canonical_or_absolute(base.to_path_buf()) != canonical_or_absolute(repo.root.clone());
        let git_dir_parent = repo
            .git_dir
            .parent()
            .map(|parent| canonical_or_absolute(parent.to_path_buf()));
        let worktree_differs_from_git_dir_parent = git_dir_parent
            .as_ref()
            .is_some_and(|parent| canonical_or_absolute(repo.root.clone()) != *parent);
        let has_configured_worktree = git_dir_worktree(&repo.git_dir)?.is_some();
        if !repo_is_bare(repo)
            && inside_worktree
            && ((worktree_differs_from_git_dir_parent
                && (raw_path == PathBuf::from(".") || raw_path == PathBuf::from(".git")))
                || (std::env::var_os("GIT_WORK_TREE").is_some() && base_is_below_worktree))
        {
            return trace_path_bytes(&canonical_or_absolute(repo.git_dir.clone()));
        }
        if std::env::var_os("GIT_WORK_TREE").is_none()
            && inside_worktree
            && has_configured_worktree
            && raw_path
                .components()
                .any(|component| component == std::path::Component::ParentDir)
        {
            return trace_path_bytes(&canonical_or_absolute(repo.git_dir.clone()));
        }
        return trace_path_bytes(&raw_path);
    }
    let git_dir = canonical_or_absolute(repo.git_dir.clone());
    let base = canonical_or_absolute(base.to_path_buf());
    let inside_worktree = trace_path_is_inside(repo, &base);
    let git_dir_parent = repo
        .git_dir
        .parent()
        .map(|parent| canonical_or_absolute(parent.to_path_buf()));
    let worktree_differs_from_git_dir_parent = git_dir_parent
        .as_ref()
        .is_some_and(|parent| canonical_or_absolute(repo.root.clone()) != *parent);
    if repo_is_bare(repo) {
        if base == git_dir {
            return Ok(b".".to_vec());
        }
        if canonical_or_absolute(base.join(".git")) == git_dir {
            return Ok(b".git".to_vec());
        }
    } else if inside_worktree
        && (worktree_differs_from_git_dir_parent
            || (std::env::var_os("GIT_WORK_TREE").is_some()
                && canonical_or_absolute(base.to_path_buf())
                    != canonical_or_absolute(repo.root.clone())))
    {
        return trace_path_bytes(&git_dir);
    } else if base.starts_with(&git_dir) {
        return trace_path_bytes(&git_dir);
    } else {
        if repo.git_dir.file_name().is_some_and(|name| name == ".git") && repo.git_dir.is_dir() {
            return Ok(b".git".to_vec());
        }
    }
    trace_path_bytes(&git_dir)
}

fn trace_path_is_inside(repo: &GitRepo, base: &std::path::Path) -> bool {
    canonical_or_absolute(base.to_path_buf()).starts_with(&canonical_or_absolute(repo.root.clone()))
}

fn trace_prefix(repo: &GitRepo, base: &std::path::Path) -> Result<Vec<u8>> {
    if repo_is_bare(repo) {
        return Ok(b"(null)".to_vec());
    }
    let root = canonical_or_absolute(repo.root.clone());
    let cwd = canonical_or_absolute(base.to_path_buf());
    let Ok(relative) = cwd.strip_prefix(&root) else {
        return Ok(b"(null)".to_vec());
    };
    if relative.as_os_str().is_empty() {
        return Ok(b"(null)".to_vec());
    }
    let mut prefix = Vec::new();
    for component in relative.components() {
        if !prefix.is_empty() {
            prefix.push(b'/');
        }
        let component_path = PathBuf::from(component.as_os_str());
        prefix.extend_from_slice(&trace_path_bytes(&component_path)?);
    }
    prefix.push(b'/');
    Ok(prefix)
}

fn trace_path_bytes(path: &Path) -> Result<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;

        return Ok(quote_trace_bytes(path.as_os_str().as_bytes()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        let wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
        let value = String::from_utf16(&wide).map_err(|_| {
            CliError::Message("path cannot be represented as valid Windows UTF-8".to_owned())
        })?;
        return Ok(quote_trace_bytes(value.replace('\\', "/").as_bytes()));
    }
    #[cfg(not(any(unix, windows)))]
    {
        let value = path
            .to_str()
            .ok_or_else(|| CliError::Message("path cannot be represented as UTF-8".to_owned()))?;
        Ok(quote_trace_bytes(value.as_bytes()))
    }
}

fn quote_trace_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut quoted = Vec::with_capacity(bytes.len());
    for &byte in bytes {
        match byte {
            b'\\' => quoted.extend_from_slice(b"\\\\"),
            b'\n' => quoted.extend_from_slice(b"\\n"),
            b'\r' => quoted.extend_from_slice(b"\\r"),
            _ => quoted.push(byte),
        }
    }
    quoted
}

fn trace_cwd(repo: &GitRepo, base: &std::path::Path) -> PathBuf {
    if repo_is_bare(repo) {
        return canonical_or_absolute(base.to_path_buf());
    }
    let root = canonical_or_absolute(repo.root.clone());
    let cwd = canonical_or_absolute(base.to_path_buf());
    if cwd == root || cwd.starts_with(&root) {
        root
    } else {
        cwd
    }
}

fn effective_work_tree(
    git_dir: &std::path::Path,
    discovered_root: &std::path::Path,
    base: &std::path::Path,
    explicit_work_tree: Option<&std::path::Path>,
    explicit_bare: bool,
) -> Result<PathBuf> {
    if let Some(work_tree) = explicit_work_tree {
        return Ok(work_tree.to_path_buf());
    }
    if let Some(work_tree_raw) = std::env::var_os("GIT_WORK_TREE") {
        return absolute_path_from_base(
            base,
            &normalize_windows_input_path(PathBuf::from(work_tree_raw)),
        );
    }
    if let Some(work_tree) = git_dir_worktree(git_dir)? {
        return Ok(canonical_or_absolute(work_tree));
    }
    if explicit_bare || is_bare_git_dir(git_dir) || discovered_root == git_dir {
        return Ok(git_dir.to_path_buf());
    }
    Ok(discovered_root.to_path_buf())
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
        return repo_from_git_dir(&git_dir_path, &root, &root, Some(&root), false);
    }
    if git_dir_path.is_file() {
        let actual_git_dir = read_gitdir_file(&git_dir_path)?;
        if !is_git_dir_or_linked_worktree_git_dir(&actual_git_dir) {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("not a git repository: {}", git_path_output(&actual_git_dir)),
            });
        }
        return repo_from_git_dir(&actual_git_dir, &root, &root, Some(&root), false);
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!("not a git repository: '{}'", git_path_output(&root)),
    })
}

pub(crate) fn repo_with_env_index_path(repo: GitRepo) -> Result<GitRepo> {
    repo_with_env_index_path_at(repo, &std::env::current_dir()?)
}

fn repo_with_env_index_path_at(mut repo: GitRepo, base: &std::path::Path) -> Result<GitRepo> {
    let Some(index_raw) = std::env::var_os("GIT_INDEX_FILE") else {
        return Ok(repo);
    };
    let path = normalize_windows_input_path(PathBuf::from(index_raw));
    repo.index_path = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    Ok(repo)
}

fn repo_from_global_options(base: &std::path::Path) -> Result<Option<GitRepo>> {
    let Some(options) = GLOBAL_REPO_OPTIONS.get() else {
        return Ok(None);
    };
    let git_dir = if let Some(git_dir) = options.git_dir.as_ref() {
        resolve_git_dir_path(git_dir)?
    } else if options.work_tree.is_some() {
        match find_discovered_repo(base, false) {
            Ok(repo) => repo.git_dir,
            Err(CliError::Fatal { code: 128, .. }) => return Ok(None),
            Err(error) => return Err(error),
        }
    } else {
        return Ok(None);
    };
    let explicit_work_tree = options.work_tree.as_deref();
    let repo = repo_from_git_dir(&git_dir, base, base, explicit_work_tree, options.bare)?;
    Ok(Some(repo))
}

fn repo_from_env_options(base: &std::path::Path) -> Result<Option<GitRepo>> {
    let Some(git_dir_raw) = std::env::var_os("GIT_DIR") else {
        return Ok(None);
    };
    let git_dir_input = normalize_windows_input_path(PathBuf::from(git_dir_raw));
    let work_tree_raw = std::env::var_os("GIT_WORK_TREE");
    let git_dir_path = absolute_path_from_base(base, &git_dir_input)?;
    let git_dir_path = if work_tree_raw.is_some()
        && !git_dir_input.is_absolute()
        && git_dir_input
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        canonical_or_absolute(git_dir_path)
    } else {
        git_dir_path
    };
    let git_dir = resolve_git_dir_path(&git_dir_path)?;
    let explicit_work_tree = work_tree_raw.map(|raw| {
        absolute_path_from_base(base, &normalize_windows_input_path(PathBuf::from(raw)))
    });
    let explicit_work_tree = explicit_work_tree.transpose()?.map(canonical_or_absolute);
    let repo = repo_from_git_dir(&git_dir, base, base, explicit_work_tree.as_deref(), false)?;
    Ok(Some(repo))
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

fn resolve_git_dir_path(path: &std::path::Path) -> Result<PathBuf> {
    if path.is_file() {
        read_gitdir_file(path)
    } else {
        Ok(path.to_path_buf())
    }
}

enum DotGitEntry {
    MissingOrIgnoredDir,
    ValidDir,
    GitFile,
    InvalidSpecialFile,
}

fn inspect_dot_git_entry(path: &std::path::Path) -> Result<DotGitEntry> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(DotGitEntry::MissingOrIgnoredDir);
        }
        Err(error) => return Err(CliError::Io(error)),
    };

    let file_type = metadata.file_type();
    if file_type.is_dir() {
        return Ok(if is_git_dir(path) {
            DotGitEntry::ValidDir
        } else {
            DotGitEntry::MissingOrIgnoredDir
        });
    }
    if file_type.is_file() {
        return Ok(DotGitEntry::GitFile);
    }
    if file_type.is_symlink() {
        let target = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(DotGitEntry::MissingOrIgnoredDir);
            }
            Err(error) => return Err(CliError::Io(error)),
        };
        let target_type = target.file_type();
        if target_type.is_dir() {
            return Ok(if is_git_dir(path) {
                DotGitEntry::ValidDir
            } else {
                DotGitEntry::MissingOrIgnoredDir
            });
        }
        if target_type.is_file() {
            return Ok(DotGitEntry::GitFile);
        }
        return Ok(DotGitEntry::InvalidSpecialFile);
    }

    Ok(DotGitEntry::InvalidSpecialFile)
}

pub(crate) fn read_common_git_dir(git_dir: &std::path::Path) -> Result<PathBuf> {
    read_common_git_dir_at(git_dir, &std::env::current_dir()?)
}

fn read_common_git_dir_at(git_dir: &std::path::Path, base: &std::path::Path) -> Result<PathBuf> {
    if let Some(common_dir_raw) = std::env::var_os("GIT_COMMON_DIR") {
        return absolute_path_from_base(base, std::path::Path::new(&common_dir_raw));
    }
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
    let cwd = std::env::current_dir()?;
    prepare_trace2_perf_target()?;
    if let Some(repo) = repo_from_global_options(&cwd)? {
        enforce_safe_bare_repository_access(&repo, &cwd)?;
        return Ok(repo);
    }
    if let Some(repo) = repo_from_env_options(&cwd)? {
        enforce_safe_bare_repository_access(&repo, &cwd)?;
        return Ok(repo);
    }
    find_discovered_repo(&cwd, true)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SafeBareRepositoryMode {
    All,
    Explicit,
}

fn protected_safe_bare_repository_mode() -> Result<SafeBareRepositoryMode> {
    let mut mode = SafeBareRepositoryMode::All;
    for entry in read_protected_config_entries()? {
        if entry.section != "safe" || !entry.subsection.is_empty() || entry.key != "barerepository"
        {
            continue;
        }
        mode = match entry.value.to_ascii_lowercase().as_str() {
            "" | "all" => SafeBareRepositoryMode::All,
            "explicit" => SafeBareRepositoryMode::Explicit,
            _ => SafeBareRepositoryMode::All,
        };
    }
    Ok(mode)
}

fn append_implicit_bare_repository_trace(path: &std::path::Path) -> Result<()> {
    let Some(target) = std::env::var_os("GIT_TRACE2_PERF") else {
        return Ok(());
    };
    if target == "1" || target == "2" {
        return Ok(());
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(target)?;
    writeln!(file, "implicit-bare-repository:{}", git_path_output(path))?;
    Ok(())
}

fn prepare_trace2_perf_target() -> Result<()> {
    let Some(target) = std::env::var_os("GIT_TRACE2_PERF") else {
        return Ok(());
    };
    if target == "1" || target == "2" {
        return Ok(());
    }
    if TRACE2_PERF_PREPARED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(target)?;
    Ok(())
}

fn trace_implicit_git_directory_access(
    repo: &GitRepo,
    accessed_path: &std::path::Path,
) -> Result<()> {
    let accessed_path = canonical_or_absolute(accessed_path.to_path_buf());
    let git_dir = canonical_or_absolute(repo.git_dir.clone());
    if accessed_path == git_dir || accessed_path.starts_with(&git_dir) {
        append_implicit_bare_repository_trace(&accessed_path)?;
    }
    Ok(())
}

fn enforce_safe_bare_repository_access(
    repo: &GitRepo,
    accessed_path: &std::path::Path,
) -> Result<()> {
    if !repo_is_bare(repo) {
        return Ok(());
    }
    let accessed_path = canonical_or_absolute(accessed_path.to_path_buf());
    let git_dir = canonical_or_absolute(repo.git_dir.clone());
    if accessed_path != git_dir {
        return Ok(());
    }
    if protected_safe_bare_repository_mode()? != SafeBareRepositoryMode::Explicit {
        return Ok(());
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!(
            "cannot use bare repository '{}' (safe.bareRepository is 'explicit')",
            git_path_output(&git_dir)
        ),
    })
}

pub(crate) fn enforce_safe_directory_access(
    repo: &GitRepo,
    accessed_path: &std::path::Path,
) -> Result<()> {
    if !repository_requires_safe_directory(repo)? {
        return Ok(());
    }
    let accessed_path = canonical_or_absolute(accessed_path.to_path_buf());
    if protected_safe_directory_allows(repo, &accessed_path)? {
        return Ok(());
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!(
            "detected dubious ownership in repository at '{}'\nTo add an exception for this directory, call:\n\n\tgit config --global --add safe.directory {}",
            git_path_output(&accessed_path),
            git_path_output(&accessed_path)
        ),
    })
}

pub(crate) fn protected_safe_directory_allows(
    repo: &GitRepo,
    path: &std::path::Path,
) -> Result<bool> {
    let path = canonical_or_absolute(path.to_path_buf());
    let path_text = git_path_config_output(&path);
    let repo_root = canonical_or_absolute(repo.root.clone());
    let repo_git_dir = canonical_or_absolute(repo.git_dir.clone());
    let mut entries = Vec::new();
    for entry in read_protected_config_entries()? {
        if entry.section != "safe" || !entry.subsection.is_empty() || entry.key != "directory" {
            continue;
        }
        if entry.value.is_empty() {
            entries.clear();
            continue;
        }
        entries.push(entry.value);
    }
    for value in entries {
        if value == "*" {
            return Ok(true);
        }
        if value == "." {
            if path == repo_root || path == repo_git_dir {
                return Ok(true);
            }
            continue;
        }
        let configured_text = normalize_safe_directory_pattern(&value);
        if configured_text == path_text
            || configured_text.contains(['*', '?', '['])
                && wildcard_match_pathspec(&configured_text, &path_text, false, true)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn normalize_safe_directory_pattern(value: &str) -> String {
    let first_glob = value
        .bytes()
        .position(|byte| matches!(byte, b'*' | b'?' | b'['));
    let Some(first_glob) = first_glob else {
        return git_path_config_output(&canonical_or_absolute(PathBuf::from(value)));
    };
    let slash_index = value[..first_glob].rfind('/');
    let Some(slash_index) = slash_index else {
        return value.to_owned();
    };
    let prefix = &value[..slash_index];
    let suffix = &value[slash_index..];
    let normalized_prefix = git_path_config_output(&canonical_or_absolute(PathBuf::from(prefix)));
    format!("{normalized_prefix}{suffix}")
}

fn repository_requires_safe_directory(repo: &GitRepo) -> Result<bool> {
    if std::env::var_os("GIT_TEST_ASSUME_DIFFERENT_OWNER").is_some() {
        return Ok(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        unsafe extern "C" {
            fn geteuid() -> u32;
        }

        let repo_uid = fs::symlink_metadata(&repo.git_dir)?.uid();
        // SAFETY: libc geteuid has no preconditions and returns the current effective uid.
        let current_uid = unsafe { geteuid() };
        return Ok(repo_uid != current_uid);
    }
    #[cfg(not(unix))]
    {
        let _ = repo;
        Ok(false)
    }
}

fn repo_search_ceiling_dirs_at(base: &std::path::Path) -> Result<Vec<PathBuf>> {
    let Some(raw) = std::env::var_os("GIT_CEILING_DIRECTORIES") else {
        return Ok(Vec::new());
    };
    let mut preserve_symlinks = false;
    let mut ceilings = Vec::new();
    for path in std::env::split_paths(&raw) {
        let path = normalize_windows_input_path(path);
        if path.as_os_str().is_empty() {
            preserve_symlinks = true;
            continue;
        }
        let absolute = if path.is_absolute() {
            path
        } else {
            base.join(path)
        };
        ceilings.push(if preserve_symlinks {
            absolute
        } else {
            canonical_or_absolute(absolute)
        });
        preserve_symlinks = false;
    }
    Ok(ceilings)
}

fn repo_search_stops_before_parent(dir: &std::path::Path, ceiling_dirs: &[PathBuf]) -> bool {
    let Some(parent) = dir.parent() else {
        return false;
    };
    let parent = canonical_or_absolute(parent.to_path_buf());
    ceiling_dirs.iter().any(|ceiling| *ceiling == parent)
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
    use super::{
        file_url_to_path, normalize_repo_relative_path, repo_search_stops_before_parent,
        trace_path_bytes,
    };
    use std::path::{Path, PathBuf};

    #[cfg(unix)]
    #[test]
    fn trace_path_bytes_preserves_raw_unix_pathnames() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let path = PathBuf::from(OsStr::from_bytes(b"/tmp/raw-\x80\\name\n"));
        assert_eq!(
            trace_path_bytes(&path).expect("trace path bytes"),
            b"/tmp/raw-\x80\\\\name\\n"
        );
    }

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

    #[test]
    fn repo_search_stops_before_parent_ceiling_dir() {
        let root = std::env::temp_dir().join("zmin-repo-search-ceiling-root");
        let child = root.join("non-repo");
        let ceilings = vec![root.clone()];

        assert!(repo_search_stops_before_parent(&child, &ceilings));
        assert!(!repo_search_stops_before_parent(Path::new("/"), &ceilings));
    }

    #[test]
    fn repo_relative_path_normalization_resolves_parent_components() {
        let normalized = normalize_repo_relative_path(Path::new("top/sub/../x"))
            .expect("normalize in-repository parent traversal");

        assert_eq!(normalized, Path::new("top/x"));
        assert!(normalize_repo_relative_path(Path::new("../outside")).is_err());
    }
}
