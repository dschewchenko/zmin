use std::path::{Component, Path, PathBuf};

use zmin_git_core::{GitIndex, IndexEntry, IndexMode};

use super::{
    CliError, GitRepo, Result, absolute_path_from_arg, normalize_git_path, pathspec_matches,
    repo_relative_path,
};

pub(crate) fn parse_index_mode(mode: &str) -> Result<IndexMode> {
    let bits = u32::from_str_radix(mode, 8).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("invalid object type '{mode}'"),
    })?;
    Ok(IndexMode::from_bits(bits)?)
}

pub(crate) fn find_index_entry<'a>(index: &'a GitIndex, path: &[u8]) -> Option<&'a IndexEntry> {
    index.entry(path, 0)
}

pub(crate) fn matching_index_entries(index: &GitIndex, pathspec: &[u8]) -> Vec<IndexEntry> {
    let pathspecs = [pathspec.to_vec()];
    index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && pathspec_matches(&entry.path, &pathspecs))
        .cloned()
        .collect()
}

pub(crate) fn path_arg_to_repo_relative(repo: &GitRepo, path: &Path) -> Result<Vec<u8>> {
    path_arg_to_repo_relative_inner(repo, path, false)
}

pub(crate) fn path_arg_to_repo_relative_allow_root(repo: &GitRepo, path: &Path) -> Result<Vec<u8>> {
    path_arg_to_repo_relative_inner(repo, path, true)
}

fn path_arg_to_repo_relative_inner(
    repo: &GitRepo,
    path: &Path,
    allow_root: bool,
) -> Result<Vec<u8>> {
    if let Some(relative) = pathspec_arg_to_repo_relative(repo, path, allow_root)? {
        return Ok(relative);
    }
    let raw = path.to_string_lossy();
    let unescaped = unescape_pathspec_literal_arg(&raw);
    let path_for_lookup;
    let path = if unescaped.as_ref() == raw.as_ref() {
        path
    } else {
        path_for_lookup = PathBuf::from(unescaped.into_owned());
        &path_for_lookup
    };
    let absolute = lexical_normalize_path(&absolute_path_from_arg(path)?);
    let mut relative = repo_relative_path(&repo.root, &absolute)?;
    if relative.is_empty() && !allow_root {
        return Err(CliError::Fatal {
            code: 128,
            message: "pathspec resolves to repository root".into(),
        });
    }
    let normalized = normalize_git_path(&String::from_utf8_lossy(&relative))?;
    relative = normalized.into_bytes();
    if relative.is_empty() && !allow_root {
        return Err(CliError::Fatal {
            code: 128,
            message: "pathspec resolves to repository root".into(),
        });
    }
    Ok(relative)
}

fn lexical_normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

pub(crate) fn unescape_pathspec_literal_arg(raw: &str) -> std::borrow::Cow<'_, str> {
    if !raw.as_bytes().contains(&b'\\') {
        return std::borrow::Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\'
            && let Some(next) = chars.peek().copied()
            && matches!(next, '[' | ']' | '*' | '?' | '\\')
        {
            out.push(next);
            chars.next();
            continue;
        }
        out.push(ch);
    }
    std::borrow::Cow::Owned(out)
}

fn pathspec_arg_to_repo_relative(
    repo: &GitRepo,
    path: &Path,
    allow_root: bool,
) -> Result<Option<Vec<u8>>> {
    let raw = path.to_string_lossy();
    if let Some(pattern) = raw.strip_prefix(":/") {
        return Ok(Some(pathspec_magic_with_root_pattern(
            ":/", pattern, allow_root,
        )?));
    }
    if let Some(pattern) = raw.strip_prefix(":!") {
        return pathspec_magic_with_cwd_pattern(repo, ":!", pattern, allow_root).map(Some);
    }
    if let Some(pattern) = raw.strip_prefix(":^") {
        return pathspec_magic_with_cwd_pattern(repo, ":^", pattern, allow_root).map(Some);
    }
    if let Some(rest) = raw.strip_prefix(":(")
        && let Some(close) = rest.find(')')
    {
        let magic = &rest[..close];
        let pattern = &rest[close + 1..];
        let prefix = &raw[..close + 3];
        if magic.split(',').any(|token| token == "top") {
            return Ok(Some(pathspec_magic_with_root_pattern(
                prefix, pattern, allow_root,
            )?));
        }
        return pathspec_magic_with_cwd_pattern(repo, prefix, pattern, allow_root).map(Some);
    }
    Ok(None)
}

fn pathspec_magic_with_root_pattern(
    prefix: &str,
    pattern: &str,
    allow_root: bool,
) -> Result<Vec<u8>> {
    let normalized = normalize_git_path(pattern)?;
    if normalized.is_empty() && !allow_root {
        return Err(CliError::Fatal {
            code: 128,
            message: "pathspec resolves to repository root".into(),
        });
    }
    let mut out = prefix.as_bytes().to_vec();
    out.extend_from_slice(normalized.as_bytes());
    Ok(out)
}

fn pathspec_magic_with_cwd_pattern(
    repo: &GitRepo,
    prefix: &str,
    pattern: &str,
    allow_root: bool,
) -> Result<Vec<u8>> {
    let relative = path_arg_to_repo_relative_inner(repo, Path::new(pattern), allow_root)?;
    let mut out = prefix.as_bytes().to_vec();
    out.extend_from_slice(&relative);
    Ok(out)
}

pub(crate) fn remove_index_path_or_dir(index: &mut GitIndex, path: &[u8]) -> Result<()> {
    index.remove_path(path)?;
    index.remove_dir(path)?;
    Ok(())
}

pub(crate) fn path_dir_prefix(path: &[u8]) -> Vec<u8> {
    let mut prefix = path.to_vec();
    if !prefix.ends_with(b"/") {
        prefix.push(b'/');
    }
    prefix
}

pub(crate) fn path_join_bytes(parent: &[u8], child: &[u8]) -> Vec<u8> {
    if parent.is_empty() {
        return child.to_vec();
    }
    let mut out = parent.to_vec();
    if !out.ends_with(b"/") {
        out.push(b'/');
    }
    out.extend_from_slice(child);
    out
}

pub(crate) fn index_mode_octal(mode: IndexMode) -> &'static str {
    match mode {
        IndexMode::File => "100644",
        IndexMode::Executable => "100755",
        IndexMode::Symlink => "120000",
        IndexMode::Tree => "040000",
        IndexMode::Gitlink => "160000",
    }
}

#[cfg(unix)]
pub(crate) fn index_mode_for_metadata(metadata: &std::fs::Metadata) -> IndexMode {
    use std::os::unix::fs::PermissionsExt;
    if metadata.permissions().mode() & 0o111 != 0 {
        IndexMode::Executable
    } else {
        IndexMode::File
    }
}

#[cfg(not(unix))]
pub(crate) fn index_mode_for_metadata(_metadata: &std::fs::Metadata) -> IndexMode {
    IndexMode::File
}
