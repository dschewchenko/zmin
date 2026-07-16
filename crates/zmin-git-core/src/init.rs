use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitRepositoryOptions {
    pub bare: bool,
    pub initial_branch: String,
    pub objects_directory: Option<PathBuf>,
    pub populate_template_files: bool,
    pub write_log_all_ref_updates: bool,
}

impl Default for InitRepositoryOptions {
    fn default() -> Self {
        Self {
            bare: false,
            initial_branch: "main".to_owned(),
            objects_directory: None,
            populate_template_files: true,
            write_log_all_ref_updates: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitRepositoryResult {
    pub worktree: PathBuf,
    pub git_dir: PathBuf,
}

pub fn init_repository(
    directory: impl AsRef<Path>,
    options: InitRepositoryOptions,
) -> io::Result<InitRepositoryResult> {
    validate_ref_name_component(&options.initial_branch)?;

    let worktree = directory.as_ref();
    let git_dir = if options.bare {
        worktree.to_path_buf()
    } else {
        worktree.join(".git")
    };

    fs::create_dir_all(git_dir.join("refs/heads"))?;
    fs::create_dir_all(git_dir.join("refs/tags"))?;
    if !options.bare {
        fs::create_dir_all(worktree)?;
    }
    if options.populate_template_files {
        fs::create_dir_all(git_dir.join("hooks"))?;
        write_default_sample_hooks(&git_dir)?;
        fs::create_dir_all(git_dir.join("info"))?;
        let exclude = git_dir.join("info/exclude");
        if !exclude.exists() {
            fs::write(exclude, default_exclude_contents())?;
        }
    }

    fs::write(
        git_dir.join("HEAD"),
        format!("ref: refs/heads/{}\n", options.initial_branch),
    )?;
    if options.populate_template_files {
        fs::write(
            git_dir.join("description"),
            "Unnamed repository; edit this file 'description' to name the repository.\n",
        )?;
    }
    fs::write(
        git_dir.join("config"),
        config_contents(
            options.bare,
            detect_case_insensitive_filesystem(worktree),
            options.write_log_all_ref_updates,
        ),
    )?;

    let objects_directory = options
        .objects_directory
        .unwrap_or_else(|| git_dir.join("objects"));
    fs::create_dir_all(&objects_directory)?;
    fs::create_dir_all(objects_directory.join("info"))?;
    fs::create_dir_all(objects_directory.join("pack"))?;

    Ok(InitRepositoryResult {
        worktree: worktree.to_path_buf(),
        git_dir,
    })
}

fn write_default_sample_hooks(git_dir: &Path) -> io::Result<()> {
    for name in [
        "applypatch-msg.sample",
        "commit-msg.sample",
        "fsmonitor-watchman.sample",
        "post-update.sample",
        "pre-applypatch.sample",
        "pre-commit.sample",
        "pre-merge-commit.sample",
        "pre-push.sample",
        "pre-rebase.sample",
        "pre-receive.sample",
        "prepare-commit-msg.sample",
        "push-to-checkout.sample",
        "sendemail-validate.sample",
        "update.sample",
    ] {
        let path = git_dir.join("hooks").join(name);
        if !path.exists() {
            fs::write(path, "#!/bin/sh\n")?;
        }
    }
    Ok(())
}

fn default_exclude_contents() -> &'static str {
    "# git ls-files --others --exclude-from=.git/info/exclude\n# Lines that start with '#' are comments.\n# For a project mostly in C, the following would be a good set of\n# exclude patterns (uncomment them if you want to use them):\n# *.[oa]\n# *~\n"
}

fn config_contents(bare: bool, ignorecase: bool, write_log_all_ref_updates: bool) -> String {
    let filemode = if cfg!(unix) { "true" } else { "false" };
    let mut config = format!(
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = {filemode}\n\tbare = {}\n",
        if bare { "true" } else { "false" },
    );
    if write_log_all_ref_updates {
        config.push_str(&format!(
            "\tlogallrefupdates = {}\n",
            if bare { "false" } else { "true" },
        ));
    }
    if ignorecase {
        config.push_str("\tignorecase = true\n");
    }
    config
}

fn detect_case_insensitive_filesystem(worktree: &Path) -> bool {
    if !worktree.is_dir() {
        return false;
    }
    let upper = worktree.join(".zmin-ignorecase-probe");
    let lower = worktree.join(".zmin-IGNORECASE-probe");
    let detected = fs::write(&upper, b"probe")
        .ok()
        .and_then(|_| fs::symlink_metadata(&lower).ok())
        .is_some();
    let _ = fs::remove_file(&upper);
    let _ = fs::remove_file(&lower);
    detected
}

fn validate_ref_name_component(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name.starts_with('-')
        || name.starts_with('/')
        || name.ends_with('/')
        || name.ends_with(".lock")
        || name.contains("..")
        || name.contains("//")
        || name.bytes().any(|byte| {
            matches!(
                byte,
                0..=32 | 127 | b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\'
            )
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid initial branch name",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::stock_git_support;

    #[test]
    fn initializes_repository_readable_by_stock_git() {
        let dir = TempDir::new().expect("temp repo");
        init_repository(
            dir.path(),
            InitRepositoryOptions {
                bare: false,
                initial_branch: "trunk".to_owned(),
                objects_directory: None,
                populate_template_files: true,
                write_log_all_ref_updates: true,
            },
        )
        .expect("init repo");

        assert_eq!(git(&dir, ["rev-parse", "--git-dir"]), ".git");
        assert_eq!(git(&dir, ["symbolic-ref", "HEAD"]), "refs/heads/trunk");
        assert_eq!(git(&dir, ["config", "--get", "core.bare"]), "false");
    }

    #[test]
    fn initializes_bare_repository_readable_by_stock_git() {
        let dir = TempDir::new().expect("temp repo");
        init_repository(
            dir.path(),
            InitRepositoryOptions {
                bare: true,
                initial_branch: "main".to_owned(),
                objects_directory: None,
                populate_template_files: true,
                write_log_all_ref_updates: true,
            },
        )
        .expect("init bare repo");

        assert_eq!(git(&dir, ["rev-parse", "--is-bare-repository"]), "true");
        assert_eq!(git(&dir, ["symbolic-ref", "HEAD"]), "refs/heads/main");
    }

    fn git<const N: usize>(repo: &TempDir, args: [&str; N]) -> String {
        stock_git_support::git(repo, &args)
    }
}
