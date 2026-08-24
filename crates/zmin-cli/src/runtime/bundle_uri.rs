use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::{BundleList, BundleListEntry, BundleListHeuristic, BundleListMode, CliError, Result};

pub(crate) const MAX_BUNDLE_URI_DEPTH: usize = 4;
const MAX_BUNDLE_LIST_BYTES: usize = 4 * 1024 * 1024;
const MAX_BUNDLE_LIST_LINES: usize = 100_000;
const MAX_BUNDLE_LIST_LINE_BYTES: usize = 64 * 1024;
const MAX_BUNDLE_LIST_ENTRIES: usize = 100_000;
const MAX_BUNDLE_LIST_ID_BYTES: usize = 256;
const MAX_BUNDLE_HEADER_PROBE_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BundleListSource {
    pub(crate) uri: String,
    pub(crate) contents: String,
}

/// A source loader is deliberately injected so bundle acquisition can be
/// tested without network code and the later transport layer can choose its
/// own HTTP/SSH/file implementation.
pub(crate) trait BundleSourceLoader {
    fn load(&mut self, uri: &str) -> Result<BundleLoadedSource>;
}

impl<F> BundleSourceLoader for F
where
    F: FnMut(&str) -> Result<BundleLoadedSource>,
{
    fn load(&mut self, uri: &str) -> Result<BundleLoadedSource> {
        self(uri)
    }
}

#[derive(Debug)]
pub(crate) struct BundleLoadedSource {
    storage: BundleSourceStorage,
}

#[derive(Debug)]
enum BundleSourceStorage {
    Memory(Vec<u8>),
    Path(PathBuf),
    OwnedTemp(BundleTempGuard),
}

impl BundleLoadedSource {
    pub(crate) fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            storage: BundleSourceStorage::Memory(bytes.into()),
        }
    }

    pub(crate) fn from_path(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        validate_source_file(&path)?;
        Ok(Self {
            storage: BundleSourceStorage::Path(path),
        })
    }

    pub(crate) fn from_owned_temp(temp_path: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self {
            storage: BundleSourceStorage::OwnedTemp(BundleTempGuard::new(temp_path)?),
        })
    }

    fn is_direct_bundle(&self) -> Result<bool> {
        match &self.storage {
            BundleSourceStorage::Memory(bytes) => Ok(is_direct_bundle_payload(bytes)),
            BundleSourceStorage::Path(path) => is_direct_bundle_file(path),
            BundleSourceStorage::OwnedTemp(guard) => is_direct_bundle_file(guard.path()),
        }
    }

    fn read_list_contents(&self, uri: &str) -> Result<String> {
        match &self.storage {
            BundleSourceStorage::Memory(bytes) => bounded_utf8_bundle_list(bytes, uri),
            BundleSourceStorage::Path(path) => read_bounded_bundle_list_file(path, uri),
            BundleSourceStorage::OwnedTemp(guard) => {
                read_bounded_bundle_list_file(guard.path(), uri)
            }
        }
    }

    fn into_acquisition_storage(self) -> BundleAcquisitionStorage {
        match self.storage {
            BundleSourceStorage::Memory(bytes) => BundleAcquisitionStorage::Memory(bytes),
            BundleSourceStorage::Path(path) => BundleAcquisitionStorage::Path(path),
            BundleSourceStorage::OwnedTemp(guard) => BundleAcquisitionStorage::OwnedTemp(guard),
        }
    }
}

#[derive(Debug)]
pub(crate) enum BundleAcquisitionStorage {
    Memory(Vec<u8>),
    Path(PathBuf),
    OwnedTemp(BundleTempGuard),
}

impl BundleAcquisitionStorage {
    pub(crate) fn path(&self) -> Option<&Path> {
        match self {
            Self::Memory(_) => None,
            Self::Path(path) => Some(path),
            Self::OwnedTemp(guard) => Some(guard.path()),
        }
    }
}

