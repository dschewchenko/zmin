use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitRepositoryOptions {
    pub bare: bool,
    pub initial_branch: String,
}

impl Default for InitRepositoryOptions {
    fn default() -> Self {
        Self {
            bare: false,
            initial_branch: "main".to_owned(),
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

    fs::create_dir_all(git_dir.join("objects/info"))?;
    fs::create_dir_all(git_dir.join("objects/pack"))?;
    fs::create_dir_all(git_dir.join("refs/heads"))?;
    fs::create_dir_all(git_dir.join("refs/tags"))?;
    fs::create_dir_all(git_dir.join("branches"))?;
    fs::create_dir_all(git_dir.join("hooks"))?;
    fs::create_dir_all(git_dir.join("info"))?;
    let exclude = git_dir.join("info/exclude");
    if !exclude.exists() {
        fs::write(exclude, default_exclude_contents())?;
    }
    if !options.bare {
        fs::create_dir_all(worktree)?;
    }

    fs::write(
        git_dir.join("HEAD"),
        format!("ref: refs/heads/{}\n", options.initial_branch),
    )?;
    fs::write(
        git_dir.join("description"),
        "Unnamed repository; edit this file 'description' to name the repository.\n",
    )?;
    fs::write(git_dir.join("config"), config_contents(options.bare))?;

    Ok(InitRepositoryResult {
        worktree: worktree.to_path_buf(),
        git_dir,
    })
}

fn default_exclude_contents() -> &'static str {
    "# git ls-files --others --exclude-from=.git/info/exclude\n# Lines that start with '#' are comments.\n# For a project mostly in C, the following would be a good set of\n# exclude patterns (uncomment them if you want to use them):\n# *.[oa]\n# *~\n"
}

fn config_contents(bare: bool) -> String {
    let filemode = if cfg!(unix) { "true" } else { "false" };
    format!(
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = {filemode}\n\tbare = {}\n\tlogallrefupdates = {}\n",
        if bare { "true" } else { "false" },
        if bare { "false" } else { "true" },
    )
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
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn initializes_repository_readable_by_stock_git() {
        let dir = TempDir::new().expect("temp repo");
        init_repository(
            dir.path(),
            InitRepositoryOptions {
                bare: false,
                initial_branch: "trunk".to_owned(),
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
            },
        )
        .expect("init bare repo");

        assert_eq!(git(&dir, ["rev-parse", "--is-bare-repository"]), "true");
        assert_eq!(git(&dir, ["symbolic-ref", "HEAD"]), "refs/heads/main");
    }

    fn git<const N: usize>(repo: &TempDir, args: [&str; N]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo.path())
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git stdout utf8")
            .trim_end_matches('\n')
            .to_owned()
    }
}
