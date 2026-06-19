use std::fs;
use std::io;
use std::path::Path;

use super::{CliError, GitIndex, GitRepo, Result};

pub(crate) fn remove_worktree_path(repo: &GitRepo, path: &[u8]) -> Result<()> {
    let absolute = repo.root.join(String::from_utf8_lossy(path).as_ref());
    match fs::symlink_metadata(&absolute) {
        Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
            fs::remove_file(&absolute)?;
            remove_empty_parent_dirs(&repo.root, absolute.parent())?;
        }
        Ok(metadata) if metadata.is_dir() => {
            remove_empty_parent_dirs(&repo.root, Some(&absolute))?;
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(CliError::Io(error)),
    }
    Ok(())
}

pub(crate) fn remove_tracked_paths_missing_from_target(
    repo: &GitRepo,
    old_index: &GitIndex,
    new_index: &GitIndex,
) -> Result<()> {
    let old_entries = old_index.entries();
    let new_entries = new_index.entries();
    let mut new_pos = new_entries.len();

    for (old_pos, entry) in old_entries.iter().enumerate().rev() {
        let path = entry.path.as_slice();
        if old_entries
            .get(old_pos + 1)
            .is_some_and(|next| next.path.as_slice() == path)
        {
            continue;
        }
        while new_pos > 0 && new_entries[new_pos - 1].path.as_slice() > path {
            new_pos -= 1;
        }
        if new_pos > 0 && new_entries[new_pos - 1].path.as_slice() == path {
            continue;
        }

        let absolute = repo.root.join(String::from_utf8_lossy(path).as_ref());
        match fs::symlink_metadata(&absolute) {
            Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
                fs::remove_file(&absolute)?;
                remove_empty_parent_dirs(&repo.root, absolute.parent())?;
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    Ok(())
}

fn remove_empty_parent_dirs(root: &Path, mut dir: Option<&Path>) -> Result<()> {
    while let Some(path) = dir {
        if path == root || !path.starts_with(root) {
            break;
        }
        match fs::remove_dir(path) {
            Ok(()) => dir = path.parent(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(_) if dir_has_entries(path)? => break,
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    Ok(())
}

fn dir_has_entries(path: &Path) -> Result<bool> {
    match fs::read_dir(path) {
        Ok(mut entries) => Ok(entries.next().is_some()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CliError::Io(error)),
    }
}