/// Owns a loader-created temporary source until the acquisition plan is
/// either handed to the installer or dropped after a failure.
#[derive(Debug)]
pub(crate) struct BundleTempGuard {
    path: PathBuf,
    armed: bool,
    identity: BundleTempIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BundleTempIdentity {
    len: u64,
    modified_nanos: u128,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl BundleTempGuard {
    pub(crate) fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let identity = source_file_identity(&path)?;
        Ok(Self {
            path,
            armed: true,
            identity,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn release(mut self) -> PathBuf {
        self.armed = false;
        std::mem::take(&mut self.path)
    }
}

impl Drop for BundleTempGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if source_file_identity(&self.path)
            .ok()
            .is_some_and(|identity| identity == self.identity)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BundleAcquisitionKind {
    Direct,
    ListEntry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BundleAcquisitionMetadata {
    pub(crate) id: Option<String>,
    pub(crate) creation_token: Option<u64>,
    pub(crate) filter: Option<String>,
    pub(crate) location: Option<String>,
}

#[derive(Debug)]
pub(crate) struct BundleAcquisitionSource {
    pub(crate) uri: String,
    pub(crate) kind: BundleAcquisitionKind,
    pub(crate) storage: BundleAcquisitionStorage,
    pub(crate) metadata: BundleAcquisitionMetadata,
}

#[derive(Debug)]
pub(crate) struct BundleAcquisitionPlan {
    pub(crate) sources: Vec<BundleAcquisitionSource>,
    pub(crate) warnings: Vec<BundleAcquisitionWarning>,
    pub(crate) failures: Vec<BundleAcquisitionFailure>,
    root_mode: BundleListMode,
    heuristic: BundleListHeuristic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BundleAcquisitionFailure {
    pub(crate) uri: String,
    pub(crate) metadata: BundleAcquisitionMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BundleAcquisitionWarning {
    List { uri: String, message: String },
    Candidate { uri: String, message: String },
}

impl fmt::Display for BundleAcquisitionWarning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::List { uri, message } => write!(formatter, "bundle-uri: {uri}: {message}"),
            Self::Candidate { uri, message } => {
                write!(
                    formatter,
                    "bundle-uri: failed to acquire '{uri}': {message}"
                )
            }
        }
    }
}

impl BundleAcquisitionPlan {
    pub(crate) fn mode(&self) -> BundleListMode {
        self.root_mode
    }

    pub(crate) fn heuristic(&self) -> BundleListHeuristic {
        self.heuristic
    }

    pub(crate) fn download_order(&self) -> Vec<&BundleAcquisitionSource> {
        self.ordered_sources(true)
    }

    pub(crate) fn import_order(&self) -> Vec<&BundleAcquisitionSource> {
        self.ordered_sources(false)
    }

    fn ordered_sources(&self, descending: bool) -> Vec<&BundleAcquisitionSource> {
        let mut sources = self.sources.iter().collect::<Vec<_>>();
        if self.heuristic == BundleListHeuristic::None {
            return sources;
        }
        sources.sort_by(|left, right| {
            let left_token = left.metadata.creation_token.unwrap_or_default();
            let right_token = right.metadata.creation_token.unwrap_or_default();
            let token_order = if descending {
                right_token.cmp(&left_token)
            } else {
                left_token.cmp(&right_token)
            };
            token_order.then_with(|| source_sort_key(left).cmp(&source_sort_key(right)))
        });
        sources
    }
}

fn source_sort_key(source: &BundleAcquisitionSource) -> (&str, &str) {
    (
        source.metadata.id.as_deref().unwrap_or(""),
        source.uri.as_str(),
    )
}

/// Read a local path or `file://` URI into owned bytes.  This is intentionally
/// only a loader helper; it does not infer or install Git objects.
pub(crate) fn load_bundle_source_from_filesystem(uri: &str) -> Result<BundleLoadedSource> {
    let path = filesystem_path_from_uri(uri)?;
    BundleLoadedSource::from_path(path)
}

fn filesystem_path_from_uri(uri: &str) -> Result<PathBuf> {
    if let Some(path) = uri.strip_prefix("file://") {
        if path.is_empty() {
            return Err(bundle_list_error("file URI path is empty"));
        }
        return Ok(PathBuf::from(path));
    }
    if has_uri_scheme(uri) {
        return Err(bundle_list_error(format!(
            "filesystem loader does not support URI '{uri}'"
        )));
    }
    Ok(PathBuf::from(uri))
}

fn validate_source_file(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path).map_err(|error| {
        bundle_list_error(format!(
            "failed to inspect bundle source '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(bundle_list_error(format!(
            "bundle source '{}' is not a regular file",
            path.display()
        )));
    }
    Ok(())
}

fn source_file_identity(path: &Path) -> Result<BundleTempIdentity> {
    let link_metadata = fs::symlink_metadata(path).map_err(|error| CliError::Io(error))?;
    if !link_metadata.file_type().is_file() {
        return Err(bundle_list_error(format!(
            "owned bundle source '{}' is not a regular file",
            path.display()
        )));
    }
    let metadata = fs::metadata(path).map_err(|error| CliError::Io(error))?;
    if !metadata.is_file() {
        return Err(bundle_list_error(format!(
            "owned bundle source '{}' is not a regular file",
            path.display()
        )));
    }
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |value| value.as_nanos());
    Ok(BundleTempIdentity {
        len: metadata.len(),
        modified_nanos,
        #[cfg(unix)]
        device: std::os::unix::fs::MetadataExt::dev(&metadata),
        #[cfg(unix)]
        inode: std::os::unix::fs::MetadataExt::ino(&metadata),
    })
}

fn is_direct_bundle_file(path: &Path) -> Result<bool> {
    validate_source_file(path)?;
    let mut file = fs::File::open(path).map_err(CliError::Io)?;
    let mut probe = Vec::with_capacity(MAX_BUNDLE_HEADER_PROBE_BYTES);
    let mut limited = file.by_ref().take(MAX_BUNDLE_HEADER_PROBE_BYTES as u64);
    limited.read_to_end(&mut probe).map_err(CliError::Io)?;
    Ok(is_direct_bundle_payload(&probe))
}

fn bounded_utf8_bundle_list(bytes: &[u8], uri: &str) -> Result<String> {
    if bytes.len() > MAX_BUNDLE_LIST_BYTES {
        return Err(bundle_list_error(format!(
            "bundle list '{uri}' exceeds {MAX_BUNDLE_LIST_BYTES} bytes"
        )));
    }
    String::from_utf8(bytes.to_owned()).map_err(|_| {
        bundle_list_error(format!(
            "bundle source '{uri}' is neither a bundle nor UTF-8 bundle-list text"
        ))
    })
}

fn read_bounded_bundle_list_file(path: &Path, uri: &str) -> Result<String> {
    validate_source_file(path)?;
    let file = fs::File::open(path).map_err(CliError::Io)?;
    let mut bytes = Vec::new();
    file.take((MAX_BUNDLE_LIST_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(CliError::Io)?;
    bounded_utf8_bundle_list(&bytes, uri)
}

/// Acquire a direct bundle or recursively resolve a bundle list.  The loader
/// is the only I/O boundary; HTTP(S) callers can inject their own source
/// implementation later without changing list semantics.
pub(crate) fn acquire_bundle_uri<L: BundleSourceLoader>(
    root_uri: &str,
    loader: &mut L,
) -> Result<BundleAcquisitionPlan> {
    let mut callback =
        |_: &BundleAcquisitionSource, _: BundleListMode, _: BundleListHeuristic| false;
    acquire_bundle_uri_with_callback(root_uri, loader, &mut callback)
}

pub(crate) fn acquire_bundle_uri_with_callback<L: BundleSourceLoader>(
    root_uri: &str,
    loader: &mut L,
    callback: &mut dyn FnMut(&BundleAcquisitionSource, BundleListMode, BundleListHeuristic) -> bool,
) -> Result<BundleAcquisitionPlan> {
    let mut state = BundleAcquisitionState {
        loader,
        callback,
        stack: Vec::new(),
        warnings: Vec::new(),
        failures: Vec::new(),
        root_mode: BundleListMode::Unknown,
        root_heuristic: BundleListHeuristic::None,
        stop: false,
    };
    let sources = state
        .acquire_uri(root_uri, 0, None)
        .map_err(|failure| failure.error)?;
    if sources.is_empty() {
        return Err(bundle_list_error("bundle acquisition produced no sources"));
    }
    Ok(BundleAcquisitionPlan {
        sources,
        warnings: state.warnings,
        failures: state.failures,
        root_mode: state.root_mode,
        heuristic: state.root_heuristic,
    })
}

struct BundleAcquisitionState<'a, L> {
    loader: &'a mut L,
    callback:
        &'a mut dyn FnMut(&BundleAcquisitionSource, BundleListMode, BundleListHeuristic) -> bool,
    stack: Vec<String>,
    warnings: Vec<BundleAcquisitionWarning>,
    failures: Vec<BundleAcquisitionFailure>,
    root_mode: BundleListMode,
    root_heuristic: BundleListHeuristic,
    stop: bool,
}

struct BundleAttemptFailure {
    error: CliError,
}

impl<'a, L: BundleSourceLoader> BundleAcquisitionState<'a, L> {
    fn acquire_uri(
        &mut self,
        uri: &str,
        depth: usize,
        metadata: Option<BundleAcquisitionMetadata>,
    ) -> std::result::Result<Vec<BundleAcquisitionSource>, BundleAttemptFailure> {
        if depth >= MAX_BUNDLE_URI_DEPTH {
            return Err(BundleAttemptFailure {
                error: bundle_list_error(format!(
                    "bundle URI recursion exceeds maximum depth {MAX_BUNDLE_URI_DEPTH}"
                )),
            });
        }
        if self.stack.iter().any(|candidate| candidate == uri) {
            return Err(BundleAttemptFailure {
                error: bundle_list_error(format!("bundle URI cycle detected at '{uri}'")),
            });
        }

        let loaded = self
            .loader
            .load(uri)
            .map_err(|error| BundleAttemptFailure { error })?;
        let direct = loaded
            .is_direct_bundle()
            .map_err(|error| BundleAttemptFailure { error })?;
        if direct {
            let source = BundleAcquisitionSource {
                uri: uri.to_owned(),
                kind: if metadata.is_some() {
                    BundleAcquisitionKind::ListEntry
                } else {
                    BundleAcquisitionKind::Direct
                },
                storage: loaded.into_acquisition_storage(),
                metadata: metadata.unwrap_or(BundleAcquisitionMetadata {
                    id: None,
                    creation_token: None,
                    filter: None,
                    location: None,
                }),
            };
            if (self.callback)(&source, self.root_mode, self.root_heuristic) {
                self.stop = true;
            }
            return Ok(vec![source]);
        }

        let contents = loaded
            .read_list_contents(uri)
            .map_err(|error| BundleAttemptFailure { error })?;
        self.stack.push(uri.to_owned());
        let list =
            parse_bundle_list(uri, &contents).map_err(|error| BundleAttemptFailure { error });
        let list = match list {
            Ok(list) => list,
            Err(failure) => {
                self.stack.pop();
                return Err(failure);
            }
        };
        if depth == 0 {
            self.root_mode = list.mode;
            self.root_heuristic = list.heuristic;
        }
        self.warnings
            .extend(
                list.warnings
                    .iter()
                    .cloned()
                    .map(|message| BundleAcquisitionWarning::List {
                        uri: uri.to_owned(),
                        message,
                    }),
            );

        let mut acquired = Vec::new();
        for entry in list.ordered_entries() {
            if self.stop {
                break;
            }
            let entry_uri = entry.uri.as_deref().expect("validated bundle URI entry");
            let entry_metadata = BundleAcquisitionMetadata {
                id: Some(entry.id.clone()),
                creation_token: entry.creation_token,
                filter: entry.filter.clone(),
                location: entry.location.clone(),
            };
            match self.acquire_uri(entry_uri, depth + 1, Some(entry_metadata.clone())) {
                Ok(mut sources) => {
                    let had_success = !sources.is_empty();
                    acquired.append(&mut sources);
                    if list.mode == BundleListMode::Any && had_success {
                        break;
                    }
                }
                Err(failure) => {
                    self.failures.push(BundleAcquisitionFailure {
                        uri: entry_uri.to_owned(),
                        metadata: entry_metadata,
                    });
                    self.warnings
                        .push(format_bundle_attempt_warning(entry_uri, &failure.error));
                }
            }
        }
        self.stack.pop();

        if acquired.is_empty() {
            return Err(BundleAttemptFailure {
                error: bundle_list_error(format!(
                    "bundle list '{uri}' did not yield an acquired bundle"
                )),
            });
        }
        Ok(acquired)
    }
}

fn format_bundle_attempt_warning(uri: &str, error: &CliError) -> BundleAcquisitionWarning {
    BundleAcquisitionWarning::Candidate {
        uri: uri.to_owned(),
        message: format!("{error:?}"),
    }
}

fn is_direct_bundle_payload(bytes: &[u8]) -> bool {
    let first_line = bytes
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    first_line.starts_with(b"# v2 git bundle") || first_line.starts_with(b"# v3 git bundle")
}

pub(crate) fn parse_bundle_list(uri: &str, contents: &str) -> Result<BundleList> {
    if contents.len() > MAX_BUNDLE_LIST_BYTES {
        return Err(bundle_list_error(format!(
            "bundle list '{uri}' exceeds {MAX_BUNDLE_LIST_BYTES} bytes"
        )));
    }
    let mut list = BundleList {
        base_uri: bundle_list_base_uri(uri),
        version: 1,
        mode: BundleListMode::All,
        heuristic: BundleListHeuristic::None,
        entries: Vec::new(),
        warnings: Vec::new(),
    };
    let mut section = BundleListSection::Global;

    for logical_line in bundle_list_logical_lines(contents)? {
        let line_number = logical_line.line_no;
        let mut line = trim_bundle_syntax(&logical_line.text);
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(after_open) = line.strip_prefix('[') {
            let Some((body, rest)) = after_open.split_once(']') else {
                return Err(bundle_list_line_error(
                    line_number,
                    "malformed section header",
                ));
            };
            section = parse_section(body)
                .map_err(|message| bundle_list_error(format!("line {line_number}: {message}")))?;
            line = trim_bundle_syntax(rest);
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
        }

        let Some((raw_key, raw_value)) = line.split_once('=') else {
            return Err(bundle_list_line_error(line_number, "expected key=value"));
        };
        let key = trim_bundle_key(raw_key);
        if key.is_empty() {
            return Err(bundle_list_line_error(line_number, "key must be non-empty"));
        }
        let value = parse_bundle_config_value(raw_value, line_number)?;
        let (line_section, key) = split_flat_bundle_key(&section, key)
            .map_err(|message| bundle_list_error(format!("line {line_number}: {message}")))?;
        update_bundle_list(&mut list, line_section, &key, &value)?;
    }

    if list.entries.iter().any(|entry| entry.uri.is_none()) {
        let id = list
            .entries
            .iter()
            .find(|entry| entry.uri.is_none())
            .map(|entry| entry.id.as_str())
            .unwrap_or("<unknown>");
        return Err(bundle_list_error(format!("bundle '{id}' has no uri")));
    }
    Ok(list)
}

pub(crate) fn resolve_bundle_uri(base_uri: &str, value: &str) -> Result<String> {
    if value.is_empty() {
        return Err(bundle_list_error("bundle URI is empty"));
    }
    if value
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(bundle_list_error(
            "bundle URI contains whitespace or control bytes",
        ));
    }
    if has_uri_scheme(value) || value.starts_with('/') {
        if value.starts_with('/') && base_uri.starts_with("file://") {
            let (prefix, _) = split_file_uri(base_uri)?;
            return Ok(format!("{prefix}{value}"));
        }
        if value.starts_with('/')
            && (base_uri.starts_with("http://") || base_uri.starts_with("https://"))
        {
            let authority = uri_authority(base_uri)?;
            return Ok(format!("{authority}{value}"));
        }
        return Ok(value.to_owned());
    }

    let base = bundle_list_base_uri(base_uri);
    let (prefix, path) = split_uri_path(&base)?;
    let mut components = path
        .split('/')
        .filter(|component| !component.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for component in value.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(bundle_list_error(format!(
                        "cannot resolve relative bundle URI '{value}' against '{base_uri}'"
                    )));
                }
            }
            component => components.push(component.to_owned()),
        }
    }
    let suffix = components.join("/");
    if prefix.is_empty() {
        Ok(suffix)
    } else {
        Ok(format!("{prefix}/{suffix}"))
    }
}

