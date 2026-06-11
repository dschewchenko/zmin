use std::io;
use std::path::PathBuf;

use skron_git_core::ObjectId;

pub(crate) type Result<T> = std::result::Result<T, CliError>;

#[derive(Debug)]
pub(crate) enum CliError {
    Exit(i32),
    Fatal { code: i32, message: String },
    Stderr { code: i32, text: String },
    Message(String),
    Io(io::Error),
}

impl From<io::Error> for CliError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BundleHead {
    pub(crate) id: ObjectId,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrailerPlacement {
    End,
    Start,
    After,
    Before,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrailerIfExists {
    AddIfDifferentNeighbor,
    AddIfDifferent,
    Add,
    Replace,
    DoNothing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrailerIfMissing {
    Add,
    DoNothing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrailerEntry {
    pub(crate) lines: Vec<String>,
    pub(crate) key: String,
    pub(crate) value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchMode {
    Check,
    Contents,
    Command,
}

pub(crate) struct CloneOptions {
    pub(crate) quiet: bool,
    pub(crate) configs: Vec<String>,
    pub(crate) template: Option<PathBuf>,
    pub(crate) reject_shallow: bool,
    pub(crate) recurse_submodules: Vec<String>,
    pub(crate) remote_submodules: bool,
    pub(crate) shallow_submodules: bool,
    pub(crate) bare: bool,
    pub(crate) mirror: bool,
    pub(crate) no_checkout: bool,
    pub(crate) remote_name: String,
    pub(crate) no_tags: bool,
    pub(crate) single_branch: bool,
    pub(crate) no_single_branch: bool,
    pub(crate) separate_git_dir: Option<PathBuf>,
    pub(crate) references: Vec<PathBuf>,
    pub(crate) reference_if_able: Vec<PathBuf>,
    pub(crate) shared: bool,
    pub(crate) dissociate: bool,
    pub(crate) no_hardlinks: bool,
    pub(crate) depth: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) keep_partial_on_missing_branch: bool,
    pub(crate) repository: String,
    pub(crate) directory: Option<PathBuf>,
}
