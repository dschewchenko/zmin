use zmin_git_core::{GitHashAlgorithm, ObjectId};

pub(crate) use zmin_cli_runtime::{CliError, CloneOptions, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BundleHead {
    pub(crate) id: ObjectId,
    pub(crate) name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BundleMetadata {
    pub(crate) version: u8,
    pub(crate) hash_algorithm: GitHashAlgorithm,
    pub(crate) filters: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BundleListMode {
    Unknown,
    All,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BundleListHeuristic {
    None,
    CreationToken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BundleListEntry {
    pub(crate) id: String,
    pub(crate) uri: Option<String>,
    pub(crate) filter: Option<String>,
    pub(crate) location: Option<String>,
    pub(crate) creation_token: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BundleList {
    pub(crate) base_uri: String,
    pub(crate) version: u8,
    pub(crate) mode: BundleListMode,
    pub(crate) heuristic: BundleListHeuristic,
    pub(crate) entries: Vec<BundleListEntry>,
    pub(crate) warnings: Vec<String>,
}

impl BundleList {
    pub(crate) fn ordered_entries(&self) -> Vec<&BundleListEntry> {
        let mut entries = self.entries.iter().collect::<Vec<_>>();
        if self.heuristic == BundleListHeuristic::None {
            return entries;
        }
        entries.sort_by(|left, right| {
            right
                .creation_token
                .unwrap_or_default()
                .cmp(&left.creation_token.unwrap_or_default())
                .then_with(|| left.id.cmp(&right.id))
        });
        entries
    }
}

impl BundleMetadata {
    pub(crate) fn filter_spec(&self) -> Option<String> {
        match self.filters.as_slice() {
            [] => None,
            [filter] => Some(filter.clone()),
            filters => {
                let mut combined = String::from("combine:");
                for (index, filter) in filters.iter().enumerate() {
                    if index != 0 {
                        combined.push('+');
                    }
                    for byte in filter.bytes() {
                        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                            combined.push(byte as char);
                        } else {
                            combined.push('%');
                            combined.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
                            combined.push(char::from(b"0123456789ABCDEF"[(byte & 0x0f) as usize]));
                        }
                    }
                }
                Some(combined)
            }
        }
    }
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