pub(crate) fn resolve_bundle_list_sources(
    root_uri: &str,
    sources: &BTreeMap<String, String>,
) -> Result<Vec<BundleList>> {
    let mut stack = Vec::new();
    let mut resolved = Vec::new();
    resolve_bundle_list_sources_at(root_uri, sources, 0, &mut stack, &mut resolved)?;
    Ok(resolved)
}

fn resolve_bundle_list_sources_at(
    uri: &str,
    sources: &BTreeMap<String, String>,
    depth: usize,
    stack: &mut Vec<String>,
    resolved: &mut Vec<BundleList>,
) -> Result<()> {
    if depth >= MAX_BUNDLE_URI_DEPTH {
        return Err(bundle_list_error(format!(
            "bundle URI recursion exceeds maximum depth {MAX_BUNDLE_URI_DEPTH}"
        )));
    }
    if stack.iter().any(|candidate| candidate == uri) {
        return Err(bundle_list_error(format!(
            "bundle URI cycle detected at '{uri}'"
        )));
    }
    let contents = sources
        .get(uri)
        .ok_or_else(|| bundle_list_error(format!("bundle list source is missing: {uri}")))?;
    stack.push(uri.to_owned());
    let list = parse_bundle_list(uri, contents)?;
    let child_uris = list
        .ordered_entries()
        .into_iter()
        .filter_map(|entry| entry.uri.clone())
        .filter(|child| sources.contains_key(child))
        .collect::<Vec<_>>();
    resolved.push(list);
    for child_uri in child_uris {
        resolve_bundle_list_sources_at(&child_uri, sources, depth + 1, stack, resolved)?;
    }
    stack.pop();
    Ok(())
}

