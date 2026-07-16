use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const DEFAULT_AUTHOR_NAME: &str = "Zmin Test";
const DEFAULT_AUTHOR_EMAIL: &str = "zmin@example.invalid";
const DEFAULT_AUTHOR_DATE: &str = "1700000000 +0000";

pub fn git_init_main() -> TempDir {
    let repo = TempDir::new().expect("temp repo");
    let output = Command::new(stock_git_bin())
        .arg("init")
        .args(["-b", "main"])
        .arg("--quiet")
        .current_dir(repo.path())
        .output()
        .expect("run git init");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    repo
}

pub fn git(repo: &TempDir, args: &[&str]) -> String {
    String::from_utf8(git_raw(repo, args))
        .expect("git stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

pub fn git_raw(repo: &TempDir, args: &[&str]) -> Vec<u8> {
    let output = git_output(repo, args);
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

pub fn git_output(repo: &TempDir, args: &[&str]) -> Output {
    git_output_at(repo.path(), args)
}

pub fn git_output_at(cwd: &Path, args: &[&str]) -> Output {
    Command::new(stock_git_bin())
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git")
}

pub fn git_env(repo: &TempDir, args: &[&str]) {
    let output = Command::new(stock_git_bin())
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(repo.path())
        .env("GIT_AUTHOR_NAME", DEFAULT_AUTHOR_NAME)
        .env("GIT_AUTHOR_EMAIL", DEFAULT_AUTHOR_EMAIL)
        .env("GIT_AUTHOR_DATE", DEFAULT_AUTHOR_DATE)
        .env("GIT_COMMITTER_NAME", DEFAULT_AUTHOR_NAME)
        .env("GIT_COMMITTER_EMAIL", DEFAULT_AUTHOR_EMAIL)
        .env("GIT_COMMITTER_DATE", DEFAULT_AUTHOR_DATE)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn git_config_add(cwd: &Path, key: &str, value: &str) {
    let output = git_output_at(cwd, &["config", "--add", key, value]);
    assert!(
        output.status.success(),
        "git config failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn stock_git_bin() -> PathBuf {
    for key in ["ZMIN_STOCK_GIT", "GIT_BIN"] {
        if let Ok(value) = std::env::var(key)
            && !value.trim().is_empty()
        {
            let path = PathBuf::from(value);
            assert!(is_stock_git(&path), "{key} does not point to stock Git");
            return path;
        }
    }
    for path in stock_git_candidates() {
        if is_stock_git(&path) {
            return path;
        }
    }
    for path in std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .flat_map(|dir| {
            stock_git_names()
                .into_iter()
                .map(move |name| dir.join(name))
        })
    {
        if is_stock_git(&path) {
            return path;
        }
    }
    panic!("could not find stock Git; set ZMIN_STOCK_GIT to a Git binary");
}

fn stock_git_candidates() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![
            PathBuf::from(r"C:\Program Files\Git\cmd\git.exe"),
            PathBuf::from(r"C:\Program Files\Git\bin\git.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Git\cmd\git.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Git\bin\git.exe"),
        ]
    }
    #[cfg(not(windows))]
    {
        vec![PathBuf::from("/usr/bin/git"), PathBuf::from("/bin/git")]
    }
}

fn stock_git_names() -> Vec<&'static str> {
    if cfg!(windows) {
        vec!["git.exe", "git"]
    } else {
        vec!["git"]
    }
}

fn is_stock_git(path: &Path) -> bool {
    let Ok(output) = Command::new(path).arg("--version").output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let version = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    version.starts_with("git version ") && !version.contains("zmin")
}
