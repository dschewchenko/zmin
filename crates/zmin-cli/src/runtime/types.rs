use zmin_git_core::ObjectId;

pub(crate) use zmin_cli_runtime::{CliError, CloneOptions, Result};

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