fn update_bundle_list(
    list: &mut BundleList,
    section: BundleListSection,
    key: &str,
    value: &str,
) -> Result<()> {
    match section {
        BundleListSection::Global => match key.to_ascii_lowercase().as_str() {
            "version" => {
                let version = value.parse::<u8>().map_err(|_| {
                    bundle_list_error(format!("invalid bundle.version value '{value}'"))
                })?;
                if version != 1 {
                    return Err(bundle_list_error(format!(
                        "unsupported bundle.version value '{value}'"
                    )));
                }
                list.version = version;
            }
            "mode" => {
                list.mode = match value {
                    "all" => BundleListMode::All,
                    "any" => BundleListMode::Any,
                    _ => {
                        return Err(bundle_list_error(format!(
                            "invalid bundle.mode value '{value}'"
                        )));
                    }
                };
            }
            "heuristic" => {
                if value == "creationToken" {
                    list.heuristic = BundleListHeuristic::CreationToken;
                }
            }
            _ => {}
        },
        BundleListSection::Bundle(id) => {
            let base_uri = list.base_uri.clone();
            let entry = get_or_insert_entry(list, &id)?;
            match key.to_ascii_lowercase().as_str() {
                "uri" => {
                    if entry.uri.is_some() {
                        return Err(bundle_list_error(format!(
                            "bundle '{id}' has duplicate uri"
                        )));
                    }
                    entry.uri = Some(resolve_bundle_uri(&base_uri, value)?);
                }
                "creationtoken" => match value.parse::<u64>() {
                    Ok(token) => entry.creation_token = Some(token),
                    Err(_) => list.warnings.push(format!(
                        "could not parse bundle list key creationToken with value '{value}'"
                    )),
                },
                "filter" => entry.filter = Some(value.to_owned()),
                "location" => entry.location = Some(value.to_owned()),
                _ => {}
            }
        }
    }
    Ok(())
}

fn get_or_insert_entry<'a>(list: &'a mut BundleList, id: &str) -> Result<&'a mut BundleListEntry> {
    if let Some(index) = list.entries.iter().position(|entry| entry.id == id) {
        return Ok(&mut list.entries[index]);
    }
    if list.entries.len() >= MAX_BUNDLE_LIST_ENTRIES {
        return Err(bundle_list_error(format!(
            "bundle list exceeds {MAX_BUNDLE_LIST_ENTRIES} entries"
        )));
    }
    list.entries.push(BundleListEntry {
        id: id.to_owned(),
        uri: None,
        filter: None,
        location: None,
        creation_token: None,
    });
    Ok(list.entries.last_mut().expect("entry was just inserted"))
}

fn parse_section(body: &str) -> std::result::Result<BundleListSection, String> {
    let body = body.trim();
    if body.eq_ignore_ascii_case("bundle") {
        return Ok(BundleListSection::Global);
    }
    let Some((section, id)) = body.split_once(char::is_whitespace) else {
        return Err("malformed bundle section".to_owned());
    };
    if !section.eq_ignore_ascii_case("bundle") {
        return Err("malformed bundle section".to_owned());
    }
    let id = id.trim();
    let Some(id) = id.strip_prefix('"').and_then(|id| id.strip_suffix('"')) else {
        return Err("malformed bundle section".to_owned());
    };
    if id.is_empty()
        || id.len() > MAX_BUNDLE_LIST_ID_BYTES
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(format!("invalid bundle id '{id}'"));
    }
    Ok(BundleListSection::Bundle(id.to_owned()))
}

fn split_flat_bundle_key(
    section: &BundleListSection,
    key: &str,
) -> std::result::Result<(BundleListSection, String), String> {
    if !matches!(section, BundleListSection::Global) {
        return Ok((section.clone(), key.to_owned()));
    }
    let normalized = key.to_ascii_lowercase();
    let Some(rest) = key
        .get("bundle.".len()..)
        .filter(|_| normalized.starts_with("bundle."))
    else {
        return Ok((BundleListSection::Global, key.to_owned()));
    };
    if let Some((id, key)) = rest.split_once('.') {
        if id.is_empty() {
            return Err("bundle id is empty".to_owned());
        }
        if id.len() > MAX_BUNDLE_LIST_ID_BYTES {
            return Err(format!(
                "bundle id exceeds {MAX_BUNDLE_LIST_ID_BYTES} bytes"
            ));
        }
        if !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(format!("invalid bundle id '{id}'"));
        }
        return Ok((
            BundleListSection::Bundle(id.to_owned()),
            key.to_ascii_lowercase(),
        ));
    }
    Ok((BundleListSection::Global, rest.to_ascii_lowercase()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BundleListLogicalLine {
    text: String,
    line_no: usize,
}

// Keep bundle-list parsing aligned with Git's config syntax without making
// the private runtime config parser part of this foundation API.
fn bundle_list_logical_lines(contents: &str) -> Result<Vec<BundleListLogicalLine>> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut start_line = 1usize;
    let physical_count = contents.split('\n').count();
    if physical_count > MAX_BUNDLE_LIST_LINES {
        return Err(bundle_list_error(format!(
            "bundle list exceeds {MAX_BUNDLE_LIST_LINES} lines"
        )));
    }
    for (index, physical) in contents.split('\n').enumerate() {
        if physical.len() > MAX_BUNDLE_LIST_LINE_BYTES
            || current.len().saturating_add(physical.len()) > MAX_BUNDLE_LIST_LINE_BYTES
        {
            return Err(bundle_list_line_error(
                index + 1,
                format!("bundle list line exceeds {MAX_BUNDLE_LIST_LINE_BYTES} bytes"),
            ));
        }
        if current.is_empty() {
            start_line = index + 1;
        }
        current.push_str(physical);
        if bundle_list_physical_line_continues(physical) && index + 1 < physical_count {
            current.pop();
            continue;
        }
        lines.push(BundleListLogicalLine {
            text: std::mem::take(&mut current),
            line_no: start_line,
        });
    }
    Ok(lines)
}

fn bundle_list_physical_line_continues(line: &str) -> bool {
    let mut quoted = false;
    let mut escaped = false;
    let mut syntax_end = line.len();
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => quoted = !quoted,
            '#' | ';' if !quoted => {
                syntax_end = index;
                break;
            }
            _ => {}
        }
    }
    line[..syntax_end]
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

fn trim_bundle_syntax(value: &str) -> &str {
    value.trim_matches([' ', '\t', '\r'])
}

fn trim_bundle_key(value: &str) -> &str {
    value.trim_matches([' ', '\t', '\r'])
}

fn parse_bundle_config_value(value: &str, line_no: usize) -> Result<String> {
    let mut parsed = String::with_capacity(value.len());
    let mut quoted = false;
    let mut escaped = false;
    let mut keep_len = 0usize;
    for ch in value.trim_start_matches([' ', '\t']).chars() {
        if escaped {
            let decoded = match ch {
                'n' => '\n',
                't' => '\t',
                'b' => '\u{0008}',
                '"' => '"',
                '\\' => '\\',
                _ => return Err(bundle_list_line_error(line_no, "invalid escape")),
            };
            parsed.push(decoded);
            keep_len = parsed.len();
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => quoted = !quoted,
            '#' | ';' if !quoted => break,
            _ => {
                parsed.push(ch);
                if quoted || !matches!(ch, ' ' | '\t' | '\r') {
                    keep_len = parsed.len();
                }
            }
        }
    }
    if quoted || escaped {
        return Err(bundle_list_line_error(
            line_no,
            "unterminated quote or escape",
        ));
    }
    parsed.truncate(keep_len);
    Ok(parsed)
}

fn bundle_list_base_uri(uri: &str) -> String {
    if uri.ends_with('/') {
        return uri.to_owned();
    }
    uri.rsplit_once('/')
        .map(|(base, _)| format!("{base}/"))
        .unwrap_or_else(|| format!("{uri}/"))
}

fn has_uri_scheme(value: &str) -> bool {
    value.find(':').is_some_and(|colon| {
        colon > 0
            && value[..colon]
                .bytes()
                .all(|byte| byte.is_ascii_alphabetic())
    })
}

fn split_file_uri(uri: &str) -> Result<(&str, &str)> {
    let Some(path) = uri.strip_prefix("file://") else {
        return Err(bundle_list_error("invalid file URI base"));
    };
    let slash = path.find('/').unwrap_or(path.len());
    Ok((&uri[..uri.len() - path.len() + slash], &path[slash..]))
}

fn uri_authority(uri: &str) -> Result<&str> {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return Err(bundle_list_error("invalid URI base"));
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    Ok(&uri[..scheme.len() + 3 + authority_end])
}

fn split_uri_path(uri: &str) -> Result<(&str, &str)> {
    if let Some((scheme, rest)) = uri.split_once("://") {
        let slash = rest.find('/').unwrap_or(rest.len());
        let prefix_len = scheme.len() + 3 + slash;
        return Ok((&uri[..prefix_len], &uri[prefix_len..]));
    }
    if uri.starts_with('/') {
        return Ok(("", uri));
    }
    Ok(("", uri))
}

fn bundle_list_error(message: impl Into<String>) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("bundle-uri: {}", message.into()),
    }
}

fn bundle_list_line_error(line_no: usize, message: impl Into<String>) -> CliError {
    bundle_list_error(format!("line {line_no}: {}", message.into()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BundleListSection {
    Global,
    Bundle(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn direct_bundle_bytes(label: &str) -> Vec<u8> {
        let mut bytes = format!("# v2 git bundle\n{label}\n").into_bytes();
        bytes.extend_from_slice(&[0xff, 0x00]);
        bytes
    }

    fn map_loader(sources: BTreeMap<String, Vec<u8>>) -> impl BundleSourceLoader {
        move |uri: &str| {
            sources
                .get(uri)
                .cloned()
                .map(BundleLoadedSource::from_bytes)
                .ok_or_else(|| bundle_list_error(format!("injected source missing: {uri}")))
        }
    }

    #[test]
    fn parses_modes_heuristic_tokens_and_relative_uris() {
        let list = parse_bundle_list(
            "https://example.test/git/bundle-list",
            r#"[bundle]
version = 1
mode = all
heuristic = creationToken
[bundle "new"]
uri = newer.bundle
creationToken = 20
[bundle "old"]
uri = ../old.bundle
creationToken = 10
"#,
        )
        .expect("bundle list");
        assert_eq!(list.mode, BundleListMode::All);
        assert_eq!(list.heuristic, BundleListHeuristic::CreationToken);
        assert_eq!(
            list.entries[0].uri.as_deref(),
            Some("https://example.test/git/newer.bundle")
        );
        assert_eq!(
            list.entries[1].uri.as_deref(),
            Some("https://example.test/old.bundle")
        );
        assert_eq!(
            list.ordered_entries()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["new", "old"]
        );
        assert_eq!(list.version, 1);
    }

    #[test]
    fn parses_flat_any_list_and_orders_creation_token_ties_by_id() {
        let list = parse_bundle_list(
            "https://example.test/lists/root.list",
            "bundle.version=1\nbundle.mode=any\nbundle.heuristic=creationToken\n\
             bundle.beta.uri=beta.bundle\nbundle.beta.creationToken=7\n\
             bundle.alpha.uri=alpha.bundle\nbundle.alpha.creationToken=7\n",
        )
        .expect("flat bundle list");
        assert_eq!(list.mode, BundleListMode::Any);
        assert_eq!(list.entries.len(), 2);
        assert_eq!(
            list.ordered_entries()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "beta"]
        );
    }

    #[test]
    fn parses_filter_location_quotes_comments_and_case_insensitively() {
        let list = parse_bundle_list(
            "https://example.test/lists/root.list",
            r#"[BUNDLE] ; global comment
VERSION = 1
MODE = all # inline comment
Heuristic = creationToken
[bundle "edge"]
URI = "bundles/edge.bundle" ; uri comment
FILTER = "blob:none"
LOCATION = "edge\twest # site"
CREATIONTOKEN = 9
unknown = ignored
"#,
        )
        .expect("quoted bundle list");
        assert_eq!(list.version, 1);
        assert_eq!(list.mode, BundleListMode::All);
        assert_eq!(list.entries[0].filter.as_deref(), Some("blob:none"));
        assert_eq!(
            list.entries[0].location.as_deref(),
            Some("edge\twest # site")
        );
        assert_eq!(
            list.entries[0].uri.as_deref(),
            Some("https://example.test/lists/bundles/edge.bundle")
        );
    }

    #[test]
    fn resolves_file_and_root_relative_uris() {
        assert_eq!(
            resolve_bundle_uri("file:///tmp/bundles/list", "../one.bundle").unwrap(),
            "file:///tmp/one.bundle"
        );
        assert_eq!(
            resolve_bundle_uri("https://example.test/bundles/list", "/one.bundle").unwrap(),
            "https://example.test/one.bundle"
        );
    }

    #[test]
    fn rejects_duplicate_uri_missing_entry_uri_and_bad_version() {
        let duplicate = parse_bundle_list(
            "https://example.test/list",
            r#"[bundle]
mode=any
[bundle "one"]
uri=one
uri=two
"#,
        );
        assert!(duplicate.is_err());
        let defaults = parse_bundle_list("https://example.test/list", "[bundle]\n").unwrap();
        assert_eq!(defaults.version, 1);
        assert_eq!(defaults.mode, BundleListMode::All);
        assert!(
            parse_bundle_list(
                "https://example.test/list",
                "[bundle]\nversion=1\n[bundle \"one\"]\ncreationToken=1\n"
            )
            .is_err()
        );
        assert!(
            parse_bundle_list(
                "https://example.test/list",
                "[bundle]\nversion=2\nmode=all\n"
            )
            .is_err()
        );
        assert!(
            parse_bundle_list(
                "https://example.test/list",
                "[bundle]\nversion=1\nmode=all\n[bundle \"one\"]\nuri=\"unterminated\n"
            )
            .is_err()
        );
        assert!(
            parse_bundle_list(
                "https://example.test/list",
                "[bundle]\nversion=1\nmode=all\n[bundle \"one\"]\nuri=one\\q\n"
            )
            .is_err()
        );
        assert!(
            parse_bundle_list(
                "https://example.test/list",
                "[bundle]\nversion=1\nmode=all\nnot-a-key\n"
            )
            .is_err()
        );
    }

    #[test]
    fn malformed_creation_token_is_a_warning_and_unknown_keys_are_ignored() {
        let list = parse_bundle_list(
            "https://example.test/list",
            r#"[bundle]
version=1
mode=any
unknown=value
[bundle "one"]
uri=one
creationToken=bogus
future=value
"#,
        )
        .expect("bundle list");
        assert_eq!(list.entries[0].creation_token, None);
        assert_eq!(list.warnings.len(), 1);
    }

    #[test]
    fn detects_cycles_and_limits_nested_list_depth() {
        let mut sources = BTreeMap::new();
        sources.insert(
            "file:///tmp/root.list".to_owned(),
            r#"[bundle]
version=1
mode=all
[bundle "one"]
uri=one.list
"#
            .to_owned(),
        );
        sources.insert(
            "file:///tmp/one.list".to_owned(),
            r#"[bundle]
version=1
mode=all
[bundle "root"]
uri=root.list
"#
            .to_owned(),
        );
        assert!(resolve_bundle_list_sources("file:///tmp/root.list", &sources).is_err());

        let mut chain = BTreeMap::new();
        for index in 0..=MAX_BUNDLE_URI_DEPTH {
            let uri = format!("file:///tmp/{index}.list");
            let next = format!("file:///tmp/{}.list", index + 1);
            chain.insert(
                uri,
                format!("[bundle]\nversion=1\nmode=all\n[bundle \"next\"]\nuri={next}\n"),
            );
        }
        assert!(resolve_bundle_list_sources("file:///tmp/0.list", &chain).is_err());
    }

    #[test]
    fn acquires_direct_path_and_file_uri_sources_without_network_io() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("one.bundle");
        let bytes = direct_bundle_bytes("path");
        fs::write(&path, &bytes).expect("write bundle");

        let mut path_loader = load_bundle_source_from_filesystem;
        let path_plan =
            acquire_bundle_uri(path.to_str().unwrap(), &mut path_loader).expect("path bundle");
        assert_eq!(path_plan.sources.len(), 1);
        assert_eq!(path_plan.sources[0].kind, BundleAcquisitionKind::Direct);
        assert_eq!(path_plan.sources[0].storage.path(), Some(path.as_path()));
        assert_eq!(fs::read(&path).expect("read path bundle"), bytes);

        let file_uri = format!("file://{}", path.display());
        let mut file_loader = load_bundle_source_from_filesystem;
        let file_plan = acquire_bundle_uri(&file_uri, &mut file_loader).expect("file URI bundle");
        assert_eq!(file_plan.sources.len(), 1);
        assert_eq!(file_plan.sources[0].storage.path(), Some(path.as_path()));
    }

    #[test]
    fn all_attempts_every_entry_and_exposes_download_and_import_order() {
        let root = "https://example.test/root.list";
        let mut sources = BTreeMap::new();
        sources.insert(
            root.to_owned(),
            r#"[bundle]
version=1
mode=all
heuristic=creationToken
[bundle "new"]
uri=new.bundle
creationToken=20
[bundle "missing"]
uri=missing.bundle
creationToken=15
[bundle "old"]
uri=old.bundle
creationToken=10
"#
            .as_bytes()
            .to_vec(),
        );
        sources.insert(
            "https://example.test/new.bundle".to_owned(),
            direct_bundle_bytes("new"),
        );
        sources.insert(
            "https://example.test/old.bundle".to_owned(),
            direct_bundle_bytes("old"),
        );
        let mut loader = map_loader(sources);

        let plan = acquire_bundle_uri(root, &mut loader).expect("partial all success");
        assert_eq!(
            plan.sources
                .iter()
                .map(|source| source.metadata.id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("new"), Some("old")]
        );
        assert_eq!(plan.warnings.len(), 1);
        assert!(plan.warnings[0].to_string().contains("missing.bundle"));
        assert_eq!(
            plan.download_order()
                .iter()
                .map(|source| source.metadata.id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("new"), Some("old")]
        );
        assert_eq!(
            plan.import_order()
                .iter()
                .map(|source| source.metadata.id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("old"), Some("new")]
        );
    }

    #[test]
    fn any_falls_back_after_failures_and_stops_at_first_success() {
        let root = "https://example.test/root.list";
        let mut sources = BTreeMap::new();
        sources.insert(
            root.to_owned(),
            r#"[bundle]
mode=any
[bundle "first"]
uri=first.bundle
[bundle "second"]
uri=second.bundle
[bundle "third"]
uri=third.bundle
"#
            .as_bytes()
            .to_vec(),
        );
        sources.insert(
            "https://example.test/second.bundle".to_owned(),
            direct_bundle_bytes("second"),
        );
        sources.insert(
            "https://example.test/third.bundle".to_owned(),
            direct_bundle_bytes("third"),
        );
        let mut loader = map_loader(sources);

        let plan = acquire_bundle_uri(root, &mut loader).expect("any fallback");
        assert_eq!(plan.sources.len(), 1);
        assert_eq!(plan.sources[0].metadata.id.as_deref(), Some("second"));
        assert_eq!(plan.warnings.len(), 1);
        assert!(plan.warnings[0].to_string().contains("first.bundle"));
    }

    #[test]
    fn any_continues_after_a_cycle_candidate() {
        let root = "https://example.test/root.list";
        let cycle = "https://example.test/cycle.list";
        let good = "https://example.test/good.bundle";
        let mut sources = BTreeMap::new();
        sources.insert(
            root.to_owned(),
            format!(
                "[bundle]\nmode=any\n[bundle \"cycle\"]\nuri=cycle.list\n\
                 [bundle \"good\"]\nuri={good}\n"
            )
            .into_bytes(),
        );
        sources.insert(
            cycle.to_owned(),
            b"[bundle]\nmode=all\n[bundle \"root\"]\nuri=root.list\n".to_vec(),
        );
        sources.insert(good.to_owned(), direct_bundle_bytes("good"));

        let mut loader = map_loader(sources);
        let plan = acquire_bundle_uri(root, &mut loader).expect("cycle fallback");
        assert_eq!(plan.sources.len(), 1);
        assert_eq!(plan.sources[0].metadata.id.as_deref(), Some("good"));
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.to_string().contains("cycle.list"))
        );
    }

    #[test]
    fn nested_all_failure_is_a_warning_for_an_any_parent() {
        let root = "https://example.test/root.list";
        let failed = "https://example.test/failed.list";
        let good = "https://example.test/good.bundle";
        let mut sources = BTreeMap::new();
        sources.insert(
            root.to_owned(),
            format!(
                "[bundle]\nmode=any\n[bundle \"failed\"]\nuri=failed.list\n\
                 [bundle \"good\"]\nuri={good}\n"
            )
            .into_bytes(),
        );
        sources.insert(
            failed.to_owned(),
            b"[bundle]\nmode=all\n[bundle \"missing\"]\nuri=missing.bundle\n".to_vec(),
        );
        sources.insert(good.to_owned(), direct_bundle_bytes("good"));

        let mut loader = map_loader(sources);
        let plan = acquire_bundle_uri(root, &mut loader).expect("nested failure fallback");
        assert_eq!(plan.sources.len(), 1);
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.to_string().contains("failed.list"))
        );
    }

    #[test]
    fn source_order_is_preserved_without_creation_token_heuristic() {
        let root = "https://example.test/root.list";
        let first = "https://example.test/first.bundle";
        let second = "https://example.test/second.bundle";
        let mut sources = BTreeMap::new();
        sources.insert(
            root.to_owned(),
            b"[bundle]\nmode=all\n[bundle \"z-first\"]\nuri=first.bundle\n\
              [bundle \"a-second\"]\nuri=second.bundle\n"
                .to_vec(),
        );
        sources.insert(first.to_owned(), direct_bundle_bytes("first"));
        sources.insert(second.to_owned(), direct_bundle_bytes("second"));
        let mut loader = map_loader(sources);
        let plan = acquire_bundle_uri(root, &mut loader).expect("source order");
        let ids = |sources: Vec<&BundleAcquisitionSource>| {
            sources
                .into_iter()
                .map(|source| source.metadata.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(plan.download_order()),
            vec![Some("z-first".to_owned()), Some("a-second".to_owned())]
        );
        assert_eq!(
            ids(plan.import_order()),
            vec![Some("z-first".to_owned()), Some("a-second".to_owned())]
        );
    }

    #[test]
    fn parser_bounds_list_bytes_lines_and_entries() {
        assert!(
            parse_bundle_list(
                "https://example.test/large.list",
                &"x".repeat(MAX_BUNDLE_LIST_BYTES + 1)
            )
            .is_err()
        );

        assert!(
            parse_bundle_list(
                "https://example.test/lines.list",
                &"#\n".repeat(MAX_BUNDLE_LIST_LINES + 1)
            )
            .is_err()
        );

        let mut many_entries = String::from("[bundle]\n");
        for index in 0..=MAX_BUNDLE_LIST_ENTRIES {
            many_entries.push_str(&format!("bundle.id{index:05}.uri=entry{index}.bundle\n"));
        }
        assert!(parse_bundle_list("https://example.test/entries.list", &many_entries).is_err());
    }

    #[test]
    fn owned_temp_guard_removes_only_the_original_regular_file() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("owned.bundle");
        fs::write(&path, b"bundle").expect("write owned file");
        let source = BundleLoadedSource::from_owned_temp(&path).expect("owned source");
        drop(source);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn owned_temp_guard_does_not_remove_a_replacement_symlink() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("owned.bundle");
        let target = temp.path().join("target.bundle");
        fs::write(&path, b"bundle").expect("write owned file");
        fs::write(&target, b"target").expect("write target file");
        let source = BundleLoadedSource::from_owned_temp(&path).expect("owned source");
        fs::remove_file(&path).expect("remove original");
        symlink(&target, &path).expect("replace with symlink");
        drop(source);
        assert!(path.exists());
        assert_eq!(fs::read(&target).expect("read target"), b"target");
    }

    #[cfg(unix)]
    #[test]
    fn owned_temp_guard_rejects_symlink_sources() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("owned.bundle");
        let target = temp.path().join("target.bundle");
        fs::write(&target, b"target").expect("write target file");
        symlink(&target, &path).expect("create source symlink");
        assert!(BundleLoadedSource::from_owned_temp(&path).is_err());
        assert_eq!(fs::read(&target).expect("read target"), b"target");
    }

    #[test]
    fn all_failure_and_cycle_drop_every_owned_temporary_source() {
        let temp = TempDir::new().expect("temp dir");
        let temp_path = temp.path().to_path_buf();
        let root = "file:///virtual/root.list";
        let child = "file:///virtual/child.list";
        let root_bytes = b"[bundle]\nmode=all\n[bundle \"child\"]\nuri=child.list\n".to_vec();
        let child_bytes = b"[bundle]\nmode=all\n[bundle \"root\"]\nuri=root.list\n".to_vec();
        let mut load_count = 0usize;
        let mut loader = move |uri: &str| {
            let bytes = match uri {
                value if value == root => root_bytes.clone(),
                value if value == child => child_bytes.clone(),
                _ => return Err(bundle_list_error("unexpected injected URI")),
            };
            let path = temp_path.join(format!("source-{load_count}"));
            load_count += 1;
            fs::write(&path, &bytes).expect("write owned source");
            Ok(BundleLoadedSource::from_owned_temp(path).expect("owned source"))
        };

        assert!(acquire_bundle_uri(root, &mut loader).is_err());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    #[test]
    fn all_failure_drops_root_list_temporary_source() {
        let temp = TempDir::new().expect("temp dir");
        let temp_path = temp.path().to_path_buf();
        let root = "file:///virtual/root.list";
        let root_bytes = b"[bundle]\nmode=all\n[bundle \"missing\"]\nuri=missing.bundle\n".to_vec();
        let mut load_count = 0usize;
        let mut loader = move |uri: &str| {
            if uri != root {
                return Err(bundle_list_error("injected missing bundle"));
            }
            let path = temp_path.join(format!("source-{load_count}"));
            load_count += 1;
            fs::write(&path, &root_bytes).expect("write owned source");
            Ok(BundleLoadedSource::from_owned_temp(path).expect("owned source"))
        };

        assert!(acquire_bundle_uri(root, &mut loader).is_err());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }
}
